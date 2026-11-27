//! JSON text as Python's `json.dumps` and `json.loads` produce and read it.

use crate::error::{Result, StoreError};
use codegraph_engine::architecture::tree::dumps;
use codegraph_engine::py_json::{self, JsonFailure};
use serde_json::{Map, Value};

fn sorted(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            // Byte order of UTF-8 is code point order, which is how Python sorts str keys.
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let mut out = Map::new();
            for (key, item) in entries {
                out.insert(key.clone(), sorted(item));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sorted).collect()),
        other => other.clone(),
    }
}

/// `json.dumps(value, sort_keys=True, allow_nan=False)`.
pub fn dumps_sorted(value: &Value) -> String {
    dumps(&sorted(value))
}

/// `json.dumps(value, allow_nan=False)`: keys stay in insertion order.
pub fn dumps_ordered(value: &Value) -> String {
    dumps(value)
}

/// `json.loads(text)`, as a `serde_json` value. A number `serde_json` cannot carry
/// exactly (a huge integer, NaN) is refused, which Python would not do.
pub fn loads(text: &str) -> Result<Value> {
    let decoded = py_json::loads_decoded(text, None).map_err(|failure| match failure {
        JsonFailure::Decode(message) => StoreError::new("JSONDecodeError", message),
        JsonFailure::Value(message) => StoreError::value(&message),
        JsonFailure::Recursion => StoreError::new("RecursionError", "maximum recursion depth"),
    })?;
    decoded
        .to_value()
        .ok_or_else(|| StoreError::value("Stored JSON holds a number outside the supported range"))
}
