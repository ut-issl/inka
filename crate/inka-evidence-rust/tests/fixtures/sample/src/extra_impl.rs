//! Loaded as `mod extra` from src/sub.rs through `#[path]`, which in a
//! non-mod-rs file resolves beside the declaring file.

/// Quadruples a number.
pub fn quadruple(a: u64) -> u64 {
    a * 4
}

#[cfg(test)]
mod tests {
    #[test]
    fn via_sibling_path() {
        assert_eq!(super::quadruple(2), 8);
    }
}
