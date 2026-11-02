//! Answering positional requests: hover, goto definition, references, and
//! rename (with its prepare step).
//!
//! Every request loads the project fresh through the overlay store, exactly
//! like diagnostics do, so an answer always describes what is on screen. A
//! request about something that is not an identifier (or not inside a
//! loadable project) answers `null`; only rename reports errors, because a
//! refused rename needs a reason the editor can show.

use std::collections::HashMap;

use lsp_server::{ErrorCode, Request, Response};
use lsp_types::request::{
    GotoDefinition, HoverRequest, PrepareRenameRequest, References, Rename, Request as _,
};
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams, Location,
    MarkupContent, MarkupKind, Position, PrepareRenameResponse, ReferenceParams, RenameParams,
    TextDocumentIdentifier, TextDocumentPositionParams, TextEdit, Uri, WorkspaceEdit,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use inka_core::project::{Project, load_project_with};
use inka_core::source::{FileId, SourceMap, Span};

use crate::state::Overlays;
use crate::{navigate, position, rename, roots, uri};

/// Answers one request, or hands it back when the method is not one of ours.
pub fn answer(overlays: &Overlays, request: Request) -> Result<Response, Request> {
    match request.method.as_str() {
        HoverRequest::METHOD => Ok(dispatch(overlays, request, hover)),
        GotoDefinition::METHOD => Ok(dispatch(overlays, request, definition)),
        References::METHOD => Ok(dispatch(overlays, request, references)),
        PrepareRenameRequest::METHOD => Ok(dispatch(overlays, request, prepare_rename)),
        Rename::METHOD => Ok(dispatch(overlays, request, rename_to)),
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

fn locate(
    overlays: &Overlays,
    text_document: &TextDocumentIdentifier,
    at: Position,
) -> Option<Located> {
    let path = uri::to_path(&text_document.uri)?;
    let root = roots::project_root(&path)?;
    let project = load_project_with(&root, overlays).result.ok()?;
    let (file, offset) = {
        let (file, source) = project
            .source_map
            .files()
            .find(|(_, source)| source.path == path)?;
        (file, position::offset_of(source, at))
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
    let Some(markdown) = navigate::hover(&located.project, symbol.name()) else {
        return Ok(None);
    };
    let file = located
        .project
        .source_map
        .file(located.file)
        .expect("the file was just resolved from this map");
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
    let Some(block) = navigate::definition(&located.project, symbol.name()) else {
        return Ok(None);
    };
    Ok(
        location_of(&located.project.source_map, block.common().id.span)
            .map(GotoDefinitionResponse::Scalar),
    )
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
    // A dangling reference is fixed by editing it, not renamed; refusing at
    // prepare time keeps the editor from even opening the rename box.
    if navigate::definition(&located.project, symbol.name()).is_none() {
        return Ok(None);
    }
    let file = located
        .project
        .source_map
        .file(located.file)
        .expect("the file was just resolved from this map");
    Ok(Some(PrepareRenameResponse::Range(position::range(
        file,
        symbol.span(),
    ))))
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
    let block =
        navigate::definition(&located.project, &old).expect("validated: the definition exists");
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
    let rewrites = rename::approval_rewrites(&located.root, mode_keyword, &old, overlays)
        .map_err(|error| format!("cannot read the approvals: {error}"))?;
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
