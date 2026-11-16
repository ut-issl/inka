//! Embeds the built graph viewer into the `inka` binary.
//!
//! `npm run build` in `web/` writes the viewer bundle to `web-dist/` next to
//! this file. Every file under it becomes a `(path, bytes)` entry of the
//! `WEB_ASSETS` table that `serve` answers from, so an installed `inka` needs
//! nothing beside itself. Without the directory the table is empty and `serve`
//! says so; set `INKA_WEB_ASSETS=require` (the release workflow does) to turn
//! that into a build error instead.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const BUNDLE_DIR: &str = "web-dist";

fn main() {
    println!("cargo:rerun-if-changed={BUNDLE_DIR}");
    println!("cargo:rerun-if-env-changed=INKA_WEB_ASSETS");

    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let bundle = manifest_dir.join(BUNDLE_DIR);
    let mut files = Vec::new();
    if bundle.is_dir() {
        collect(&bundle, &bundle, &mut files);
    }
    files.sort();

    let required = env::var("INKA_WEB_ASSETS").as_deref() == Ok("require");
    assert!(
        !(required && files.is_empty()),
        "INKA_WEB_ASSETS=require but `{}` holds no viewer bundle; run `npm run build` in `web/` first",
        bundle.display()
    );
    if files.is_empty() {
        println!(
            "cargo:warning=no viewer bundle under {BUNDLE_DIR}/; `inka serve` will serve the argument only"
        );
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("web_assets.rs");
    let mut source = fs::File::create(&out).expect("the generated source should be writable");
    writeln!(
        source,
        "/// The viewer bundle, as `(path, bytes)` with `/`-separated paths."
    )
    .unwrap();
    writeln!(source, "pub static WEB_ASSETS: &[(&str, &[u8])] = &[").unwrap();
    for (relative, absolute) in &files {
        writeln!(
            source,
            "    ({relative:?}, include_bytes!({:?})),",
            absolute.display().to_string()
        )
        .unwrap();
    }
    writeln!(source, "];").unwrap();
}

/// Every regular file under `dir`, as (path relative to `root` with `/`
/// separators, absolute path). Hidden entries are skipped.
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(dir).expect("the bundle directory should be readable") {
        let path = entry.expect("the bundle entry should be readable").path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("the file is under the bundle root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path));
        }
    }
}
