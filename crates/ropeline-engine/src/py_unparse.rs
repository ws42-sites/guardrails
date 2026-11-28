//! `ast.unparse` for expressions, as CPython 3.12's `ast._Unparser` writes them.
//!
//! Architecture analysis uses the text of callees, bases and annotations: it is
//! matched against a character class, split on dots and compared. So spelling,
//! spacing and the parentheses CPython adds for precedence must match exactly.
//! The one deliberate simplification is the body of an f-string (see `fstring`):
//! its text always contains a quote, which no caller treats as a plain name.

use crate::py_value::{float_repr, printable, str_repr};
use crate::text;
use ruff_python_ast::{
    BoolOp, CmpOp, Expr, ExprContext, Number, Operator, Parameter, Parameters, UnaryOp,
};

// `_Precedence`, lowest to highest.
const NAMED_EXPR: u8 = 1;
const TUPLE: u8 = 2;
const YIELD: u8 = 3;
const TEST: u8 = 4;
const OR: u8 = 5;
const AND: u8 = 6;
const NOT: u8 = 7;
const CMP: u8 = 8;
const EXPR: u8 = 9;
const BXOR: u8 = 10;
const BAND: u8 = 11;
const SHIFT: u8 = 12;
const ARITH: u8 = 13;
const TERM: u8 = 14;
const FACTOR: u8 = 15;
const POWER: u8 = 16;
const AWAIT: u8 = 17;
const ATOM: u8 = 18;

fn next(precedence: u8) -> u8 {
    (precedence + 1).min(ATOM)
}

/// `ast.unparse(expression)`.
pub fn unparse(src: &str, expression: &Expr) -> String {
    let mut out = String::new();
    write_expr(src, &mut out, expression, TEST);
    out
}

/// `with self.delimit_if("(", ")", wrap)`.
fn delimited(out: &mut String, wrap: bool, body: impl FnOnce(&mut String)) {
    if wrap {
        out.push('(');
    }
    body(out);
    if wrap {
        out.push(')');
    }
}

fn interleave<T>(
    out: &mut String,
    items: impl IntoIterator<Item = T>,
    mut each: impl FnMut(&mut String, T),
) {
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        each(out, item);
    }
}

/// `items_view`: a one-item tuple keeps its trailing comma.
fn items_view(src: &str, out: &mut String, items: &[Expr]) {
    if let [only] = items {
        write_expr(src, out, only, TEST);
        out.push(',');
    } else {
        interleave(out, items, |out, item| write_expr(src, out, item, TEST));
    }
}

fn binary_operator(operator: Operator) -> (&'static str, u8) {
    match operator {
        Operator::Add => ("+", ARITH),
        Operator::Sub => ("-", ARITH),
        Operator::Mult => ("*", TERM),
        Operator::MatMult => ("@", TERM),
        Operator::Div => ("/", TERM),
        Operator::Mod => ("%", TERM),
        Operator::LShift => ("<<", SHIFT),
        Operator::RShift => (">>", SHIFT),
        Operator::BitOr => ("|", EXPR),
        Operator::BitXor => ("^", BXOR),
        Operator::BitAnd => ("&", BAND),
        Operator::FloorDiv => ("//", TERM),
        Operator::Pow => ("**", POWER),
    }
}

fn comparison(operator: CmpOp) -> &'static str {
    match operator {
        CmpOp::Eq => "==",
        CmpOp::NotEq => "!=",
        CmpOp::Lt => "<",
        CmpOp::LtE => "<=",
        CmpOp::Gt => ">",
        CmpOp::GtE => ">=",
        CmpOp::Is => "is",
        CmpOp::IsNot => "is not",
        CmpOp::In => "in",
        CmpOp::NotIn => "not in",
    }
}

fn write_parameter(src: &str, out: &mut String, parameter: &Parameter) {
    out.push_str(parameter.name.as_str());
    if let Some(annotation) = &parameter.annotation {
        out.push_str(": ");
        write_expr(src, out, annotation, TEST);
    }
}

/// `visit_arguments`.
fn write_parameters(src: &str, out: &mut String, parameters: &Parameters) {
    let mut first = true;
    let positional: Vec<_> = parameters
        .posonlyargs
        .iter()
        .chain(parameters.args.iter())
        .collect();
    for (index, item) in positional.iter().enumerate() {
        if !first {
            out.push_str(", ");
        }
        first = false;
        write_parameter(src, out, &item.parameter);
        if let Some(default) = &item.default {
            out.push('=');
            write_expr(src, out, default, TEST);
        }
        if index + 1 == parameters.posonlyargs.len() {
            out.push_str(", /");
        }
    }
    if parameters.vararg.is_some() || !parameters.kwonlyargs.is_empty() {
        if !first {
            out.push_str(", ");
        }
        first = false;
        out.push('*');
        if let Some(vararg) = &parameters.vararg {
            write_parameter(src, out, vararg);
        }
    }
    for item in &parameters.kwonlyargs {
        out.push_str(", ");
        write_parameter(src, out, &item.parameter);
        if let Some(default) = &item.default {
            out.push('=');
            write_expr(src, out, default, TEST);
        }
    }
    if let Some(kwarg) = &parameters.kwarg {
        if !first {
            out.push_str(", ");
        }
        out.push_str("**");
        write_parameter(src, out, kwarg);
    }
}

fn write_generators(src: &str, out: &mut String, generators: &[ruff_python_ast::Comprehension]) {
    for generator in generators {
        out.push_str(if generator.is_async {
            " async for "
        } else {
            " for "
        });
        write_expr(src, out, &generator.target, TUPLE);
        out.push_str(" in ");
        write_expr(src, out, &generator.iter, next(TEST));
        for condition in &generator.ifs {
            out.push_str(" if ");
            write_expr(src, out, condition, next(TEST));
        }
    }
}

/// `repr(complex)` for a literal such as `1j`.
fn complex_repr(real: f64, imag: f64) -> String {
    let part = |value: f64| {
        let shown = float_repr(value);
        shown
            .strip_suffix(".0")
            .map_or(shown.clone(), str::to_owned)
    };
    if real == 0.0 && !real.is_sign_negative() {
        format!("{}j", part(imag))
    } else {
        let sign = if imag < 0.0 || (imag == 0.0 && imag.is_sign_negative()) {
            ""
        } else {
            "+"
        };
        format!("({}{sign}{}j)", part(real), part(imag))
    }
}

fn bytes_repr(data: &[u8]) -> String {
    let quote = if data.contains(&b'\'') && !data.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from("b");
    out.push(quote);
    for byte in data {
        match *byte {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            other if char::from(other) == quote => {
                out.push('\\');
                out.push(quote);
            }
            0x20..=0x7e => out.push(char::from(*byte)),
            other => out.push_str(&format!("\\x{other:02x}")),
        }
    }
    out.push(quote);
    out
}

/// `_write_constant` for floats and complex numbers: infinities spell as `1e309`.
fn constant_float(shown: String) -> String {
    shown
        .replace("inf", "1e309")
        .replace("nan", "(1e309-1e309)")
}

/// One piece of an f-string as CPython's `JoinedStr.values` holds it.
enum Piece<'a> {
    Text(String),
    Formatted(&'a ruff_python_ast::InterpolatedElement),
}

/// Merge literal text into pieces: adjacent text joins, empty text vanishes, and the
/// text of a `=` debug expression joins the text before it.
fn pieces<'a>(
    out: &mut Vec<Piece<'a>>,
    elements: impl Iterator<Item = &'a ruff_python_ast::InterpolatedStringElement>,
) {
    use ruff_python_ast::InterpolatedStringElement as Element;
    let mut pending = String::new();
    for element in elements {
        match element {
            Element::Literal(literal) => pending.push_str(&literal.value),
            Element::Interpolation(interpolation) => {
                if let Some(debug) = &interpolation.debug_text {
                    pending.push_str(debug.as_str());
                }
                if !pending.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut pending)));
                }
                out.push(Piece::Formatted(interpolation));
            }
        }
    }
    if !pending.is_empty() {
        out.push(Piece::Text(pending));
    }
}

/// Join pieces that two source parts left side by side (`'a' f'{b}'`, `f'{a}' 'b'`).
fn merge_adjacent_text(pieces: Vec<Piece<'_>>) -> Vec<Piece<'_>> {
    let mut out: Vec<Piece<'_>> = Vec::new();
    for piece in pieces {
        match (out.last_mut(), piece) {
            (Some(Piece::Text(previous)), Piece::Text(more)) => previous.push_str(&more),
            (_, piece) => out.push(piece),
        }
    }
    out
}

/// `_write_fstring_inner`'s text for literal text.
fn fstring_text(value: &str, is_format_spec: bool) -> String {
    let mut text = value.replace('{', "{{").replace('}', "}}");
    if is_format_spec {
        text = text
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
    }
    text
}

/// `visit_FormattedValue`.
fn formatted(src: &str, node: &ruff_python_ast::InterpolatedElement) -> String {
    use ruff_python_ast::ConversionFlag;
    let mut out = String::from("{");
    let mut inner = String::new();
    // A fresh unparser: the expression sits one level above TEST.
    write_expr(src, &mut inner, &node.expression, next(TEST));
    if inner.starts_with('{') {
        out.push(' ');
    }
    out.push_str(&inner);
    // `=` without a conversion or a format spec means `!r`.
    let conversion = match node.conversion {
        ConversionFlag::None if node.debug_text.is_some() && node.format_spec.is_none() => {
            ConversionFlag::Repr
        }
        other => other,
    };
    if let Some(byte) = conversion.to_byte() {
        out.push('!');
        out.push(char::from(byte));
    }
    if let Some(spec) = &node.format_spec {
        out.push(':');
        let mut parts = Vec::new();
        pieces(&mut parts, spec.elements.iter());
        for part in parts {
            match part {
                Piece::Text(text) => out.push_str(&fstring_text(&text, true)),
                Piece::Formatted(inner) => out.push_str(&formatted(src, inner)),
            }
        }
    }
    out.push('}');
    out
}

/// `c.encode("unicode_escape")` for one character that is not printable, or a backslash.
fn unicode_escape(c: char) -> String {
    match c {
        '\\' => "\\\\".into(),
        '\t' => "\\t".into(),
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        c if (c as u32) < 0x100 => format!("\\x{:02x}", c as u32),
        c if (c as u32) < 0x10000 => format!("\\u{:04x}", c as u32),
        c => format!("\\U{:08x}", c as u32),
    }
}

const ALL_QUOTES: [&str; 4] = ["'", "\"", "\"\"\"", "'''"];
const MULTI_QUOTES: [&str; 2] = ["\"\"\"", "'''"];

/// `_str_literal_helper(string, quote_types=..., escape_special_whitespace=True)`.
fn str_literal(value: &str, quote_types: &[&'static str]) -> (String, Vec<&'static str>) {
    let escaped: String = value
        .chars()
        .map(|c| {
            if c == '\\' || !printable(c) {
                unicode_escape(c)
            } else {
                c.to_string()
            }
        })
        .collect();
    let mut possible: Vec<&'static str> = quote_types.to_vec();
    if escaped.contains('\n') {
        possible.retain(|quote| MULTI_QUOTES.contains(quote));
    }
    possible.retain(|quote| !escaped.contains(quote));
    if possible.is_empty() {
        // No quote fits: fall back to `repr` of the original and keep the quote it chose.
        let shown = str_repr(value);
        let first = shown.chars().next().unwrap_or('\'');
        let quote = quote_types
            .iter()
            .find(|quote| quote.starts_with(first))
            .copied()
            .unwrap_or(if first == '"' { "\"" } else { "'" });
        let inner: String = shown
            .chars()
            .skip(1)
            .take(shown.chars().count() - 2)
            .collect();
        return (inner, vec![quote]);
    }
    let mut escaped = escaped;
    if let Some(last) = escaped.chars().last() {
        // Prefer a quote that does not end where the text does (a stable sort).
        possible.sort_by_key(|quote| quote.starts_with(last));
        if possible[0].starts_with(last) {
            escaped.pop();
            escaped.push('\\');
            escaped.push(last);
        }
    }
    (escaped, possible)
}

/// `visit_JoinedStr`.
fn fstring(src: &str, out: &mut String, value: &ruff_python_ast::FStringValue) {
    use ruff_python_ast::FStringPartRef;
    let mut parts: Vec<Piece<'_>> = Vec::new();
    for part in value.iter() {
        match part {
            FStringPartRef::Literal(literal) => {
                if !literal.value.is_empty() {
                    parts.push(Piece::Text(literal.value.to_string()));
                }
            }
            FStringPartRef::FString(inner) => pieces(&mut parts, inner.elements.iter()),
        }
    }
    let parts = merge_adjacent_text(parts);
    let inner: Vec<(String, bool)> = parts
        .iter()
        .map(|part| match part {
            Piece::Text(text) => (fstring_text(text, false), true),
            Piece::Formatted(node) => (formatted(src, node), false),
        })
        .collect();

    out.push('f');
    let mut quote_types: Vec<&'static str> = ALL_QUOTES.to_vec();
    let mut written: Vec<String> = Vec::new();
    let mut fallback = false;
    for (text, is_constant) in &inner {
        if *is_constant {
            let (shown, allowed) = str_literal(text, &quote_types);
            if !allowed.iter().any(|quote| quote_types.contains(quote)) {
                fallback = true;
                break;
            }
            quote_types = allowed;
            written.push(shown);
        } else {
            if text.contains('\n') {
                quote_types.retain(|quote| MULTI_QUOTES.contains(quote));
            }
            let fitting: Vec<&'static str> = quote_types
                .iter()
                .filter(|quote| !text.contains(**quote))
                .copied()
                .collect();
            if !fitting.is_empty() {
                quote_types = fitting;
            }
            written.push(text.clone());
        }
    }
    if fallback {
        quote_types = vec!["'''"];
        written = inner
            .iter()
            .map(|(text, is_constant)| {
                if *is_constant {
                    str_literal(text, &quote_types).0
                } else {
                    text.clone()
                }
            })
            .collect();
    }
    let quote = quote_types.first().copied().unwrap_or("'");
    out.push_str(quote);
    out.push_str(&written.concat());
    out.push_str(quote);
}

/// `visit_*` for one expression; `precedence` is what its parent set for it.
fn write_expr(src: &str, out: &mut String, expression: &Expr, precedence: u8) {
    match expression {
        Expr::BoolOp(node) => {
            let (word, level) = match node.op {
                BoolOp::And => ("and", AND),
                BoolOp::Or => ("or", OR),
            };
            delimited(out, precedence > level, |out| {
                let mut current = level;
                for (index, value) in node.values.iter().enumerate() {
                    if index > 0 {
                        out.push_str(&format!(" {word} "));
                    }
                    // Each operand is set one level higher than the one before.
                    current = next(current);
                    write_expr(src, out, value, current);
                }
            });
        }
        Expr::Named(node) => delimited(out, precedence > NAMED_EXPR, |out| {
            write_expr(src, out, &node.target, ATOM);
            out.push_str(" := ");
            write_expr(src, out, &node.value, ATOM);
        }),
        Expr::BinOp(node) => {
            let (symbol, level) = binary_operator(node.op);
            delimited(out, precedence > level, |out| {
                let (left, right) = if symbol == "**" {
                    (next(level), level)
                } else {
                    (level, next(level))
                };
                write_expr(src, out, &node.left, left);
                out.push_str(&format!(" {symbol} "));
                write_expr(src, out, &node.right, right);
            });
        }
        Expr::UnaryOp(node) => {
            let (symbol, level) = match node.op {
                UnaryOp::Invert => ("~", FACTOR),
                UnaryOp::Not => ("not", NOT),
                UnaryOp::UAdd => ("+", FACTOR),
                UnaryOp::USub => ("-", FACTOR),
            };
            delimited(out, precedence > level, |out| {
                out.push_str(symbol);
                if level != FACTOR {
                    out.push(' ');
                }
                write_expr(src, out, &node.operand, level);
            });
        }
        Expr::Lambda(node) => delimited(out, precedence > TEST, |out| {
            out.push_str("lambda");
            let mut parameters = String::new();
            if let Some(found) = &node.parameters {
                write_parameters(src, &mut parameters, found);
            }
            if !parameters.is_empty() {
                out.push(' ');
                out.push_str(&parameters);
            }
            out.push_str(": ");
            write_expr(src, out, &node.body, TEST);
        }),
        Expr::If(node) => delimited(out, precedence > TEST, |out| {
            write_expr(src, out, &node.body, next(TEST));
            out.push_str(" if ");
            write_expr(src, out, &node.test, next(TEST));
            out.push_str(" else ");
            write_expr(src, out, &node.orelse, TEST);
        }),
        Expr::Dict(node) => {
            out.push('{');
            interleave(out, &node.items, |out, item| match &item.key {
                Some(key) => {
                    write_expr(src, out, key, TEST);
                    out.push_str(": ");
                    write_expr(src, out, &item.value, TEST);
                }
                None => {
                    out.push_str("**");
                    write_expr(src, out, &item.value, EXPR);
                }
            });
            out.push('}');
        }
        Expr::Set(node) => {
            if node.elts.is_empty() {
                out.push_str("{*()}");
            } else {
                out.push('{');
                interleave(out, &node.elts, |out, item| {
                    write_expr(src, out, item, TEST)
                });
                out.push('}');
            }
        }
        Expr::ListComp(node) => {
            out.push('[');
            write_expr(src, out, &node.elt, TEST);
            write_generators(src, out, &node.generators);
            out.push(']');
        }
        Expr::SetComp(node) => {
            out.push('{');
            write_expr(src, out, &node.elt, TEST);
            write_generators(src, out, &node.generators);
            out.push('}');
        }
        Expr::DictComp(node) => {
            out.push('{');
            if let Some(key) = &node.key {
                write_expr(src, out, key, TEST);
            }
            out.push_str(": ");
            write_expr(src, out, &node.value, TEST);
            write_generators(src, out, &node.generators);
            out.push('}');
        }
        Expr::Generator(node) => {
            out.push('(');
            write_expr(src, out, &node.elt, TEST);
            write_generators(src, out, &node.generators);
            out.push(')');
        }
        Expr::Await(node) => delimited(out, precedence > AWAIT, |out| {
            out.push_str("await ");
            write_expr(src, out, &node.value, ATOM);
        }),
        Expr::Yield(node) => delimited(out, precedence > YIELD, |out| {
            out.push_str("yield");
            if let Some(value) = &node.value {
                out.push(' ');
                write_expr(src, out, value, ATOM);
            }
        }),
        Expr::YieldFrom(node) => delimited(out, precedence > YIELD, |out| {
            out.push_str("yield from ");
            write_expr(src, out, &node.value, ATOM);
        }),
        Expr::Compare(node) => delimited(out, precedence > CMP, |out| {
            let [first, rest @ ..] = &*node.operands else {
                return;
            };
            write_expr(src, out, first, next(CMP));
            for (operator, operand) in node.ops.iter().zip(rest) {
                out.push_str(&format!(" {} ", comparison(*operator)));
                write_expr(src, out, operand, next(CMP));
            }
        }),
        Expr::Call(node) => {
            write_expr(src, out, &node.func, ATOM);
            out.push('(');
            let mut first = true;
            for argument in &node.arguments.args {
                if !first {
                    out.push_str(", ");
                }
                first = false;
                write_expr(src, out, argument, TEST);
            }
            for keyword in &node.arguments.keywords {
                if !first {
                    out.push_str(", ");
                }
                first = false;
                match &keyword.arg {
                    Some(name) => {
                        out.push_str(name.as_str());
                        out.push('=');
                    }
                    None => out.push_str("**"),
                }
                write_expr(src, out, &keyword.value, TEST);
            }
            out.push(')');
        }
        Expr::FString(node) => fstring(src, out, &node.value),
        Expr::TString(_) => out.push_str("t''"),
        Expr::StringLiteral(node) => {
            // CPython's `kind` is "u" only for a lowercase `u` prefix on the first part.
            let first_is_unicode = node.value.iter().next().is_some_and(|part| {
                part.flags.prefix().is_unicode()
                    && src.as_bytes().get(usize::from(part.range.start())) == Some(&b'u')
            });
            if first_is_unicode {
                out.push('u');
            }
            out.push_str(&str_repr(node.value.to_str()));
        }
        Expr::BytesLiteral(node) => {
            let data: Vec<u8> = node.value.bytes().collect();
            out.push_str(&bytes_repr(&data));
        }
        Expr::NumberLiteral(node) => match &node.value {
            Number::Int(int) => out.push_str(&int.to_string()),
            Number::Float(float) => out.push_str(&constant_float(float_repr(*float))),
            Number::Complex { real, imag } => {
                out.push_str(&constant_float(complex_repr(*real, *imag)));
            }
        },
        Expr::BooleanLiteral(node) => out.push_str(if node.value { "True" } else { "False" }),
        Expr::NoneLiteral(_) => out.push_str("None"),
        Expr::EllipsisLiteral(_) => out.push_str("..."),
        Expr::Attribute(node) => {
            write_expr(src, out, &node.value, ATOM);
            // `3 .real`: a dot straight after an integer literal would read as a float.
            if matches!(
                node.value.as_ref(),
                Expr::NumberLiteral(number) if matches!(number.value, Number::Int(_))
            ) {
                out.push(' ');
            }
            out.push('.');
            out.push_str(node.attr.as_str());
        }
        Expr::Subscript(node) => {
            write_expr(src, out, &node.value, ATOM);
            out.push('[');
            match node.slice.as_ref() {
                Expr::Tuple(tuple) if !tuple.elts.is_empty() => items_view(src, out, &tuple.elts),
                other => write_expr(src, out, other, TEST),
            }
            out.push(']');
        }
        Expr::Starred(node) => {
            out.push('*');
            write_expr(src, out, &node.value, EXPR);
        }
        Expr::Name(node) => out.push_str(node.id.as_str()),
        Expr::List(node) => {
            out.push('[');
            interleave(out, &node.elts, |out, item| {
                write_expr(src, out, item, TEST)
            });
            out.push(']');
        }
        Expr::Tuple(node) => delimited(out, node.elts.is_empty() || precedence > TUPLE, |out| {
            items_view(src, out, &node.elts);
        }),
        Expr::Slice(node) => {
            if let Some(lower) = &node.lower {
                write_expr(src, out, lower, TEST);
            }
            out.push(':');
            if let Some(upper) = &node.upper {
                write_expr(src, out, upper, TEST);
            }
            if let Some(step) = &node.step {
                out.push(':');
                write_expr(src, out, step, TEST);
            }
        }
        Expr::IpyEscapeCommand(node) => out.push_str(&node.value),
    }
}

/// `ExprContext` is not part of the text; kept so callers can match on it without a second import.
pub fn is_store(context: ExprContext) -> bool {
    matches!(context, ExprContext::Store)
}

/// Python's `str.startswith(prefixes)` helper for already unparsed text.
pub fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}

/// `text.split(".")[0]`.
pub fn first_segment(value: &str) -> &str {
    value.split('.').next().unwrap_or(value)
}

/// Whether `value` is entirely characters the identifier class allows; see `structure`.
pub fn is_word_text(value: &str) -> bool {
    value.chars().all(text::is_word)
}
