//! Architecture analysis, ported from the `architecture_*` Python modules.
//!
//! Facts arrive as values (a snapshot's files and edges), results leave as values.
//! Nothing here reads a file, a database or the network; callers own all I/O.

pub mod analysis;
pub mod analysis_budget;
pub mod analysis_check;
pub mod anomalies;
pub mod ci;
pub mod contract;
pub mod contract_models;
pub mod evidence;
pub mod extract;
pub mod extract_rust;
pub mod patterns;
pub mod presentation;
pub mod recognition;
pub mod rules;
pub mod structure;
pub mod structure_facts;
pub mod structure_python;
pub mod structure_rust;
pub mod structure_script;
pub mod topology;
pub mod tree;
pub mod util;
pub mod validate;
pub mod yaml;

use serde_json::{Value, json};
use validate::Issue;

/// An exception the Python code would raise, with the type and message callers can see.
#[derive(Debug, Clone, PartialEq)]
pub struct PyError {
    pub kind: &'static str,
    pub message: String,
    /// For `ValidationError`: the model's name and every issue found.
    pub title: Option<String>,
    pub issues: Vec<Issue>,
}

impl PyError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            title: None,
            issues: Vec::new(),
        }
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    pub fn validation(title: &str, issues: Vec<Issue>) -> Self {
        Self {
            kind: "ValidationError",
            message: format!(
                "{} validation error{} for {title}",
                issues.len(),
                if issues.len() == 1 { "" } else { "s" }
            ),
            title: Some(title.to_owned()),
            issues,
        }
    }

    /// The recorded shape: `error` and `message`, plus the issues of a validation error.
    pub fn to_value(&self) -> Value {
        match &self.title {
            Some(title) => json!({
                "error": self.kind,
                "title": title,
                "count": self.issues.len(),
                "issues": self.issues.iter().map(Issue::to_value).collect::<Vec<_>>(),
            }),
            None => json!({"error": self.kind, "message": self.message}),
        }
    }
}

/// `data.decode("utf-8")`, with CPython's `UnicodeDecodeError` text for the first bad sequence.
pub fn decode_utf8(data: &[u8]) -> Result<String, PyError> {
    match std::str::from_utf8(data) {
        Ok(text) => Ok(text.to_owned()),
        Err(error) => {
            let start = error.valid_up_to();
            let byte = data[start];
            let (end, reason) = match error.error_len() {
                None => (data.len(), "unexpected end of data"),
                Some(length) => (
                    start + length,
                    if (0xC2..=0xF4).contains(&byte) {
                        "invalid continuation byte"
                    } else {
                        "invalid start byte"
                    },
                ),
            };
            let message = if end - start <= 1 {
                format!(
                    "'utf-8' codec can't decode byte 0x{byte:02x} in position {start}: {reason}"
                )
            } else {
                format!(
                    "'utf-8' codec can't decode bytes in position {start}-{}: {reason}",
                    end - 1
                )
            };
            Err(PyError::new("UnicodeDecodeError", message))
        }
    }
}

impl std::fmt::Display for PyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for PyError {}
