//! The `inka serve` command: an interactive, read-only view of one argument.
//!
//! Confirming an argument is the reason this exists. A reviewer walks the DAG,
//! focuses a node or a namespace, reads the descriptions, and follows the
//! objective down to its evidence — none of which is reading a static picture,
//! and all of which happens in the browser. The server's whole job is to hand
//! the front-end bundle and one JSON document to it.
//!
//! The server keeps no state of its own. Every request for the page reads the
//! project from disk through the core's [`wire::page`] and answers with what
//! it finds then, so the browser and the command line never disagree about
//! what the repository says, and an edit shows on the next request without a
//! restart. A project that does not check is answered too — with its findings
//! — rather than refused: the page is where the reader is looking.
//!
//! Started plainly, the server is read-only: the router answers `GET` and
//! offers no route that writes anything — neither to the argument, nor to
//! approvals, nor to any other state.
//!
//! Started with an approver (`--as`), it also records decisions: one route
//! that appends one approval record through the core's [`wire::decide`], the
//! code path `inka review` writes through. That is the only thing it can
//! change, and it does so under conditions the router enforces rather than
//! trusts the page to respect:
//!
//! - the approver and the key id are fixed here, at startup; a request cannot
//!   name them;
//! - a recording server listens on a loopback address only;
//! - a request must be addressed to this server by a name it answers to (its
//!   `Host`), so a foreign name pointed at the loopback address is refused;
//! - a decision must come from a page this server served for this run: it
//!   carries the session token only such a page can read, and, where the
//!   browser says where the request comes from (`Origin`), that is this
//!   server;
//! - a decision names the view hash that was on screen and is recorded only
//!   if that is still the target's current hash.
//!
//! The core is synchronous and reads files, so each load runs on the
//! runtime's blocking pool rather than on the threads that accept
//! connections.
//!
//! The bundle itself is compiled into the binary (see `build.rs`), so an
//! installed `inka` serves the viewer with nothing else on disk. `--assets`
//! points at a directory instead, for working on the viewer.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use inka_core::wire::{self, DecisionRequest, DecisionResponse, Page, ReviewerSession, Session};
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

use crate::check::Outcome;
use crate::load::load_project_view;
use crate::review::now;

/// Path the viewer fetches the page from.
const PAGE_PATH: &str = "/api/page";

/// Path the viewer asks who the server records as.
const SESSION_PATH: &str = "/api/session";

/// Path a decision is posted to; routed only on a recording server.
const DECISIONS_PATH: &str = "/api/decisions";

/// The port a browser leaves out of `Host` and `Origin`.
const DEFAULT_HTTP_PORT: u16 = 80;

/// Header a decision carries the session token in.
const TOKEN_HEADER: &str = "x-inka-session";

/// The viewer bundle `build.rs` found at compile time, as `(path, bytes)`.
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// A viewer bundle held in memory: `(path, bytes)` pairs with `/`-separated
/// paths relative to the bundle root, `index.html` among them.
type Bundle = &'static [(&'static str, &'static [u8])];

/// Where the viewer bundle comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Assets {
    /// A directory on disk, named by `--assets`.
    Directory(PathBuf),
    /// The bundle compiled into this binary.
    Embedded(Bundle),
    /// No bundle at all: the page endpoint is still served.
    None,
}

/// Where to listen, and what to serve alongside the argument.
#[derive(Debug, Clone)]
pub struct Options {
    pub host: IpAddr,
    pub port: u16,
    /// Directory holding the built viewer bundle.
    pub assets: Option<PathBuf>,
    /// Who decisions are recorded as; `None` serves read-only.
    pub reviewer: Option<ReviewerOptions>,
}

/// The identity `--as` and `--key-id` give a recording server.
#[derive(Debug, Clone)]
pub struct ReviewerOptions {
    /// Who is reviewing; records append to `approvals/<approver>.jsonl`.
    pub approver: String,
    /// The key id records name; the approver name when not given.
    pub key_id: Option<String>,
}

/// What a recording server holds for the length of its run.
#[derive(Debug)]
struct Recording {
    approver: String,
    key_id: Option<String>,
    /// Proof a request comes from a page served for this run.
    token: String,
    /// The names this server answers to, as a `Host` header spells them.
    hosts: Vec<String>,
}

impl Recording {
    /// A recording session on the address the server bound, under a token no
    /// earlier run had.
    fn new(reviewer: &ReviewerOptions, bound: SocketAddr) -> io::Result<Self> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        let token = bytes.iter().fold(String::new(), |mut token, byte| {
            // Writing to a `String` cannot fail.
            let _ = write!(token, "{byte:02x}");
            token
        });
        Ok(Self {
            approver: reviewer.approver.clone(),
            key_id: reviewer.key_id.clone(),
            token,
            hosts: loopback_hosts(bound),
        })
    }

    fn session(&self) -> ReviewerSession {
        ReviewerSession {
            approver: self.approver.clone(),
            key_id: self.key_id.clone().unwrap_or_else(|| self.approver.clone()),
            token: self.token.clone(),
        }
    }
}

/// Every spelling of a loopback server bound to `bound` that a browser may
/// put in `Host`: the address it was bound to — any of `127.0.0.0/8` is
/// loopback, not only `127.0.0.1` — and the usual names for the local machine.
/// On the default HTTP port a browser leaves the port out, so those spellings
/// are answered to as well.
fn loopback_hosts(bound: SocketAddr) -> Vec<String> {
    let bound_name = match bound.ip() {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    let mut names = vec![
        bound_name,
        "127.0.0.1".to_owned(),
        "localhost".to_owned(),
        "[::1]".to_owned(),
    ];
    names.dedup();
    let port = bound.port();
    let mut hosts: Vec<String> = names.iter().map(|name| format!("{name}:{port}")).collect();
    if port == DEFAULT_HTTP_PORT {
        hosts.extend(names);
    }
    hosts
}

/// What every handler reads: the project, and the recording session if any.
#[derive(Debug, Clone)]
struct App {
    project_dir: PathBuf,
    recording: Option<Arc<Recording>>,
}

/// Runs `inka serve` against `project_dir` until the process is interrupted.
///
/// The project is loaded once before the socket is bound, only to tell the
/// operator what the page will say: findings go to `err` as `inka check`
/// prints them. The server comes up either way, because fixing the project
/// and reloading the page is how the findings go away.
///
/// # Errors
///
/// Propagates any I/O error from writing diagnostics, from binding the socket,
/// or from the server itself.
pub fn run(
    project_dir: &Path,
    options: &Options,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    if load_project_view(project_dir, err, color)?.is_none() {
        writeln!(
            err,
            "warning: the project does not check; the page shows these findings until it does"
        )?;
    }

    let assets = match resolve_assets(options.assets.as_deref(), embedded::WEB_ASSETS) {
        Ok(assets) => assets,
        Err(message) => {
            writeln!(err, "error: {message}")?;
            return Ok(Outcome::Failed);
        }
    };
    if assets == Assets::None {
        writeln!(
            err,
            "warning: this inka was built without the viewer bundle; serving {PAGE_PATH} only \
             (run `npm run build` in `web/` and rebuild inka, or pass --assets)"
        )?;
    }

    if options.reviewer.is_some() && !options.host.is_loopback() {
        writeln!(
            err,
            "error: a server that records decisions listens on a loopback address only; \
             drop --as to serve `{}` read-only",
            options.host
        )?;
        return Ok(Outcome::Failed);
    }

    let address = SocketAddr::new(options.host, options.port);
    let project_dir = project_dir.to_path_buf();
    let reviewer = options.reviewer.clone();

    // Multi-threaded, so a slow asset read does not stall the page endpoint.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = TcpListener::bind(address).await?;
        let bound = listener.local_addr()?;
        // The session needs the port that was actually bound, which `--port 0`
        // only settles here.
        let recording = match &reviewer {
            Some(reviewer) => Some(Arc::new(Recording::new(reviewer, bound)?)),
            None => None,
        };
        if let Some(recording) = &recording {
            writeln!(
                err,
                "inka serve: recording decisions as `{}` in approvals/{}.jsonl",
                recording.approver, recording.approver
            )?;
        }
        let app = router(
            App {
                project_dir,
                recording,
            },
            assets,
        );
        writeln!(err, "inka serve: http://{bound}")?;
        err.flush()?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
    })?;

    Ok(Outcome::Ok)
}

/// Builds the router: the API, then the viewer bundle behind it.
///
/// Without a recording session every route is a `get`, so any other method
/// is refused with 405 by the router itself rather than by a check someone
/// has to remember to write. With one, the single `post` is added and the
/// whole API sits behind [`guard`].
fn router(app: App, assets: Assets) -> Router {
    let api = Router::new()
        .route(PAGE_PATH, get(page))
        .route(SESSION_PATH, get(session));
    let api = match &app.recording {
        Some(recording) => api
            .route(DECISIONS_PATH, post(decide))
            .layer(middleware::from_fn_with_state(recording.clone(), guard)),
        None => api,
    };
    let argument = api.with_state(app);

    match assets {
        // Unknown paths fall back to the bundle's entry point, so the viewer
        // keeps working if it ever routes in the browser.
        Assets::Directory(directory) => argument.fallback_service(
            ServeDir::new(&directory).fallback(ServeFile::new(directory.join("index.html"))),
        ),
        Assets::Embedded(bundle) => {
            argument.fallback(move |uri: Uri| async move { embedded_file(bundle, &uri) })
        }
        Assets::None => argument.fallback(missing_bundle),
    }
}

/// Reads the project and answers with its page.
///
/// The response is never cached: it is a reading of files that may have
/// changed by the next request.
async fn page(State(app): State<App>) -> Response {
    match tokio::task::spawn_blocking(move || wire::page(&app.project_dir)).await {
        Ok(page) => page_response(&page),
        // The load panicked; the core does not, so this is a bug to surface
        // rather than a state to present.
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Says who the server records as, or that it does not record.
async fn session(State(app): State<App>) -> Response {
    let session = Session {
        reviewer: app.recording.as_deref().map(Recording::session),
    };
    ([(header::CACHE_CONTROL, "no-store")], Json(session)).into_response()
}

/// Records one decision. Routed only on a recording server, behind [`guard`].
async fn decide(State(app): State<App>, Json(request): Json<DecisionRequest>) -> Response {
    let Some(recording) = app.recording.clone() else {
        // The route exists only with a session; this cannot be reached.
        return StatusCode::NOT_FOUND.into_response();
    };
    let outcome = tokio::task::spawn_blocking(move || {
        wire::decide(
            &app.project_dir,
            &recording.approver,
            recording.key_id.as_deref(),
            request,
            now(),
        )
    })
    .await;
    let Ok(response) = outcome else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let status = match response {
        DecisionResponse::Recorded => StatusCode::OK,
        DecisionResponse::Changed => StatusCode::CONFLICT,
        DecisionResponse::Refused { .. } => StatusCode::UNPROCESSABLE_ENTITY,
        DecisionResponse::Failed { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(response)).into_response()
}

/// Lets a request through to a recording server's API only if it is
/// addressed to this server, and lets a request that is not a read through
/// only if it comes from one of this server's own pages.
///
/// The `Host` check is what defeats a foreign name resolved to the loopback
/// address: such a page is same-origin with itself and could otherwise read
/// the session, token included. The token and the `Origin` check are what
/// stop another site open in the same browser from posting a decision.
async fn guard(State(recording): State<Arc<Recording>>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !addressed_here(headers, &recording.hosts) {
        return refuse("this request is not addressed to this server");
    }
    if request.method() != Method::GET
        && !(from_own_page(headers, &recording.hosts) && carries(headers, &recording.token))
    {
        return refuse("this request does not come from a page this server served for this run");
    }
    next.run(request).await
}

fn refuse(reason: &'static str) -> Response {
    (StatusCode::FORBIDDEN, reason).into_response()
}

/// Whether the `Host` header is one of the names this server answers to.
fn addressed_here(headers: &HeaderMap, hosts: &[String]) -> bool {
    headers
        .get(header::HOST)
        .and_then(|host| host.to_str().ok())
        .is_some_and(|host| hosts.iter().any(|known| known.eq_ignore_ascii_case(host)))
}

/// Whether the browser, where it says which page sent the request, names one
/// of this server's. A request without `Origin` is left to the token.
fn from_own_page(headers: &HeaderMap, hosts: &[String]) -> bool {
    match headers.get(header::ORIGIN) {
        None => true,
        Some(origin) => origin.to_str().is_ok_and(|origin| {
            hosts
                .iter()
                .any(|host| origin.eq_ignore_ascii_case(&format!("http://{host}")))
        }),
    }
}

/// Whether the request carries the session token, compared without stopping
/// at the first difference.
fn carries(headers: &HeaderMap, token: &str) -> bool {
    let Some(sent) = headers.get(TOKEN_HEADER) else {
        return false;
    };
    let (sent, token) = (sent.as_bytes(), token.as_bytes());
    sent.len() == token.len()
        && sent
            .iter()
            .zip(token)
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

fn page_response(page: &Page) -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Json(page)).into_response()
}

/// Answers a path from the embedded bundle, with the entry point standing in
/// for `/` and for anything the bundle does not have, as the directory
/// variant does.
fn embedded_file(bundle: Bundle, uri: &Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let found = lookup(bundle, path).or_else(|| lookup(bundle, "index.html"));
    match found {
        Some((path, bytes)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type(path))],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The bundle entry at `path`, if any.
fn lookup(bundle: Bundle, path: &str) -> Option<(&'static str, &'static [u8])> {
    bundle.iter().copied().find(|(name, _)| *name == path)
}

/// The media type a file of the bundle is served as, from its extension.
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Answer for every non-argument path when no bundle was found.
///
/// Explaining the gap beats serving a blank page: the argument is being served
/// correctly, only the viewer that renders it is absent.
async fn missing_bundle() -> impl IntoResponse {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Html(format!(
            "<h1>inka serve</h1><p>This inka was built without the viewer bundle. \
             Build it with <code>npm run build</code> in <code>web/</code> and rebuild inka, \
             or start the server with <code>--assets &lt;DIR&gt;</code>.</p>\
             <p>The argument itself is being served at <code>{PAGE_PATH}</code>.</p>"
        )),
    )
}

/// Resolves where the bundle comes from.
///
/// An explicit `--assets` that is not a directory is an error: the operator
/// asked for something specific, and silently serving nothing would hide the
/// typo. Otherwise the embedded bundle is used, and its absence (a build
/// without `web-dist/`) is allowed, because the page endpoint is useful on
/// its own.
fn resolve_assets(explicit: Option<&Path>, embedded: Bundle) -> Result<Assets, String> {
    match explicit {
        Some(directory) if directory.is_dir() => Ok(Assets::Directory(directory.to_path_buf())),
        Some(directory) => Err(format!("no viewer bundle at `{}`", directory.display())),
        None if embedded.is_empty() => Ok(Assets::None),
        None => Ok(Assets::Embedded(embedded)),
    }
}

/// Resolves when the process is asked to stop, so the socket closes cleanly.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use tower::ServiceExt;

    /// A project of one approved-by-nobody objective over one fiat.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        std::fs::write(
            dir.path().join("inka.toml"),
            "[project]\nname = \"t\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n[values]\nfiles = []\n",
        )
        .expect("the manifest is written");
        std::fs::create_dir(dir.path().join("argument")).expect("the directory is created");
        write_argument(dir.path(), "the claim");
        dir
    }

    fn write_argument(dir: &Path, claim: &str) {
        std::fs::write(
            dir.join("argument/a.inka"),
            format!(
                "objective root {{\n    ```\n    {claim}\n    ```\n    children:\n        leaf\n}}\n\n\
                 fiat leaf {{\n    ```\n    given\n    ```\n}}\n"
            ),
        )
        .expect("the argument is written");
    }

    /// A router over a directory that holds no project; enough for the routes
    /// that never read one.
    fn bundle_router(assets: Assets) -> Router {
        read_only(Path::new("no/such/project"), assets)
    }

    /// A read-only server over `project_dir`.
    fn read_only(project_dir: &Path, assets: Assets) -> Router {
        router(
            App {
                project_dir: project_dir.to_path_buf(),
                recording: None,
            },
            assets,
        )
    }

    async fn page_of(app: Router) -> serde_json::Value {
        let response = call(app, Method::GET, PAGE_PATH).await;
        serde_json::from_str(&body_of(response).await).expect("the page is JSON")
    }

    /// A bundle standing in for a built viewer.
    const BUNDLE: Bundle = &[
        ("index.html", b"<title>inka</title>"),
        ("assets/app.js", b"console.log('inka')"),
    ];

    /// Sends one request through the router and returns the response.
    async fn call(app: Router, method: Method, path: &str) -> axum::response::Response {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("the test request should build");
        app.oneshot(request)
            .await
            .expect("the router should answer")
    }

    async fn body_of(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("the body should read");
        String::from_utf8(bytes.to_vec()).expect("the body should be UTF-8")
    }

    #[tokio::test]
    async fn serves_the_page_as_json_that_is_never_cached() {
        // Arrange
        let dir = project();
        let app = read_only(dir.path(), Assets::None);

        // Act
        let response = call(app, Method::GET, PAGE_PATH).await;

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("the endpoint should declare its type"),
            "application/json"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .expect("the endpoint should say how long the answer holds"),
            "no-store"
        );
        let page: serde_json::Value =
            serde_json::from_str(&body_of(response).await).expect("the page is JSON");
        assert_eq!(page["kind"], "argument");
        assert_eq!(page["roots"][0], "root");
    }

    #[tokio::test]
    async fn an_edit_shows_on_the_next_request_without_a_restart() {
        // Arrange: one router, kept across the edit.
        let dir = project();
        let app = read_only(dir.path(), Assets::None);
        let description = |page: &serde_json::Value| {
            page["nodes"]
                .as_array()
                .expect("a page lists its nodes")
                .iter()
                .find(|node| node["id"] == "root")
                .expect("the root is listed")["local"]["description"]
                .clone()
        };
        let before = page_of(app.clone()).await;

        // Act
        write_argument(dir.path(), "the claim, reworded");
        let after = page_of(app).await;

        // Assert
        assert_eq!(description(&before), "the claim");
        assert_eq!(description(&after), "the claim, reworded");
    }

    #[tokio::test]
    async fn a_recorded_approval_shows_on_the_next_request() {
        // Arrange
        let dir = project();
        let app = read_only(dir.path(), Assets::None);
        let before = page_of(app.clone()).await;
        let hash = before["nodes"][0]["view_hash"]
            .as_str()
            .expect("the leaf can be approved");

        // Act: a record appended by anything else, here by hand.
        std::fs::create_dir(dir.path().join("approvals")).expect("the store is created");
        std::fs::write(
            dir.path().join("approvals/iwata.jsonl"),
            format!(
                "{{\"v\":1,\"target\":\"fiat:leaf\",\"view_hash\":\"{hash}\",\"label\":\"approve\",\
                 \"comment\":null,\"approver\":\"iwata\",\"key_id\":\"iwata\",\
                 \"timestamp\":\"2026-09-10T08:30:00Z\",\"phase\":\"M1\",\"until\":null,\
                 \"policy_hash\":null,\"sig\":\"\"}}\n"
            ),
        )
        .expect("the record is written");
        let after = page_of(app).await;

        // Assert
        assert_eq!(before["established"], 0);
        assert_eq!(after["established"], 1);
    }

    #[tokio::test]
    async fn a_project_that_fails_the_check_is_answered_with_its_findings() {
        // Arrange
        let dir = project();
        std::fs::write(dir.path().join("argument/a.inka"), "fiat leaf {\n")
            .expect("the argument is broken");
        let app = read_only(dir.path(), Assets::None);

        // Act
        let broken = page_of(app.clone()).await;
        write_argument(dir.path(), "the claim");
        let fixed = page_of(app).await;

        // Assert: findings as data, and the same server presents the fix.
        assert_eq!(broken["kind"], "rejected");
        assert_eq!(broken["findings"][0]["severity"], "error");
        assert_eq!(broken["findings"][0]["location"]["line"], 1);
        assert_eq!(fixed["kind"], "argument");
    }

    #[tokio::test]
    async fn refuses_every_method_that_would_write() {
        for method in [Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
            let app = bundle_router(Assets::None);

            let response = call(app, method.clone(), PAGE_PATH).await;

            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} should be refused"
            );
        }
    }

    const TOKEN: &str = "the-session-token";
    const HOST: &str = "127.0.0.1:8080";

    /// The usual loopback address, on `port`.
    fn local(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    /// A server recording as `iwata` over `project_dir`, answering to [`HOST`].
    fn recording(project_dir: &Path) -> Router {
        router(
            App {
                project_dir: project_dir.to_path_buf(),
                recording: Some(Arc::new(Recording {
                    approver: "iwata".to_owned(),
                    key_id: None,
                    token: TOKEN.to_owned(),
                    hosts: loopback_hosts(local(8080)),
                })),
            },
            Assets::None,
        )
    }

    /// The hash the page shows for `id`.
    async fn hash_of(app: Router, id: &str) -> String {
        let response = send(app, Method::GET, PAGE_PATH, &[("host", HOST)], None).await;
        let page: serde_json::Value =
            serde_json::from_str(&body_of(response).await).expect("the page is JSON");
        page["nodes"]
            .as_array()
            .expect("a page lists its nodes")
            .iter()
            .find(|node| node["id"] == id)
            .expect("the node is listed")["view_hash"]
            .as_str()
            .expect("the node can be decided on")
            .to_owned()
    }

    fn approval(target: &str, view_hash: &str) -> String {
        format!(
            r#"{{"target":"{target}","view_hash":"{view_hash}","decision":{{"label":"approve","comment":null}}}}"#
        )
    }

    /// Sends one request with headers and an optional JSON body.
    async fn send(
        app: Router,
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> axum::response::Response {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if body.is_some() {
            request = request.header(header::CONTENT_TYPE, "application/json");
        }
        let request = request
            .body(Body::from(body.unwrap_or_default().to_owned()))
            .expect("the test request should build");
        app.oneshot(request)
            .await
            .expect("the router should answer")
    }

    /// The headers a decision from one of this server's own pages carries.
    const OWN_PAGE: [(&str, &str); 3] = [
        ("host", HOST),
        ("origin", "http://127.0.0.1:8080"),
        (TOKEN_HEADER, TOKEN),
    ];

    fn records(dir: &Path) -> Vec<serde_json::Value> {
        std::fs::read_to_string(dir.join("approvals/iwata.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a record is JSON"))
            .collect()
    }

    #[tokio::test]
    async fn a_read_only_server_says_it_records_as_nobody_and_routes_no_write() {
        // Arrange
        let dir = project();
        let app = read_only(dir.path(), Assets::None);

        // Act
        let session = call(app.clone(), Method::GET, SESSION_PATH).await;
        let attempt = send(
            app,
            Method::POST,
            DECISIONS_PATH,
            &OWN_PAGE,
            Some(&approval("leaf", "sha256:00")),
        )
        .await;

        // Assert: no session, and the path is simply not there to post to.
        let session: serde_json::Value =
            serde_json::from_str(&body_of(session).await).expect("the session is JSON");
        assert_eq!(session["reviewer"], serde_json::Value::Null);
        assert_ne!(attempt.status(), StatusCode::OK);
        assert_eq!(records(dir.path()), Vec::<serde_json::Value>::new());
    }

    #[tokio::test]
    async fn a_recording_server_tells_its_own_page_who_it_records_as() {
        // Arrange
        let dir = project();

        // Act
        let response = send(
            recording(dir.path()),
            Method::GET,
            SESSION_PATH,
            &[("host", HOST)],
            None,
        )
        .await;

        // Assert
        let session: serde_json::Value =
            serde_json::from_str(&body_of(response).await).expect("the session is JSON");
        assert_eq!(session["reviewer"]["approver"], "iwata");
        assert_eq!(session["reviewer"]["key_id"], "iwata");
        assert_eq!(session["reviewer"]["token"], TOKEN);
    }

    #[tokio::test]
    async fn a_decision_from_the_server_s_own_page_appends_the_record_review_writes() {
        // Arrange
        let dir = project();
        let app = recording(dir.path());
        let hash = hash_of(app.clone(), "leaf").await;

        // Act
        let response = send(
            app.clone(),
            Method::POST,
            DECISIONS_PATH,
            &OWN_PAGE,
            Some(&approval("leaf", &hash)),
        )
        .await;

        // Assert: one record, in the form and the file `inka review` uses.
        assert_eq!(response.status(), StatusCode::OK);
        let written = records(dir.path());
        assert_eq!(written.len(), 1);
        let record = &written[0];
        assert_eq!(record["v"], 1);
        assert_eq!(record["target"], "fiat:leaf");
        assert_eq!(record["view_hash"], hash.as_str());
        assert_eq!(record["label"], "approve");
        assert_eq!(record["approver"], "iwata");
        assert_eq!(record["key_id"], "iwata");
        assert_eq!(record["phase"], "M1");
        // And the next page reflects it.
        let page = send(app, Method::GET, PAGE_PATH, &[("host", HOST)], None).await;
        let page: serde_json::Value =
            serde_json::from_str(&body_of(page).await).expect("the page is JSON");
        assert_eq!(page["established"], 1);
    }

    #[tokio::test]
    async fn a_decision_cannot_name_who_it_is_recorded_as() {
        // Arrange: a request that tries to bring its own approver.
        let dir = project();
        let app = recording(dir.path());
        let hash = hash_of(app.clone(), "leaf").await;
        let body = format!(
            r#"{{"target":"leaf","view_hash":"{hash}","approver":"mallory","key_id":"mallory","decision":{{"label":"approve","comment":null}}}}"#
        );

        // Act
        send(app, Method::POST, DECISIONS_PATH, &OWN_PAGE, Some(&body)).await;

        // Assert: whatever was recorded, it was recorded as the server's reviewer.
        assert!(!dir.path().join("approvals/mallory.jsonl").exists());
        for record in records(dir.path()) {
            assert_eq!(record["approver"], "iwata");
        }
    }

    #[tokio::test]
    async fn a_decision_on_content_edited_since_it_was_shown_is_refused_and_writes_nothing() {
        // Arrange: the page showed the root, then the root was reworded.
        let dir = project();
        let app = recording(dir.path());
        let shown = hash_of(app.clone(), "root").await;
        write_argument(dir.path(), "the claim, reworded");

        // Act
        let response = send(
            app,
            Method::POST,
            DECISIONS_PATH,
            &OWN_PAGE,
            Some(&approval("root", &shown)),
        )
        .await;

        // Assert
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(body_of(response).await.contains("\"changed\""));
        assert_eq!(records(dir.path()), Vec::<serde_json::Value>::new());
    }

    #[tokio::test]
    async fn a_decline_without_a_reason_is_refused() {
        // Arrange
        let dir = project();
        let app = recording(dir.path());
        let hash = hash_of(app.clone(), "leaf").await;
        let body = format!(
            r#"{{"target":"leaf","view_hash":"{hash}","decision":{{"label":"decline","comment":" "}}}}"#
        );

        // Act
        let response = send(app, Method::POST, DECISIONS_PATH, &OWN_PAGE, Some(&body)).await;

        // Assert
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(records(dir.path()), Vec::<serde_json::Value>::new());
    }

    #[tokio::test]
    async fn a_decision_that_does_not_come_from_the_server_s_own_page_is_refused() {
        // Arrange: each request is one header away from a valid one.
        let dir = project();
        let app = recording(dir.path());
        let hash = hash_of(app.clone(), "leaf").await;
        let body = approval("leaf", &hash);
        let attempts: [(&str, Vec<(&str, &str)>); 5] = [
            (
                "no token",
                vec![("host", HOST), ("origin", "http://127.0.0.1:8080")],
            ),
            (
                "a wrong token",
                vec![("host", HOST), (TOKEN_HEADER, "the-session-tokeX")],
            ),
            (
                "another site's page",
                vec![
                    ("host", HOST),
                    ("origin", "https://evil.example"),
                    (TOKEN_HEADER, TOKEN),
                ],
            ),
            (
                "this server on another port",
                vec![
                    ("host", HOST),
                    ("origin", "http://127.0.0.1:9999"),
                    (TOKEN_HEADER, TOKEN),
                ],
            ),
            (
                "a foreign name pointed at loopback",
                vec![
                    ("host", "evil.example:8080"),
                    ("origin", "http://evil.example:8080"),
                    (TOKEN_HEADER, TOKEN),
                ],
            ),
        ];

        for (what, headers) in attempts {
            // Act
            let response = send(
                app.clone(),
                Method::POST,
                DECISIONS_PATH,
                &headers,
                Some(&body),
            )
            .await;

            // Assert
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{what}");
        }
        assert_eq!(records(dir.path()), Vec::<serde_json::Value>::new());
    }

    #[tokio::test]
    async fn a_recording_server_answers_reads_only_under_its_own_name() {
        // A page under a foreign name resolved to loopback is same-origin
        // with itself; it must not be able to read the session's token.
        let dir = project();
        let app = recording(dir.path());

        for path in [SESSION_PATH, PAGE_PATH] {
            let foreign = send(
                app.clone(),
                Method::GET,
                path,
                &[("host", "evil.example:8080")],
                None,
            )
            .await;
            let own = send(
                app.clone(),
                Method::GET,
                path,
                &[("host", "localhost:8080")],
                None,
            )
            .await;

            assert_eq!(foreign.status(), StatusCode::FORBIDDEN, "{path}");
            assert_eq!(own.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn a_recording_server_offers_no_retraction_edit_or_deletion() {
        let dir = project();
        let app = recording(dir.path());

        for method in [Method::PUT, Method::DELETE, Method::PATCH] {
            for path in [DECISIONS_PATH, PAGE_PATH, SESSION_PATH] {
                let response = send(app.clone(), method.clone(), path, &OWN_PAGE, None).await;

                assert_eq!(
                    response.status(),
                    StatusCode::METHOD_NOT_ALLOWED,
                    "{method} {path}"
                );
            }
        }
    }

    /// A recording router that was bound to `bound`.
    fn recording_on(project_dir: &Path, bound: SocketAddr) -> Router {
        router(
            App {
                project_dir: project_dir.to_path_buf(),
                recording: Some(Arc::new(Recording {
                    approver: "iwata".to_owned(),
                    key_id: None,
                    token: TOKEN.to_owned(),
                    hosts: loopback_hosts(bound),
                })),
            },
            Assets::None,
        )
    }

    /// Whether a decision with these `Host` and `Origin` headers gets past
    /// the guard; a stale hash is refused later, with 409, not 403.
    async fn passes_guard(app: Router, host: &str, origin: &str) -> bool {
        let headers = [("host", host), ("origin", origin), (TOKEN_HEADER, TOKEN)];
        let response = send(
            app,
            Method::POST,
            DECISIONS_PATH,
            &headers,
            Some(&approval("leaf", "sha256:00")),
        )
        .await;
        response.status() != StatusCode::FORBIDDEN
    }

    #[tokio::test]
    async fn a_server_bound_to_another_loopback_address_answers_to_that_address() {
        // Any of 127.0.0.0/8 is loopback, and the operator is told to open
        // the address that was bound.
        let dir = project();
        let app = recording_on(dir.path(), SocketAddr::from(([127, 0, 0, 2], 8080)));

        assert!(passes_guard(app.clone(), "127.0.0.2:8080", "http://127.0.0.2:8080").await);
        assert!(passes_guard(app.clone(), "localhost:8080", "http://localhost:8080").await);
        assert!(!passes_guard(app.clone(), "127.0.0.3:8080", "http://127.0.0.3:8080").await);
        assert!(!passes_guard(app, "evil.example:8080", "http://evil.example:8080").await);
    }

    #[tokio::test]
    async fn a_server_on_the_default_port_answers_to_names_without_a_port() {
        // A browser drops `:80` from both headers.
        let dir = project();
        let app = recording_on(dir.path(), local(80));

        assert!(passes_guard(app.clone(), "127.0.0.1", "http://127.0.0.1").await);
        assert!(passes_guard(app.clone(), "localhost:80", "http://localhost").await);
        assert!(!passes_guard(app.clone(), "127.0.0.1:8080", "http://127.0.0.1:8080").await);
        assert!(!passes_guard(app, "evil.example", "http://evil.example").await);
    }

    #[tokio::test]
    async fn a_server_on_another_port_does_not_answer_to_names_without_one() {
        let dir = project();
        let app = recording_on(dir.path(), local(8080));

        assert!(!passes_guard(app, "127.0.0.1", "http://127.0.0.1").await);
    }

    #[test]
    fn each_run_records_under_a_token_of_its_own() {
        let reviewer = ReviewerOptions {
            approver: "iwata".to_owned(),
            key_id: Some("key-1".to_owned()),
        };

        let first = Recording::new(&reviewer, local(8080)).expect("a session starts");
        let second = Recording::new(&reviewer, local(8080)).expect("a session starts");

        assert_eq!(first.token.len(), 64);
        assert_ne!(first.token, second.token);
        assert_eq!(first.session().key_id, "key-1");
    }

    #[tokio::test]
    async fn explains_itself_when_the_viewer_bundle_is_missing() {
        let app = bundle_router(Assets::None);

        let response = call(app, Method::GET, "/").await;

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(body_of(response).await.contains("npm run build"));
    }

    #[tokio::test]
    async fn serves_the_viewer_bundle_when_there_is_one() {
        // Arrange: a directory standing in for a built bundle.
        let bundle = std::env::temp_dir().join("inka-serve-test-bundle");
        std::fs::create_dir_all(&bundle).expect("the test bundle should be creatable");
        std::fs::write(bundle.join("index.html"), "<title>inka</title>")
            .expect("the test bundle should be writable");
        let app = bundle_router(Assets::Directory(bundle.clone()));

        // Act
        let response = call(app, Method::GET, "/index.html").await;

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_of(response).await.contains("inka"));
        std::fs::remove_dir_all(&bundle).expect("the test bundle should be removable");
    }

    #[tokio::test]
    async fn serves_the_embedded_bundle_with_its_entry_point_at_the_root() {
        let app = bundle_router(Assets::Embedded(BUNDLE));

        let response = call(app, Method::GET, "/").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("the page should declare its type"),
            "text/html; charset=utf-8"
        );
        assert_eq!(body_of(response).await, "<title>inka</title>");
    }

    #[tokio::test]
    async fn serves_an_embedded_asset_with_its_media_type() {
        let app = bundle_router(Assets::Embedded(BUNDLE));

        let response = call(app, Method::GET, "/assets/app.js").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("the asset should declare its type"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(body_of(response).await, "console.log('inka')");
    }

    #[tokio::test]
    async fn falls_back_to_the_embedded_entry_point_for_unknown_paths() {
        // The same shape as the directory variant: browser-side routes land
        // on the viewer rather than on a 404.
        let app = bundle_router(Assets::Embedded(BUNDLE));

        let response = call(app, Method::GET, "/some/route").await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body_of(response).await, "<title>inka</title>");
    }

    #[test]
    fn refuses_an_assets_directory_that_is_not_there() {
        let missing = Path::new("definitely/not/a/bundle");

        let resolved = resolve_assets(Some(missing), BUNDLE);

        assert!(resolved.is_err_and(|message| message.contains("definitely/not/a/bundle")));
    }

    #[test]
    fn prefers_an_explicit_directory_over_the_embedded_bundle() {
        let directory = std::env::temp_dir();

        let resolved = resolve_assets(Some(&directory), BUNDLE);

        assert_eq!(resolved, Ok(Assets::Directory(directory)));
    }

    #[test]
    fn uses_the_embedded_bundle_when_nothing_is_asked_for() {
        assert_eq!(resolve_assets(None, BUNDLE), Ok(Assets::Embedded(BUNDLE)));
    }

    #[test]
    fn accepts_a_build_without_a_bundle() {
        // Nothing was asked for, and the bundle may simply not have been built.
        assert_eq!(resolve_assets(None, &[]), Ok(Assets::None));
    }

    #[test]
    fn names_the_media_type_of_every_kind_of_file_the_bundle_holds() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(
            content_type("assets/index-abc123.js"),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type("assets/index-abc123.css"),
            "text/css; charset=utf-8"
        );
        assert_eq!(content_type("favicon.svg"), "image/svg+xml");
        assert_eq!(content_type("unknown.bin"), "application/octet-stream");
        assert_eq!(content_type("README"), "application/octet-stream");
    }
}
