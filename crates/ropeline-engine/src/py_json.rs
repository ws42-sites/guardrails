//! `json.loads` as CPython 3.12 runs it: the same accepted syntax and the same error
//! text and positions, because callers swallow some failures and report others.
//!
//! Differences from `serde_json`: `NaN`, `Infinity` and `-Infinity` are numbers, integers
//! have any size, a repeated key keeps its first position with its last value, and
//! messages read `Expecting value: line 1 column 1 (char 0)`.

use crate::py_value::PyValue;

/// Why `loads` stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonFailure {
    /// `json.JSONDecodeError`, a `ValueError`; the text is its `str()`.
    Decode(String),
    /// Another `ValueError`: the integer digit limit, or a key hook that refused.
    Value(String),
    /// `RecursionError`: nested more deeply than the interpreter allows.
    Recursion,
}

/// Nesting CPython's scanner accepts from a shallow caller. Python counts C-level
/// recursion, which is not one fixed figure.
pub const MAX_NESTING: usize = 2997;

/// Stands in for a lone surrogate (`"\ud800"`), which a Rust string cannot hold. Python
/// decodes it; validation then refuses it as `string_unicode`.
pub const LONE_SURROGATE: char = '\u{10ffff}';

/// CPython's default `sys.get_int_max_str_digits()`.
const INT_DIGIT_LIMIT: usize = 4300;

/// Parse JSON text. With `refuse_duplicates`, a repeated key in one object fails with
/// the message `duplicate_message`, as an `object_pairs_hook` that raises would.
pub fn loads(
    source: &str,
    duplicate_message: Option<&'static str>,
) -> Result<PyValue, JsonFailure> {
    decode(source, duplicate_message, true)
}

/// `json.loads(bytes)` after decoding: only text input checks for a byte order mark.
pub fn loads_decoded(
    source: &str,
    duplicate_message: Option<&'static str>,
) -> Result<PyValue, JsonFailure> {
    decode(source, duplicate_message, false)
}

fn decode(
    source: &str,
    duplicate_message: Option<&'static str>,
    check_bom: bool,
) -> Result<PyValue, JsonFailure> {
    // Nesting up to `MAX_NESTING` recurses once per level; give it a roomy stack.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, || decode_text(source, duplicate_message, check_bom))
            .map_or(Err(JsonFailure::Recursion), |handle| {
                handle.join().unwrap_or(Err(JsonFailure::Recursion))
            })
    })
}

fn decode_text(
    source: &str,
    duplicate_message: Option<&'static str>,
    check_bom: bool,
) -> Result<PyValue, JsonFailure> {
    let chars: Vec<char> = source.chars().collect();
    if check_bom && chars.first() == Some(&'\u{feff}') {
        return Err(decode_error(
            "Unexpected UTF-8 BOM (decode using utf-8-sig)",
            &chars,
            0,
        ));
    }
    let mut parser = Parser {
        chars: &chars,
        duplicate_message,
        depth: 0,
    };
    let start = parser.skip_space(0);
    let (value, end) = match parser.value(start) {
        Ok(found) => found,
        Err(Stop::Missing(at)) => return Err(decode_error("Expecting value", &chars, at)),
        Err(Stop::Fail(failure)) => return Err(failure),
    };
    let end = parser.skip_space(end);
    if end != chars.len() {
        return Err(decode_error("Extra data", &chars, end));
    }
    Ok(value)
}

fn decode_error(message: &str, chars: &[char], position: usize) -> JsonFailure {
    let before = &chars[..position.min(chars.len())];
    let line = before.iter().filter(|c| **c == '\n').count() + 1;
    let column = match before.iter().rposition(|c| *c == '\n') {
        Some(newline) => position - newline,
        None => position + 1,
    };
    JsonFailure::Decode(format!(
        "{message}: line {line} column {column} (char {position})"
    ))
}

/// `StopIteration` from the C scanner is "no value here"; everything else is final.
enum Stop {
    Missing(usize),
    Fail(JsonFailure),
}

struct Parser<'a> {
    chars: &'a [char],
    duplicate_message: Option<&'static str>,
    depth: usize,
}

impl Parser<'_> {
    fn at(&self, index: usize) -> Option<char> {
        self.chars.get(index).copied()
    }

    fn skip_space(&self, mut index: usize) -> usize {
        while matches!(self.at(index), Some(' ' | '\t' | '\n' | '\r')) {
            index += 1;
        }
        index
    }

    fn fail<T>(&self, message: &str, position: usize) -> Result<T, Stop> {
        Err(Stop::Fail(decode_error(message, self.chars, position)))
    }

    fn literal(&self, index: usize, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(offset, c)| self.at(index + offset) == Some(c))
    }

    fn value(&mut self, index: usize) -> Result<(PyValue, usize), Stop> {
        let length = self.chars.len();
        let Some(c) = self.at(index) else {
            return Err(Stop::Missing(index));
        };
        match c {
            '"' => self
                .string(index + 1)
                .map(|(text, end)| (PyValue::Str(text), end)),
            '{' => self.object(index + 1),
            '[' => self.array(index + 1),
            'n' if self.literal(index, "null") => Ok((PyValue::Null, index + 4)),
            't' if self.literal(index, "true") => Ok((PyValue::Bool(true), index + 4)),
            'f' if self.literal(index, "false") => Ok((PyValue::Bool(false), index + 5)),
            // The scanner's bounds checks are exact: the last character must exist.
            'N' if index + 2 < length && self.literal(index, "NaN") => {
                Ok((PyValue::Float(f64::NAN), index + 3))
            }
            'I' if index + 7 < length && self.literal(index, "Infinity") => {
                Ok((PyValue::Float(f64::INFINITY), index + 8))
            }
            '-' if index + 8 < length && self.literal(index, "-Infinity") => {
                Ok((PyValue::Float(f64::NEG_INFINITY), index + 9))
            }
            _ => self.number(index),
        }
    }

    fn number(&self, start: usize) -> Result<(PyValue, usize), Stop> {
        let digit = |index: usize| self.at(index).is_some_and(|c| c.is_ascii_digit());
        let mut index = start;
        if self.at(index) == Some('-') {
            index += 1;
            if index >= self.chars.len() {
                return Err(Stop::Missing(start));
            }
        }
        match self.at(index) {
            Some('1'..='9') => {
                index += 1;
                while digit(index) {
                    index += 1;
                }
            }
            Some('0') => index += 1,
            _ => return Err(Stop::Missing(start)),
        }
        let last = self.chars.len() - 1;
        let mut is_float = false;
        if index < last && self.at(index) == Some('.') && digit(index + 1) {
            is_float = true;
            index += 2;
            while digit(index) {
                index += 1;
            }
        }
        if index < last && matches!(self.at(index), Some('e' | 'E')) {
            let exponent_start = index;
            index += 1;
            if index < last && matches!(self.at(index), Some('-' | '+')) {
                index += 1;
            }
            while digit(index) {
                index += 1;
            }
            if digit(index - 1) {
                is_float = true;
            } else {
                index = exponent_start;
            }
        }
        let text: String = self.chars[start..index].iter().collect();
        if is_float {
            // Rust and CPython both round correctly, and both overflow to infinity.
            return Ok((PyValue::Float(text.parse().unwrap_or(f64::NAN)), index));
        }
        let digits = text.trim_start_matches('-').len();
        if digits > INT_DIGIT_LIMIT {
            return Err(Stop::Fail(JsonFailure::Value(format!(
                "Exceeds the limit ({INT_DIGIT_LIMIT} digits) for integer string conversion: value has {digits} digits; use sys.set_int_max_str_digits() to increase the limit"
            ))));
        }
        let value = match text.parse::<i64>() {
            Ok(small) => PyValue::Int(small),
            Err(_) => PyValue::BigInt(text),
        };
        Ok((value, index))
    }

    fn hex4(&self, from: usize) -> Option<u32> {
        let mut code = 0u32;
        for offset in 0..4 {
            code = (code << 4) | self.at(from + offset)?.to_digit(16)?;
        }
        Some(code)
    }

    /// `scanstring`: `from` is the index just after the opening quote.
    fn string(&self, from: usize) -> Result<(String, usize), Stop> {
        let length = self.chars.len();
        let begin = from - 1;
        let mut out = String::new();
        let mut index = from;
        loop {
            let mut next = index;
            let mut found = None;
            while next < length {
                let d = self.chars[next];
                if d == '"' || d == '\\' {
                    found = Some(d);
                    break;
                }
                if (d as u32) <= 0x1f {
                    return self.fail("Invalid control character at", next);
                }
                next += 1;
            }
            out.extend(&self.chars[index..next]);
            match found {
                Some('"') => return Ok((out, next + 1)),
                Some(_) => {}
                None => return self.fail("Unterminated string starting at", begin),
            }
            next += 1;
            if next == length {
                return self.fail("Unterminated string starting at", begin);
            }
            let escape = self.chars[next];
            if escape != 'u' {
                index = next + 1;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    _ => return self.fail("Invalid \\escape", index - 2),
                }
                continue;
            }
            let hex_start = next + 1;
            index = hex_start + 4;
            if index >= length {
                return self.fail("Invalid \\uXXXX escape", hex_start - 1);
            }
            let Some(mut code) = self.hex4(hex_start) else {
                return self.fail("Invalid \\uXXXX escape", index - 5);
            };
            if (0xD800..0xDC00).contains(&code)
                && index + 6 < length
                && self.at(index) == Some('\\')
                && self.at(index + 1) == Some('u')
            {
                let Some(low) = self.hex4(index + 2) else {
                    return self.fail("Invalid \\uXXXX escape", index + 1);
                };
                index += 6;
                if (0xDC00..0xE000).contains(&low) {
                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                } else {
                    index -= 6;
                }
            }
            out.push(char::from_u32(code).unwrap_or(LONE_SURROGATE));
        }
    }

    fn nested<T>(&mut self, body: impl FnOnce(&mut Self) -> Result<T, Stop>) -> Result<T, Stop> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(Stop::Fail(JsonFailure::Recursion));
        }
        let result = body(self);
        self.depth -= 1;
        result
    }

    fn array(&mut self, from: usize) -> Result<(PyValue, usize), Stop> {
        self.nested(|this| {
            let mut index = this.skip_space(from);
            let mut items = Vec::new();
            if this.at(index) != Some(']') {
                loop {
                    let (item, next) = this.value(index)?;
                    items.push(item);
                    index = this.skip_space(next);
                    if this.at(index) == Some(']') {
                        break;
                    }
                    if this.at(index) != Some(',') {
                        return this.fail("Expecting ',' delimiter", index);
                    }
                    index = this.skip_space(index + 1);
                }
            }
            Ok((PyValue::List(items), index + 1))
        })
    }

    fn object(&mut self, from: usize) -> Result<(PyValue, usize), Stop> {
        self.nested(|this| {
            let mut index = this.skip_space(from);
            let mut items: Vec<(String, PyValue)> = Vec::new();
            let mut duplicated = false;
            if this.at(index) != Some('}') {
                loop {
                    if this.at(index) != Some('"') {
                        return this
                            .fail("Expecting property name enclosed in double quotes", index);
                    }
                    let (key, next) = this.string(index + 1)?;
                    index = this.skip_space(next);
                    if this.at(index) != Some(':') {
                        return this.fail("Expecting ':' delimiter", index);
                    }
                    index = this.skip_space(index + 1);
                    let (value, next) = this.value(index)?;
                    duplicated |= items.iter().any(|(existing, _)| *existing == key);
                    PyValue::dict_insert(&mut items, key, value);
                    index = this.skip_space(next);
                    if this.at(index) == Some('}') {
                        break;
                    }
                    if this.at(index) != Some(',') {
                        return this.fail("Expecting ',' delimiter", index);
                    }
                    index = this.skip_space(index + 1);
                }
            }
            if duplicated && let Some(message) = this.duplicate_message {
                return Err(Stop::Fail(JsonFailure::Value(message.into())));
            }
            Ok((PyValue::Dict(items), index + 1))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(source: &str) -> String {
        match loads(source, None) {
            Err(JsonFailure::Decode(text)) => text,
            other => panic!("expected a decode error, got {other:?}"),
        }
    }

    #[test]
    fn errors_read_like_pythons() {
        assert_eq!(message(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(
            message("{\"a\": 1,}"),
            "Expecting property name enclosed in double quotes: line 1 column 9 (char 8)"
        );
        assert_eq!(message("[1] x"), "Extra data: line 1 column 5 (char 4)");
    }

    #[test]
    fn python_only_numbers_parse() {
        assert_eq!(
            loads("[NaN, -Infinity, 12345678901234567890]", None)
                .unwrap()
                .repr(),
            "[nan, -inf, 12345678901234567890]"
        );
    }
}
