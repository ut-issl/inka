//! Child of the `#[path]`-loaded `aliased` module, living next to it as
//! rustc resolves children of a mod-rs-like file. Also declares its own
//! `#[path]` module: in a non-mod-rs file that resolves beside this file
//! (src/extra_impl.rs), not under src/sub/ (that decoy must stay unused).

#[path = "extra_impl.rs"]
pub mod extra;

/// Triples a number.
pub fn triple(a: u64) -> u64 {
    a * 3
}

#[cfg(test)]
mod tests {
    #[test]
    fn via_path() {
        assert_eq!(super::triple(2), 6);
    }
}
