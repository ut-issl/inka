//! Core library for inka: structured argumentation with accountable approvals.
pub mod ast;
pub mod graph;
pub mod hash;
pub mod interpolate;
pub mod lexer;
pub mod manifest;
pub mod normalize;
pub mod parser;
pub mod project;
pub mod source;
pub mod values;
pub mod view;

/// Returns the version of the inka core library.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_package_version() {
        // Arrange & Act
        let actual = version();

        // Assert
        assert_eq!(actual, env!("CARGO_PKG_VERSION"));
    }
}
