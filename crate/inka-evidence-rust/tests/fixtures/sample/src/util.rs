//! File module exercising `name.rs` resolution in the body extractor.

/// Doubles a number.
pub fn double(a: u64) -> u64 {
    a * 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn doubles() {
        assert_eq!(super::double(2), 4);
    }
}
