//! Rewriting approval-record targets when a block id changes.
//!
//! Ids are filing, not contract: a record's `view_hash` — the thing the
//! approver actually judged — never contains one. Evaluation still routes
//! records to nodes by `<mode>:<id>`, so a rename that only touched the
//! argument files would orphan the node's own approvals. The rename edit
//! therefore rewrites the `target` field of every matching record, keeping
//! each record attached to the judgement it has always expressed. The edits
//! go through the editor's workspace edit like the source edits, so they are
//! visible and revertible rather than silently applied.
//!
//! Note for the signature era (full-plan capability 5): signing the canonical
//! record bytes freezes them, so this rewrite strategy must be revisited —
//! re-signing, id-free targets, or an explicit migration record — before
//! signatures land.

use std::path::{Path, PathBuf};

use inka_core::approvals::APPROVALS_DIR;
use inka_core::read::ReadText;

/// One approvals file to rewrite: its current text and the byte ranges of
/// the old id inside each matching record's target value (the id alone, not
/// the mode or the quotes).
pub struct FileRewrites {
    pub path: PathBuf,
    pub text: String,
    pub ranges: Vec<(usize, usize)>,
}

/// The rewrites a rename of `<mode_keyword>:<old>` needs across every
/// `approvals/*.jsonl` of the project at `root`. Files without a matching
/// record are omitted; a project without an approvals directory has none.
///
/// # Errors
///
/// Returns the underlying I/O error when the approvals directory or one of
/// its files cannot be read (a missing directory is not an error).
pub fn approval_rewrites(
    root: &Path,
    mode_keyword: &str,
    old: &str,
    reader: &dyn ReadText,
) -> std::io::Result<Vec<FileRewrites>> {
    let directory = root.join(APPROVALS_DIR);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths: Vec<PathBuf> = entries
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    paths.sort();

    let mut rewrites = Vec::new();
    for path in paths {
        let text = reader.read_text(&path)?;
        let ranges = target_ranges(&text, mode_keyword, old);
        if !ranges.is_empty() {
            rewrites.push(FileRewrites { path, text, ranges });
        }
    }
    Ok(rewrites)
}

/// Byte ranges of the id inside the `target` value of every record line
/// whose parsed target equals `<mode_keyword>:<old>`.
///
/// Parsing decides *whether* a line matches; the textual scan only locates
/// where the already-confirmed value sits. The key check guards against the
/// same spelling appearing earlier in the line inside another string (a
/// comment quoting the target, say) — interior quotes there are escaped, so
/// they can never follow a bare `"target":` key.
fn target_ranges(text: &str, mode_keyword: &str, old: &str) -> Vec<(usize, usize)> {
    let target = format!("{mode_keyword}:{old}");
    let needle = format!("\"{target}\"");
    let mut ranges = Vec::new();
    let mut line_start = 0usize;
    for line in text.split_inclusive('\n') {
        let record = line.trim_end_matches(['\n', '\r']);
        if !record.trim().is_empty()
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(record)
            && value.get("target").and_then(serde_json::Value::as_str) == Some(target.as_str())
            && let Some(found) = value_offset(record, &needle)
        {
            let id_start = line_start + found + 1 + mode_keyword.len() + 1;
            ranges.push((id_start, id_start + old.len()));
        }
        line_start += line.len();
    }
    ranges
}

/// The byte offset of the occurrence of `needle` that sits in value
/// position right after a `"target"` key.
fn value_offset(line: &str, needle: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(found) = line[from..].find(needle) {
        let at = from + found;
        if follows_target_key(&line[..at]) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

fn follows_target_key(prefix: &str) -> bool {
    let Some(before_colon) = prefix.trim_end().strip_suffix(':') else {
        return false;
    };
    before_colon.trim_end().ends_with("\"target\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use inka_core::read::DiskText;

    #[test]
    fn target_ranges_find_matching_records_only() {
        // Arrange: a matching approve, a different node, a different mode.
        let text = concat!(
            "{\"v\":1,\"target\":\"fiat:leaf\",\"view_hash\":\"sha256:aa\"}\n",
            "{\"v\":1,\"target\":\"fiat:other\",\"view_hash\":\"sha256:bb\"}\n",
            "{\"v\":1,\"target\":\"structure:leaf\",\"view_hash\":\"sha256:cc\"}\n",
        );

        // Act
        let ranges = target_ranges(text, "fiat", "leaf");

        // Assert: exactly the first line's id, by its bytes.
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(&text[start..end], "leaf");
        assert!(text[..start].ends_with("fiat:"));
    }

    #[test]
    fn a_comment_quoting_the_target_does_not_shift_the_range() {
        // Arrange: the comment spells the same quoted target before the
        // target field — its interior quotes are escaped in JSON, so the key
        // check must skip it.
        let text = concat!(
            "{\"v\":1,\"comment\":\"keep \\\"fiat:leaf\\\" as is\",",
            "\"target\":\"fiat:leaf\",\"view_hash\":\"sha256:aa\"}\n",
        );

        // Act
        let ranges = target_ranges(text, "fiat", "leaf");

        // Assert
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(&text[start..end], "leaf");
        assert!(text[..start].ends_with("\"target\":\"fiat:"));
    }

    #[test]
    fn approval_rewrites_scan_only_jsonl_files_and_tolerate_no_directory() {
        // Arrange
        let temp = tempfile::tempdir().expect("temp dir");
        let approvals = temp.path().join(APPROVALS_DIR);
        std::fs::create_dir_all(&approvals).expect("approvals dir");
        std::fs::write(
            approvals.join("alice.jsonl"),
            "{\"v\":1,\"target\":\"fiat:leaf\",\"view_hash\":\"sha256:aa\"}\n",
        )
        .expect("alice");
        std::fs::write(
            approvals.join("notes.txt"),
            "\"target\":\"fiat:leaf\" in prose\n",
        )
        .expect("notes");
        let empty = tempfile::tempdir().expect("temp dir");

        // Act
        let rewrites =
            approval_rewrites(temp.path(), "fiat", "leaf", &DiskText).expect("scan succeeds");
        let none = approval_rewrites(empty.path(), "fiat", "leaf", &DiskText)
            .expect("missing directory is fine");

        // Assert
        assert_eq!(rewrites.len(), 1);
        assert!(rewrites[0].path.ends_with("alice.jsonl"));
        assert_eq!(rewrites[0].ranges.len(), 1);
        assert!(none.is_empty());
    }
}
