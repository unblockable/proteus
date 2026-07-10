use crate::lang::types::Identifier;
use crate::lang::{interpreter, message};

pub type Result<T> = std::result::Result<T, self::Error>;

pub trait ResultExt<T> {
    fn is_success(&self) -> bool;
}

impl<T> ResultExt<T> for Result<T> {
    fn is_success(&self) -> bool {
        self.is_ok() || self.as_ref().is_err_and(|e| e.is_eof())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Cannot get bytes from field {field_id:?}: {err:?}")]
    GetField {
        field_id: Identifier,
        err: message::GetFieldError,
    },
    #[error("Cannot set bytes to field {field_id:?}: {err:?}")]
    SetField {
        field_id: Identifier,
        err: message::SetFieldError,
    },
    #[error("An interpreter crypto error occurred: {0:?}")]
    Crypto(interpreter::crypto::Error),
    #[error("An interpreter mem error occurred: {0:?}")]
    Mem(interpreter::mem::Error),
    #[error("An interpreter io error occurred: {0:?}")]
    Io(#[from] interpreter::io::Error),

    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

impl self::Error {
    pub fn is_eof(&self) -> bool {
        if let self::Error::Io(e) = self
            && let interpreter::io::Error::Eof = e
        {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::lang::{self, interpreter};

    #[test]
    fn is_eof() {
        let eof_err = lang::Error::Io(interpreter::io::Error::Eof);
        assert!(eof_err.is_eof());

        let std_err = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        let std_err = lang::Error::Io(interpreter::io::Error::Read(std_err));
        assert!(!std_err.is_eof());
    }
}
