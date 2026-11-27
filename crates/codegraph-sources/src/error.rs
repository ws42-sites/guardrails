//! What Python raised, as a type name and message, so parity tests can compare failures
//! the way the recorder wrote them. Messages never quote file content.

use codegraph_engine::EngineError;
use codegraph_engine::architecture::PyError;
use codegraph_engine::waypoints::WaypointError;
use codegraph_store::StoreError;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceError {
    /// The Python exception class (`ValueError`, `TimeoutError`, ...).
    pub kind: &'static str,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, SourceError>;

impl SourceError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn value(message: &str) -> Self {
        Self::new("ValueError", message)
    }

    pub fn timeout() -> Self {
        Self::new("TimeoutError", "Indexing time limit exceeded")
    }
}

impl fmt::Display for SourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for SourceError {}

impl From<StoreError> for SourceError {
    fn from(error: StoreError) -> Self {
        Self::new(error.kind, error.message)
    }
}

impl From<rusqlite::Error> for SourceError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::from(error).into()
    }
}

impl From<PyError> for SourceError {
    fn from(error: PyError) -> Self {
        Self::new(error.kind, error.message)
    }
}

impl From<WaypointError> for SourceError {
    fn from(error: WaypointError) -> Self {
        Self::new(error.kind, error.message)
    }
}

impl From<EngineError> for SourceError {
    fn from(error: EngineError) -> Self {
        let kind = match &error {
            EngineError::UnsupportedLanguage(_) => "ValueError",
            EngineError::Parser(_) => "RuntimeError",
            EngineError::Recursion | EngineError::Nesting => "RecursionError",
            EngineError::InvalidInput(_) => "AttributeError",
        };
        Self::new(kind, error.to_string())
    }
}
