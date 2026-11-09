//! Protocol-level tests for `textDocument/formatting`: the editor gets the
//! same text `inka fmt` writes, as one edit over the open buffer, and nothing
//! at all whenever `inka fmt` would format nothing.

use std::collections::HashMap;

use lsp_types::request::Formatting;
use lsp_types::{Position, Range, TextEdit, Uri};

use inka_core::format::format_source;
use inka_core::source::SourceMap;

mod common;
use common::{Client, MANIFEST, scenario_project, uri_of};

const UNFORMATTED: &str = "fiat leaf {\n  ```\n  accepted\n  ```\n}\n";
const FORMATTED: &str = "fiat leaf {\n    ```\n    accepted\n    ```\n}\n";

fn format_request(client: &Client, id: i32, uri: &Uri) -> serde_json::Value {
    client.request::<Formatting>(
        id,
        serde_json::json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 4, "insertSpaces": true },
        }),
    );
    client
        .receive_response(id)
        .response_result
        .expect("the request succeeds")
}

fn edits(result: serde_json::Value) -> Vec<TextEdit> {
    serde_json::from_value(result).expect("a TextEdit list")
}

#[test]
fn an_open_buffer_is_formatted_as_one_edit_over_the_whole_text() {
    // Arrange: the buffer differs from the disk, and only the buffer counts.
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, UNFORMATTED);

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    assert_eq!(
        edits(result),
        [TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(5, 0)),
            new_text: FORMATTED.to_owned(),
        }]
    );
    client.shutdown();
}

#[test]
fn the_edit_is_what_inka_fmt_writes() {
    // Arrange: comments, children, and a property order to normalize.
    let text = concat!(
        "objective root { // the goal\n",
        "until: P0\n",
        "```\n",
        "ok\n",
        "```\n",
        "children: a, // first\n",
        "  b }\n",
        "fiat a {\n```\na\n```\n}\n",
        "fiat b {\n```\nb\n```\n}\n",
    );
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, text);
    let mut map = SourceMap::new();
    let file_id = map.add_file(argument.clone(), text.to_owned());
    let expected =
        format_source(file_id, text, &HashMap::<String, String>::new()).expect("the text parses");

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    let edits = edits(result);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, expected);
    client.shutdown();
}

#[test]
fn a_formatted_document_needs_no_edit() {
    // Arrange
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, FORMATTED);

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    assert_eq!(edits(result), []);
    client.shutdown();
}

#[test]
fn a_document_that_does_not_parse_is_left_untouched() {
    // Arrange
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, "fiat leaf {\n  ```\n  x\n  ```\n  reason: gone\n}\n");

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    assert_eq!(result, serde_json::Value::Null);
    client.shutdown();
}

#[test]
fn a_project_whose_values_cannot_load_is_not_formatted() {
    // Arrange: the manifest names a values file that does not exist.
    let (temp, argument) = scenario_project();
    std::fs::write(
        temp.path().join("inka.toml"),
        MANIFEST.replace("files = []", "files = [\"missing.toml\"]"),
    )
    .expect("manifest");
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, UNFORMATTED);

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    assert_eq!(result, serde_json::Value::Null);
    client.shutdown();
}

#[test]
fn a_document_outside_any_project_is_not_formatted() {
    // Arrange
    let temp = tempfile::tempdir().expect("temp dir");
    let loose = temp.path().join("loose.inka");
    std::fs::write(&loose, UNFORMATTED).expect("loose file");
    let uri = uri_of(&loose);
    let client = Client::start();
    client.open(&uri, UNFORMATTED);

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert
    assert_eq!(result, serde_json::Value::Null);
    client.shutdown();
}

#[test]
fn the_edit_range_ends_at_the_buffer_end_in_utf16_units() {
    // Arrange: no final newline, and a last line whose characters take more
    // bytes than UTF-16 units, so a byte-based end would overshoot.
    let text = "fiat leaf {\n  ```\n  x\n  ```\n} // 日本語";
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, text);

    // Act
    let result = format_request(&client, 2, &uri);

    // Assert: "} // 日本語" is 8 UTF-16 units long.
    let edits = edits(result);
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].range.end, Position::new(4, 8));
    assert_eq!(
        edits[0].new_text,
        "// 日本語\nfiat leaf {\n    ```\n    x\n    ```\n}\n"
    );
    client.shutdown();
}
