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
//! Read-only means read-only. The router answers `GET` and offers no route
//! that writes anything — neither to the argument, nor to approvals, nor to
//! any other state.
//!
//! The core is synchronous and reads files, so each load runs on the
//! runtime's blocking pool rather than on the threads that accept
//! connections.
//!
//! The bundle itself is compiled into the binary (see `build.rs`), so an
//! installed `inka` serves the viewer with nothing else on disk. `--assets`
//! points at a directory instead, for working on the viewer.

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use inka_core::wire::{self, Page};
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

use crate::check::Outcome;
use crate::load::load_project_view;

/// Path the viewer fetches the page from.
const PAGE_PATH: &str = "/api/page";

/// Path the server used to answer with `inka export` output.
const RETIRED_ELEMENTS_PATH: &str = "/elements.json";

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

    let app = router(project_dir.to_path_buf(), assets);
    let address = SocketAddr::new(options.host, options.port);

    // Multi-threaded, so a slow asset read does not stall the page endpoint.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = TcpListener::bind(address).await?;
        writeln!(err, "inka serve: http://{}", listener.local_addr()?)?;
        err.flush()?;
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
    })?;

    Ok(Outcome::Ok)
}

/// Builds the router: the page, then the viewer bundle behind it.
///
/// Every route is a `get`, so any other method is refused with 405 by the
/// router itself rather than by a check someone has to remember to write.
fn router(project_dir: PathBuf, assets: Assets) -> Router {
    let argument = Router::new()
        .route(
            PAGE_PATH,
            get(move || {
                let project_dir = project_dir.clone();
                async move { page(project_dir).await }
            }),
        )
        .route(RETIRED_ELEMENTS_PATH, get(retired_elements));

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
async fn page(project_dir: PathBuf) -> Response {
    match tokio::task::spawn_blocking(move || wire::page(&project_dir)).await {
        Ok(page) => page_response(&page),
        // The load panicked; the core does not, so this is a bug to surface
        // rather than a state to present.
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Says where the export went. Without this the path would fall through to
/// the viewer's entry point, and a tool that still asks for JSON here would
/// be handed a page of HTML with a success status.
async fn retired_elements() -> impl IntoResponse {
    (
        StatusCode::GONE,
        "inka serve no longer answers /elements.json; run `inka export` for the cytoscape elements\n",
    )
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
        router(PathBuf::from("no/such/project"), assets)
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
        let app = router(dir.path().to_path_buf(), Assets::None);

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
        let app = router(dir.path().to_path_buf(), Assets::None);
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
        let app = router(dir.path().to_path_buf(), Assets::None);
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
        let app = router(dir.path().to_path_buf(), Assets::None);

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

    #[tokio::test]
    async fn the_retired_export_path_says_where_the_export_went() {
        // With a bundle, an unknown path answers the viewer's entry point; a
        // tool still asking here for JSON must not be handed that.
        let app = bundle_router(Assets::Embedded(BUNDLE));

        let response = call(app, Method::GET, "/elements.json").await;

        assert_eq!(response.status(), StatusCode::GONE);
        assert!(body_of(response).await.contains("inka export"));
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
