//! End-to-end tests for `inka serve`: a real process, a real socket, real
//! HTTP.
//!
//! The unit tests in the command drive the router directly; what they cannot
//! show is that the binary binds, reads the right project, and answers a
//! browser. That is the closing condition of the server, so it is tested
//! against both fixture projects here.
//!
//! Requests are written by hand over TCP rather than through an HTTP client:
//! the server exists to answer plain `GET`s, and a dependency to prove it would
//! be a dependency shipped forever for one test.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use serde_json::Value;

mod common;
use common::{local_fixture, workspace_fixture};

/// A running `inka serve`, stopped when the test drops it.
struct Server {
    process: Child,
    address: String,
    /// What the server wrote to stderr before it reported its address.
    startup: String,
}

impl Server {
    /// Starts the server on an operating-system-chosen port and waits until it
    /// reports the address it bound.
    fn start(fixture: &str) -> Self {
        Self::start_in(&workspace_fixture(fixture))
    }

    /// As [`Server::start`], on any project directory.
    fn start_in(project: &Path) -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_inka"))
            .arg("serve")
            .arg(project)
            .args(["--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the inka binary should start");

        let stderr = process.stderr.take().expect("stderr should be piped");
        let mut lines = BufReader::new(stderr).lines();
        let mut startup = String::new();
        let address = loop {
            let line = lines
                .next()
                .expect("the server should report its address")
                .expect("stderr should be readable");
            if let Some(url) = line.strip_prefix("inka serve: http://") {
                break url.to_owned();
            }
            startup.push_str(&line);
            startup.push('\n');
        };

        Self {
            process,
            address,
            startup,
        }
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

/// The page a server answers with.
fn page(server: &Server) -> Value {
    let (status, body) = server.get("/api/page");
    assert!(status.contains("200 OK"), "{status}");
    serde_json::from_str(&body).expect("the page should be JSON")
}

fn node_ids(page: &Value) -> Vec<&str> {
    page["nodes"]
        .as_array()
        .expect("a page lists its nodes")
        .iter()
        .map(|node| node["id"].as_str().expect("a node has an id"))
        .collect()
}

#[test]
fn serves_the_aocs_argument_to_a_browser() {
    // Arrange
    let server = Server::start("aocs");

    // Act
    let page = page(&server);

    // Assert: the whole argument, evaluated, as the viewer expects to read it.
    assert_eq!(page["kind"], "argument");
    assert_eq!(node_ids(&page).len(), 17);
    assert!(node_ids(&page).contains(&"aocs_ok"));
    assert_eq!(page["total"], 17);
}

#[test]
fn serves_the_dogfood_argument_to_a_browser() {
    let server = Server::start("dogfood");

    let page = page(&server);

    assert_eq!(page["kind"], "argument");
    assert!(node_ids(&page).contains(&"inka_ok"));
    assert_eq!(page["roots"][0], "inka_ok");
}

#[test]
fn declares_the_page_as_json() {
    let server = Server::start("aocs");

    let response = server.request("GET", "/api/page");

    assert!(response.contains("content-type: application/json"));
}

#[test]
fn refuses_a_request_that_would_write() {
    // Read-only is a property of the surface, not a convention: nothing in the
    // server accepts a method that could change anything.
    let server = Server::start("aocs");

    let response = server.request("POST", "/api/page");

    assert!(response.contains("405 Method Not Allowed"), "{response}");
}

#[test]
fn serves_the_viewer_or_says_how_to_get_one() {
    // The bundle is embedded at build time when `web-dist/` exists (CI builds
    // the Rust crates without it), so either answer is the right one as long
    // as the server never leaves a visitor with a blank page.
    let server = Server::start("aocs");

    let (status, body) = server.get("/");

    if status.contains("200 OK") {
        assert!(body.to_ascii_lowercase().contains("<html"), "{body}");
    } else {
        assert!(status.contains("503 Service Unavailable"), "{status}");
        assert!(body.contains("npm run build"), "{body}");
    }
}

#[test]
fn a_project_that_fails_the_check_is_served_as_its_findings() {
    // Arrange & Act: a fixture `inka check` rejects. The server comes up and
    // says so on the page, because the page is where the fix will be seen.
    let server = Server::start_in(&local_fixture("no-objective"));
    let (status, body) = server.get("/api/page");

    // Assert
    assert!(status.contains("200 OK"), "{status}");
    let page: Value = serde_json::from_str(&body).expect("the page should be JSON");
    assert_eq!(page["kind"], "rejected");
    assert!(
        page["findings"][0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no objective block found")),
        "{page}"
    );
}

#[test]
fn a_project_that_fails_the_check_is_reported_to_the_operator_at_startup() {
    let server = Server::start_in(&local_fixture("no-objective"));

    assert!(
        server.startup.contains("no objective block found"),
        "{}",
        server.startup
    );
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
