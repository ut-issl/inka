//! Protocol-level tests: a real client conversation over in-memory channels.
//!
//! Each test drives the server exactly as an editor would — initialize,
//! document notifications, shutdown — and asserts on the
//! `textDocument/publishDiagnostics` notifications that come back. The server
//! runs on its own thread, the test plays the client.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::notification::{
    DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument, Exit,
    Initialized, Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Initialize, Shutdown};
use lsp_types::{
    DiagnosticSeverity, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, FileChangeType, FileEvent,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, Uri, VersionedTextDocumentIdentifier,
};

const MANIFEST: &str = concat!(
    "[project]\nname = \"t\"\n\n",
    "[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n",
    "[values]\nfiles = []\n",
);

const CLEAN_ARGUMENT: &str = concat!(
    "objective root {\n  ```\n  ok\n  ```\n  children: leaf\n}\n",
    "\n",
    "fiat leaf {\n  ```\n  ok\n  ```\n}\n",
);

const BROKEN_ARGUMENT: &str = concat!(
    "objective root {\n  ```\n  ok\n  ```\n  children: ghost\n}\n",
    "\n",
    "fiat leaf {\n  ```\n  ok\n  ```\n}\n",
);

/// A test client: the editor half of an in-memory connection, with the server
/// running on its own thread.
struct Client {
    connection: Connection,
    server: Option<JoinHandle<()>>,
}

impl Client {
    fn start() -> Self {
        let (server_side, client_side) = Connection::memory();
        let server = std::thread::spawn(move || {
            inka_lsp::run(&server_side).expect("server runs to a clean end");
        });
        let client = Client {
            connection: client_side,
            server: Some(server),
        };
        client.request::<Initialize>(1, serde_json::json!({ "capabilities": {} }));
        let response = client.receive();
        assert!(
            matches!(response, Message::Response(ref r) if r.response_result.is_ok()),
            "initialize should succeed, got {response:?}"
        );
        client.notify::<Initialized>(serde_json::json!({}));
        client
    }

    fn request<R: lsp_types::request::Request>(&self, id: i32, params: serde_json::Value) {
        self.connection
            .sender
            .send(Message::Request(Request::new(
                RequestId::from(id),
                R::METHOD.to_owned(),
                params,
            )))
            .expect("request reaches the server");
    }

    fn notify<N: lsp_types::notification::Notification>(&self, params: serde_json::Value) {
        self.connection
            .sender
            .send(Message::Notification(Notification::new(
                N::METHOD.to_owned(),
                params,
            )))
            .expect("notification reaches the server");
    }

    fn receive(&self) -> Message {
        self.connection
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the server answers within the timeout")
    }

    /// The next publishDiagnostics notification.
    fn receive_diagnostics(&self) -> PublishDiagnosticsParams {
        let message = self.receive();
        let Message::Notification(notification) = message else {
            panic!("expected a notification, got {message:?}");
        };
        assert_eq!(notification.method, PublishDiagnostics::METHOD);
        serde_json::from_value(notification.params).expect("valid publishDiagnostics params")
    }

    fn open(&self, uri: &Uri, text: &str) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "inka".to_owned(),
                version: 1,
                text: text.to_owned(),
            },
        };
        self.notify::<DidOpenTextDocument>(serde_json::to_value(params).expect("valid params"));
    }

    fn change(&self, uri: &Uri, version: i32, text: &str) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.to_owned(),
            }],
        };
        self.notify::<DidChangeTextDocument>(serde_json::to_value(params).expect("valid params"));
    }

    fn close(&self, uri: &Uri) {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        };
        self.notify::<DidCloseTextDocument>(serde_json::to_value(params).expect("valid params"));
    }

    fn shutdown(mut self) {
        self.request::<Shutdown>(999, serde_json::Value::Null);
        let response = self.receive();
        assert!(
            matches!(response, Message::Response(ref r) if r.response_result.is_ok()),
            "shutdown should succeed, got {response:?}"
        );
        self.notify::<Exit>(serde_json::Value::Null);
        self.server
            .take()
            .expect("server thread is still owned")
            .join()
            .expect("server thread ends cleanly");
    }
}

fn uri_of(path: &Path) -> Uri {
    let text: String = path
        .to_str()
        .expect("test paths are UTF-8")
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
                String::from(char::from(byte))
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    Uri::from_str(&format!("file://{text}")).expect("valid file uri")
}

/// A project directory the tests edit, with one argument file.
fn scenario_project() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
    std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
    let argument = temp.path().join("argument/a.inka");
    std::fs::write(&argument, CLEAN_ARGUMENT).expect("argument file");
    (temp, argument)
}

#[test]
fn opening_a_clean_document_publishes_an_empty_diagnostic_set() {
    // Arrange
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();

    // Act
    client.open(&uri, CLEAN_ARGUMENT);
    let published = client.receive_diagnostics();

    // Assert
    assert_eq!(published.uri, uri);
    assert!(
        published.diagnostics.is_empty(),
        "clean project should publish empty diagnostics, got {:?}",
        published.diagnostics
    );

    client.shutdown();
}

#[test]
fn an_unsaved_edit_is_diagnosed_and_a_fix_clears_it() {
    // Arrange: the disk stays clean the whole time; only the buffer breaks.
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, CLEAN_ARGUMENT);
    assert_eq!(
        client.receive_diagnostics().diagnostics,
        [] as [lsp_types::Diagnostic; 0]
    );

    // Act: break the buffer without saving.
    client.change(&uri, 2, BROKEN_ARGUMENT);
    let broken = client.receive_diagnostics();

    // Assert: the undefined reference is reported against the buffer...
    assert_eq!(broken.uri, uri);
    let ghost = broken
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("undefined reference `ghost`"))
        .expect("undefined-reference diagnostic");
    assert_eq!(ghost.severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(ghost.range.start.line, 4);
    assert_eq!(
        std::fs::read_to_string(&argument).expect("disk file"),
        CLEAN_ARGUMENT,
        "the disk never changed; the diagnostic came from the overlay"
    );

    // ...and fixing the buffer clears it.
    client.change(&uri, 3, CLEAN_ARGUMENT);
    let fixed = client.receive_diagnostics();
    assert_eq!(fixed.uri, uri);
    assert!(
        fixed.diagnostics.is_empty(),
        "expected cleared diagnostics, got {:?}",
        fixed.diagnostics
    );

    client.shutdown();
}

#[test]
fn closing_a_dirty_document_falls_back_to_the_saved_text() {
    // Arrange: clean disk, broken buffer.
    let (_temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, BROKEN_ARGUMENT);
    assert_ne!(
        client.receive_diagnostics().diagnostics,
        [] as [lsp_types::Diagnostic; 0]
    );

    // Act: discard the buffer.
    client.close(&uri);
    let published = client.receive_diagnostics();

    // Assert: the disk text is clean, so the findings disappear.
    assert_eq!(published.uri, uri);
    assert!(
        published.diagnostics.is_empty(),
        "expected diagnostics from the saved text, got {:?}",
        published.diagnostics
    );

    client.shutdown();
}

#[test]
fn a_broken_manifest_is_reported_on_the_manifest_file() {
    // Arrange
    let (temp, argument) = scenario_project();
    std::fs::write(temp.path().join("inka.toml"), "not toml [").expect("break manifest");
    let uri = uri_of(&argument);
    let manifest_uri = uri_of(&temp.path().join("inka.toml"));
    let client = Client::start();

    // Act
    client.open(&uri, CLEAN_ARGUMENT);

    // Assert: two publishes arrive (order unspecified) — findings on the
    // manifest, an empty set for the open argument file.
    let first = client.receive_diagnostics();
    let second = client.receive_diagnostics();
    let (on_manifest, on_argument) = if first.uri == manifest_uri {
        (first, second)
    } else {
        (second, first)
    };
    assert_eq!(on_manifest.uri, manifest_uri);
    assert_eq!(on_manifest.diagnostics.len(), 1);
    assert_eq!(
        on_manifest.diagnostics[0].severity,
        Some(DiagnosticSeverity::ERROR)
    );
    assert_eq!(on_argument.uri, uri);
    assert_eq!(on_argument.diagnostics, [] as [lsp_types::Diagnostic; 0]);

    client.shutdown();
}

#[test]
fn an_unsupported_request_is_refused_not_ignored() {
    // Arrange
    let client = Client::start();

    // Act
    client.request::<lsp_types::request::HoverRequest>(
        7,
        serde_json::json!({
            "textDocument": { "uri": "file:///nowhere/a.inka" },
            "position": { "line": 0, "character": 0 },
        }),
    );
    let message = client.receive();

    // Assert
    let Message::Response(response) = message else {
        panic!("expected a response, got {message:?}");
    };
    assert_eq!(response.id, RequestId::from(7));
    let error = response
        .response_result
        .expect_err("hover is not implemented yet");
    assert!(error.message.contains("textDocument/hover"));

    client.shutdown();
}

#[test]
fn deleting_the_manifest_clears_the_projects_diagnostics() {
    // Arrange: a broken buffer, so the project carries findings.
    let (temp, argument) = scenario_project();
    let uri = uri_of(&argument);
    let client = Client::start();
    client.open(&uri, BROKEN_ARGUMENT);
    assert_ne!(
        client.receive_diagnostics().diagnostics,
        [] as [lsp_types::Diagnostic; 0]
    );

    // Act: the project stops being one — its manifest disappears.
    let manifest = temp.path().join("inka.toml");
    std::fs::remove_file(&manifest).expect("manifest is removable");
    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: uri_of(&manifest),
            typ: FileChangeType::DELETED,
        }],
    };
    client.notify::<DidChangeWatchedFiles>(serde_json::to_value(params).expect("valid params"));
    let published = client.receive_diagnostics();

    // Assert: the gone project's findings are cleared, not re-anchored to a
    // file that no longer exists.
    assert_eq!(published.uri, uri);
    assert!(
        published.diagnostics.is_empty(),
        "expected cleared diagnostics, got {:?}",
        published.diagnostics
    );

    client.shutdown();
}

#[test]
fn the_dogfood_project_is_clean_under_the_server() {
    // Arrange: the checked-in dogfood project, opened as an editor would.
    let dogfood = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/dogfood");
    let argument = dogfood
        .join("argument/objective.inka")
        .canonicalize()
        .expect("dogfood fixture exists");
    let text = std::fs::read_to_string(&argument).expect("objective.inka is readable");
    let uri = uri_of(&argument);
    let client = Client::start();

    // Act
    client.open(&uri, &text);
    let published = client.receive_diagnostics();

    // Assert: what `inka check` accepts, the server reports clean.
    assert_eq!(published.uri, uri);
    assert!(
        published.diagnostics.is_empty(),
        "dogfood should be clean, got {:?}",
        published.diagnostics
    );

    client.shutdown();
}
