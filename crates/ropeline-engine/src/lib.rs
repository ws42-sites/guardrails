//! Ropeline's pure engine core, ported from CodeGraph's Python one module at a time.
//!
//! Bytes and strings go in, plain values come out. Nothing here reads files, the
//! network, the environment or other processes, and repository code is never
//! executed. `tests/purity.rs` and `clippy.toml` hold that line.

#![forbid(unsafe_code)]

pub mod architecture;
pub mod codeowners;
pub mod file_categories;
pub mod parsing;
pub mod parsing_rust;
pub mod posix_path;
pub mod py_fnmatch;
pub mod py_json;
pub mod py_regex;
pub mod py_shlex;
pub mod py_toml;
pub mod py_unparse;
pub mod py_value;
pub mod pyast;
pub mod relationships;
pub mod relationships_rust;
pub mod text;
pub mod waypoint_signals;
pub mod waypoints;
#[rustfmt::skip]
mod unicode_tables;

pub use unicode_tables::UNICODE_VERSION;

use std::fmt;

/// Why the engine could not answer. Python raised exceptions for these cases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    UnsupportedLanguage(String),
    Parser(String),
    /// CPython would raise `RecursionError` while building the syntax tree.
    Recursion,
    /// CPython would raise `RecursionError` while walking a deeply nested `use` tree.
    Nesting,
    /// The caller supplied facts of a shape the Python code would also choke on.
    InvalidInput(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedLanguage(language) => {
                write!(formatter, "unsupported language {language:?}")
            }
            Self::Recursion => write!(formatter, "syntax tree nested too deeply"),
            Self::Nesting => write!(formatter, "use tree nested too deeply"),
            Self::InvalidInput(message) => write!(formatter, "invalid input: {message}"),
            Self::Parser(message) => write!(formatter, "parser failure: {message}"),
        }
    }
}

impl std::error::Error for EngineError {}
