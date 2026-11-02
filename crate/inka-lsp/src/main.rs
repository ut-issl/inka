//! The `inka-lsp` binary: the language server spoken over stdio, which is how
//! every LSP client launches a server it manages itself.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    inka_lsp::run(&connection)?;
    // The channels must close before the I/O threads can finish.
    drop(connection);
    io_threads.join()?;
    Ok(())
}
