//! `fnmatch.fnmatchcase` as CPython 3.12 defines it: the pattern is translated to a regular
//! expression, and the translation (not the shell's glob rules) is what decides a match.
//! `*` crosses `/`, `?` is any one character, and `[...]` sets follow the translation's
//! handling of `!`, ranges and hyphens.

enum Token {
    Star,
    Any,
    Literal(char),
    Set {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    Never,
}

/// The set a translated `stuff` string means once the regular-expression engine reads it:
/// a backslash makes the next character literal, and `a-b` is a range.
fn parse_set(stuff: &str) -> Token {
    let mut chars: Vec<char> = stuff.chars().collect();
    let negated = chars.first() == Some(&'^');
    if negated {
        chars.remove(0);
    }
    // Each item is a character and whether it was written as an escape.
    let mut items: Vec<(char, bool)> = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '\\' && at + 1 < chars.len() {
            items.push((chars[at + 1], true));
            at += 2;
        } else {
            items.push((chars[at], false));
            at += 1;
        }
    }
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < items.len() {
        let (low, _) = items[index];
        // A plain hyphen between two items makes a range; after a range it is literal.
        if items.get(index + 1) == Some(&('-', false)) && index + 2 < items.len() {
            ranges.push((low, items[index + 2].0));
            index += 3;
        } else {
            ranges.push((low, low));
            index += 1;
        }
    }
    Token::Set { negated, ranges }
}

/// `fnmatch.translate`, as a token list.
fn translate(pattern: &str) -> Vec<Token> {
    let pat: Vec<char> = pattern.chars().collect();
    let n = pat.len();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < n {
        let c = pat[i];
        i += 1;
        match c {
            '*' => {
                if !matches!(out.last(), Some(Token::Star)) {
                    out.push(Token::Star);
                }
            }
            '?' => out.push(Token::Any),
            '[' => {
                let mut j = i;
                if j < n && pat[j] == '!' {
                    j += 1;
                }
                if j < n && pat[j] == ']' {
                    j += 1;
                }
                while j < n && pat[j] != ']' {
                    j += 1;
                }
                if j >= n {
                    out.push(Token::Literal('['));
                    continue;
                }
                let mut start = i;
                let raw: String = pat[start..j].iter().collect();
                let mut stuff = if !raw.contains('-') {
                    raw.replace('\\', "\\\\")
                } else {
                    let mut chunks: Vec<String> = Vec::new();
                    let mut k = if pat[start] == '!' {
                        start + 2
                    } else {
                        start + 1
                    };
                    loop {
                        let found = (k..j).find(|position| pat[*position] == '-');
                        let Some(hyphen) = found else { break };
                        chunks.push(pat[start..hyphen].iter().collect());
                        start = hyphen + 1;
                        k = hyphen + 3;
                    }
                    let chunk: String = pat[start..j.max(start)].iter().collect();
                    if chunk.is_empty() {
                        if let Some(last) = chunks.last_mut() {
                            last.push('-');
                        }
                    } else {
                        chunks.push(chunk);
                    }
                    // Drop empty ranges: they are invalid in a regular expression.
                    let mut index = chunks.len();
                    while index > 1 {
                        index -= 1;
                        let before = chunks[index - 1].chars().last();
                        let after = chunks[index].chars().next();
                        if let (Some(before), Some(after)) = (before, after)
                            && before > after
                        {
                            let mut merged: String = chunks[index - 1].clone();
                            merged.pop();
                            merged.extend(chunks[index].chars().skip(1));
                            chunks[index - 1] = merged;
                            chunks.remove(index);
                        }
                    }
                    chunks
                        .iter()
                        .map(|piece| piece.replace('\\', "\\\\").replace('-', "\\-"))
                        .collect::<Vec<_>>()
                        .join("-")
                };
                // Escape set operations (&&, ~~ and ||).
                stuff = stuff
                    .chars()
                    .flat_map(|c| {
                        if matches!(c, '&' | '~' | '|') {
                            vec!['\\', c]
                        } else {
                            vec![c]
                        }
                    })
                    .collect();
                i = j + 1;
                if stuff.is_empty() {
                    out.push(Token::Never);
                } else if stuff == "!" {
                    out.push(Token::Any);
                } else {
                    if stuff.starts_with('!') {
                        stuff = format!("^{}", &stuff[1..]);
                    } else if stuff.starts_with(['^', '[']) {
                        stuff = format!("\\{stuff}");
                    }
                    out.push(parse_set(&stuff));
                }
            }
            other => out.push(Token::Literal(other)),
        }
    }
    out
}

fn accepts(token: &Token, c: char) -> bool {
    match token {
        Token::Any => true,
        Token::Literal(expected) => *expected == c,
        Token::Set { negated, ranges } => {
            ranges.iter().any(|(low, high)| *low <= c && c <= *high) != *negated
        }
        Token::Never | Token::Star => false,
    }
}

/// `fnmatch.fnmatchcase(name, pattern)`.
pub fn fnmatchcase(name: &str, pattern: &str) -> bool {
    let tokens = translate(pattern);
    let text: Vec<char> = name.chars().collect();
    let (mut at, mut token) = (0, 0);
    // Where the last star can resume if a later token fails.
    let mut resume: Option<(usize, usize)> = None;
    while at < text.len() {
        match tokens.get(token) {
            Some(Token::Star) => {
                resume = Some((token, at));
                token += 1;
            }
            Some(found) if accepts(found, text[at]) => {
                at += 1;
                token += 1;
            }
            _ => match resume {
                Some((star, from)) => {
                    resume = Some((star, from + 1));
                    token = star + 1;
                    at = from + 1;
                }
                None => return false,
            },
        }
    }
    tokens[token..]
        .iter()
        .all(|rest| matches!(rest, Token::Star))
}
