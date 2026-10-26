//! DECOY — rustc never loads this file. A resolver that wrongly treats the
//! `#[path]`-loaded renamed.rs as a non-mod-rs file would land here.

#[cfg(test)]
mod tests {
    #[test]
    fn via_path() {
        unreachable!("DECOY: this file is not part of the crate");
    }
}
