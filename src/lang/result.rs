use std::io;

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

impl RuntimeError {
    pub fn is_eof(&self) -> bool {
        if let RuntimeError::Io(e) = self && e.kind() == io::ErrorKind::UnexpectedEof {
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::lang::RuntimeError;

    #[test]
    fn is_eof_error() {
        let error = RuntimeError::Io(io::ErrorKind::UnexpectedEof.into());
        assert!(error.is_eof())
    }
}
