//! Integration-test target mixing a proptest block and a plain test.

use proptest::prelude::*;

proptest! {
    #[test]
    fn add_commutes(a in 0u64..1000, b in 0u64..1000) {
        prop_assert_eq!(sample::add(a, b), sample::add(b, a));
    }
}

#[test]
fn plain_integration() {
    assert_eq!(sample::add(2, 2), 4);
}
