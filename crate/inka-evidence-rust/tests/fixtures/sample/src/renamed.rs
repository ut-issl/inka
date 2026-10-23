//! Loaded as `mod aliased` through `#[path = "renamed.rs"]`. Being
//! `#[path]`-loaded makes this file mod-rs-like: `mod sub;` resolves to
//! src/sub.rs, not src/renamed/sub.rs (that decoy must stay unused).

pub mod sub;
