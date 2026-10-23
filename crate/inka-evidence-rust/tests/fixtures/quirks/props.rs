//! Several proptest blocks, a block-level config, a qualified fn and
//! nested braces, for the token-level slicer.

use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5))]

    #[test]
    fn first(x in prop::collection::vec(0u8.., 0..10)) {
        let _probe = nested { braces };
        prop_assert!(x.len() < 10);
    }

    pub fn second(y in 0u64..10) {
        prop_assert!(y < 10);
    }
}

proptest! {
    #[test]
    fn third(z in 0i32..5) {
        prop_assert!(z >= 0);
    }
}
