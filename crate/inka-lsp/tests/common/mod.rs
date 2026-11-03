//! Shared helpers for the protocol-level test binaries: a client over
//! in-memory channels with the server on its own thread, plus the scenario
//! project the conversations run against.
//!
//! Each test binary compiles this module independently, so a helper unused
//! by one binary is expected.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Exit, Initialized,
    Notification as _, PublishDiagnostics,
};
use lsp_types::request::{Initialize, Shutdown};
use lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentIdentifier,
    TextDocumentItem, Uri, VersionedTextDocumentIdentifier,
};

pub const MANIFEST: &str = concat!(
    "[project]\nname = \"t\"\n\n",
    "[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n",
    "[values]\nfiles = []\n",
);

pub const CLEAN_ARGUMENT: &str = concat!(
    "objective root {\n  ```\n  ok\n  ```\n  children: leaf\n}\n",
    "\n",
    "fiat leaf {\n  ```\n  ok\n  ```\n}\n",
);

pub const BROKEN_ARGUMENT: &str = concat!(
    "objective root {\n  ```\n  ok\n  ```\n  children: ghost\n}\n",
    "\n",
    "fiat leaf {\n  ```\n  ok\n  ```\n}\n",
);

/// A test client: the editor half of an in-memory connection, with the server
/// running on its own thread.
pub struct Client {
    connection: Connection,
    server: Option<JoinHandle<()>>,
}

impl Client {
    pub fn start() -> Self {
        Self::start_with(serde_json::json!({}))
    }

    /// Starts a client that declares `capabilities` in its `initialize`.
    #[allow(clippy::needless_pass_by_value)] // call sites hand over a `json!` literal
    pub fn start_with(capabilities: serde_json::Value) -> Self {
        let (server_side, client_side) = Connection::memory();
        let server = std::thread::spawn(move || {
            inka_lsp::run(&server_side).expect("server runs to a clean end");
        });
        let client = Client {
            connection: client_side,
            server: Some(server),
        };
        client.request::<Initialize>(1, serde_json::json!({ "capabilities": capabilities }));
        let response = client.receive();
        assert!(
            matches!(response, Message::Response(ref r) if r.response_result.is_ok()),
            "initialize should succeed, got {response:?}"
        );
        client.notify::<Initialized>(serde_json::json!({}));
        client
    }

    pub fn request<R: lsp_types::request::Request>(&self, id: i32, params: serde_json::Value) {
        self.connection
            .sender
            .send(Message::Request(Request::new(
                RequestId::from(id),
                R::METHOD.to_owned(),
                params,
            )))
            .expect("request reaches the server");
    }

    pub fn notify<N: lsp_types::notification::Notification>(&self, params: serde_json::Value) {
        self.connection
            .sender
            .send(Message::Notification(Notification::new(
                N::METHOD.to_owned(),
                params,
            )))
            .expect("notification reaches the server");
    }

    pub fn receive(&self) -> Message {
        self.connection
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the server answers within the timeout")
    }

    /// The next publishDiagnostics notification.
    pub fn receive_diagnostics(&self) -> PublishDiagnosticsParams {
        let message = self.receive();
        let Message::Notification(notification) = message else {
            panic!("expected a notification, got {message:?}");
        };
        assert_eq!(notification.method, PublishDiagnostics::METHOD);
        serde_json::from_value(notification.params).expect("valid publishDiagnostics params")
    }

    /// The response to request `id`, skipping any notifications on the way.
    pub fn receive_response(&self, id: i32) -> Response {
        loop {
            match self.receive() {
                Message::Response(response) => {
                    assert_eq!(response.id, RequestId::from(id));
                    return response;
                }
                Message::Notification(_) => {}
                Message::Request(request) => {
                    panic!("the server sent an unexpected request: {request:?}")
                }
            }
        }
    }

    pub fn open(&self, uri: &Uri, text: &str) {
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

    pub fn change(&self, uri: &Uri, version: i32, text: &str) {
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

    pub fn close(&self, uri: &Uri) {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        };
        self.notify::<DidCloseTextDocument>(serde_json::to_value(params).expect("valid params"));
    }

    pub fn shutdown(mut self) {
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

pub fn uri_of(path: &Path) -> Uri {
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
pub fn scenario_project() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
    std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
    let argument = temp.path().join("argument/a.inka");
    std::fs::write(&argument, CLEAN_ARGUMENT).expect("argument file");
    (temp, argument)
}
