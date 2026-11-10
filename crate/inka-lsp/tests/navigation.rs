//! Protocol-level tests for hover, goto definition, references, and rename —
//! including the demo the full-plan names: renaming a block id invalidates
//! nothing, because ids are filing, not contract.

use std::path::{Path, PathBuf};

use lsp_types::request::{GotoDefinition, HoverRequest, PrepareRenameRequest, References, Rename};
use lsp_types::{
    GotoDefinitionResponse, Hover, HoverContents, Location, Position, PrepareRenameResponse, Range,
    TextEdit, Uri, WorkspaceEdit,
};

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::eval::evaluate;
use inka_core::graph::Graph;
use inka_core::hash::LocalView;
use inka_core::project::load_project;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::ArgumentView;

mod common;
use common::{Client, MANIFEST, uri_of};

const ROOT_FILE: &str = "objective root {\n  ```\n  ok\n  ```\n  children: leaf\n}\n";
const LEAF_FILE: &str = "fiat leaf {\n  ```\n  accepted\n  ```\n}\n";

/// A two-file project: `root` in a.inka referencing `leaf` defined in b.inka.
fn two_file_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
    std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
    let a = temp.path().join("argument/a.inka");
    let b = temp.path().join("argument/b.inka");
    std::fs::write(&a, ROOT_FILE).expect("a.inka");
    std::fs::write(&b, LEAF_FILE).expect("b.inka");
    (temp, a, b)
}

fn position_params(uri: &Uri, line: u32, character: u32) -> serde_json::Value {
    serde_json::json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": character },
    })
}

fn result_of(client: &Client, id: i32) -> serde_json::Value {
    client
        .receive_response(id)
        .response_result
        .expect("the request succeeds")
}

#[test]
fn hover_on_a_reference_shows_the_referenced_contract() {
    // Arrange
    let (_temp, a, _b) = two_file_project();
    let uri = uri_of(&a);
    let client = Client::start();

    // Act: hover inside `leaf` on the children line.
    client.request::<HoverRequest>(2, position_params(&uri, 4, 13));
    let hover: Hover = serde_json::from_value(result_of(&client, 2)).expect("a hover comes back");

    // Assert
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup hover contents");
    };
    assert!(markup.value.contains("**fiat** `leaf`"), "{}", markup.value);
    assert!(markup.value.contains("accepted"), "{}", markup.value);

    client.shutdown();
}

#[test]
fn goto_definition_jumps_from_a_reference_into_the_defining_file() {
    // Arrange
    let (_temp, a, b) = two_file_project();
    let client = Client::start();

    // Act
    client.request::<GotoDefinition>(2, position_params(&uri_of(&a), 4, 13));
    let response: GotoDefinitionResponse =
        serde_json::from_value(result_of(&client, 2)).expect("a definition comes back");

    // Assert: the id token of `fiat leaf {` in b.inka.
    let GotoDefinitionResponse::Scalar(location) = response else {
        panic!("expected one definition location");
    };
    assert_eq!(location.uri, uri_of(&b));
    assert_eq!(
        location.range,
        Range::new(Position::new(0, 5), Position::new(0, 9))
    );

    client.shutdown();
}

#[test]
fn references_list_every_site_and_the_declaration_on_request() {
    // Arrange
    let (_temp, a, b) = two_file_project();
    let client = Client::start();

    // Act: references from the definition, declaration included.
    client.request::<References>(
        2,
        serde_json::json!({
            "textDocument": { "uri": uri_of(&b) },
            "position": { "line": 0, "character": 6 },
            "context": { "includeDeclaration": true },
        }),
    );
    let locations: Vec<Location> =
        serde_json::from_value(result_of(&client, 2)).expect("locations come back");

    // Assert: the children entry in a.inka and the declaration in b.inka.
    assert_eq!(locations.len(), 2);
    assert!(locations.iter().any(|location| location.uri == uri_of(&a)
        && location.range == Range::new(Position::new(4, 12), Position::new(4, 16))));
    assert!(locations.iter().any(|location| location.uri == uri_of(&b)
        && location.range == Range::new(Position::new(0, 5), Position::new(0, 9))));

    client.shutdown();
}

#[test]
fn prepare_rename_offers_the_identifier_and_refuses_prose() {
    // Arrange
    let (_temp, _a, b) = two_file_project();
    let uri = uri_of(&b);
    let client = Client::start();

    // Act & Assert: on the id, the range comes back.
    client.request::<PrepareRenameRequest>(2, position_params(&uri, 0, 6));
    let prepared: PrepareRenameResponse =
        serde_json::from_value(result_of(&client, 2)).expect("a range comes back");
    let PrepareRenameResponse::Range(range) = prepared else {
        panic!("expected a plain range");
    };
    assert_eq!(range, Range::new(Position::new(0, 5), Position::new(0, 9)));

    // On the `fiat` keyword, there is nothing to rename.
    client.request::<PrepareRenameRequest>(3, position_params(&uri, 0, 1));
    assert_eq!(result_of(&client, 3), serde_json::Value::Null);

    client.shutdown();
}

#[test]
fn rename_to_an_existing_id_is_refused_with_the_reason() {
    // Arrange
    let (_temp, _a, b) = two_file_project();
    let client = Client::start();

    // Act
    client.request::<Rename>(
        2,
        serde_json::json!({
            "textDocument": { "uri": uri_of(&b) },
            "position": { "line": 0, "character": 6 },
            "newName": "root",
        }),
    );
    let error = client
        .receive_response(2)
        .response_result
        .expect_err("the collision is refused");

    // Assert
    assert!(
        error.message.contains("already defined"),
        "{}",
        error.message
    );

    client.shutdown();
}

#[test]
fn rename_rewrites_sources_and_approvals_and_invalidates_nothing() {
    // Arrange: an argument fully approved by two approvers — the established
    // count is the thing the rename must not move.
    let (temp, a, b) = two_file_project();
    approve_all(temp.path(), "alice");
    approve_all(temp.path(), "bob");
    assert_eq!(established(temp.path()), 2, "both nodes start established");
    let client = Client::start();

    // Act: rename `leaf` to `sheet` from a *reference* site.
    client.request::<Rename>(
        2,
        serde_json::json!({
            "textDocument": { "uri": uri_of(&a) },
            "position": { "line": 4, "character": 13 },
            "newName": "sheet",
        }),
    );
    let edit: WorkspaceEdit =
        serde_json::from_value(result_of(&client, 2)).expect("a workspace edit comes back");
    client.shutdown();
    apply(&edit);

    // Assert: the definition, the reference, and both approvers' records all
    // moved to the new name...
    assert!(
        std::fs::read_to_string(&b)
            .expect("b.inka")
            .starts_with("fiat sheet {")
    );
    assert!(
        std::fs::read_to_string(&a)
            .expect("a.inka")
            .contains("children: sheet")
    );
    for approver in ["alice", "bob"] {
        let records =
            std::fs::read_to_string(temp.path().join(format!("approvals/{approver}.jsonl")))
                .expect("approvals");
        assert!(records.contains("\"target\":\"fiat:sheet\""), "{records}");
        assert!(!records.contains("leaf"), "{records}");
    }

    // ...and nothing needed re-approval: ids are filing, not contract.
    assert_eq!(
        established(temp.path()),
        2,
        "the rename invalidated nothing"
    );
}

#[test]
fn a_duplicated_id_refuses_rename_and_lists_every_definition() {
    // Arrange: `leaf` defined in b.inka and again in c.inka — broken, but it
    // still loads and navigation still answers.
    let (_temp, a, b) = two_file_project();
    let c = b.with_file_name("c.inka");
    std::fs::write(&c, "fiat leaf {\n  ```\n  twin\n  ```\n}\n").expect("c.inka");
    let client = Client::start();

    // Act & Assert: goto definition from the reference lists both twins.
    client.request::<GotoDefinition>(2, position_params(&uri_of(&a), 4, 13));
    let response: GotoDefinitionResponse =
        serde_json::from_value(result_of(&client, 2)).expect("definitions come back");
    let GotoDefinitionResponse::Array(locations) = response else {
        panic!("expected every definition, got {response:?}");
    };
    assert_eq!(locations.len(), 2);

    // Rename is refused: any edit would mix the two nodes' references.
    client.request::<Rename>(
        3,
        serde_json::json!({
            "textDocument": { "uri": uri_of(&b) },
            "position": { "line": 0, "character": 6 },
            "newName": "sheet",
        }),
    );
    let error = client
        .receive_response(3)
        .response_result
        .expect_err("ambiguous rename is refused");
    assert!(
        error.message.contains("defined more than once"),
        "{}",
        error.message
    );

    client.shutdown();
}

/// Approves every node as `approver`, with hashes computed the way eval
/// computes them.
fn approve_all(root: &Path, approver: &str) {
    let project = load_project(root).result.expect("project loads");
    let graph = Graph::build(&project.blocks).expect("graph builds");
    let view = ArgumentView::build(&graph, &project.source_map);
    for node in &view.nodes {
        let hash = LocalView::of_node(&view, node, None)
            .expect("nodes here carry no evidence")
            .view_hash();
        let fields = RecordFields {
            target: Target::new(node.mode, &node.id).expect("valid target"),
            view_hash: hash,
            approver: approver.to_owned(),
            key_id: approver.to_owned(),
            timestamp: "2026-11-02T00:00:00Z".parse().expect("valid timestamp"),
            phase: "P0".to_owned(),
            until: None,
            policy_hash: None,
        };
        let record = Record::approve(fields, None).expect("valid record");
        approvals::append(&root.join(APPROVALS_DIR), &record).expect("record appends");
    }
}

/// The established count of the project on disk, straight through core eval.
fn established(root: &Path) -> usize {
    let project = load_project(root).result.expect("project loads");
    let graph = Graph::build(&project.blocks).expect("graph builds");
    let view = ArgumentView::build(&graph, &project.source_map);
    let records = approvals::load_dir(&root.join(APPROVALS_DIR)).expect("records load");
    evaluate(
        &view,
        &project.manifest.phases,
        &records,
        &project.evidence,
        &project.calc_evidence,
    )
    .expect("eval runs")
    .established_count()
}

/// Applies a workspace edit to the files on disk, the way an editor would.
/// Test content is ASCII, so UTF-16 columns equal byte columns.
fn apply(edit: &WorkspaceEdit) {
    // `Uri` hashes by its string; the interior mutability clippy sees is a
    // parse cache inside fluent-uri, and `WorkspaceEdit` fixes this key type.
    #[allow(clippy::mutable_key_type)]
    let changes = edit.changes.as_ref().expect("edits use the changes form");
    for (uri, edits) in changes {
        let path = uri
            .as_str()
            .strip_prefix("file://")
            .expect("test uris are file uris");
        let mut text = std::fs::read_to_string(path).expect("edited file exists");
        let mut ordered: Vec<&TextEdit> = edits.iter().collect();
        ordered.sort_by_key(|edit| (edit.range.start.line, edit.range.start.character));
        for edit in ordered.into_iter().rev() {
            let start = byte_offset(&text, edit.range.start);
            let end = byte_offset(&text, edit.range.end);
            text.replace_range(start..end, &edit.new_text);
        }
        std::fs::write(path, text).expect("edited file writes back");
    }
}

fn byte_offset(text: &str, position: Position) -> usize {
    let line_start = text
        .split_inclusive('\n')
        .take(position.line as usize)
        .map(str::len)
        .sum::<usize>();
    line_start + position.character as usize
}
