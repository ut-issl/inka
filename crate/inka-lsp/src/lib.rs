//! Language server for the inka DSL.
//!
//! The server is a thin, synchronous shell around `inka-core`: every change
//! to an open buffer reloads the affected project through an overlay reader
//! and republishes the diagnostics `inka check` would print. There is no
//! incremental state to invalidate — full recomputation is the project-wide
//! policy until measured otherwise — so what the editor shows is always the
//! result of one plain load over current text.

mod diagnostics;
mod hints;
mod lens;
mod navigate;
mod position;
mod rename;
mod requests;
mod roots;
mod server;
mod state;
mod tokens;
mod uri;

pub use server::run;
