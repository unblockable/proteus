pub type Result<T> = std::result::Result<T, RuntimeError>;

#[derive(thiserror::Error, Debug)]
pub enum RuntimeError {
    #[error("An io error occurred: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    // #[error("File not found: {path}")]
    // NotFound { path: String, #[source] io_error: std::io::Error }, // Adds context and source
}
