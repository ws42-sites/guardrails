//! A Python-shaped value, for data that `json` and `tomllib` produce.
//!
//! `serde_json::Value` cannot hold what Python's decoders can: `NaN`, `Infinity`,
//! integers of any size. Config parsing and contract validation need to tell those
//! apart, and `str()` of a parsed value must read as Python prints it.

use crate::text;
use serde_json::{Number, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum PyValue {
    Null,
    Bool(bool),
    Int(i64),
    /// An integer outside `i64`, as its decimal digits (with a leading `-` if negative).
    BigInt(String),
    Float(f64),
    Str(String),
    List(Vec<PyValue>),
    /// Insertion order, unique keys: a repeated key keeps its first position and its last value.
    Dict(Vec<(String, PyValue)>),
    /// A value `tomllib` builds that JSON has no word for (a date or time), as `str` and `repr` show it.
    Opaque {
        text: String,
        repr: String,
    },
}

impl PyValue {
    pub fn get(&self, key: &str) -> Option<&PyValue> {
        match self {
            Self::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Self::Dict(_))
    }

    /// Insert as a Python dict would.
    pub fn dict_insert(items: &mut Vec<(String, PyValue)>, key: String, value: PyValue) {
        match items.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => items.push((key, value)),
        }
    }

    /// Python's type name, as pydantic and error messages use it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "NoneType",
            Self::Bool(_) => "bool",
            Self::Int(_) | Self::BigInt(_) => "int",
            Self::Float(_) => "float",
            Self::Str(_) => "str",
            Self::List(_) => "list",
            Self::Dict(_) => "dict",
            Self::Opaque { .. } => "datetime",
        }
    }

    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(flag) => Self::Bool(*flag),
            Value::Number(number) => {
                if let Some(int) = number.as_i64() {
                    Self::Int(int)
                } else if let Some(big) = number.as_u64() {
                    Self::BigInt(big.to_string())
                } else {
                    Self::Float(number.as_f64().unwrap_or(f64::NAN))
                }
            }
            Value::String(text) => Self::Str(text.clone()),
            Value::Array(items) => Self::List(items.iter().map(Self::from_value).collect()),
            Value::Object(map) => Self::Dict(
                map.iter()
                    .map(|(key, item)| (key.clone(), Self::from_value(item)))
                    .collect(),
            ),
        }
    }

    /// `None` when the value holds `NaN`, an infinity or an integer that JSON
    /// numbers here cannot carry exactly.
    pub fn to_value(&self) -> Option<Value> {
        Some(match self {
            Self::Null => Value::Null,
            Self::Bool(flag) => Value::Bool(*flag),
            Self::Int(int) => Value::Number((*int).into()),
            Self::BigInt(text) => Value::Number(text.parse::<u64>().ok()?.into()),
            Self::Float(float) => Value::Number(Number::from_f64(*float)?),
            Self::Str(text) => Value::String(text.clone()),
            Self::Opaque { .. } => return None,
            Self::List(items) => Value::Array(
                items
                    .iter()
                    .map(Self::to_value)
                    .collect::<Option<Vec<_>>>()?,
            ),
            Self::Dict(items) => {
                let mut map = serde_json::Map::new();
                for (key, item) in items {
                    map.insert(key.clone(), item.to_value()?);
                }
                Value::Object(map)
            }
        })
    }

    /// `repr(value)`.
    pub fn repr(&self) -> String {
        match self {
            Self::Null => "None".into(),
            Self::Bool(true) => "True".into(),
            Self::Bool(false) => "False".into(),
            Self::Int(int) => int.to_string(),
            Self::BigInt(digits) => digits.clone(),
            Self::Float(float) => float_repr(*float),
            Self::Str(text) => str_repr(text),
            Self::Opaque { repr, .. } => repr.clone(),
            Self::List(items) => {
                let parts: Vec<String> = items.iter().map(Self::repr).collect();
                format!("[{}]", parts.join(", "))
            }
            Self::Dict(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|(key, item)| format!("{}: {}", str_repr(key), item.repr()))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
        }
    }

    /// `str(value)`: a string is itself, everything else prints as its repr.
    pub fn py_str(&self) -> String {
        match self {
            Self::Str(text) => text.clone(),
            Self::Opaque { text, .. } => text.clone(),
            other => other.repr(),
        }
    }
}

/// `repr(float)`: shortest digits that round-trip, in fixed or exponent form as CPython picks.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exponent: i32 = exponent.parse().unwrap_or(0);
    // Position of the decimal point relative to the first digit.
    let decimal_point = exponent + 1;
    let sign = if value < 0.0 { "-" } else { "" };
    if !(-3..=16).contains(&decimal_point) {
        let mut out = String::from(sign);
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let shown = decimal_point - 1;
        out.push_str(&format!(
            "e{}{:02}",
            if shown < 0 { '-' } else { '+' },
            shown.abs()
        ));
        return out;
    }
    let mut out = String::from(sign);
    if decimal_point <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decimal_point) as usize));
        out.push_str(&digits);
    } else if digits.len() as i32 <= decimal_point {
        out.push_str(&digits);
        out.push_str(&"0".repeat((decimal_point - digits.len() as i32) as usize));
        out.push_str(".0");
    } else {
        out.push_str(&digits[..decimal_point as usize]);
        out.push('.');
        out.push_str(&digits[decimal_point as usize..]);
    }
    out
}

/// Python's `str.isprintable` for one character.
pub fn printable(c: char) -> bool {
    c == ' ' || !(text::is_other(c) || text::is_space(c))
}

/// `repr(str)`.
pub fn str_repr(value: &str) -> String {
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if printable(c) => out.push(c),
            c if (c as u32) < 0x100 => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_python_does() {
        for (value, shown) in [
            (1.0, "1.0"),
            (0.1, "0.1"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e300, "1.5e+300"),
            (-2.5, "-2.5"),
            (123456.789, "123456.789"),
        ] {
            assert_eq!(float_repr(value), shown);
        }
    }

    #[test]
    fn strings_choose_their_quotes() {
        assert_eq!(str_repr("a'b"), "\"a'b\"");
        assert_eq!(str_repr("a'b\""), "'a\\'b\"'");
        assert_eq!(str_repr("tab\there\u{7f}"), "'tab\\there\\x7f'");
    }
}

/// `format(value, "g")`: six significant digits, trailing zeros dropped, exponent form
/// below `1e-4` and from `1e6` up.
pub fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    const PRECISION: i32 = 6;
    // Rounded to the precision first: rounding can carry into the next power of ten.
    let scientific = format!("{:.*e}", (PRECISION - 1) as usize, value);
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |text: &str| -> String {
        if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        } else {
            text.to_owned()
        }
    };
    if !(-4..PRECISION).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{}e{sign}{:02}", trim(mantissa), exponent.abs());
    }
    let decimals = (PRECISION - 1 - exponent).max(0) as usize;
    trim(&format!("{value:.decimals$}"))
}

#[cfg(test)]
mod g_tests {
    use super::format_g;

    #[test]
    fn matches_python_g() {
        for (value, shown) in [
            (3.0, "3"),
            (7.5, "7.5"),
            (123456.0, "123456"),
            (1234567.0, "1.23457e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (100000.0, "100000"),
            (999999.5, "1e+06"),
            (0.1 + 0.2, "0.3"),
            (-2.5, "-2.5"),
        ] {
            assert_eq!(format_g(value), shown);
        }
    }
}
