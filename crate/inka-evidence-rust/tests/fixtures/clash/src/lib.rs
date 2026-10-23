//! The module `props` makes the lib test `clash::props::x` collide with
//! the integration-test suite `clash::props` and its test `x`.

#[cfg(test)]
mod props {
    #[test]
    fn x() {
        assert!(true);
    }
}
