//! `file://` URI ↔ filesystem path conversion.
//!
//! lsp-types 0.97 carries a generic RFC 3986 [`Uri`]; mapping the `file`
//! scheme (RFC 8089) onto paths is the server's business. Only absolute
//! `file` URIs with no meaningful authority are accepted — that is what
//! editors send for local workspaces — and anything else resolves to `None`,
//! which callers treat as "not a document this server analyzes".
//!
//! Paths are compared textually throughout the server, so the two directions
//! must round-trip: a path published here and the same path arriving in a
//! later notification have to name the same overlay entry. Both directions
//! therefore normalize nothing beyond percent-coding.
//!
//! Knowingly out of scope: non-UTF-8 path bytes resolve to `None` (the
//! project loader works in `String` anyway), and Windows drive-letter URIs
//! (`file:///c:/…`) are not mapped — the decoded `/c:/…` is not an absolute
//! `PathBuf` there. Both would extend `decode`/`from_path`, not the callers.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// Resolves a document URI to the filesystem path it names.
pub fn to_path(uri: &Uri) -> Option<PathBuf> {
    if !uri.scheme()?.as_str().eq_ignore_ascii_case("file") {
        return None;
    }
    // RFC 8089: an empty authority and `localhost` both mean this machine.
    if uri.authority().is_some_and(|authority| {
        !authority.as_str().is_empty() && !authority.as_str().eq_ignore_ascii_case("localhost")
    }) {
        return None;
    }
    let decoded = decode(uri.path().as_str())?;
    if !decoded.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(decoded))
}

/// Builds the `file://` URI naming an absolute path.
///
/// Returns `None` for a relative path or one that is not valid UTF-8: the
/// server only ever publishes about files it resolved from URIs, so neither
/// occurs in practice.
pub fn from_path(path: &Path) -> Option<Uri> {
    if !path.is_absolute() {
        return None;
    }
    let text = path.to_str()?;
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(char::from(byte));
        } else {
            write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    Uri::from_str(&format!("file://{encoded}")).ok()
}

/// Percent-decodes a URI path into UTF-8 text.
fn decode(encoded: &str) -> Option<String> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        if byte == b'%' {
            let hex = rest.get(1..3)?;
            // from_str_radix alone would admit signs ("%+1"); an escape is
            // exactly two hex digits.
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            let hex = std::str::from_utf8(hex).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &rest[3..];
        } else {
            decoded.push(byte);
            rest = &rest[1..];
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ascii_uri_and_path_round_trip() {
        // Arrange
        let path = Path::new("/home/user/project/argument/a.inka");

        // Act
        let uri = from_path(path).expect("absolute path converts");
        let back = to_path(&uri).expect("file uri resolves");

        // Assert
        assert_eq!(uri.as_str(), "file:///home/user/project/argument/a.inka");
        assert_eq!(back, path);
    }

    #[test]
    fn multibyte_and_space_segments_round_trip_percent_encoded() {
        // Arrange
        let path = Path::new("/tmp/議論 project/a.inka");

        // Act
        let uri = from_path(path).expect("absolute path converts");
        let back = to_path(&uri).expect("file uri resolves");

        // Assert
        assert!(uri.as_str().contains("%20"), "space is encoded: {uri:?}");
        assert_eq!(back, path);
    }

    #[test]
    fn rejects_non_file_schemes_relative_paths_and_remote_authorities() {
        // Arrange
        let untitled = Uri::from_str("untitled:Untitled-1").expect("valid uri");
        let remote = Uri::from_str("file://host/share/a.inka").expect("valid uri");

        // Act & Assert
        assert_eq!(to_path(&untitled), None);
        assert_eq!(to_path(&remote), None);
        assert_eq!(from_path(Path::new("relative/a.inka")), None);
    }

    #[test]
    fn decodes_percent_encoded_documents_sent_by_editors() {
        // Arrange: VS Code sends multibyte path segments percent-encoded.
        let uri = Uri::from_str("file:///tmp/%E8%AD%B0%E8%AB%96/a.inka").expect("valid uri");

        // Act & Assert
        assert_eq!(to_path(&uri), Some(PathBuf::from("/tmp/議論/a.inka")));
    }
}
