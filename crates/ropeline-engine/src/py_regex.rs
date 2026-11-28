//! Python `re` patterns on Rust's engine.
//!
//! The Python `\b`, `\s` and `\w` are Unicode-aware and follow CPython 3.12's
//! tables, so patterns are built from the generated classes, never Rust's.

use crate::text;
use fancy_regex::{Regex, RegexBuilder};
use std::sync::LazyLock;

struct Classes {
    space: String,
    word: String,
}

static CLASSES: LazyLock<Classes> = LazyLock::new(|| Classes {
    space: text::space_class(),
    word: text::word_class(),
});

/// Translate Python regex shorthand into explicit classes.
pub fn python_pattern(pattern: &str) -> String {
    let classes = &*CLASSES;
    let boundary = format!(
        "(?:(?<=[{w}])(?![{w}])|(?<![{w}])(?=[{w}]))",
        w = classes.word
    );
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('s') if in_class => out.push_str(&classes.space),
                Some('s') => out.push_str(&format!("[{}]", classes.space)),
                Some('S') => out.push_str(&format!("[^{}]", classes.space)),
                Some('w') => out.push_str(&format!("[{}]", classes.word)),
                Some('b') => out.push_str(&boundary),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
            continue;
        }
        match c {
            '[' if !in_class => in_class = true,
            ']' if in_class => in_class = false,
            _ => {}
        }
        out.push(c);
    }
    out
}

/// Compile a Python pattern; the backtracking limit is lifted so long inputs behave
/// as they do under Python's engine.
pub fn regex(pattern: &str) -> Regex {
    RegexBuilder::new(&python_pattern(pattern))
        .backtrack_limit(usize::MAX)
        .build()
        .expect("static pattern is valid")
}

/// `pattern.search(haystack) is not None`.
pub fn search(pattern: &Regex, haystack: &str) -> bool {
    pattern.is_match(haystack).unwrap_or(false)
}

/// Python's `re.IGNORECASE` also equates U+0130 and U+0131 with `i`; Rust's does not.
pub fn for_case_insensitive(value: &str) -> String {
    value.replace(['\u{130}', '\u{131}'], "i")
}
