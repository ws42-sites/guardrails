//! Source signals for waypoints: the comment scanner shared by the script languages
//! and Rust, and Rust's own comment and test patterns. Port of `_comment_signals`,
//! `_RUST_COMMENTS` and `_RUST_TEST` in `codegraph.waypoints`.

use crate::py_regex::regex;
use crate::text;
use fancy_regex::Regex;
use std::sync::LazyLock;

/// One signal: the text, its line and how it was found.
pub type Signal = (String, i64, &'static str);

const MAX_COMMENTS: usize = 16;
const MAX_TESTS: usize = 128;
/// Comments past this many characters are not read.
const COMMENT_WINDOW: usize = 128_000;
/// Comment text that is tooling or legal boilerplate, not a description.
const BOILERPLATE: [&str; 5] = ["@", "eslint", "prettier", "Copyright", "SPDX"];

/// Rust adds doc markers to the usual comments: `///`, `//!`, `/**` and `/*!`.
pub static RUST_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| regex(r"/\*[*!]?\s*([^*][\s\S]*?)\*/|(?m:^\s*//[/!]?\s*(.+)$)"));

/// A `#[test]` or `#[tokio::test]` attribute, any further attributes, then the
/// function it marks.
static RUST_TEST: LazyLock<Regex> = LazyLock::new(|| {
    regex(concat!(
        r"(?m)^[ \t]*#\[(?:\w+::)*test\b[^\]\n]*\]",
        r"(?:\s*#\[[^\]\n]*\])*",
        r"\s*(?:pub(?:\([^)\n]*\))?\s+)?(?:async\s+)?fn\s+(\w+)"
    ))
});

fn line_of(source: &str, offset: usize) -> i64 {
    source.as_bytes()[..offset]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count() as i64
        + 1
}

/// The first line of each leading comment, up to sixteen, as description signals.
pub fn comment_signals(pattern: &Regex, source: &str) -> Vec<Signal> {
    let head = text::char_slice(source, 0, Some(COMMENT_WINDOW));
    let mut descriptions: Vec<Signal> = Vec::new();
    let mut position = 0;
    while let Some(found) = pattern.captures_from_pos(head, position).ok().flatten() {
        let whole = found.get(0).expect("whole match");
        let raw = found
            .iter()
            .skip(1)
            .flatten()
            .map(|group| group.as_str())
            .find(|group| !group.is_empty())
            .unwrap_or("");
        let first = text::splitlines(raw)
            .into_iter()
            .map(|line| line.trim_matches([' ', '*']))
            .find(|line| !line.is_empty())
            .unwrap_or("");
        if !first.is_empty() && !BOILERPLATE.iter().any(|prefix| first.starts_with(prefix)) {
            descriptions.push((
                first.to_owned(),
                line_of(head, whole.start()),
                "source-comment",
            ));
        }
        if descriptions.len() >= MAX_COMMENTS {
            break;
        }
        // The pattern cannot match nothing, so the search always advances.
        position = whole.end();
    }
    descriptions
}

/// Names of the functions Rust marks as tests, with their lines, up to 128.
pub fn rust_test_names(source: &str) -> Vec<(String, i64)> {
    let mut tests = Vec::new();
    let mut position = 0;
    while let Ok(Some(found)) = RUST_TEST.captures_from_pos(source, position) {
        let whole = found.get(0).expect("whole match");
        let name = found.get(1).expect("function name");
        tests.push((name.as_str().to_owned(), line_of(source, name.start())));
        if tests.len() >= MAX_TESTS {
            break;
        }
        position = whole.end();
    }
    tests
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_comments_describe_a_rust_file() {
        let source = "//! Crate docs.\n/// Item docs.\n/** Block docs. */\n// plain\n";
        let found = comment_signals(&RUST_COMMENT, source);
        let words: Vec<&str> = found.iter().map(|item| item.0.as_str()).collect();
        assert_eq!(words, ["Crate docs.", "Item docs.", "Block docs.", "plain"]);
        assert_eq!(found[2].1, 3);
    }

    #[test]
    fn boilerplate_comments_are_not_descriptions() {
        let source = "// Copyright 2026\n// SPDX-License-Identifier: MIT\n// @allow\n// real\n";
        let found = comment_signals(&RUST_COMMENT, source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "real");
    }

    #[test]
    fn only_the_first_sixteen_comments_count() {
        let source: String = (0..40).map(|index| format!("// c{index}\n")).collect();
        assert_eq!(comment_signals(&RUST_COMMENT, &source).len(), MAX_COMMENTS);
    }

    #[test]
    fn comments_past_the_window_are_not_read() {
        let source = format!("{}\n// late\n", "x".repeat(COMMENT_WINDOW));
        assert!(comment_signals(&RUST_COMMENT, &source).is_empty());
    }

    #[test]
    fn test_attributes_name_their_functions() {
        let source = "#[test]\nfn plain() {}\n\n#[tokio::test]\n#[ignore]\npub(crate) async fn later() {}\n\n#[test_case]\nfn skipped() {}\n";
        let found = rust_test_names(source);
        assert_eq!(found, [("plain".to_owned(), 2), ("later".to_owned(), 6)]);
    }

    #[test]
    fn at_most_128_tests_are_listed() {
        let source: String = (0..200)
            .map(|index| format!("#[test]\nfn t{index}() {{}}\n"))
            .collect();
        assert_eq!(rust_test_names(&source).len(), MAX_TESTS);
    }
}
