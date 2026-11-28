//! `tomllib.loads` on the `toml` crate: TOML 1.0, tables in document order, dates as
//! Python prints them.

use crate::py_value::PyValue;

/// Nesting `tomllib` accepts from a shallow caller: inline tables stop at 330 levels, arrays at
/// 496 (Windows, CPython 3.12). One figure serves both.
pub const MAX_NESTING: usize = 330;

/// The deepest `[` or `{` nesting outside strings and comments. The parser below has no
/// recursion limit of its own, so hostile depth is refused here before it can run.
fn nesting(source: &str) -> usize {
    let chars: Vec<char> = source.chars().collect();
    let (mut depth, mut deepest, mut at) = (0usize, 0usize, 0usize);
    while at < chars.len() {
        let c = chars[at];
        match c {
            '#' => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
                continue;
            }
            '"' | '\'' => {
                let triple = chars.get(at + 1) == Some(&c) && chars.get(at + 2) == Some(&c);
                at += if triple { 3 } else { 1 };
                while at < chars.len() {
                    if c == '"' && chars[at] == '\\' {
                        at += 2;
                        continue;
                    }
                    let closes = chars[at] == c
                        && (!triple
                            || (chars.get(at + 1) == Some(&c) && chars.get(at + 2) == Some(&c)));
                    if closes {
                        at += if triple { 3 } else { 1 };
                        break;
                    }
                    if !triple && chars[at] == '\n' {
                        break;
                    }
                    at += 1;
                }
                continue;
            }
            '[' | '{' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        at += 1;
    }
    deepest
}

/// Parse TOML text. Any failure is one error: callers treat every `ValueError`,
/// `TypeError` and `RecursionError` from `tomllib` the same way.
pub fn loads(source: &str) -> Result<PyValue, String> {
    // `tomllib` has no byte order mark rule: it is not valid TOML text.
    if source.starts_with('\u{feff}') {
        return Err("Invalid statement".into());
    }
    if nesting(source) > MAX_NESTING {
        return Err("maximum recursion depth exceeded".into());
    }
    // A roomy stack: the parser recurses once per level of nesting.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 * 1024 * 1024)
            .spawn_scoped(scope, || {
                source
                    .parse::<toml::Table>()
                    .map(|table| convert(&toml::Value::Table(table)))
                    .map_err(|error| error.to_string())
            })
            .map_err(|error| error.to_string())
            .and_then(|handle| handle.join().unwrap_or_else(|_| Err("panicked".into())))
    })
}

fn convert(value: &toml::Value) -> PyValue {
    match value {
        toml::Value::String(text) => PyValue::Str(text.clone()),
        toml::Value::Integer(int) => PyValue::Int(*int),
        toml::Value::Float(float) => PyValue::Float(*float),
        toml::Value::Boolean(flag) => PyValue::Bool(*flag),
        toml::Value::Datetime(found) => datetime(found),
        toml::Value::Array(items) => PyValue::List(items.iter().map(convert).collect()),
        toml::Value::Table(table) => PyValue::Dict(
            table
                .iter()
                .map(|(key, item)| (key.clone(), convert(item)))
                .collect(),
        ),
    }
}

/// `repr` and `str` of the `datetime.date`, `datetime.time` or `datetime.datetime` tomllib builds.
fn datetime(found: &toml::value::Datetime) -> PyValue {
    let date = found.date.map(|date| (date.year, date.month, date.day));
    let time = found.time.map(|time| {
        // tomllib keeps microseconds: further digits are dropped.
        (time.hour, time.minute, time.second, time.nanosecond / 1000)
    });
    let time_text = |(hour, minute, second, micro): (u8, u8, u8, u32)| {
        if micro == 0 {
            format!("{hour:02}:{minute:02}:{second:02}")
        } else {
            format!("{hour:02}:{minute:02}:{second:02}.{micro:06}")
        }
    };
    // `datetime.time(12, 30)` leaves out trailing zero fields.
    let time_args = |(hour, minute, second, micro): (u8, u8, u8, u32)| {
        if micro != 0 {
            format!("{hour}, {minute}, {second}, {micro}")
        } else if second != 0 {
            format!("{hour}, {minute}, {second}")
        } else {
            format!("{hour}, {minute}")
        }
    };
    match (date, time) {
        (Some((year, month, day)), None) => PyValue::Opaque {
            text: format!("{year:04}-{month:02}-{day:02}"),
            repr: format!("datetime.date({year}, {month}, {day})"),
        },
        (None, Some(clock)) => PyValue::Opaque {
            text: time_text(clock),
            repr: format!("datetime.time({})", time_args(clock)),
        },
        (Some((year, month, day)), Some(clock)) => {
            let seconds = match found.offset {
                None => None,
                Some(toml::value::Offset::Z) => Some(0i64),
                Some(toml::value::Offset::Custom { minutes }) => Some(i64::from(minutes) * 60),
            };
            let zone_text = seconds.map_or(String::new(), |total| {
                let sign = if total < 0 { '-' } else { '+' };
                let magnitude = total.abs();
                format!("{sign}{:02}:{:02}", magnitude / 3600, magnitude % 3600 / 60)
            });
            let zone_repr = seconds.map_or(String::new(), |total| {
                if total == 0 {
                    ", tzinfo=datetime.timezone.utc".to_owned()
                } else {
                    // timedelta normalises to days and non-negative seconds.
                    let days = total.div_euclid(86400);
                    let rest = total.rem_euclid(86400);
                    let mut parts = Vec::new();
                    if days != 0 {
                        parts.push(format!("days={days}"));
                    }
                    if rest != 0 {
                        parts.push(format!("seconds={rest}"));
                    }
                    format!(
                        ", tzinfo=datetime.timezone(datetime.timedelta({}))",
                        parts.join(", ")
                    )
                }
            });
            PyValue::Opaque {
                text: format!(
                    "{year:04}-{month:02}-{day:02} {}{zone_text}",
                    time_text(clock)
                ),
                repr: format!(
                    "datetime.datetime({year}, {month}, {day}, {}{zone_repr})",
                    time_args(clock)
                ),
            }
        }
        (None, None) => PyValue::Opaque {
            text: String::new(),
            repr: String::new(),
        },
    }
}
