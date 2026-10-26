//! Starts with a UTF-8 byte-order mark, which the parser strips.

mod inner {
    #[test]
    fn uni() {
        let _s = "日本語é";
    }
}
