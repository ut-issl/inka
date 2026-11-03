//! Protocol-level coverage of the decoration surfaces: semantic tokens,
//! inlay hints, and code lenses, plus the code-lens refresh nudge.

mod common;

use std::path::{Path, PathBuf};

use lsp_server::Message;
use lsp_types::notification::DidChangeWatchedFiles;
use lsp_types::request::{CodeLensRequest, InlayHintRequest, SemanticTokensFullRequest};
use lsp_types::{CodeLens, InlayHint, InlayHintLabel, Position, Range, SemanticTokens, Uri};

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::graph::Graph;
use inka_core::hash::LocalView;
use inka_core::project::load_project;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::ArgumentView;

use common::{Client, MANIFEST, uri_of};

/// An argument whose description carries `CommonMark` worth tokenizing.
const MARKED_ARGUMENT: &str = concat!(
    "objective root {\n",
    "  ```\n",
    "  # Goal\n",
    "\n",
    "  holds **firmly** with `margin`\n",
    "  ```\n",
    "  children: leaf\n",
    "}\n",
);

const FIAT_ARGUMENT: &str = "fiat leaf {\n  ```\n  taken as given\n  ```\n}\n";

/// A two-file project: `a.inka` holds the marked objective, `b.inka` the leaf.
fn marked_project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
    std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
    let a = temp.path().join("argument/a.inka");
    let b = temp.path().join("argument/b.inka");
    std::fs::write(&a, MARKED_ARGUMENT).expect("a.inka");
    std::fs::write(&b, FIAT_ARGUMENT).expect("b.inka");
    (temp, a, b)
}

/// Undoes the wire's delta encoding into absolute
/// `(line, start, length, type, modifiers)` tuples.
fn decode(tokens: &SemanticTokens) -> Vec<(u32, u32, u32, u32, u32)> {
    let mut absolute = Vec::new();
    let (mut line, mut start) = (0, 0);
    for token in &tokens.data {
        line += token.delta_line;
        if token.delta_line == 0 {
            start += token.delta_start;
        } else {
            start = token.delta_start;
        }
        absolute.push((
            line,
            start,
            token.length,
            token.token_type,
            token.token_modifiers_bitset,
        ));
    }
    absolute
}

fn document(uri: &Uri) -> serde_json::Value {
    serde_json::json!({ "textDocument": { "uri": uri } })
}

#[test]
fn semantic_tokens_color_ids_and_description_markup() {
    // Arrange
    let (_temp, a, _b) = marked_project();
    let client = Client::start();
    let uri = uri_of(&a);
    client.open(&uri, MARKED_ARGUMENT);
    client.receive_diagnostics();

    // Act
    client.request::<SemanticTokensFullRequest>(2, document(&uri));
    let response = client.receive_response(2);

    // Assert: legend order is type=0, heading=1, bold=2, italic=3, code=4;
    // the declaration modifier (bit 0) marks the definition only.
    let tokens: SemanticTokens =
        serde_json::from_value(response.response_result.expect("tokens are answered"))
            .expect("valid semantic tokens");
    assert_eq!(
        decode(&tokens),
        [
            (0, 10, 4, 0, 1), // `root`, a type declaration
            (2, 2, 6, 1, 0),  // `# Goal`, a heading
            (4, 8, 10, 2, 0), // `**firmly**`, bold
            (4, 24, 8, 4, 0), // "`margin`", inline code
            (6, 12, 4, 0, 0), // `leaf`, a type reference
        ]
    );

    client.shutdown();
}

#[test]
fn inlay_hints_carry_the_referenced_first_contract_line() {
    // Arrange
    let (_temp, a, _b) = marked_project();
    let client = Client::start();
    let uri = uri_of(&a);

    // Act: the visible range covers the whole file.
    client.request::<InlayHintRequest>(
        2,
        serde_json::json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 7, "character": 1 },
            },
        }),
    );
    let response = client.receive_response(2);

    // Assert: one hint, right after the `leaf` reference, padded off the id.
    let hints: Vec<InlayHint> =
        serde_json::from_value(response.response_result.expect("hints are answered"))
            .expect("valid inlay hints");
    assert_eq!(hints.len(), 1, "got {hints:?}");
    assert_eq!(hints[0].position, Position::new(6, 16));
    let InlayHintLabel::String(label) = &hints[0].label else {
        panic!("expected a plain string label, got {:?}", hints[0].label);
    };
    assert_eq!(label, "taken as given");
    assert_eq!(hints[0].padding_left, Some(true));

    client.shutdown();
}

#[test]
fn code_lenses_follow_the_approval_state() {
    // Arrange
    let (temp, a, b) = marked_project();
    let client = Client::start();
    let uri = uri_of(&a);

    // Act & Assert: unapproved, the objective is blamed for itself.
    let lenses = request_lenses(&client, 2, &uri);
    assert_eq!(lenses.len(), 1);
    assert_eq!(
        lenses[0].range,
        Range::new(Position::new(0, 10), Position::new(0, 14))
    );
    assert_eq!(
        lens_title(&lenses[0]),
        "✗ no approval covers the current content"
    );

    // Both nodes approved, both files read established.
    approve_all(temp.path(), "alice");
    let approved = request_lenses(&client, 3, &uri);
    assert_eq!(lens_title(&approved[0]), "✓ established");
    let leaf = request_lenses(&client, 4, &uri_of(&b));
    assert_eq!(lens_title(&leaf[0]), "✓ established");

    client.shutdown();
}

#[test]
fn an_unreadable_approvals_file_is_one_loud_lens() {
    // Arrange: a record line no loader accepts.
    let (temp, a, _b) = marked_project();
    std::fs::create_dir_all(temp.path().join(APPROVALS_DIR)).expect("approvals dir");
    std::fs::write(temp.path().join("approvals/broken.jsonl"), "not a record\n")
        .expect("broken records");
    let client = Client::start();

    // Act
    let lenses = request_lenses(&client, 2, &uri_of(&a));

    // Assert: the file-level notice replaces every per-block verdict.
    assert_eq!(lenses.len(), 1);
    assert_eq!(lenses[0].range, Range::default());
    let title = lens_title(&lenses[0]);
    assert!(title.starts_with("approvals unreadable:"), "got: {title}");

    client.shutdown();
}

#[test]
fn a_watched_change_nudges_a_refresh_capable_client() {
    // Arrange: the client declares workspace/codeLens/refresh support.
    let (temp, _a, _b) = marked_project();
    let client = Client::start_with(serde_json::json!({
        "workspace": { "codeLens": { "refreshSupport": true } }
    }));

    // Act: an approval record appears on disk.
    approve_all(temp.path(), "alice");
    client.notify::<DidChangeWatchedFiles>(serde_json::json!({
        "changes": [{ "uri": uri_of(&temp.path().join("approvals/alice.jsonl")), "type": 1 }]
    }));

    // Assert: the server asks this client to re-request its lenses.
    let request = loop {
        match client.receive() {
            Message::Request(request) => break request,
            Message::Notification(_) => {}
            Message::Response(response) => panic!("unexpected response: {response:?}"),
        }
    };
    assert_eq!(request.method, "workspace/codeLens/refresh");

    client.shutdown();
}

fn request_lenses(client: &Client, id: i32, uri: &Uri) -> Vec<CodeLens> {
    client.request::<CodeLensRequest>(id, document(uri));
    let response = client.receive_response(id);
    serde_json::from_value(response.response_result.expect("lenses are answered"))
        .expect("valid code lenses")
}

fn lens_title(lens: &CodeLens) -> String {
    let command = lens.command.as_ref().expect("a lens carries its title");
    assert!(command.command.is_empty(), "lenses are read-only");
    command.title.clone()
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
            timestamp: "2026-11-04T00:00:00Z".parse().expect("valid timestamp"),
            phase: "P0".to_owned(),
            until: None,
            policy_hash: None,
        };
        let record = Record::approve(fields, None).expect("valid record");
        approvals::append(&root.join(APPROVALS_DIR), &record).expect("record appends");
    }
}
