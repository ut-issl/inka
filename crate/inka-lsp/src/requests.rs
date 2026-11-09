//! Answering document requests: hover, goto definition, references, rename
//! (with its prepare step), semantic tokens, inlay hints, code lenses, and
//! document formatting.
//!
//! Every request loads the project fresh through the overlay store, exactly
//! like diagnostics do, so an answer always describes what is on screen. A
//! request about something that is not an identifier (or not inside a
//! loadable project) answers `null`; only rename reports errors, because a
//! refused rename needs a reason the editor can show.
//!
//! The decoration surfaces follow the same rule by design: while an edit
//! keeps the project from loading, tokens, hints, and lenses answer `null`
//! and diagnostics are the surface that says why. The server keeps no
//! last-good result to serve instead — the no-incremental-state policy — so
//! a client may show the editor's fallback styling until the text loads
//! again.

use std::collections::HashMap;

use lsp_server::{ErrorCode, Request, Response};
use lsp_types::request::{
    CodeLensRequest, Formatting, GotoDefinition, HoverRequest, InlayHintRequest,
    PrepareRenameRequest, References, Rename, Request as _, SemanticTokensFullRequest,
};
use lsp_types::{
    CodeLens, CodeLensParams, DocumentFormattingParams, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, InlayHint, InlayHintParams,
    Location, MarkupContent, MarkupKind, Position, PrepareRenameResponse, ReferenceParams,
    RenameParams, SemanticTokens, SemanticTokensParams, TextDocumentIdentifier,
    TextDocumentPositionParams, TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use inka_core::project::{Project, format_project, load_project_with};
use inka_core::source::{FileId, SourceMap, Span};

use crate::state::Overlays;
use crate::{hints, lens, navigate, position, rename, roots, tokens, uri};

/// Answers one request, or hands it back when the method is not one of ours.
pub fn answer(overlays: &Overlays, request: Request) -> Result<Response, Request> {
    match request.method.as_str() {
        HoverRequest::METHOD => Ok(dispatch(overlays, request, hover)),
        GotoDefinition::METHOD => Ok(dispatch(overlays, request, definition)),
        References::METHOD => Ok(dispatch(overlays, request, references)),
        PrepareRenameRequest::METHOD => Ok(dispatch(overlays, request, prepare_rename)),
        Rename::METHOD => Ok(dispatch(overlays, request, rename_to)),
        SemanticTokensFullRequest::METHOD => Ok(dispatch(overlays, request, semantic_tokens)),
        InlayHintRequest::METHOD => Ok(dispatch(overlays, request, inlay_hints)),
        CodeLensRequest::METHOD => Ok(dispatch(overlays, request, code_lens)),
        Formatting::METHOD => Ok(dispatch(overlays, request, formatting)),
        _ => Err(request),
    }
}

/// Parses the params, runs the handler, and shapes the outcome into a
/// response: `Ok(None)` is the protocol's `null`, an `Err` message becomes a
/// `RequestFailed` the editor shows.
fn dispatch<P: DeserializeOwned, R: Serialize>(
    overlays: &Overlays,
    request: Request,
    handler: fn(&Overlays, P) -> Result<Option<R>, String>,
) -> Response {
    let id = request.id.clone();
    let params: P = match serde_json::from_value(request.params) {
        Ok(params) => params,
        Err(error) => {
            return Response::new_err(id, ErrorCode::InvalidParams as i32, error.to_string());
        }
    };
    match handler(overlays, params) {
        Ok(result) => Response::new_ok(id, result),
        Err(message) => Response::new_err(id, ErrorCode::RequestFailed as i32, message),
    }
}

/// A request position resolved against a freshly loaded project.
struct Located {
    root: std::path::PathBuf,
    project: Project,
    file: FileId,
    offset: usize,
}

/// A request's document resolved against a freshly loaded project: the root
/// it belongs to, the loaded project, and the document's file id in it.
fn load_document(
    overlays: &Overlays,
    text_document: &TextDocumentIdentifier,
) -> Option<(std::path::PathBuf, Project, FileId)> {
    let path = uri::to_path(&text_document.uri)?;
    let root = roots::project_root(&path)?;
    let project = load_project_with(&root, overlays).result.ok()?;
    let file = project
        .source_map
        .files()
        .find(|(_, source)| source.path == path)
        .map(|(file, _)| file)?;
    Some((root, project, file))
}

fn locate(
    overlays: &Overlays,
    text_document: &TextDocumentIdentifier,
    at: Position,
) -> Option<Located> {
    let (root, project, file) = load_document(overlays, text_document)?;
    let offset = {
        let source = project.source_map.file(file)?;
        position::offset_of(source, at)
    };
    Some(Located {
        root,
        project,
        file,
        offset,
    })
}

fn location_of(source_map: &SourceMap, span: Span) -> Option<Location> {
    let file = source_map.file(span.file_id)?;
    Some(Location {
        uri: uri::from_path(&file.path)?,
        range: position::range(file, span),
    })
}

// Handlers share the `dispatch` fn-pointer shape even where a particular one
// never fails or could borrow its params, so the per-handler lints about the
// uniform signature are expected.
#[allow(clippy::unnecessary_wraps)]
fn hover(overlays: &Overlays, params: HoverParams) -> Result<Option<Hover>, String> {
    let document = params.text_document_position_params;
    let Some(located) = locate(overlays, &document.text_document, document.position) else {
        return Ok(None);
    };
    let Some(symbol) = navigate::symbol_at(&located.project, located.file, located.offset) else {
        return Ok(None);
    };
    // A definition hovers as itself, so a duplicated id still reads exactly;
    // a reference resolves by name (ambiguity there is a check error).
    let block = match symbol {
        navigate::Symbol::Definition(block) => block,
        navigate::Symbol::Reference(_) => {
            let Some(block) = navigate::definition(&located.project, symbol.name()) else {
                return Ok(None);
            };
            block
        }
    };
    let markdown = navigate::hover(block);
    let Some(file) = located.project.source_map.file(located.file) else {
        return Ok(None);
    };
    Ok(Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(position::range(file, symbol.span())),
    }))
}

#[allow(clippy::unnecessary_wraps)] // see `hover`
fn definition(
    overlays: &Overlays,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>, String> {
    let document = params.text_document_position_params;
    let Some(located) = locate(overlays, &document.text_document, document.position) else {
        return Ok(None);
    };
    let Some(symbol) = navigate::symbol_at(&located.project, located.file, located.offset) else {
        return Ok(None);
    };
    let mut locations: Vec<Location> = navigate::definitions(&located.project, symbol.name())
        .into_iter()
        .filter_map(|block| location_of(&located.project.source_map, block.common().id.span))
        .collect();
    // A duplicated id (a check error the user is about to fix) lists every
    // definition instead of silently picking the first.
    Ok(match locations.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(locations.remove(0))),
        _ => Some(GotoDefinitionResponse::Array(locations)),
    })
}

#[allow(clippy::unnecessary_wraps)] // see `hover`
fn references(
    overlays: &Overlays,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>, String> {
    let document = params.text_document_position;
    let Some(located) = locate(overlays, &document.text_document, document.position) else {
        return Ok(None);
    };
    let Some(symbol) = navigate::symbol_at(&located.project, located.file, located.offset) else {
        return Ok(None);
    };
    let name = symbol.name().to_owned();
    let mut spans = navigate::references(&located.project, &name);
    if params.context.include_declaration
        && let Some(block) = navigate::definition(&located.project, &name)
    {
        spans.push(block.common().id.span);
    }
    let locations = spans
        .into_iter()
        .filter_map(|span| location_of(&located.project.source_map, span))
        .collect();
    Ok(Some(locations))
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // see `hover`
fn prepare_rename(
    overlays: &Overlays,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>, String> {
    let Some(located) = locate(overlays, &params.text_document, params.position) else {
        return Ok(None);
    };
    let Some(symbol) = navigate::symbol_at(&located.project, located.file, located.offset) else {
        return Ok(None);
    };
    // A dangling reference is fixed by editing it (not renamed), and a
    // duplicated id is ambiguous; refusing at prepare time keeps the editor
    // from even opening the rename box.
    if navigate::definitions(&located.project, symbol.name()).len() != 1 {
        return Ok(None);
    }
    let Some(file) = located.project.source_map.file(located.file) else {
        return Ok(None);
    };
    Ok(Some(PrepareRenameResponse::Range(position::range(
        file,
        symbol.span(),
    ))))
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // see `hover`
fn semantic_tokens(
    overlays: &Overlays,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokens>, String> {
    let Some((_, project, file)) = load_document(overlays, &params.text_document) else {
        return Ok(None);
    };
    Ok(tokens::full(&project, file))
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // see `hover`
fn inlay_hints(
    overlays: &Overlays,
    params: InlayHintParams,
) -> Result<Option<Vec<InlayHint>>, String> {
    let Some((_, project, file)) = load_document(overlays, &params.text_document) else {
        return Ok(None);
    };
    let Some(source) = project.source_map.file(file) else {
        return Ok(None);
    };
    let start = position::offset_of(source, params.range.start);
    let end = position::offset_of(source, params.range.end);
    Ok(Some(hints::hints(&project, file, start, end)))
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // see `hover`
fn code_lens(overlays: &Overlays, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>, String> {
    let Some((root, project, file)) = load_document(overlays, &params.text_document) else {
        return Ok(None);
    };
    Ok(lens::lenses(&root, &project, file))
}

/// Formats the document exactly as `inka fmt` would, as one edit replacing
/// the whole text — an empty list when it is already formatted.
///
/// Like `inka fmt`, it formats nothing — answering `null` — when the
/// document is outside a project, its manifest or values cannot be loaded
/// (they decide whether a fence may be re-indented), or the document does
/// not parse; diagnostics already say why.
#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)] // see `hover`
fn formatting(
    overlays: &Overlays,
    params: DocumentFormattingParams,
) -> Result<Option<Vec<TextEdit>>, String> {
    let Some(path) = uri::to_path(&params.text_document.uri) else {
        return Ok(None);
    };
    let Some(root) = roots::project_root(&path) else {
        return Ok(None);
    };
    let Ok(format) = format_project(&root, overlays) else {
        return Ok(None);
    };
    let Some(document) = format.files.into_iter().find(|file| file.path == path) else {
        return Ok(None);
    };
    let Ok(formatted) = document.result else {
        return Ok(None);
    };
    let Some(formatted) = formatted else {
        return Ok(Some(Vec::new()));
    };
    let Some((file_id, source)) = format
        .source_map
        .files()
        .find(|(_, source)| source.path == path)
    else {
        return Ok(None);
    };
    let whole = Span {
        file_id,
        start: 0,
        end: source.text.len(),
    };
    Ok(Some(vec![TextEdit {
        range: position::range(source, whole),
        new_text: formatted,
    }]))
}

fn rename_to(overlays: &Overlays, params: RenameParams) -> Result<Option<WorkspaceEdit>, String> {
    let document = params.text_document_position;
    let located = locate(overlays, &document.text_document, document.position)
        .ok_or("the document is not part of a loadable inka project")?;
    let symbol = navigate::symbol_at(&located.project, located.file, located.offset)
        .ok_or("there is no block id or reference here")?;
    let old = symbol.name().to_owned();
    let new = params.new_name;
    navigate::validate_rename(&located.project, &old, &new)?;
    if new == old {
        return Ok(None);
    }
    let block = navigate::definition(&located.project, &old)
        .ok_or("the definition disappeared while renaming")?;
    let mode_keyword = navigate::mode_keyword(block);

    // `Uri` hashes by its string; the interior mutability clippy sees is a
    // parse cache inside fluent-uri, and `WorkspaceEdit` fixes this key type.
    #[allow(clippy::mutable_key_type)]
    let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
    for span in navigate::rename_spans(&located.project, &old) {
        let file = located
            .project
            .source_map
            .file(span.file_id)
            .ok_or("a span names a file outside the loaded project")?;
        let uri = uri::from_path(&file.path).ok_or("a project path cannot be named as a URI")?;
        changes.entry(uri).or_default().push(TextEdit {
            range: position::range(file, span),
            new_text: new.clone(),
        });
    }

    // The approvals move with the node: ids are filing, not contract, so the
    // records keep expressing the same judgement under the new name.
    let rewrites = rename::approval_rewrites(&located.root, mode_keyword, &old, overlays)?;
    for rewrite in rewrites {
        let uri =
            uri::from_path(&rewrite.path).ok_or("an approvals path cannot be named as a URI")?;
        let mut map = SourceMap::new();
        let file_id = map.add_file(rewrite.path, rewrite.text);
        let file = map.file(file_id).expect("the file was just added");
        let edits = changes.entry(uri).or_default();
        for (start, end) in rewrite.ranges {
            edits.push(TextEdit {
                range: position::range(
                    file,
                    Span {
                        file_id,
                        start,
                        end,
                    },
                ),
                new_text: new.clone(),
            });
        }
    }

    Ok(Some(WorkspaceEdit {
        changes: Some(changes),
        ..WorkspaceEdit::default()
    }))
}
