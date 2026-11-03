//! The protocol shell: handshake, message loop, and diagnostic publishing.
//!
//! Document-sync notifications maintain the overlay store; every one of them
//! then triggers a full reanalysis of the project the document belongs to and
//! a republish of its diagnostics. Publishing covers three sets — files with
//! findings, files published last round (to clear what moved or vanished),
//! and open buffers (so an editor always holds current state) — which keeps
//! the client exactly in step with one load over current text.
//!
//! Known limitation: diagnostics are published per URI, so when two projects
//! both report findings on the same file (one project's values or evidence
//! file living inside another project), whichever root recomputed last wins
//! that URI. Projects in one workspace are normally disjoint, so this stays
//! a documented edge instead of a merge layer.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument,
    DidSaveTextDocument, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{CodeLensRefresh, Request as _};
use lsp_types::{
    CodeLensOptions, Diagnostic, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    HoverProviderCapability, InitializeParams, OneOf, PublishDiagnosticsParams, RenameOptions,
    SemanticTokensFullOptions, SemanticTokensOptions, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, WorkDoneProgressOptions,
};
use serde::de::DeserializeOwned;

use crate::state::Overlays;
use crate::{diagnostics, requests, roots, tokens, uri};

/// One boxed error type for everything that can end the server: protocol
/// violations, closed channels, and serialization failures alike.
type ServerError = Box<dyn std::error::Error + Send + Sync>;

/// Runs the server over `connection` until the client disconnects or asks it
/// to shut down. The caller still owns the connection and is the one to drop
/// it before joining the transport's I/O threads.
///
/// # Errors
///
/// Returns an error when the protocol is violated or the transport fails;
/// a clean `shutdown`/`exit` is `Ok`.
pub fn run(connection: &Connection) -> Result<(), ServerError> {
    let capabilities = serde_json::to_value(capabilities())?;
    let initialize = connection.initialize(capabilities)?;

    let mut server = Server {
        overlays: Overlays::default(),
        published: HashMap::new(),
        lens_refresh: supports_lens_refresh(initialize),
        sent_requests: 0,
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    return Ok(());
                }
                match requests::answer(&server.overlays, request) {
                    Ok(response) => respond(connection, response)?,
                    Err(request) => refuse(connection, request)?,
                }
            }
            Message::Notification(notification) => server.handle(connection, notification)?,
            // The only requests this server sends are refresh nudges, whose
            // responses carry nothing worth reading.
            Message::Response(_) => {}
        }
    }
    Ok(())
}

/// Whether the client takes `workspace/codeLens/refresh`. A params value that
/// does not parse reads as not supporting it, never as a startup failure.
fn supports_lens_refresh(initialize: serde_json::Value) -> bool {
    serde_json::from_value::<InitializeParams>(initialize)
        .ok()
        .and_then(|params| params.capabilities.workspace)
        .and_then(|workspace| workspace.code_lens)
        .and_then(|code_lens| code_lens.refresh_support)
        .unwrap_or(false)
}

/// What this server can do. Full-text sync matches the analysis model: every
/// change reloads the whole project anyway, so incremental edits would buy
/// nothing.
fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        semantic_tokens_provider: Some(
            SemanticTokensOptions {
                work_done_progress_options: WorkDoneProgressOptions::default(),
                legend: tokens::legend(),
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            }
            .into(),
        ),
        inlay_hint_provider: Some(OneOf::Left(true)),
        code_lens_provider: Some(CodeLensOptions {
            resolve_provider: Some(false),
        }),
        ..ServerCapabilities::default()
    }
}

struct Server {
    overlays: Overlays,
    /// Files that carried findings in the last publish, per project root, so
    /// a finding that disappears is cleared on the client too.
    published: HashMap<PathBuf, HashSet<PathBuf>>,
    /// Whether the client takes `workspace/codeLens/refresh`.
    lens_refresh: bool,
    /// How many requests this server has sent, numbering the next one.
    sent_requests: i32,
}

impl Server {
    fn handle(
        &mut self,
        connection: &Connection,
        notification: Notification,
    ) -> Result<(), ServerError> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Some(params) = cast::<DidOpenTextDocumentParams>(notification) else {
                    return Ok(());
                };
                let Some(path) = uri::to_path(&params.text_document.uri) else {
                    return Ok(());
                };
                self.overlays.open(path.clone(), params.text_document.text);
                self.refresh(connection, &[path])
            }
            DidChangeTextDocument::METHOD => {
                let Some(params) = cast::<DidChangeTextDocumentParams>(notification) else {
                    return Ok(());
                };
                let Some(path) = uri::to_path(&params.text_document.uri) else {
                    return Ok(());
                };
                // Full sync: the last change carries the whole new text.
                let Some(change) = params.content_changes.into_iter().next_back() else {
                    return Ok(());
                };
                self.overlays.open(path.clone(), change.text);
                self.refresh(connection, &[path])
            }
            DidSaveTextDocument::METHOD => {
                // The overlay already holds the saved text; a reanalysis
                // still runs because sibling files may read differently now
                // (a save can be the editor's response to a disk change).
                let Some(params) = cast::<DidSaveTextDocumentParams>(notification) else {
                    return Ok(());
                };
                match uri::to_path(&params.text_document.uri) {
                    Some(path) => self.refresh(connection, &[path]),
                    None => Ok(()),
                }
            }
            DidCloseTextDocument::METHOD => {
                let Some(params) = cast::<DidCloseTextDocumentParams>(notification) else {
                    return Ok(());
                };
                let Some(path) = uri::to_path(&params.text_document.uri) else {
                    return Ok(());
                };
                self.overlays.close(&path);
                self.refresh(connection, &[path])
            }
            DidChangeWatchedFiles::METHOD => {
                let Some(params) = cast::<DidChangeWatchedFilesParams>(notification) else {
                    return Ok(());
                };
                let paths: Vec<PathBuf> = params
                    .changes
                    .iter()
                    .filter_map(|event| uri::to_path(&event.uri))
                    .collect();
                self.refresh(connection, &paths)?;
                // A watched change (an approval record, evidence, a manifest)
                // moves code-lens content without any document edit, and
                // lenses are pulled, not published — so nudge a client that
                // opted in to re-request them.
                self.request_lens_refresh(connection)
            }
            _ => Ok(()),
        }
    }

    /// Reanalyzes every project the changed files belong to, each once.
    ///
    /// A manifest event redraws project boundaries — a created `inka.toml`
    /// claims files from an outer project, a deleted one hands them back —
    /// so it reanalyzes every known project, not just the one the path
    /// resolves to now. A file with no manifest above it gets the same
    /// treatment: it may have just left a project that still shows findings.
    fn refresh(&mut self, connection: &Connection, changed: &[PathBuf]) -> Result<(), ServerError> {
        let mut affected: HashSet<PathBuf> = HashSet::new();
        let mut boundaries_moved = false;
        for path in changed {
            if path.file_name().is_some_and(|name| name == "inka.toml") {
                boundaries_moved = true;
            }
            match roots::project_root(path) {
                Some(root) => {
                    affected.insert(root);
                }
                None => boundaries_moved = true,
            }
        }
        if boundaries_moved {
            affected.extend(self.published.keys().cloned());
        }
        for root in affected {
            self.recompute(connection, &root)?;
        }
        Ok(())
    }

    fn recompute(&mut self, connection: &Connection, root: &Path) -> Result<(), ServerError> {
        // A root whose manifest vanished is not a broken project but a gone
        // one: clear everything it published and forget it, instead of
        // anchoring a read error to a file that no longer exists.
        if !root.join("inka.toml").is_file() {
            for path in self.published.remove(root).unwrap_or_default() {
                publish(connection, &path, Vec::new())?;
            }
            return Ok(());
        }

        let found = diagnostics::analyze(root, &self.overlays);

        let mut targets: HashSet<PathBuf> = found.keys().cloned().collect();
        targets.extend(self.published.remove(root).unwrap_or_default());
        targets.extend(
            self.overlays
                .paths()
                .filter(|path| roots::project_root(path).as_deref() == Some(root))
                .cloned(),
        );

        let mut carrying = HashSet::new();
        for path in targets {
            let current = found.get(&path).cloned().unwrap_or_default();
            if !current.is_empty() {
                carrying.insert(path.clone());
            }
            publish(connection, &path, current)?;
        }
        self.published.insert(root.to_path_buf(), carrying);
        Ok(())
    }

    /// Asks the client to re-request every code lens, when it said it takes
    /// the request.
    fn request_lens_refresh(&mut self, connection: &Connection) -> Result<(), ServerError> {
        if !self.lens_refresh {
            return Ok(());
        }
        self.sent_requests += 1;
        let request = Request::new(
            RequestId::from(self.sent_requests),
            CodeLensRefresh::METHOD.to_owned(),
            serde_json::Value::Null,
        );
        connection
            .sender
            .send(Message::Request(request))
            .map_err(|error| format!("cannot send a code lens refresh: {error}"))?;
        Ok(())
    }
}

fn publish(
    connection: &Connection,
    path: &Path,
    diagnostics: Vec<Diagnostic>,
) -> Result<(), ServerError> {
    // A path that cannot be a URI was never received as one, so nothing
    // on the client side could be showing diagnostics for it.
    let Some(uri) = uri::from_path(path) else {
        return Ok(());
    };
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version: None,
    };
    let notification = Notification::new(PublishDiagnostics::METHOD.to_owned(), params);
    connection
        .sender
        .send(Message::Notification(notification))
        .map_err(|error| format!("cannot send diagnostics: {error}"))?;
    Ok(())
}

/// Answers a request this server does not implement, which keeps a waiting
/// client from hanging on it.
fn refuse(connection: &Connection, request: Request) -> Result<(), ServerError> {
    respond(
        connection,
        Response::new_err(
            request.id,
            ErrorCode::MethodNotFound as i32,
            format!("inka-lsp does not implement {}", request.method),
        ),
    )
}

fn respond(connection: &Connection, response: Response) -> Result<(), ServerError> {
    connection
        .sender
        .send(Message::Response(response))
        .map_err(|error| format!("cannot send response: {error}"))?;
    Ok(())
}

/// Deserializes notification params, dropping (with a stderr note) what does
/// not parse: one malformed notification must not take the session down.
fn cast<P: DeserializeOwned>(notification: Notification) -> Option<P> {
    match serde_json::from_value(notification.params) {
        Ok(params) => Some(params),
        Err(error) => {
            eprintln!(
                "inka-lsp: ignoring malformed {} notification: {error}",
                notification.method
            );
            None
        }
    }
}
