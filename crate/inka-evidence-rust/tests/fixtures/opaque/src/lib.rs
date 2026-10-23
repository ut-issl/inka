//! Fixture whose only test is produced by a macro the body extractor does
//! not understand, so the adapter must refuse rather than guess.

macro_rules! opaque_test {
    () => {
        #[test]
        fn generated() {
            assert_eq!(1 + 1, 2);
        }
    };
}

#[cfg(test)]
mod tests {
    opaque_test!();
}
