//! Fixture whose only test is produced by a macro the body extractor does
//! not understand, so the adapter must refuse rather than guess.

#[cfg(test)]
mod tests {
    // Defined inside the test module so the non-test build, which CI
    // compiles with `-D warnings`, sees no unused macro.
    macro_rules! opaque_test {
        () => {
            #[test]
            fn generated() {
                assert_eq!(1 + 1, 2);
            }
        };
    }

    opaque_test!();
}
