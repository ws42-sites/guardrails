//! What Python raised, as a type name and message, so callers and parity tests can
//! compare failures the way the recorder wrote them.

use rusqlite::ErrorCode;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreError {
    /// The Python exception class (`sqlite3.IntegrityError`, `ValueError`, ...).
    pub kind: &'static str,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl StoreError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn runtime(message: &str) -> Self {
        Self::new("RuntimeError", message)
    }

    pub fn value(message: &str) -> Self {
        Self::new("ValueError", message)
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for StoreError {}

/// Python's `sqlite3` module sorts SQLite result codes into a few exception classes.
impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        let kind = match &error {
            rusqlite::Error::SqliteFailure(failure, _) => match failure.code {
                ErrorCode::ConstraintViolation | ErrorCode::TypeMismatch => "IntegrityError",
                ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt => "DatabaseError",
                ErrorCode::TooBig => "DataError",
                ErrorCode::ApiMisuse => "ProgrammingError",
                _ => "OperationalError",
            },
            _ => "ProgrammingError",
        };
        Self::new(kind, error.to_string())
    }
}
