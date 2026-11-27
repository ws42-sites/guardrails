//! Strict, pydantic-style validation, small enough to read.
//!
//! The Python contracts are pydantic models in strict mode with unknown fields
//! forbidden. Callers see which field failed and why, so each issue keeps pydantic's
//! location, error type and message. Only those three are the contract; pydantic's
//! echo of the offending input and its documentation link are not reproduced.

use crate::py_value::{PyValue, str_repr};
use regex::Regex;
use serde_json::{Map, Number, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loc {
    Key(String),
    Index(usize),
}

/// One validation failure, as `ValidationError.errors()` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub loc: Vec<Loc>,
    pub kind: String,
    pub message: String,
}

impl Issue {
    /// `[loc, type, msg]`, the shape the parity recordings use.
    pub fn to_value(&self) -> Value {
        let loc = self
            .loc
            .iter()
            .map(|part| match part {
                Loc::Key(key) => Value::String(key.clone()),
                Loc::Index(index) => Value::Number((*index as u64).into()),
            })
            .collect();
        Value::Array(vec![
            Value::Array(loc),
            Value::String(self.kind.clone()),
            Value::String(self.message.clone()),
        ])
    }
}

/// How a missing field is filled.
pub enum Fill {
    Required,
    Value(Value),
}

pub struct Spec {
    pub name: &'static str,
    pub default: Fill,
}

pub fn required(name: &'static str) -> Spec {
    Spec {
        name,
        default: Fill::Required,
    }
}

pub fn optional(name: &'static str, default: Value) -> Spec {
    Spec {
        name,
        default: Fill::Value(default),
    }
}

/// Bounds for a string field.
#[derive(Clone, Copy, Default)]
pub struct StrRule {
    pub min: Option<usize>,
    pub max: Option<usize>,
    pub pattern: Option<&'static str>,
}

pub const fn text(min: usize, max: usize) -> StrRule {
    StrRule {
        min: Some(min),
        max: Some(max),
        pattern: None,
    }
}

pub const fn text_max(max: usize) -> StrRule {
    StrRule {
        min: None,
        max: Some(max),
        pattern: None,
    }
}

pub const ANY_TEXT: StrRule = StrRule {
    min: None,
    max: None,
    pattern: None,
};

pub const IDENTIFIER: StrRule = StrRule {
    min: Some(1),
    max: Some(128),
    pattern: Some("^[A-Za-z0-9_.:-]+$"),
};

pub const SHA256: StrRule = StrRule {
    min: None,
    max: None,
    pattern: Some("^[0-9a-f]{64}$"),
};

/// Compile a pattern the way pydantic does: the `regex` crate, so `$` is the true end.
fn matches_pattern(pattern: &str, value: &str) -> bool {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};
    static CACHE: LazyLock<Mutex<HashMap<String, Regex>>> = LazyLock::new(Default::default);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let regex = cache
        .entry(pattern.to_owned())
        .or_insert_with(|| Regex::new(pattern).expect("the patterns are fixed and valid"));
    regex.is_match(value)
}

pub struct Validator {
    pub issues: Vec<Issue>,
    path: Vec<Loc>,
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new()
    }
}

impl Validator {
    pub fn new() -> Self {
        Self {
            issues: Vec::new(),
            path: Vec::new(),
        }
    }

    pub fn at<T>(&mut self, loc: Loc, body: impl FnOnce(&mut Self) -> T) -> T {
        self.path.push(loc);
        let result = body(self);
        self.path.pop();
        result
    }

    pub fn field<T>(&mut self, name: &str, body: impl FnOnce(&mut Self) -> T) -> T {
        self.at(Loc::Key(name.to_owned()), body)
    }

    /// Record an issue at the current location.
    pub fn issue(&mut self, kind: &str, message: impl Into<String>) {
        self.issues.push(Issue {
            loc: self.path.clone(),
            kind: kind.into(),
            message: message.into(),
        });
    }

    /// An issue from a validator function that raised `ValueError`.
    pub fn value_error(&mut self, message: &str) {
        self.issue("value_error", format!("Value error, {message}"));
    }

    pub fn is_clean_since(&self, mark: usize) -> bool {
        self.issues.len() == mark
    }

    pub fn str_field(&mut self, input: &PyValue, rule: StrRule) -> Option<String> {
        let PyValue::Str(text) = input else {
            self.issue("string_type", "Input should be a valid string");
            return None;
        };
        if text.contains(crate::py_json::LONE_SURROGATE) {
            self.issue(
                "string_unicode",
                "Input should be a valid string, unable to parse raw data as a unicode string",
            );
            return None;
        }
        let length = text.chars().count();
        if let Some(min) = rule.min
            && length < min
        {
            self.issue(
                "string_too_short",
                format!("String should have at least {min} character{}", plural(min)),
            );
            return None;
        }
        if let Some(max) = rule.max
            && length > max
        {
            self.issue(
                "string_too_long",
                format!("String should have at most {max} character{}", plural(max)),
            );
            return None;
        }
        if let Some(pattern) = rule.pattern
            && !matches_pattern(pattern, text)
        {
            self.issue(
                "string_pattern_mismatch",
                format!("String should match pattern '{pattern}'"),
            );
            return None;
        }
        Some(text.clone())
    }

    /// A nullable string: `None` is a valid value.
    pub fn nullable_str(&mut self, input: &PyValue, rule: StrRule) -> Option<Value> {
        if matches!(input, PyValue::Null) {
            return Some(Value::Null);
        }
        self.str_field(input, rule).map(Value::String)
    }

    /// A strict integer within optional bounds; `bool` is not an integer here.
    pub fn int_field(&mut self, input: &PyValue, ge: Option<i64>, le: Option<i64>) -> Option<i64> {
        let value = match input {
            PyValue::Int(int) => *int,
            PyValue::BigInt(digits) => {
                // Out of range for any bound used here.
                if digits.starts_with('-') {
                    if let Some(ge) = ge {
                        self.issue(
                            "greater_than_equal",
                            format!("Input should be greater than or equal to {ge}"),
                        );
                        return None;
                    }
                    return Some(i64::MIN);
                }
                if let Some(le) = le {
                    self.issue(
                        "less_than_equal",
                        format!("Input should be less than or equal to {le}"),
                    );
                    return None;
                }
                return Some(i64::MAX);
            }
            _ => {
                self.issue("int_type", "Input should be a valid integer");
                return None;
            }
        };
        if let Some(ge) = ge
            && value < ge
        {
            self.issue(
                "greater_than_equal",
                format!("Input should be greater than or equal to {ge}"),
            );
            return None;
        }
        if let Some(le) = le
            && value > le
        {
            self.issue(
                "less_than_equal",
                format!("Input should be less than or equal to {le}"),
            );
            return None;
        }
        Some(value)
    }

    pub fn nullable_int(
        &mut self,
        input: &PyValue,
        ge: Option<i64>,
        le: Option<i64>,
    ) -> Option<Value> {
        if matches!(input, PyValue::Null) {
            return Some(Value::Null);
        }
        self.int_field(input, ge, le)
            .map(|int| Value::Number(int.into()))
    }

    /// A strict float: integers are accepted and become floats, `NaN` and infinities are not.
    pub fn float_field(&mut self, input: &PyValue, ge: f64, le: f64) -> Option<Value> {
        let value = match input {
            PyValue::Float(float) => *float,
            PyValue::Int(int) => *int as f64,
            PyValue::BigInt(digits) => digits.parse().unwrap_or(f64::INFINITY),
            _ => {
                self.issue("float_type", "Input should be a valid number");
                return None;
            }
        };
        if !value.is_finite() {
            self.issue("finite_number", "Input should be a finite number");
            return None;
        }
        if value < ge {
            self.issue(
                "greater_than_equal",
                format!("Input should be greater than or equal to {}", bound(ge)),
            );
            return None;
        }
        if value > le {
            self.issue(
                "less_than_equal",
                format!("Input should be less than or equal to {}", bound(le)),
            );
            return None;
        }
        Number::from_f64(value).map(Value::Number)
    }

    pub fn bool_field(&mut self, input: &PyValue) -> Option<bool> {
        match input {
            PyValue::Bool(flag) => Some(*flag),
            _ => {
                self.issue("bool_type", "Input should be a valid boolean");
                None
            }
        }
    }

    /// One of a fixed set of strings.
    pub fn literal(&mut self, input: &PyValue, allowed: &[&str]) -> Option<String> {
        if let PyValue::Str(text) = input
            && allowed.contains(&text.as_str())
        {
            return Some(text.clone());
        }
        let shown: Vec<String> = allowed.iter().map(|item| str_repr(item)).collect();
        self.issue(
            "literal_error",
            format!("Input should be {}", join_alternatives(&shown)),
        );
        None
    }

    /// A list whose items `each` validates; `None` when any item or the length fails.
    pub fn list_field(
        &mut self,
        input: &PyValue,
        min: Option<usize>,
        max: Option<usize>,
        mut each: impl FnMut(&mut Self, &PyValue) -> Option<Value>,
    ) -> Option<Vec<Value>> {
        let PyValue::List(items) = input else {
            self.issue("list_type", "Input should be a valid list");
            return None;
        };
        let mark = self.issues.len();
        let mut out = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if let Some(done) = self.at(Loc::Index(index), |this| each(this, item)) {
                out.push(done);
            }
        }
        if !self.is_clean_since(mark) {
            return None;
        }
        if let Some(min) = min
            && items.len() < min
        {
            self.issue(
                "too_short",
                format!(
                    "List should have at least {min} item{} after validation, not {}",
                    plural(min),
                    items.len()
                ),
            );
            return None;
        }
        if let Some(max) = max
            && items.len() > max
        {
            self.issue(
                "too_long",
                format!(
                    "List should have at most {max} item{} after validation, not {}",
                    plural(max),
                    items.len()
                ),
            );
            return None;
        }
        Some(out)
    }

    pub fn str_list(
        &mut self,
        input: &PyValue,
        min: Option<usize>,
        max: Option<usize>,
        rule: StrRule,
    ) -> Option<Vec<Value>> {
        self.list_field(input, min, max, |this, item| {
            this.str_field(item, rule).map(Value::String)
        })
    }

    /// A model's fields in declaration order, with defaults filled and unknown keys refused.
    /// `each` validates one present field and returns its dumped value.
    pub fn model(
        &mut self,
        name: &str,
        input: &PyValue,
        specs: &[Spec],
        mut each: impl FnMut(&mut Self, &str, &PyValue) -> Option<Value>,
    ) -> Option<Map<String, Value>> {
        let PyValue::Dict(entries) = input else {
            self.issue(
                "model_type",
                format!("Input should be a valid dictionary or instance of {name}"),
            );
            return None;
        };
        let mark = self.issues.len();
        let mut out = Map::new();
        for spec in specs {
            match entries.iter().find(|(key, _)| key == spec.name) {
                Some((_, value)) => {
                    if let Some(done) = self.field(spec.name, |this| each(this, spec.name, value)) {
                        out.insert(spec.name.to_owned(), done);
                    }
                }
                None => match &spec.default {
                    Fill::Required => {
                        self.field(spec.name, |this| this.issue("missing", "Field required"))
                    }
                    Fill::Value(value) => {
                        out.insert(spec.name.to_owned(), value.clone());
                    }
                },
            }
        }
        for (key, _) in entries {
            if !specs.iter().any(|spec| spec.name == key) {
                self.field(key, |this| {
                    this.issue("extra_forbidden", "Extra inputs are not permitted")
                });
            }
        }
        self.is_clean_since(mark).then_some(out)
    }
}

fn bound(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

/// `'a'`, `'a' or 'b'`, `'a', 'b' or 'c'`: how pydantic lists expected literals.
pub fn join_alternatives(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// `datetime.date.fromisoformat` for a string that already matches `\d{4}-\d{2}-\d{2}`.
/// The error text is CPython's.
pub fn check_iso_date(text: &str) -> Result<(), String> {
    let invalid = || format!("Invalid isoformat string: {}", str_repr(text));
    let bytes = text.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    let number = |range: std::ops::Range<usize>| -> u32 { text[range].parse().unwrap_or(0) };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    if year < 1 {
        return Err(format!("year {year} is out of range"));
    }
    if !(1..=12).contains(&month) {
        return Err("month must be in 1..12".into());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let longest = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > longest {
        return Err("day is out of range for month".into());
    }
    Ok(())
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The ISO weekday, Monday as 1, of a count of days since 1970-01-01 (a Thursday).
fn iso_weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7) + 1
}

/// Whether an ISO year has 53 weeks.
fn has_53_weeks(year: i64) -> bool {
    let january_first = iso_weekday(days_from_civil(year, 1, 1));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    january_first == 4 || (leap && january_first == 3)
}

/// `datetime.date.fromisoformat`: `YYYY-MM-DD`, `YYYYMMDD` and the ISO week forms
/// `YYYY-Www-D`, `YYYYWwwD`, `YYYY-Www` and `YYYYWww`, as `(year, month, day)`.
/// The error text is CPython's.
pub fn parse_iso_date(text: &str) -> Result<(u32, u32, u32), String> {
    let invalid = || format!("Invalid isoformat string: {}", str_repr(text));
    let bytes = text.as_bytes();
    if !text.is_ascii() || !matches!(bytes.len(), 7 | 8 | 10) {
        return Err(invalid());
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())?
    };
    let year = digits(0..4).ok_or_else(invalid)?;
    let week_form = text
        .get(4..)
        .is_some_and(|rest| rest.starts_with('W') || rest.starts_with("-W"));
    if week_form {
        let dash = bytes[4] == b'-';
        let start = if dash { 6 } else { 5 };
        let week = digits(start..start + 2).ok_or_else(invalid)?;
        let after = start + 2;
        let day = if after == bytes.len() {
            1
        } else if dash && bytes.get(after) == Some(&b'-') && after + 2 == bytes.len() {
            digits(after + 1..after + 2).ok_or_else(invalid)?
        } else if !dash && after + 1 == bytes.len() {
            digits(after..after + 1).ok_or_else(invalid)?
        } else {
            return Err(invalid());
        };
        let longest = if has_53_weeks(year) { 53 } else { 52 };
        if year < 1 || !(1..=longest).contains(&week) || !(1..=7).contains(&day) {
            return Err(invalid());
        }
        let january_fourth = days_from_civil(year, 1, 4);
        let monday = january_fourth - (iso_weekday(january_fourth) - 1);
        let (y, m, d) = civil_from_days(monday + (week - 1) * 7 + (day - 1));
        return Ok((y as u32, m as u32, d as u32));
    }
    let (month, day) = if bytes.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
        (digits(5..7), digits(8..10))
    } else if bytes.len() == 8 {
        (digits(4..6), digits(6..8))
    } else {
        return Err(invalid());
    };
    let (Some(month), Some(day)) = (month, day) else {
        return Err(invalid());
    };
    if year < 1 {
        return Err(format!("year {year} is out of range"));
    }
    if !(1..=12).contains(&month) {
        return Err("month must be in 1..12".into());
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let longest = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day < 1 || day > longest {
        return Err("day is out of range for month".into());
    }
    Ok((year as u32, month as u32, day as u32))
}
