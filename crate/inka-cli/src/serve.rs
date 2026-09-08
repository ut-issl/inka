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

use std::io::{self, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use tokio::net::TcpListener;
use tower_http::services::{ServeDir, ServeFile};

use crate::check::Outcome;
use crate::export::to_json;
use crate::load::load_view;

/// Path the viewer fetches the argument from.
const ELEMENTS_PATH: &str = "/elements.json";

/// Directory the built viewer bundle is looked for in, relative to the working
/// directory, when `--assets` says nothing.
const DEFAULT_ASSETS: &str = "web/dist";

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

    let assets = match resolve_assets(options.assets.as_deref()) {
        Ok(assets) => assets,
        Err(message) => {
            writeln!(err, "error: {message}")?;
            return Ok(Outcome::Failed);
        }
    };
    if assets.is_none() {
        writeln!(
            err,
            "warning: no viewer bundle at `{DEFAULT_ASSETS}`; serving {ELEMENTS_PATH} only \
             (build it with `npm run build` in `web/`, or pass --assets)"
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
fn router(elements: String, assets: Option<PathBuf>) -> Router {
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
        Some(directory) => argument.fallback_service(
            ServeDir::new(&directory).fallback(ServeFile::new(directory.join("index.html"))),
        ),
        None => argument.fallback(missing_bundle),
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
            "<h1>inka serve</h1><p>The viewer bundle was not found. \
             Build it with <code>npm run build</code> in <code>web/</code>, \
             or start the server with <code>--assets &lt;DIR&gt;</code>.</p>\
             <p>The argument itself is being served at <code>{ELEMENTS_PATH}</code>.</p>"
        )),
    )
}

/// Resolves the bundle directory.
///
/// An explicit `--assets` that is not a directory is an error: the operator
/// asked for something specific, and silently serving nothing would hide the
/// typo. The default is allowed to be absent, because the argument endpoint is
/// useful on its own.
fn resolve_assets(explicit: Option<&Path>) -> Result<Option<PathBuf>, String> {
    match explicit {
        Some(directory) if directory.is_dir() => Ok(Some(directory.to_path_buf())),
        Some(directory) => Err(format!("no viewer bundle at `{}`", directory.display())),
        None => {
            let default = PathBuf::from(DEFAULT_ASSETS);
            Ok(default.is_dir().then_some(default))
        }
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
        let app = router(ELEMENTS.to_owned(), None);

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
        let app = router(ELEMENTS.to_owned(), None);

        let first = body_of(call(app.clone(), Method::GET, ELEMENTS_PATH).await).await;
        let second = body_of(call(app, Method::GET, ELEMENTS_PATH).await).await;

        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn refuses_every_method_that_would_write() {
        for method in [Method::POST, Method::PUT, Method::DELETE, Method::PATCH] {
            let app = router(ELEMENTS.to_owned(), None);

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
        let app = router(ELEMENTS.to_owned(), None);

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
        let app = router(ELEMENTS.to_owned(), Some(bundle.clone()));

        // Act
        let response = call(app, Method::GET, "/index.html").await;

        // Assert
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body_of(response).await.contains("inka"));
        std::fs::remove_dir_all(&bundle).expect("the test bundle should be removable");
    }

    #[test]
    fn refuses_an_assets_directory_that_is_not_there() {
        let missing = Path::new("definitely/not/a/bundle");

        let resolved = resolve_assets(Some(missing));

        assert!(resolved.is_err_and(|message| message.contains("definitely/not/a/bundle")));
    }

    #[test]
    fn accepts_the_absence_of_the_default_bundle() {
        // Nothing was asked for, and the default may simply not be built yet.
        assert!(resolve_assets(None).is_ok());
    }
}
