//! Python `str` behaviour, reproduced where the Python modules depend on it.
//!
//! Rust's standard library tracks a newer Unicode than CPython 3.12 (15.0), and
//! its notion of whitespace, word characters and case mapping differs in edge
//! cases. Every property used here comes from `unicode_tables`, which is generated
//! from the reference interpreter, so the two implementations agree code point
//! for code point.

use crate::unicode_tables as tables;
use serde_json::Value;
use std::fmt::Write;

fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let code = c as u32;
    let index = table.partition_point(|&(_, end)| end < code);
    table.get(index).is_some_and(|&(start, _)| start <= code)
}

fn lookup(table: &'static [(u32, &'static str)], c: char) -> Option<&'static str> {
    let code = c as u32;
    table
        .binary_search_by_key(&code, |&(key, _)| key)
        .ok()
        .map(|index| table[index].1)
}

/// `str.isspace()` for one character.
pub fn is_space(c: char) -> bool {
    in_ranges(tables::SPACE, c)
}

/// The `\w` class of Python `str` regular expressions.
pub fn is_word(c: char) -> bool {
    in_ranges(tables::WORD, c)
}

/// `unicodedata.category(c).startswith("C")`.
pub fn is_other(c: char) -> bool {
    in_ranges(tables::OTHER, c)
}

/// True when the character is assigned in CPython's Unicode version (15.0).
pub fn is_assigned(c: char) -> bool {
    in_ranges(tables::ASSIGNED, c)
}

fn is_cased(c: char) -> bool {
    in_ranges(tables::CASED, c)
}

fn is_case_ignorable(c: char) -> bool {
    in_ranges(tables::CASE_IGNORABLE, c)
}

/// First character of an identifier (`str.isidentifier` on one character).
pub fn is_id_start(c: char) -> bool {
    in_ranges(tables::ID_START, c)
}

/// Later characters of an identifier.
pub fn is_id_continue(c: char) -> bool {
    in_ranges(tables::ID_CONTINUE, c)
}

/// `str.isidentifier()`.
pub fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(is_id_start) && chars.all(is_id_continue)
}

/// Character regex class text for `\s`, usable inside `[...]`.
pub fn class_body(table: &[(u32, u32)]) -> String {
    let mut out = String::new();
    for &(start, end) in table {
        if start == end {
            let _ = write!(out, "\\x{{{start:x}}}");
        } else {
            let _ = write!(out, "\\x{{{start:x}}}-\\x{{{end:x}}}");
        }
    }
    out
}

pub fn space_class() -> String {
    class_body(tables::SPACE)
}

pub fn word_class() -> String {
    class_body(tables::WORD)
}

/// `str.strip()` with no argument.
pub fn strip(value: &str) -> &str {
    value.trim_matches(is_space)
}

pub fn lstrip(value: &str) -> &str {
    value.trim_start_matches(is_space)
}

pub fn rstrip(value: &str) -> &str {
    value.trim_end_matches(is_space)
}

/// `str.split()` with no argument.
pub fn split_words(value: &str) -> Vec<&str> {
    value
        .split(is_space)
        .filter(|word| !word.is_empty())
        .collect()
}

/// `str.splitlines()`.
pub fn splitlines(value: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut iterator = value.char_indices().peekable();
    while let Some((index, c)) = iterator.next() {
        let boundary = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        lines.push(&value[start..index]);
        let mut next = index + c.len_utf8();
        if c == '\r' && matches!(iterator.peek(), Some((_, '\n'))) {
            iterator.next();
            next += 1;
        }
        start = next;
    }
    if start < value.len() {
        lines.push(&value[start..]);
    }
    lines
}

/// `str.expandtabs()` with the default tab size of 8.
pub fn expandtabs(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut column = 0usize;
    for c in value.chars() {
        match c {
            '\t' => {
                let pad = 8 - column % 8;
                out.extend(std::iter::repeat_n(' ', pad));
                column += pad;
            }
            '\n' | '\r' => {
                out.push(c);
                column = 0;
            }
            _ => {
                out.push(c);
                column += 1;
            }
        }
    }
    out
}

/// `str.casefold()`; per character, with no context.
pub fn casefold(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else if let Some(mapped) = lookup(tables::CASEFOLD, c) {
            out.push_str(mapped);
        } else {
            out.push(c);
        }
    }
    out
}

fn push_mapped(out: &mut String, table: &'static [(u32, &'static str)], c: char) {
    match lookup(table, c) {
        Some(mapped) => out.push_str(mapped),
        None => out.push(c),
    }
}

/// `str.upper()` of one character.
pub fn upper_char(c: char) -> String {
    let mut out = String::new();
    push_mapped(&mut out, tables::UPPER, c);
    out
}

/// `str.lower()` of one character (no word context, so sigma is not final).
pub fn lower_char(c: char) -> String {
    let mut out = String::new();
    push_mapped(&mut out, tables::LOWER, c);
    out
}

fn final_sigma(chars: &[char], index: usize) -> bool {
    // CPython's Final_Sigma rule: preceded by a cased letter and not followed by one,
    // looking through case-ignorable characters on both sides.
    let mut before = index;
    let mut preceding = None;
    while before > 0 {
        before -= 1;
        if !is_case_ignorable(chars[before]) {
            preceding = Some(chars[before]);
            break;
        }
    }
    if !preceding.is_some_and(is_cased) {
        return false;
    }
    for &c in &chars[index + 1..] {
        if !is_case_ignorable(c) {
            return !is_cased(c);
        }
    }
    true
}

/// `str.upper()`.
pub fn upper(value: &str) -> String {
    value.chars().map(upper_char).collect()
}

/// `str.lower()`.
pub fn lower(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    for (index, &c) in chars.iter().enumerate() {
        if c == '\u{3a3}' {
            out.push(if final_sigma(&chars, index) {
                '\u{3c2}'
            } else {
                '\u{3c3}'
            });
        } else {
            push_mapped(&mut out, tables::LOWER, c);
        }
    }
    out
}

/// `str.capitalize()`.
pub fn capitalize(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    for (index, &c) in chars.iter().enumerate() {
        if index == 0 {
            push_mapped(&mut out, tables::TITLE, c);
        } else if c == '\u{3a3}' {
            out.push(if final_sigma(&chars, index) {
                '\u{3c2}'
            } else {
                '\u{3c3}'
            });
        } else {
            push_mapped(&mut out, tables::LOWER, c);
        }
    }
    out
}

/// Slice by code point, as Python's `value[start:end]` does for non-negative bounds.
pub fn char_slice(value: &str, start: usize, end: Option<usize>) -> &str {
    let mut indices = value.char_indices().map(|(index, _)| index);
    let begin = indices.nth(start).unwrap_or(value.len());
    let finish = match end {
        None => value.len(),
        Some(end) if end <= start => begin,
        Some(end) => value
            .char_indices()
            .map(|(index, _)| index)
            .nth(end)
            .unwrap_or(value.len()),
    };
    &value[begin..finish.max(begin)]
}

pub fn char_len(value: &str) -> usize {
    value.chars().count()
}

/// `bytes.decode("utf-8", "replace")`.
pub fn decode_replace(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

/// `json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)`.
///
/// Object keys sort by UTF-8 bytes, which orders them by code point exactly as
/// Python does.
pub fn python_json(value: &Value) -> String {
    let mut out = String::new();
    write_json(&mut out, value);
    out
}

fn write_json(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) if number.is_f64() => {
            // Python prints floats with `repr`, which differs from Rust beyond 1e16.
            out.push_str(&crate::py_value::float_repr(number.as_f64().unwrap_or(0.0)));
        }
        Value::Number(number) => {
            let _ = write!(out, "{number}");
        }
        Value::String(text) => write_json_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_string(out, key);
                out.push(':');
                write_json(out, item);
            }
            out.push('}');
        }
    }
}

fn write_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(splitlines("a\r\nb\rc\n\nd"), ["a", "b", "c", "", "d"]);
        assert_eq!(splitlines("a\u{2028}b\u{85}"), ["a", "b"]);
        assert!(splitlines("").is_empty());
        assert_eq!(splitlines("x\n"), ["x"]);
    }

    #[test]
    fn whitespace_follows_python_not_rust() {
        // U+001C is whitespace to Python and not to Rust; U+FEFF is the reverse.
        assert!(is_space('\u{1c}'));
        assert!(!is_space('\u{feff}'));
        assert_eq!(split_words("a\u{1c}b\u{a0}c"), ["a", "b", "c"]);
    }

    #[test]
    fn case_mapping_follows_python() {
        assert_eq!(casefold("Stra\u{df}e"), "strasse");
        assert_eq!(casefold("\u{130}"), "i\u{307}");
        assert_eq!(capitalize("\u{df}x"), "Ssx");
        assert_eq!(capitalize("a\u{3a3}"), "A\u{3c2}");
        assert_eq!(capitalize("a\u{3a3}b"), "A\u{3c3}b");
        assert_eq!(upper_char('\u{df}'), "SS");
    }

    #[test]
    fn slices_by_code_point() {
        assert_eq!(
            char_slice("a\u{e9}\u{1f600}z", 1, Some(3)),
            "\u{e9}\u{1f600}"
        );
        assert_eq!(char_slice("abc", 5, None), "");
        assert_eq!(char_slice("abc", 1, Some(0)), "");
    }

    #[test]
    fn json_digest_text_is_ascii_sorted() {
        let value = serde_json::json!({"b": "\u{e9}\u{1f600}", "a": [1, true, null]});
        let slash = char::from(92);
        let expected =
            format!("{{\"a\":[1,true,null],\"b\":\"{slash}u00e9{slash}ud83d{slash}ude00\"}}");
        assert_eq!(python_json(&value), expected);
    }

    #[test]
    fn expandtabs_resets_on_newlines() {
        assert_eq!(expandtabs("a\tb\n\tc"), "a       b\n        c");
    }
}
