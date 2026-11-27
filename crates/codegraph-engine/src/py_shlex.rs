//! `shlex.shlex(line, posix=True, punctuation_chars=";&|")` with `whitespace_split`, as CPython 3.12 runs it.
//!
//! Splitting is purely lexical: nothing is expanded and nothing is executed.

const WHITESPACE: &str = " \t\r\n";
const QUOTES: &str = "'\"";
const PUNCTUATION: &str = ";&|";
const ACCENTED: &str = "\u{df}\u{e0}\u{e1}\u{e2}\u{e3}\u{e4}\u{e5}\u{e6}\u{e7}\u{e8}\u{e9}\u{ea}\u{eb}\u{ec}\u{ed}\u{ee}\u{ef}\u{f0}\u{f1}\u{f2}\u{f3}\u{f4}\u{f5}\u{f6}\u{f8}\u{f9}\u{fa}\u{fb}\u{fc}\u{fd}\u{fe}\u{ff}\u{c0}\u{c1}\u{c2}\u{c3}\u{c4}\u{c5}\u{c6}\u{c7}\u{c8}\u{c9}\u{ca}\u{cb}\u{cc}\u{cd}\u{ce}\u{cf}\u{d0}\u{d1}\u{d2}\u{d3}\u{d4}\u{d5}\u{d6}\u{d8}\u{d9}\u{da}\u{db}\u{dc}\u{dd}\u{de}";

/// Why splitting stopped; Python raises `ValueError` with this text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShlexError(pub &'static str);

/// The characters that continue a word: letters, digits, `_`, accented Latin letters
/// and `~-./*?=`, minus the punctuation characters.
fn is_wordchar(c: char) -> bool {
    let plain = c.is_ascii_alphanumeric() || c == '_' || ACCENTED.contains(c);
    let extra = "~-./*?=".contains(c);
    (plain || extra) && !PUNCTUATION.contains(c)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Space,
    Quote(char),
    Escape,
    Word,
    Punctuation,
    End,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escaped {
    Space,
    Word,
    Quote(char),
}

struct Lexer {
    input: Vec<char>,
    at: usize,
    pushback: Vec<char>,
    state: State,
}

impl Lexer {
    fn read(&mut self) -> Option<char> {
        let c = self.input.get(self.at).copied();
        if c.is_some() {
            self.at += 1;
        }
        c
    }

    /// `instream.readline()`: the rest of the line, newline included.
    fn skip_line(&mut self) {
        while let Some(c) = self.read() {
            if c == '\n' {
                break;
            }
        }
    }

    fn read_token(&mut self) -> Result<Option<String>, ShlexError> {
        let mut token = String::new();
        let mut quoted = false;
        let mut escaped = Escaped::Space;
        loop {
            let next = match self.pushback.pop() {
                Some(c) => Some(c),
                None => self.read(),
            };
            match self.state {
                State::End => {
                    token.clear();
                    break;
                }
                State::Space => {
                    let Some(c) = next else {
                        self.state = State::End;
                        break;
                    };
                    if WHITESPACE.contains(c) {
                        if !token.is_empty() || quoted {
                            break;
                        }
                    } else if c == '#' {
                        self.skip_line();
                    } else if c == '\\' {
                        escaped = Escaped::Word;
                        self.state = State::Escape;
                    } else if is_wordchar(c) {
                        token = c.to_string();
                        self.state = State::Word;
                    } else if PUNCTUATION.contains(c) {
                        token = c.to_string();
                        self.state = State::Punctuation;
                    } else if QUOTES.contains(c) {
                        self.state = State::Quote(c);
                    } else {
                        // `whitespace_split`: anything else starts a word.
                        token = c.to_string();
                        self.state = State::Word;
                    }
                }
                State::Quote(quote) => {
                    quoted = true;
                    let Some(c) = next else {
                        return Err(ShlexError("No closing quotation"));
                    };
                    if c == quote {
                        self.state = State::Word;
                    } else if c == '\\' && quote == '"' {
                        escaped = Escaped::Quote(quote);
                        self.state = State::Escape;
                    } else {
                        token.push(c);
                    }
                }
                State::Escape => {
                    let Some(c) = next else {
                        return Err(ShlexError("No escaped character"));
                    };
                    // Within quotes only the quote itself or the escape may be escaped.
                    if let Escaped::Quote(quote) = escaped
                        && c != '\\'
                        && c != quote
                    {
                        token.push('\\');
                    }
                    token.push(c);
                    self.state = match escaped {
                        Escaped::Space => State::Space,
                        Escaped::Word => State::Word,
                        Escaped::Quote(quote) => State::Quote(quote),
                    };
                }
                State::Word | State::Punctuation => {
                    let Some(c) = next else {
                        self.state = State::End;
                        break;
                    };
                    if WHITESPACE.contains(c) {
                        self.state = State::Space;
                        if !token.is_empty() || quoted {
                            break;
                        }
                    } else if c == '#' {
                        self.skip_line();
                        self.state = State::Space;
                        if !token.is_empty() || quoted {
                            break;
                        }
                    } else if self.state == State::Punctuation {
                        if PUNCTUATION.contains(c) {
                            token.push(c);
                        } else {
                            self.pushback.push(c);
                            self.state = State::Space;
                            break;
                        }
                    } else if QUOTES.contains(c) {
                        self.state = State::Quote(c);
                    } else if c == '\\' {
                        escaped = Escaped::Word;
                        self.state = State::Escape;
                    } else if is_wordchar(c) || !PUNCTUATION.contains(c) {
                        token.push(c);
                    } else {
                        self.pushback.push(c);
                        self.state = State::Space;
                        if !token.is_empty() || quoted {
                            break;
                        }
                    }
                }
            }
        }
        Ok(if token.is_empty() && !quoted {
            None
        } else {
            Some(token)
        })
    }
}

/// `list(shlex.shlex(line, posix=True, punctuation_chars=";&|"))` with `whitespace_split`.
pub fn split(line: &str) -> Result<Vec<String>, ShlexError> {
    let mut lexer = Lexer {
        input: line.chars().collect(),
        at: 0,
        pushback: Vec::new(),
        state: State::Space,
    };
    let mut tokens = Vec::new();
    while let Some(token) = lexer.read_token()? {
        tokens.push(token);
    }
    Ok(tokens)
}
