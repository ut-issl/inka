//! End-to-end tests for `inka serve`: a real process, a real socket, real
//! HTTP.
//!
//! The unit tests in the command drive the router directly; what they cannot
//! show is that the binary binds, projects the right argument, and answers a
//! browser. That is the closing condition of the server, so it is tested
//! against both fixture projects here.
//!
//! Requests are written by hand over TCP rather than through an HTTP client:
//! the server exists to answer plain `GET`s, and a dependency to prove it would
//! be a dependency shipped forever for one test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

mod common;
use common::{local_fixture, workspace_fixture};

/// A running `inka serve`, stopped when the test drops it.
struct Server {
    process: Child,
    address: String,
}

impl Server {
    /// Starts the server on an operating-system-chosen port and waits until it
    /// reports the address it bound.
    fn start(fixture: &str) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_inka"))
            .arg("serve")
            .arg(workspace_fixture(fixture))
            .args(["--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the inka binary should start");

        let stderr = process.stderr.take().expect("stderr should be piped");
        let mut lines = BufReader::new(stderr).lines();
        let address = loop {
            let line = lines
                .next()
                .expect("the server should report its address")
                .expect("stderr should be readable");
            if let Some(url) = line.strip_prefix("inka serve: http://") {
                break url.to_owned();
            }
        };

        Self { process, address }
    }

    /// Sends one request and returns the raw response.
    fn request(&self, method: &str, path: &str) -> String {
        let mut stream = TcpStream::connect(&self.address).expect("the server should accept");
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            self.address
        )
        .expect("the request should send");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("the response should read");
        response
    }

    /// The status line and the body of a response, split apart.
    fn get(&self, path: &str) -> (String, String) {
        let response = self.request("GET", path);
        let (head, body) = response
            .split_once("\r\n\r\n")
            .expect("a response has a header section");
        let status = head.lines().next().unwrap_or_default().to_owned();
        (status, body.to_owned())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// Node elements in a served argument.
fn node_count(body: &str) -> usize {
    let elements: Vec<Value> = serde_json::from_str(body).expect("the body should be JSON");
    elements
        .iter()
        .filter(|element| element["data"].get("source").is_none())
        .count()
}

#[test]
fn serves_the_aocs_argument_to_a_browser() {
    // Arrange
    let server = Server::start("aocs");

    // Act
    let (status, body) = server.get("/elements.json");

    // Assert: the whole argument, as the viewer expects to read it.
    assert!(status.contains("200 OK"), "{status}");
    assert_eq!(node_count(&body), 17);
    assert!(body.contains("aocs_ok"));
}

#[test]
fn serves_the_dogfood_argument_to_a_browser() {
    let server = Server::start("dogfood");

    let (status, body) = server.get("/elements.json");

    assert!(status.contains("200 OK"), "{status}");
    assert!(node_count(&body) > 0);
    assert!(body.contains("inka_ok"));
}

#[test]
fn declares_the_argument_as_json() {
    let server = Server::start("aocs");

    let response = server.request("GET", "/elements.json");

    assert!(response.contains("content-type: application/json"));
}

#[test]
fn refuses_a_request_that_would_write() {
    // Read-only is a property of the surface, not a convention: nothing in the
    // server accepts a method that could change anything.
    let server = Server::start("aocs");

    let response = server.request("POST", "/elements.json");

    assert!(response.contains("405 Method Not Allowed"), "{response}");
}

#[test]
fn says_what_to_do_when_the_viewer_bundle_is_not_built() {
    // The tests run from the crate directory, where no bundle exists.
    let server = Server::start("aocs");

    let (status, body) = server.get("/");

    assert!(status.contains("503 Service Unavailable"), "{status}");
    assert!(body.contains("npm run build"));
}

#[test]
fn a_project_that_fails_the_check_is_never_served() {
    // Arrange & Act: a fixture `inka check` rejects. The server must refuse to
    // come up rather than answer for an argument it cannot stand behind.
    let output = Command::new(env!("CARGO_BIN_EXE_inka"))
        .current_dir(local_fixture("no-objective"))
        .args(["serve", ".", "--port", "0"])
        .output()
        .expect("the inka binary should run");

    // Assert
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no objective block found"), "{stderr}");
    assert!(!stderr.contains("inka serve: http://"), "{stderr}");
}

#[test]
fn an_assets_directory_that_is_not_there_is_reported() {
    let output = Command::new(env!("CARGO_BIN_EXE_inka"))
        .arg("serve")
        .arg(workspace_fixture("aocs"))
        .args(["--port", "0", "--assets", "definitely/not/a/bundle"])
        .output()
        .expect("the inka binary should run");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("definitely/not/a/bundle"),
        "the operator should learn which path was wrong"
    );
}
