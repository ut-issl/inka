//! Append-only jsonl storage for approval records.
//!
//! One file per approver — `approvals/<name>.jsonl`, where `<name>` is the
//! approver the records inside name — and one record per line. Append-only is
//! the storage-side shape of accountability: a judgement, once written, is
//! never edited in place; a change of mind is a new record.
//!
//! Loading is loud on purpose. A line that does not parse, carries a field
//! this version does not know, breaks a record invariant, or names an approver
//! other than its file fails with the file and line number, never a skip: a
//! silently dropped record would change what the argument appears to have
//! agreed without anyone noticing. The one quiet case is a missing
//! `approvals/` directory, which is how a project that has recorded nothing
//! yet looks.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::record::Record;

/// The directory approval records live under, relative to the project root.
pub const APPROVALS_DIR: &str = "approvals";

/// The file an approver's records belong in.
///
/// Private on purpose: the only route to a path is through a validated
/// [`Record`], whose approver is held to a filename-safe allow-list, so no
/// raw caller string can ever reach this join and traverse out of the
/// approvals directory.
fn record_file(approvals_dir: &Path, approver: &str) -> PathBuf {
    approvals_dir.join(format!("{approver}.jsonl"))
}

/// Appends one record to its approver's file, creating directory and file as
/// needed.
/// A missing final newline is appended before the new record. The file is
/// locked through the boundary check and write so concurrent appends cannot
/// insert an extra separator or interleave records.
///
/// # Errors
///
/// Returns [`StoreError::Write`] when the directory or file cannot be created,
/// locked, inspected, or written.
///
/// # Panics
///
/// Panics if the record fails to serialize to JSON, which a validated record
/// cannot do.
pub fn append(approvals_dir: &Path, record: &Record) -> Result<(), StoreError> {
    fs::create_dir_all(approvals_dir).map_err(|source| StoreError::Write {
        path: approvals_dir.to_owned(),
        source,
    })?;

    let path = record_file(approvals_dir, record.approver());
    let mut line = serde_json::to_string(record).expect("a record serializes to JSON");
    line.push('\n');

    OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| {
            file.lock()?;
            if file.seek(SeekFrom::End(0))? != 0 {
                file.seek(SeekFrom::End(-1))?;
                let mut last = [0];
                file.read_exact(&mut last)?;
                if last[0] != b'\n' {
                    line.insert(0, '\n');
                }
            }
            file.write_all(line.as_bytes())
        })
        .map_err(|source| StoreError::Write { path, source })
}

/// Loads every record in one approver's file, in stored order.
///
/// # Errors
///
/// Returns [`StoreError::Read`] when the file cannot be read,
/// [`StoreError::MalformedLine`] when a line is not a well-formed v1 record —
/// including blank lines and unknown fields — and
/// [`StoreError::ApproverMismatch`] when a record names an approver other
/// than the file it sits in.
pub fn load_file(path: &Path) -> Result<Vec<Record>, StoreError> {
    let file = File::open(path).map_err(|source| StoreError::Read {
        path: path.to_owned(),
        source,
    })?;
    let owner = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    BufReader::new(file)
        .split(b'\n')
        .enumerate()
        .map(|(index, bytes)| {
            let bytes = bytes.map_err(|source| StoreError::Read {
                path: path.to_owned(),
                source,
            })?;
            let line = std::str::from_utf8(&bytes).map_err(|error| StoreError::MalformedLine {
                path: path.to_owned(),
                line: index + 1,
                message: error.to_string(),
            })?;
            parse_line(path, index + 1, line, &owner)
        })
        .collect()
}

/// Loads every record under an approvals directory, file by file in sorted
/// path order.
///
/// A missing directory is an empty record set — that is what a project that
/// has recorded nothing yet looks like. Only `*.jsonl` entries are read, so a
/// stray README does not break loading.
///
/// # Errors
///
/// Returns [`StoreError::Read`] when the directory or a file in it cannot be
/// read, and everything [`load_file`] returns for the files themselves.
pub fn load_dir(approvals_dir: &Path) -> Result<Vec<Record>, StoreError> {
    let entries = match fs::read_dir(approvals_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StoreError::Read {
                path: approvals_dir.to_owned(),
                source,
            });
        }
    };

    let mut files: Vec<PathBuf> = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| StoreError::Read {
                    path: approvals_dir.to_owned(),
                    source,
                })
        })
        .collect::<Result<_, _>>()?;
    files.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "jsonl"));
    files.sort();

    let mut records = Vec::new();
    for file in &files {
        records.extend(load_file(file)?);
    }
    Ok(records)
}

fn parse_line(
    path: &Path,
    line_number: usize,
    line: &str,
    owner: &str,
) -> Result<Record, StoreError> {
    let malformed = |message: String| StoreError::MalformedLine {
        path: path.to_owned(),
        line: line_number,
        message,
    };

    if line.trim().is_empty() {
        return Err(malformed(
            "blank line; an approvals file is one record per line".to_owned(),
        ));
    }
    let record: Record =
        serde_json::from_str(line).map_err(|error| malformed(error.to_string()))?;
    if record.approver() != owner {
        return Err(StoreError::ApproverMismatch {
            path: path.to_owned(),
            line: line_number,
            found: record.approver().to_owned(),
            expected: owner.to_owned(),
        });
    }
    Ok(record)
}

/// A reason the approvals store could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("failed to read {}: {source}", path.display())]
    Read { path: PathBuf, source: io::Error },
    #[error("failed to write {}: {source}", path.display())]
    Write { path: PathBuf, source: io::Error },
    #[error("{}:{line}: malformed approval record: {message}", path.display())]
    MalformedLine {
        path: PathBuf,
        line: usize,
        message: String,
    },
    #[error(
        "{}:{line}: record names approver `{found}` but the file belongs to `{expected}`",
        path.display()
    )]
    ApproverMismatch {
        path: PathBuf,
        line: usize,
        found: String,
        expected: String,
    },
}
