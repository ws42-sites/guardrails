//! A JSON tree whose lists and mappings have identity, as Python's do.
//!
//! The response budget code removes items from lists it finds anywhere in a packet,
//! protects particular list objects, and sees one list twice when a packet holds the
//! same list under two keys (as `context` does for its relevant scopes). Plain
//! `serde_json::Value` copies on clone and cannot say that, so the packet is held as
//! shared, mutable nodes and compared by address.

use crate::py_value::float_repr;
use serde_json::{Number, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub type List = Rc<RefCell<Vec<J>>>;
pub type Dict = Rc<RefCell<Vec<(String, J)>>>;

#[derive(Clone, Debug)]
pub enum J {
    Null,
    Bool(bool),
    Number(Number),
    Str(String),
    List(List),
    Dict(Dict),
}

pub fn list(items: Vec<J>) -> J {
    J::List(Rc::new(RefCell::new(items)))
}

pub fn dict(items: Vec<(String, J)>) -> J {
    J::Dict(Rc::new(RefCell::new(items)))
}

impl J {
    /// A value as a tree; every list and mapping is a new object.
    pub fn from_value(value: &Value) -> J {
        match value {
            Value::Null => J::Null,
            Value::Bool(flag) => J::Bool(*flag),
            Value::Number(number) => J::Number(number.clone()),
            Value::String(text) => J::Str(text.clone()),
            Value::Array(items) => list(items.iter().map(J::from_value).collect()),
            Value::Object(map) => dict(
                map.iter()
                    .map(|(key, item)| (key.clone(), J::from_value(item)))
                    .collect(),
            ),
        }
    }

    /// The tree as a value; shared objects are written out wherever they appear.
    pub fn to_value(&self) -> Value {
        match self {
            J::Null => Value::Null,
            J::Bool(flag) => Value::Bool(*flag),
            J::Number(number) => Value::Number(number.clone()),
            J::Str(text) => Value::String(text.clone()),
            J::List(items) => Value::Array(items.borrow().iter().map(J::to_value).collect()),
            J::Dict(items) => {
                let mut map = serde_json::Map::new();
                for (key, item) in items.borrow().iter() {
                    map.insert(key.clone(), item.to_value());
                }
                Value::Object(map)
            }
        }
    }

    /// `copy.deepcopy`: sharing inside the tree is kept, sharing with the outside is not.
    pub fn deep_copy(&self) -> J {
        fn copy(value: &J, seen: &mut HashMap<usize, J>) -> J {
            match value {
                J::List(items) => {
                    let address = Rc::as_ptr(items) as usize;
                    if let Some(done) = seen.get(&address) {
                        return done.clone();
                    }
                    let fresh = Rc::new(RefCell::new(Vec::new()));
                    seen.insert(address, J::List(fresh.clone()));
                    let copied: Vec<J> =
                        items.borrow().iter().map(|item| copy(item, seen)).collect();
                    *fresh.borrow_mut() = copied;
                    J::List(fresh)
                }
                J::Dict(items) => {
                    let address = Rc::as_ptr(items) as usize;
                    if let Some(done) = seen.get(&address) {
                        return done.clone();
                    }
                    let fresh = Rc::new(RefCell::new(Vec::new()));
                    seen.insert(address, J::Dict(fresh.clone()));
                    let copied: Vec<(String, J)> = items
                        .borrow()
                        .iter()
                        .map(|(key, item)| (key.clone(), copy(item, seen)))
                        .collect();
                    *fresh.borrow_mut() = copied;
                    J::Dict(fresh)
                }
                other => other.clone(),
            }
        }
        copy(self, &mut HashMap::new())
    }

    pub fn as_dict(&self) -> Option<&Dict> {
        match self {
            J::Dict(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&List> {
        match self {
            J::List(items) => Some(items),
            _ => None,
        }
    }

    /// `dict.get(key)`.
    pub fn get(&self, key: &str) -> Option<J> {
        self.as_dict().and_then(|items| {
            items
                .borrow()
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, item)| item.clone())
        })
    }

    /// `dict[key] = value`: an existing key keeps its place.
    pub fn set(&self, key: &str, value: J) {
        if let Some(items) = self.as_dict() {
            let mut items = items.borrow_mut();
            match items.iter_mut().find(|(name, _)| name == key) {
                Some(slot) => slot.1 = value,
                None => items.push((key.to_owned(), value)),
            }
        }
    }

    /// `del dict[key]`; absent keys are ignored.
    pub fn remove(&self, key: &str) {
        if let Some(items) = self.as_dict() {
            items.borrow_mut().retain(|(name, _)| name != key);
        }
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn as_str(&self) -> Option<String> {
        match self {
            J::Str(text) => Some(text.clone()),
            _ => None,
        }
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            J::Null => false,
            J::Bool(flag) => *flag,
            J::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
            J::Str(text) => !text.is_empty(),
            J::List(items) => !items.borrow().is_empty(),
            J::Dict(items) => !items.borrow().is_empty(),
        }
    }

    /// The address of a list or mapping, which is what Python's `id` gives.
    pub fn identity(&self) -> Option<usize> {
        match self {
            J::List(items) => Some(Rc::as_ptr(items) as usize),
            J::Dict(items) => Some(Rc::as_ptr(items) as usize),
            _ => None,
        }
    }

    /// Python's equality of two scalars (and of the same object).
    pub fn same_scalar(&self, other: &J) -> bool {
        match (self, other) {
            (J::Null, J::Null) => true,
            (J::Bool(left), J::Bool(right)) => left == right,
            (J::Number(left), J::Number(right)) => left == right,
            (J::Str(left), J::Str(right)) => left == right,
            _ => false,
        }
    }

    /// `json.dumps(value)` with Python's defaults (`", "` and `": "`, ASCII only).
    pub fn dumps(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            J::Number(number) => {
                if number.is_f64() {
                    out.push_str(&float_repr(number.as_f64().unwrap_or(0.0)));
                } else {
                    out.push_str(&number.to_string());
                }
            }
            J::Str(text) => write_string(out, text),
            J::List(items) => {
                out.push('[');
                for (index, item) in items.borrow().iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    item.write(out);
                }
                out.push(']');
            }
            J::Dict(items) => {
                out.push('{');
                for (index, (key, item)) in items.borrow().iter().enumerate() {
                    if index > 0 {
                        out.push_str(", ");
                    }
                    write_string(out, key);
                    out.push_str(": ");
                    item.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// `json.dumps(text)`: escapes as Python does with `ensure_ascii`.
pub fn write_string(out: &mut String, text: &str) {
    use std::fmt::Write as _;
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

/// `json.dumps(value)` for a plain value: insertion order, Python's default separators.
pub fn dumps(value: &Value) -> String {
    J::from_value(value).dumps()
}
