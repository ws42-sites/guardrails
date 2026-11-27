//! Small Python-shaped helpers the analysis modules share.

use super::PyError;
use crate::py_value::{PyValue, str_repr};
use crate::text;
use serde_json::Value;

/// `value[key]` on a decoded mapping: `KeyError` when absent.
pub fn get<'a>(value: &'a Value, key: &str) -> Result<&'a Value, PyError> {
    match value {
        Value::Object(map) => map
            .get(key)
            .ok_or_else(|| PyError::new("KeyError", str_repr(key))),
        other => Err(PyError::new("TypeError", not_subscriptable(other))),
    }
}

/// The `TypeError` text for `value["key"]` on something that is not a mapping.
pub fn not_subscriptable(value: &Value) -> String {
    match value {
        Value::String(_) => "string indices must be integers, not 'str'".into(),
        Value::Array(_) => "list indices must be integers or slices, not str".into(),
        other => format!(
            "'{}' object is not subscriptable",
            PyValue::from_value(other).type_name()
        ),
    }
}

/// `value.get(key)`: `None` for an absent key or for something that is not a mapping.
pub fn lookup<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.as_object().and_then(|map| map.get(key))
}

/// `value.get(key, default)`.
pub fn lookup_or<'a>(value: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    lookup(value, key).unwrap_or(default)
}

pub fn array(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

pub fn text_of(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// Python truthiness of a decoded value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|float| float != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// `PurePosixPath(path).name`: the last component, ignoring empty and `.` components.
pub fn posix_name(path: &str) -> &str {
    path.split('/')
        .rfind(|part| !part.is_empty() && *part != ".")
        .unwrap_or("")
}

/// `PurePosixPath(path).name.lower()`.
pub fn lower_name(path: &str) -> String {
    text::lower(posix_name(path))
}

/// `str(PurePosixPath(path).parent)`: everything before the last component, `.` when none.
pub fn posix_parent(path: &str) -> String {
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    let rooted = path.starts_with('/');
    match parts.split_last() {
        Some((_, rest)) if !rest.is_empty() => {
            format!("{}{}", if rooted { "/" } else { "" }, rest.join("/"))
        }
        _ if rooted => "/".into(),
        _ => ".".into(),
    }
}

/// `int(value)` for a decoded value, with CPython's errors.
pub fn py_int(value: &Value) -> Result<i64, PyError> {
    match value {
        Value::Bool(flag) => Ok(i64::from(*flag)),
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float as i64))
            .ok_or_else(|| PyError::value("int too large")),
        Value::String(text) => {
            let trimmed = text::strip(text);
            let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
            let clean = !digits.is_empty()
                && !digits.starts_with('_')
                && !digits.ends_with('_')
                && !digits.contains("__")
                && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
            if clean {
                trimmed
                    .replace('_', "")
                    .parse::<i64>()
                    .map_err(|_| PyError::value("int too large"))
            } else {
                Err(PyError::value(format!(
                    "invalid literal for int() with base 10: {}",
                    str_repr(text)
                )))
            }
        }
        other => Err(PyError::new(
            "TypeError",
            format!(
                "int() argument must be a string, a bytes-like object or a real number, not '{}'",
                PyValue::from_value(other).type_name()
            ),
        )),
    }
}
