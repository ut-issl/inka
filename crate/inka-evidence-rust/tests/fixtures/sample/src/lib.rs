//! Fixture library: one passing, one failing, one ignored and one nested
//! test, plus a file module. The adapter's tests pin the exact bodies
//! sliced from this file, so edits here ripple into those expectations.

/// Adds two numbers; the subject of every fixture test.
pub fn add(a: u64, b: u64) -> u64 {
    a + b
}

pub mod util;

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn passes() {
        assert_eq!(add(1, 2), 3);
    }

    #[test]
    fn fails() {
        assert_eq!(add(1, 2), 4);
    }

    #[test]
    #[ignore = "declared but skipped"]
    fn ignored() {
        unreachable!("never run by the default profile");
    }

    mod nested {
        #[test]
        fn deep_passes() {
            assert_eq!(crate::add(0, 1), 1);
        }
    }
}
