//! The `inka serve` command: an interactive, read-only view of one argument.
//!
//! Confirming an argument is the reason this exists. A reviewer walks the DAG,
//! focuses a node or a namespace, reads the descriptions, and follows the
//! objective down to its evidence — none of which is reading a static picture,
//! and all of which happens in the browser. The server's whole job is to hand
//! the front-end bundle and one JSON document to it.
//!
//! Read-only means read-only. The router answers `GET`, serves an argument
//! projected once at startup, and offers no route that writes anything —
//! neither to the argument, nor to approvals, nor to any other state.
//!
//! The argument is delivered through `GET /elements.json` rather than injected
//! into the served HTML. Both are possible and the viewer accepts either; the
//! endpoint wins here because the bundle then stays byte-for-byte what was
//! built, and the served page has nothing project-specific in it.
//!
//! The bundle itself is compiled into the binary (see `build.rs`), so an
//! installed `inka` serves the viewer with nothing else on disk. `--assets`
//! points at a directory instead, for working on the viewer.

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

use crate::check::Outcome;
use crate::export::to_json;
use crate::load::load_view;

/// Path the viewer fetches the argument from.
const ELEMENTS_PATH: &str = "/elements.json";

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
    /// No bundle at all: the argument endpoint is still served.
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
/// The argument is projected once, before the socket is bound: a project that
/// does not check is reported and nothing is served, rather than a server
/// coming up with an argument it cannot answer for. Re-reading the project on
/// change is a convenience that belongs on top of this, not inside it.
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
    let Some(view) = load_view(project_dir, err, color)? else {
        return Ok(Outcome::Failed);
    };
    let elements = to_json(&view).map_err(io::Error::other)?;

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
            "warning: this inka was built without the viewer bundle; serving {ELEMENTS_PATH} only \
             (run `npm run build` in `web/` and rebuild inka, or pass --assets)"
        )?;
    }

    let app = router(elements, assets);
    let address = SocketAddr::new(options.host, options.port);

    // A current-thread runtime would do, but the multi-thread one keeps a slow
    // file read from stalling the JSON endpoint on a wide graph.
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

/// Builds the router: the argument, then the viewer bundle behind it.
///
/// Every route is a `get`, so any other method is refused with 405 by the
/// router itself rather than by a check someone has to remember to write.
fn router(elements: String, assets: Assets) -> Router {
    let argument = Router::new().route(
        ELEMENTS_PATH,
        get(|| async move {
            (
                [(header::CONTENT_TYPE, "application/json")],
                // The projection is immutable and already serialized, so every
                // request answers from the same string.
                elements,
            )
        }),
    );

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
             <p>The argument itself is being served at <code>{ELEMENTS_PATH}</code>.</p>"
        )),
    )
}

/// Resolves where the bundle comes from.
///
/// An explicit `--assets` that is not a directory is an error: the operator
/// asked for something specific, and silently serving nothing would hide the
/// typo. Otherwise the embedded bundle is used, and its absence (a build
/// without `web-dist/`) is allowed, because the argument endpoint is useful on
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

    const ELEMENTS: &str = "[{\"data\":{\"id\":\"root\"}}]\n";

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
    async fn serves_the_argument_as_json() {
        // Arrange
        let app = router(ELEMENTS.to_owned(), Assets::None);

        // Act
        let response = call(app, Method::GET, ELEMENTS_PATH).await;

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("the endpoint should declare its type"),
            "application/json"
        );
        assert_eq!(body_of(response).await, ELEMENTS);
    }

    #[tokio::test]
    async fn answers_the_same_bytes_to_every_request() {
        // The projection happens once, at startup; requests only read it.
        let app = router(ELEMENTS.to_owned(), Assets::None);

        let first = body_of(call(app.clone(), Method::GET, ELEMENTS_PATH).await).await;
        let second = body_of(call(app, Method::GET, ELEMENTS_PATH).await).await;

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn refuses_every_method_that_would_write() {
        for method in [Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
            let app = router(ELEMENTS.to_owned(), Assets::None);

            let response = call(app, method.clone(), ELEMENTS_PATH).await;

            assert_eq!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} should be refused"
            );
        }
    }

    #[tokio::test]
    async fn explains_itself_when_the_viewer_bundle_is_missing() {
        let app = router(ELEMENTS.to_owned(), Assets::None);

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
        let app = router(ELEMENTS.to_owned(), Assets::Directory(bundle.clone()));

        // Act
        let response = call(app, Method::GET, "/index.html").await;

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_of(response).await.contains("inka"));
        std::fs::remove_dir_all(&bundle).expect("the test bundle should be removable");
    }

    #[tokio::test]
    async fn serves_the_embedded_bundle_with_its_entry_point_at_the_root() {
        let app = router(ELEMENTS.to_owned(), Assets::Embedded(BUNDLE));

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
        let app = router(ELEMENTS.to_owned(), Assets::Embedded(BUNDLE));

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
        let app = router(ELEMENTS.to_owned(), Assets::Embedded(BUNDLE));

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
