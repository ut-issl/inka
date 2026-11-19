//! Protocol-level tests for `textDocument/completion`: the editor gets block
//! ids, phases, and value keys from the open text even while it does not
//! parse, and nothing inside free text.

use lsp_types::request::Completion;
use lsp_types::{CompletionItem, CompletionTextEdit, Position, Range, Uri};

mod common;
use common::{Client, scenario_project, uri_of};

const MANIFEST: &str = concat!(
    "[project]\nname = \"t\"\n\n",
    "[phases]\norder = [\"PDR\", \"CDR\"]\ncurrent = \"PDR\"\n\n",
    "[values]\nfiles = [\"values.toml\"]\n",
);

/// A sibling file declaring a block that is still missing its `children:`,
/// so it does not parse — it is a child to offer all the same.
const SIBLING: &str = "structure half {\n    ```\n    Half written\n    ```\n}\n";

fn complete(client: &Client, id: i32, uri: &Uri, at: Position) -> Vec<CompletionItem> {
    client.request::<Completion>(
        id,
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
        }),
    );
    let result = client
        .receive_response(id)
        .response_result
        .expect("the request succeeds");
    serde_json::from_value(result).expect("a completion list")
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    let mut labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
    labels.sort_unstable();
    labels
}

/// A project with the manifest above, one value, and the sibling file; the
/// returned uri is the scenario's own argument file.
fn project() -> (tempfile::TempDir, Uri) {
    let (temp, argument) = scenario_project();
    std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
    std::fs::write(temp.path().join("values.toml"), "mass = \"12 kg\"\n").expect("values");
    std::fs::write(temp.path().join("argument/b.inka"), SIBLING).expect("sibling");
    (temp, uri_of(&argument))
}

#[test]
fn children_offer_the_other_blocks_while_the_list_is_unfinished() {
    // Arrange: a trailing comma — the text does not parse.
    let (_temp, uri) = project();
    let client = Client::start();
    let text = concat!(
        "objective root {\n    ```\n    ok\n    ```\n    children:\n        leaf,\n        \n}\n",
        "\n",
        "fiat leaf {\n    ```\n    ok\n    ```\n}\n",
    );
    client.open(&uri, text);

    // Act
    let items = complete(&client, 2, &uri, Position::new(6, 8));

    // Assert: `half` from the sibling file; not `leaf` (listed) or `root`.
    assert_eq!(labels(&items), ["half"]);
    let half = &items[0];
    assert_eq!(half.detail.as_deref(), Some("structure"));
    client.shutdown();
}

#[test]
fn a_typed_prefix_is_what_the_completion_replaces() {
    // Arrange
    let (_temp, uri) = project();
    let client = Client::start();
    let text = concat!(
        "objective root {\n    ```\n    ok\n    ```\n    children: ha\n}\n",
        "\n",
        "fiat leaf {\n    ```\n    ok\n    ```\n}\n",
    );
    client.open(&uri, text);

    // Act
    let items = complete(&client, 2, &uri, Position::new(4, 16));

    // Assert: the editor filters by the prefix; the edit spans it.
    let half = items
        .iter()
        .find(|item| item.label == "half")
        .expect("`half` is offered");
    assert_eq!(
        half.text_edit,
        Some(CompletionTextEdit::Edit(lsp_types::TextEdit {
            range: Range::new(Position::new(4, 14), Position::new(4, 16)),
            new_text: "half".to_owned(),
        }))
    );
    client.shutdown();
}

#[test]
fn until_offers_the_phases_in_order() {
    // Arrange
    let (_temp, uri) = project();
    let client = Client::start();
    client.open(
        &uri,
        "fiat leaf {\n    ```\n    ok\n    ```\n    until: \n}\n",
    );

    // Act
    let items = complete(&client, 2, &uri, Position::new(4, 11));

    // Assert
    let phases: Vec<(&str, Option<&str>)> = items
        .iter()
        .map(|item| (item.label.as_str(), item.detail.as_deref()))
        .collect();
    assert_eq!(
        phases,
        [("PDR", Some("phase (current)")), ("CDR", Some("phase"))]
    );
    client.shutdown();
}

#[test]
fn a_block_body_offers_the_properties_its_mode_still_lacks() {
    // Arrange
    let (_temp, uri) = project();
    let client = Client::start();
    client.open(
        &uri,
        "bind b {\n    ```\n    ok\n    ```\n    until: PDR\n    \n}\n",
    );

    // Act
    let items = complete(&client, 2, &uri, Position::new(5, 4));

    // Assert: until: is there; test:/veriq: remain, inserted with a colon.
    assert_eq!(labels(&items), ["test", "veriq"]);
    let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
        panic!("expected a text edit, got {:?}", items[0].text_edit);
    };
    assert_eq!(edit.new_text, "test: ");
    client.shutdown();
}

#[test]
fn a_description_offers_value_keys_after_the_dollar_only() {
    // Arrange
    let (_temp, uri) = project();
    let client = Client::start();
    client.open(
        &uri,
        "fiat leaf {\n    ```\n    Mass {$ and text\n    ```\n}\n",
    );

    // Act
    let keys = complete(&client, 2, &uri, Position::new(2, 11));
    let prose = complete(&client, 3, &uri, Position::new(2, 20));

    // Assert
    assert_eq!(labels(&keys), ["mass"]);
    assert_eq!(keys[0].detail.as_deref(), Some("12 kg"));
    let Some(CompletionTextEdit::Edit(edit)) = &keys[0].text_edit else {
        panic!("expected a text edit, got {:?}", keys[0].text_edit);
    };
    assert_eq!(edit.new_text, "mass}");
    assert!(prose.is_empty(), "plain text takes nothing, got {prose:?}");
    client.shutdown();
}

#[test]
fn between_blocks_the_mode_keywords_are_offered() {
    // Arrange
    let (_temp, uri) = project();
    let client = Client::start();
    client.open(&uri, "fiat leaf {\n    ```\n    ok\n    ```\n}\n\nst");

    // Act
    let items = complete(&client, 2, &uri, Position::new(6, 2));

    // Assert
    assert_eq!(labels(&items), ["bind", "fiat", "objective", "structure"]);
    client.shutdown();
}
