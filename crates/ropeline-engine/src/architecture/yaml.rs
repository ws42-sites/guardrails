//! Bounded YAML documents, as PyYAML's `BaseLoader` composes them.
//!
//! `configuration_documents` in `architecture_ci.py` scans, refuses anchors, aliases,
//! tags and oversized streams, then composes nodes whose kind, text and line it reads.
//! The scanner here is `yaml-rust2`'s (a libyaml descendant, as PyYAML's is); the
//! grammar on top of its tokens is PyYAML's own parser, so empty values, flow
//! pairs and node start positions come out as PyYAML gives them.
//!
//! Callers catch every failure the same way, so one error value stands for PyYAML's
//! `YAMLError`, `ValueError` and `RecursionError`.

use yaml_rust2::scanner::{Scanner, TScalarStyle, Token, TokenType};

/// The limits `configuration_documents` enforces.
const MAX_TOKENS: usize = 10_000;
const MAX_DOCUMENTS: usize = 64;
const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 10_000;

/// Why a source gave no documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigurationError(pub String);

/// A composed node; `line` is PyYAML's `start_mark.line + 1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Scalar {
        value: String,
        line: usize,
    },
    Sequence {
        items: Vec<Node>,
        line: usize,
    },
    Mapping {
        pairs: Vec<(Node, Node)>,
        line: usize,
    },
}

impl Node {
    pub fn line(&self) -> usize {
        match self {
            Self::Scalar { line, .. }
            | Self::Sequence { line, .. }
            | Self::Mapping { line, .. } => *line,
        }
    }
}

fn fail<T>(message: &str) -> Result<T, ConfigurationError> {
    Err(ConfigurationError(message.to_owned()))
}

/// PyYAML's reader refuses control characters up front, for a string stream.
fn printable(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{7e}' | '\u{85}' | '\u{a0}'..='\u{d7ff}'
        | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')
}

struct Tokens<'a> {
    items: Vec<(usize, usize, TokenType)>,
    at: usize,
    source: &'a str,
}

impl Tokens<'_> {
    fn kind(&self) -> Option<&TokenType> {
        self.items.get(self.at).map(|(_, _, kind)| kind)
    }

    fn line(&self) -> usize {
        self.items.get(self.at).map_or(0, |(line, _, _)| *line)
    }

    fn index(&self) -> usize {
        self.items.get(self.at).map_or(0, |(_, index, _)| *index)
    }

    fn take(&mut self) -> Option<(usize, TokenType)> {
        let found = self
            .items
            .get(self.at)
            .map(|(line, _, kind)| (*line, kind.clone()));
        self.at += 1;
        found
    }

    /// PyYAML only makes a key of text that starts and ends on one line and is at most
    /// 1024 characters from its `:`. The scanner used here relaxes both inside flow
    /// collections, so the pair is checked once its `:` is next.
    fn check_implicit_key(
        &self,
        key_line: usize,
        key_index: usize,
    ) -> Result<(), ConfigurationError> {
        let explicit = self.source.as_bytes().get(key_index) == Some(&b'?');
        if !explicit
            && self.is(|kind| matches!(kind, TokenType::Value))
            && (self.line() != key_line || self.index() > key_index + 1024)
        {
            return fail("could not find expected ':'");
        }
        Ok(())
    }

    fn is(&self, test: impl Fn(&TokenType) -> bool) -> bool {
        self.kind().is_some_and(test)
    }
}

fn is_document_start(kind: &TokenType) -> bool {
    matches!(kind, TokenType::DocumentStart)
}

fn is_document_end(kind: &TokenType) -> bool {
    matches!(kind, TokenType::DocumentEnd)
}

fn is_stream_end(kind: &TokenType) -> bool {
    matches!(kind, TokenType::StreamEnd)
}

fn is_directive(kind: &TokenType) -> bool {
    matches!(
        kind,
        TokenType::VersionDirective(..) | TokenType::TagDirective(..)
    )
}

/// Scan, refuse what the Python refuses, then compose every document.
pub fn configuration_documents(source: &str) -> Result<Vec<Node>, ConfigurationError> {
    if !source.chars().all(printable) {
        return fail("unacceptable character");
    }
    // Deeply nested input recurses once per level; the limits stop it at 64, but the
    // scanner and our grammar run on a roomy stack all the same.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn_scoped(scope, || compose(source))
            .map_or_else(
                |_| fail("no stack"),
                |handle| handle.join().unwrap_or_else(|_| fail("panicked")),
            )
    })
}

/// The line a block scalar's indicator (`|` or `>`) is on. The scanner reports where the
/// text starts; PyYAML reports the indicator, the first thing after the previous token.
fn indicator_line(text: &str, from: usize, line: usize) -> usize {
    let bytes = text.as_bytes();
    let mut at = from;
    let mut line = line;
    while at < bytes.len() {
        match bytes[at] {
            b'\n' => line += 1,
            b'\r' => {
                if bytes.get(at + 1) != Some(&b'\n') {
                    line += 1;
                }
            }
            b'#' => {
                while at + 1 < bytes.len() && !matches!(bytes[at + 1], b'\n' | b'\r') {
                    at += 1;
                }
            }
            b'|' | b'>' => break,
            _ => {}
        }
        at += 1;
    }
    line
}

fn compose(source: &str) -> Result<Vec<Node>, ConfigurationError> {
    // PyYAML skips a byte order mark at the start of the stream.
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    // PyYAML cannot start a token with a tab, so a tab is only legal inside quoted and block
    // scalars and comments. A stand-in character keeps those cases and breaks the rest: it
    // ends up inside a plain scalar, which is refused below. Scalar text gets its tabs back.
    let stand_in = ('\u{e000}'..='\u{f8ff}')
        .find(|candidate| !source.contains(*candidate))
        .ok_or_else(|| ConfigurationError("no stand-in character".into()))?;
    // PyYAML also ends a line at NEL, LS and PS; the scanner only knows \n and \r.
    let swapped: String = source
        .chars()
        .map(|c| match c {
            '\t' => stand_in,
            '\u{85}' | '\u{2028}' | '\u{2029}' => '\n',
            other => other,
        })
        .collect();
    let mut scanner = Scanner::new(swapped.chars());
    let mut items: Vec<(usize, usize, TokenType)> = Vec::new();
    let mut previous_end = 0;
    loop {
        let (mark, kind) = match scanner.next_token() {
            Ok(Some(Token(mark, kind))) => (mark, kind),
            Ok(None) => break,
            Err(error) => return fail(error.info()),
        };
        if items.len() > MAX_TOKENS
            || matches!(
                kind,
                TokenType::Anchor(_) | TokenType::Alias(_) | TokenType::Tag(..)
            )
        {
            return fail("Unsupported YAML indirection or size");
        }
        // Where the next token's leading whitespace and comments start. The scanner marks a
        // `:` or `?` where it stands, but a `-` after the whitespace that follows it.
        let here = mark.index()
            + match kind {
                TokenType::Key | TokenType::Value => 1,
                TokenType::DocumentStart => 3,
                _ => 0,
            };
        let (line, kind) = match kind {
            TokenType::Scalar(TScalarStyle::Plain, text) if text.contains(stand_in) => {
                return fail("found character '\\t' that cannot start any token");
            }
            TokenType::Scalar(style @ (TScalarStyle::Literal | TScalarStyle::Folded), text) => (
                indicator_line(
                    &swapped,
                    previous_end,
                    items.last().map_or(1, |(line, _, _)| *line),
                ),
                TokenType::Scalar(style, text.replace(stand_in, "\t")),
            ),
            TokenType::Scalar(style, text) => (
                mark.line(),
                TokenType::Scalar(style, text.replace(stand_in, "\t")),
            ),
            other => (mark.line(), other),
        };
        previous_end = here;
        items.push((line, mark.index(), kind));
    }
    // PyYAML keeps no final line break for a block scalar that ends the stream without one.
    if !swapped.ends_with(['\n', '\r']) {
        let last_content = items.iter_mut().rev().find(|(_, _, kind)| {
            !matches!(
                kind,
                TokenType::BlockEnd | TokenType::DocumentEnd | TokenType::StreamEnd
            )
        });
        if let Some((_, _, TokenType::Scalar(TScalarStyle::Literal | TScalarStyle::Folded, text))) =
            last_content
            && text.ends_with('\n')
        {
            text.pop();
        }
    }
    let mut tokens = Tokens {
        items,
        at: 0,
        source: &swapped,
    };
    let roots = stream(&mut tokens)?;
    if roots.len() > MAX_DOCUMENTS {
        return fail("Too many configuration documents");
    }
    let mut count = 0;
    for root in &roots {
        check_limits(root, 0, &mut count)?;
    }
    Ok(roots)
}

/// Depth and size limits over the composed trees.
fn check_limits(node: &Node, depth: usize, count: &mut usize) -> Result<(), ConfigurationError> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return fail("Configuration exceeds structural limits");
    }
    *count += 1;
    match node {
        Node::Scalar { .. } => {}
        Node::Sequence { items, .. } => {
            for item in items {
                check_limits(item, depth + 1, count)?;
            }
        }
        Node::Mapping { pairs, .. } => {
            for (key, value) in pairs {
                check_limits(key, depth + 1, count)?;
                check_limits(value, depth + 1, count)?;
            }
        }
    }
    Ok(())
}

/// `parse_stream_start`, then documents until the stream ends.
fn stream(tokens: &mut Tokens) -> Result<Vec<Node>, ConfigurationError> {
    if !tokens.is(|kind| matches!(kind, TokenType::StreamStart(_))) {
        return fail("expected '<stream start>'");
    }
    tokens.take();
    let mut roots = Vec::new();
    // An implicit first document has no `---`.
    if !tokens.is(|kind| is_directive(kind) || is_document_start(kind) || is_stream_end(kind)) {
        roots.push(block_node(tokens)?);
        document_end(tokens);
    }
    loop {
        // Extra document end indicators.
        while tokens.is(is_document_end) {
            tokens.take();
        }
        if tokens.is(is_stream_end) {
            return Ok(roots);
        }
        directives(tokens)?;
        if !tokens.is(is_document_start) {
            return fail("expected '<document start>'");
        }
        tokens.take();
        let root = if tokens.is(|kind| {
            is_directive(kind)
                || is_document_start(kind)
                || is_document_end(kind)
                || is_stream_end(kind)
        }) {
            Node::Scalar {
                value: String::new(),
                line: tokens.line(),
            }
        } else {
            block_node(tokens)?
        };
        roots.push(root);
        document_end(tokens);
    }
}

fn document_end(tokens: &mut Tokens) {
    if tokens.is(is_document_end) {
        tokens.take();
    }
}

/// `process_directives`: only duplicates and unsupported versions are errors.
fn directives(tokens: &mut Tokens) -> Result<(), ConfigurationError> {
    let mut version = false;
    let mut handles: Vec<String> = Vec::new();
    while tokens.is(is_directive) {
        match tokens.take().map(|(_, kind)| kind) {
            Some(TokenType::VersionDirective(major, _)) => {
                if version {
                    return fail("found duplicate YAML directive");
                }
                if major != 1 {
                    return fail("found incompatible YAML document (version 1.* is required)");
                }
                version = true;
            }
            Some(TokenType::TagDirective(handle, _)) => {
                if handles.contains(&handle) {
                    return fail("duplicate tag handle");
                }
                handles.push(handle);
            }
            _ => {}
        }
    }
    Ok(())
}

fn block_node(tokens: &mut Tokens) -> Result<Node, ConfigurationError> {
    node(tokens, true, false)
}

fn flow_node(tokens: &mut Tokens) -> Result<Node, ConfigurationError> {
    node(tokens, false, false)
}

/// `parse_node`: one node, however it is written.
fn node(tokens: &mut Tokens, block: bool, indentless: bool) -> Result<Node, ConfigurationError> {
    let line = tokens.line();
    if indentless && tokens.is(|kind| matches!(kind, TokenType::BlockEntry)) {
        return indentless_sequence(tokens, line);
    }
    match tokens.kind() {
        Some(TokenType::Scalar(_, value)) => {
            let value = value.to_string();
            tokens.take();
            Ok(Node::Scalar { value, line })
        }
        Some(TokenType::FlowSequenceStart) => flow_sequence(tokens, line),
        Some(TokenType::FlowMappingStart) => flow_mapping(tokens, line),
        Some(TokenType::BlockSequenceStart) if block => block_sequence(tokens, line),
        Some(TokenType::BlockMappingStart) if block => block_mapping(tokens, line),
        _ => fail("expected the node content"),
    }
}

fn empty(line: usize) -> Node {
    Node::Scalar {
        value: String::new(),
        line,
    }
}

fn block_sequence(tokens: &mut Tokens, line: usize) -> Result<Node, ConfigurationError> {
    tokens.take();
    let mut items = Vec::new();
    while tokens.is(|kind| matches!(kind, TokenType::BlockEntry)) {
        let (entry_line, _) = tokens.take().expect("checked");
        if tokens.is(|kind| matches!(kind, TokenType::BlockEntry | TokenType::BlockEnd)) {
            items.push(empty(entry_line));
        } else {
            items.push(block_node(tokens)?);
        }
    }
    if !tokens.is(|kind| matches!(kind, TokenType::BlockEnd)) {
        return fail("expected <block end>");
    }
    tokens.take();
    Ok(Node::Sequence { items, line })
}

fn indentless_sequence(tokens: &mut Tokens, line: usize) -> Result<Node, ConfigurationError> {
    let mut items = Vec::new();
    while tokens.is(|kind| matches!(kind, TokenType::BlockEntry)) {
        let (entry_line, _) = tokens.take().expect("checked");
        if tokens.is(|kind| {
            matches!(
                kind,
                TokenType::BlockEntry | TokenType::Key | TokenType::Value | TokenType::BlockEnd
            )
        }) {
            items.push(empty(entry_line));
        } else {
            items.push(block_node(tokens)?);
        }
    }
    Ok(Node::Sequence { items, line })
}

fn block_mapping(tokens: &mut Tokens, line: usize) -> Result<Node, ConfigurationError> {
    tokens.take();
    let ends_value = |kind: &TokenType| {
        matches!(
            kind,
            TokenType::Key | TokenType::Value | TokenType::BlockEnd
        )
    };
    let mut pairs = Vec::new();
    while tokens.is(|kind| matches!(kind, TokenType::Key)) {
        let (key_line, _) = tokens.take().expect("checked");
        let key = if tokens.is(ends_value) {
            empty(key_line)
        } else {
            node(tokens, true, true)?
        };
        let value = if tokens.is(|kind| matches!(kind, TokenType::Value)) {
            let (value_line, _) = tokens.take().expect("checked");
            if tokens.is(ends_value) {
                empty(value_line)
            } else {
                node(tokens, true, true)?
            }
        } else {
            empty(tokens.line())
        };
        pairs.push((key, value));
    }
    if !tokens.is(|kind| matches!(kind, TokenType::BlockEnd)) {
        return fail("expected <block end>");
    }
    tokens.take();
    Ok(Node::Mapping { pairs, line })
}

fn flow_sequence(tokens: &mut Tokens, line: usize) -> Result<Node, ConfigurationError> {
    tokens.take();
    let mut items = Vec::new();
    let mut first = true;
    while !tokens.is(|kind| matches!(kind, TokenType::FlowSequenceEnd)) {
        if !first {
            if tokens.is(|kind| matches!(kind, TokenType::FlowEntry)) {
                tokens.take();
            } else {
                return fail("expected ',' or ']'");
            }
        }
        first = false;
        if tokens.is(|kind| matches!(kind, TokenType::Key)) {
            // A single pair inside a flow sequence: `[a: b]`.
            let pair_line = tokens.line();
            let key_index = tokens.index();
            let (key_line, _) = tokens.take().expect("checked");
            let stops = |kind: &TokenType| {
                matches!(
                    kind,
                    TokenType::Value | TokenType::FlowEntry | TokenType::FlowSequenceEnd
                )
            };
            let key = if tokens.is(stops) {
                empty(key_line)
            } else {
                flow_node(tokens)?
            };
            tokens.check_implicit_key(key_line, key_index)?;
            let value = if tokens.is(|kind| matches!(kind, TokenType::Value)) {
                let (value_line, _) = tokens.take().expect("checked");
                if tokens
                    .is(|kind| matches!(kind, TokenType::FlowEntry | TokenType::FlowSequenceEnd))
                {
                    empty(value_line)
                } else {
                    flow_node(tokens)?
                }
            } else {
                empty(tokens.line())
            };
            items.push(Node::Mapping {
                pairs: vec![(key, value)],
                line: pair_line,
            });
        } else if !tokens.is(|kind| matches!(kind, TokenType::FlowSequenceEnd)) {
            items.push(flow_node(tokens)?);
        }
    }
    tokens.take();
    Ok(Node::Sequence { items, line })
}

fn flow_mapping(tokens: &mut Tokens, line: usize) -> Result<Node, ConfigurationError> {
    tokens.take();
    let mut pairs = Vec::new();
    let mut first = true;
    while !tokens.is(|kind| matches!(kind, TokenType::FlowMappingEnd)) {
        if !first {
            if tokens.is(|kind| matches!(kind, TokenType::FlowEntry)) {
                tokens.take();
            } else {
                return fail("expected ',' or '}'");
            }
        }
        first = false;
        let stops = |kind: &TokenType| {
            matches!(
                kind,
                TokenType::Value | TokenType::FlowEntry | TokenType::FlowMappingEnd
            )
        };
        let (key, key_line, key_index) = if tokens.is(|kind| matches!(kind, TokenType::Key)) {
            let key_index = tokens.index();
            let (key_line, _) = tokens.take().expect("checked");
            let key = if tokens.is(stops) {
                empty(key_line)
            } else {
                flow_node(tokens)?
            };
            (key, key_line, key_index)
        } else if !tokens.is(|kind| matches!(kind, TokenType::FlowMappingEnd)) {
            // A key with no value: `{a, b}`.
            let key = flow_node(tokens)?;
            pairs.push((key, empty(tokens.line())));
            continue;
        } else {
            break;
        };
        tokens.check_implicit_key(key_line, key_index)?;
        let value = if tokens.is(|kind| matches!(kind, TokenType::Value)) {
            let (value_line, _) = tokens.take().expect("checked");
            if tokens.is(|kind| matches!(kind, TokenType::FlowEntry | TokenType::FlowMappingEnd)) {
                empty(value_line)
            } else {
                flow_node(tokens)?
            }
        } else {
            empty(tokens.line())
        };
        pairs.push((key, value));
    }
    tokens.take();
    Ok(Node::Mapping { pairs, line })
}
