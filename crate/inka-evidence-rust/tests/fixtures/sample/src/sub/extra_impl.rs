//! DECOY — rustc never loads this file. A resolver that joins a `#[path]`
//! declared in src/sub.rs to the module directory src/sub/ would land
//! here instead of src/extra_impl.rs.

#[cfg(test)]
mod tests {
    #[test]
    fn via_sibling_path() {
        unreachable!("DECOY: this file is not part of the crate");
    }
}
