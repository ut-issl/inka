//! Parses every checked-in `.inka` fixture and asserts a clean, stable parse.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::lexer::Lexer;
use inka_core::parser::parse;
use inka_core::source::SourceMap;

mod common;

fn fixture_files() -> Vec<PathBuf> {
    let root = common::fixtures_root();
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    files
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("fixtures dir should exist") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "inka") {
            out.push(path);
        }
    }
}

#[test]
fn all_fixtures_parse_without_diagnostics() {
    let files = fixture_files();
    assert!(!files.is_empty(), "expected at least one .inka fixture");

    for path in files {
        let text = fs::read_to_string(&path).expect("fixture should read");
        let mut map = SourceMap::new();
        let file_id = map.add_file(path.clone(), text.clone());

        let tokens = Lexer::new(file_id, &text)
            .lex()
            .unwrap_or_else(|err| panic!("{}: lex error: {}", path.display(), err.message));

        let result = parse(file_id, &tokens);
        assert!(
            result.diagnostics.is_empty(),
            "{}: unexpected diagnostics: {:?}",
            path.display(),
            result
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
        );
        assert!(
            !result.blocks.is_empty(),
            "{}: produced no blocks",
            path.display()
        );
    }
}
