//! Two cfg-dependent definitions of the same test name: ambiguous.

#[cfg(unix)]
#[test]
fn same() {
    assert!(cfg!(unix));
}

#[cfg(windows)]
#[test]
fn same() {
    assert!(cfg!(windows));
}
