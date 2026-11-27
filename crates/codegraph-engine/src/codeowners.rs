//! Pure, bounded CODEOWNERS parsing and matching; the file is data, never executed.
//!
//! Port of `codegraph.codeowners`.

use crate::text;
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

pub const CANDIDATES: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_RULES: usize = 2000;
pub const MAX_OWNERS: usize = 16;
pub const MAX_TOKEN: usize = 128;
pub const MAX_PATTERN: usize = 256;
pub const MAX_DOUBLE_STARS: usize = 4;

static OWNER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\A(?:@[A-Za-z0-9][A-Za-z0-9_.-]*|@[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9_][A-Za-z0-9_.-]*|[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)\z",
    )
    .expect("owner pattern is valid")
});

/// `valid_owner`: a handle, a team or an email address of bounded length.
pub fn valid_owner(token: &str) -> bool {
    let length = text::char_len(token);
    0 < length && length <= MAX_TOKEN && OWNER.is_match(token)
}

/// A compiled CODEOWNERS pattern. The expression carries Python's `$`, which also
/// matches before one final newline, so path lookups behave the same way.
fn compile(pattern: &str) -> Option<Regex> {
    let length = text::char_len(pattern);
    let bytes = pattern.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if !(0 < length && length <= MAX_PATTERN)
        || pattern.chars().any(|c| (c as u32) < 32)
        || pattern.contains("..")
        || pattern.starts_with("//")
        || pattern.starts_with('!')
        || pattern.starts_with('~')
        || drive
        || pattern.contains(['[', ']'])
        || pattern.matches("**").count() > MAX_DOUBLE_STARS
    {
        return None;
    }
    let mut body = pattern;
    let mut anchored = body.starts_with('/');
    if anchored {
        body = &body[1..];
    }
    let directory_only = body.ends_with('/');
    if directory_only {
        body = &body[..body.len() - 1];
    }
    if body.is_empty() || body.starts_with('/') || body.contains("//") {
        return None;
    }
    anchored = anchored || body.contains('/');
    let chars: Vec<char> = body.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    let starts_with_at = |index: usize, needle: &str| {
        let needle: Vec<char> = needle.chars().collect();
        chars.get(index..index + needle.len()) == Some(needle.as_slice())
    };
    while index < chars.len() {
        let c = chars[index];
        if index == 0 && starts_with_at(0, "**/") {
            out.push_str("(?:.*/)?");
            index += 3;
        } else if starts_with_at(index, "/**/") {
            out.push_str("/(?:.*/)?");
            index += 4;
        } else if starts_with_at(index, "/**") && index + 3 == chars.len() {
            out.push_str("/.+");
            index += 3;
        } else if c == '*' {
            while index < chars.len() && chars[index] == '*' {
                index += 1;
            }
            out.push_str("[^/]*");
        } else if c == '?' {
            out.push_str("[^/]");
            index += 1;
        } else if c == '\\' && index + 1 < chars.len() {
            out.push_str(&regex::escape(&chars[index + 1].to_string()));
            index += 2;
        } else {
            out.push_str(&regex::escape(&c.to_string()));
            index += 1;
        }
    }
    let last = body.rsplit('/').next().unwrap_or(body);
    // A wildcard final segment matches that level only; a literal one owns its tree.
    let descends = !directory_only && !last.contains(['*', '?']);
    let tail = if directory_only {
        "/.*"
    } else if descends {
        "(?:/.*)?"
    } else {
        ""
    };
    let expression = format!(
        r"(?s)\A{}{}{}\n?\z",
        if anchored { "" } else { "(?:.*/)?" },
        out,
        tail
    );
    Regex::new(&expression).ok()
}

/// One parsed rule, serialised exactly as Python's rule dictionary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub line: i64,
    pub pattern: String,
    pub owners: Vec<String>,
}

/// Parsed ownership: ordered rules plus the digest of the source bytes.
#[derive(Debug)]
pub struct Ownership {
    pub path: String,
    pub rules: Vec<Rule>,
    pub digest: String,
    compiled: Vec<Regex>,
}

impl Ownership {
    fn new(path: &str, rules: Vec<Rule>, digest: String) -> Self {
        let compiled = rules
            .iter()
            .map(|rule| compile(&rule.pattern).expect("rules are validated before use"))
            .collect();
        Self {
            path: path.to_owned(),
            rules,
            digest,
            compiled,
        }
    }

    /// The stored shape: `{"path": ..., "rules": [...]}`.
    pub fn record(&self) -> Value {
        let mut map = Map::new();
        map.insert("path".into(), Value::String(self.path.clone()));
        map.insert(
            "rules".into(),
            serde_json::to_value(&self.rules).expect("rules serialise"),
        );
        Value::Object(map)
    }

    /// The owners and rule line of the last matching rule. A matching rule with no
    /// owners un-assigns the file, so it also yields `None`.
    pub fn lookup(&self, path: &str) -> Option<(Vec<String>, i64)> {
        for index in (0..self.rules.len()).rev() {
            if self.compiled[index].is_match(path) {
                let rule = &self.rules[index];
                return if rule.owners.is_empty() {
                    None
                } else {
                    Some((rule.owners.clone(), rule.line))
                };
            }
        }
        None
    }
}

/// The tokens of one line: runs of non-space characters, where a backslash also
/// takes the next character (so `\ ` keeps an escaped space inside a token).
fn tokens(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut chars = line.char_indices().peekable();
    let mut start: Option<usize> = None;
    while let Some((index, c)) = chars.next() {
        let mut consumed = false;
        if c == '\\'
            && let Some(&(_, next)) = chars.peek()
            && next != '\n'
        {
            chars.next();
            consumed = true;
        } else if !text::is_space(c) {
            consumed = true;
        }
        if consumed {
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            found.push(&line[begin..index]);
        }
    }
    if let Some(begin) = start {
        found.push(&line[begin..]);
    }
    found
}

/// Parse CODEOWNERS bytes; `None` when unusable so ownership stays unknown.
pub fn parse(data: &[u8], path: &str) -> Option<Ownership> {
    if !CANDIDATES.contains(&path) || data.len() > MAX_BYTES {
        return None;
    }
    let content = std::str::from_utf8(data).ok()?;
    let mut rules = Vec::new();
    for (index, line) in content.split('\n').enumerate() {
        let number = index as i64 + 1;
        let found = tokens(line.trim_end_matches('\r'));
        let Some(first) = found.first() else {
            continue;
        };
        if first.starts_with('#') {
            continue;
        }
        let mut owners = Vec::new();
        for token in &found[1..] {
            if token.starts_with('#') {
                break;
            }
            owners.push((*token).to_owned());
        }
        let pattern = first.replace("\\ ", " ");
        if owners.len() > MAX_OWNERS
            || !owners.iter().all(|owner| valid_owner(owner))
            || compile(&pattern).is_none()
        {
            continue;
        }
        rules.push(Rule {
            line: number,
            pattern,
            owners,
        });
        if rules.len() > MAX_RULES {
            // Dropping late rules would misattribute files they override.
            return None;
        }
    }
    if rules.is_empty() {
        return None;
    }
    let digest = format!("{:x}", Sha256::digest(data));
    Some(Ownership::new(path, rules, digest))
}

fn clean_rule(rule: &Value) -> Option<Rule> {
    let object = rule.as_object()?;
    let line = object.get("line")?.as_i64().filter(|line| *line > 0)?;
    let owners = object.get("owners")?.as_array()?;
    if owners.len() > MAX_OWNERS {
        return None;
    }
    let owners: Vec<String> = owners
        .iter()
        .map(|owner| {
            owner
                .as_str()
                .filter(|text| valid_owner(text))
                .map(str::to_owned)
        })
        .collect::<Option<_>>()?;
    let pattern = object.get("pattern")?.as_str()?;
    compile(pattern)?;
    Some(Rule {
        line,
        pattern: pattern.to_owned(),
        owners,
    })
}

/// Rebuild stored ownership, re-validating it because stored data is untrusted.
/// Accepts the metadata object, or its JSON text.
pub fn from_metadata(metadata: &Value) -> Option<Ownership> {
    let parsed;
    let metadata = if let Value::String(text) = metadata {
        parsed = serde_json::from_str::<Value>(text).ok()?;
        &parsed
    } else {
        metadata
    };
    let record = metadata.as_object()?.get("codeowners")?.as_object()?;
    let path = record.get("path")?.as_str()?;
    let rules = record.get("rules")?.as_array()?;
    if !CANDIDATES.contains(&path) || rules.is_empty() || rules.len() > MAX_RULES {
        return None;
    }
    let clean: Vec<Rule> = rules.iter().filter_map(clean_rule).collect();
    if clean.is_empty() {
        return None;
    }
    Some(Ownership::new(path, clean, String::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(text: &str) -> Ownership {
        parse(text.as_bytes(), ".github/CODEOWNERS").expect("parses")
    }

    fn who(ownership: &Ownership, path: &str) -> Option<Vec<String>> {
        ownership.lookup(path).map(|found| found.0)
    }

    #[test]
    fn last_matching_rule_wins_and_unassign_clears() {
        let ownership = owners("* @org/all\n/docs/ @docs\n/docs/private/\n*.md @writers\n");
        assert_eq!(who(&ownership, "src/a.py"), Some(vec!["@org/all".into()]));
        assert_eq!(who(&ownership, "docs/a.txt"), Some(vec!["@docs".into()]));
        assert_eq!(who(&ownership, "docs/private/a.txt"), None);
        assert_eq!(
            who(&ownership, "docs/readme.md"),
            Some(vec!["@writers".into()])
        );
    }

    #[test]
    fn anchoring_and_wildcards() {
        let ownership = owners("/build/logs/ @ops\ndocs/*.md @docs\n**/api/ @api\nweb/** @web\n");
        assert_eq!(
            who(&ownership, "build/logs/x.log"),
            Some(vec!["@ops".into()])
        );
        assert_eq!(who(&ownership, "other/build/logs/x.log"), None);
        assert_eq!(who(&ownership, "docs/a.md"), Some(vec!["@docs".into()]));
        assert_eq!(who(&ownership, "docs/sub/a.md"), None);
        assert_eq!(who(&ownership, "x/api/y.py"), Some(vec!["@api".into()]));
        assert_eq!(who(&ownership, "web/a/b"), Some(vec!["@web".into()]));
        assert_eq!(who(&ownership, "web"), None);
    }

    #[test]
    fn invalid_lines_are_skipped_and_bounds_are_enforced() {
        let ownership = owners("[bad] @a\n../x @a\n* not-an-owner\n* @ok\n");
        assert_eq!(ownership.rules.len(), 1);
        assert_eq!(ownership.rules[0].line, 4);
        assert!(parse(b"* @a\n", "src/CODEOWNERS").is_none());
        assert!(parse(&vec![b'#'; MAX_BYTES + 1], "CODEOWNERS").is_none());
        assert!(parse(&[0xff, 0xfe], "CODEOWNERS").is_none());
        assert!(parse(b"# nothing\n", "CODEOWNERS").is_none());
    }

    #[test]
    fn escaped_space_stays_in_the_pattern() {
        let ownership = owners("my\\ dir/ @a\n");
        assert_eq!(ownership.rules[0].pattern, "my dir/");
        assert_eq!(who(&ownership, "my dir/x"), Some(vec!["@a".into()]));
    }

    #[test]
    fn owner_forms() {
        for good in ["@a", "@org/team", "a@b.co", "@a.b-c_d"] {
            assert!(valid_owner(good), "{good}");
        }
        for bad in ["a", "@", "@-a", "a@b", "@org/", "", "@a b"] {
            assert!(!valid_owner(bad), "{bad}");
        }
    }

    #[test]
    fn stored_ownership_is_revalidated() {
        let stored = serde_json::json!({"codeowners": {"path": "CODEOWNERS", "rules": [
            {"line": 1, "pattern": "*", "owners": ["@a"]},
            {"line": 0, "pattern": "*", "owners": ["@a"]},
            {"line": 2, "pattern": "[x]", "owners": ["@a"]},
            {"line": 3, "pattern": "*.py", "owners": ["bad"]}
        ]}});
        let ownership = from_metadata(&stored).expect("one rule survives");
        assert_eq!(ownership.rules.len(), 1);
        assert!(
            from_metadata(&serde_json::json!({"codeowners": {"path": "x", "rules": []}})).is_none()
        );
        assert!(from_metadata(&Value::Null).is_none());
    }
}
