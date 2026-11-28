//! A CPython-shaped view of Ruff's Python syntax tree.
//!
//! The Python modules call `ast.parse` and `ast.walk`. Their results depend on
//! two CPython details that Ruff's tree does not share: which sources are
//! rejected, and the order `ast.walk` visits nodes (breadth first, over fields
//! in CPython's order, which is not source order). This module reproduces both,
//! so callers can treat the result as the tree CPython 3.12 would have built.
//!
//! Ruff's types are internal to Ruff and unstable; they are pinned exactly, and
//! nothing outside this module touches them.

use crate::text;
use ruff_python_ast::{
    self as ast, BoolOp, CmpOp, Expr, ExprContext, InterpolatedStringElement, Operator, Pattern,
    PythonVersion, Stmt, TypeParam, UnaryOp,
};
use ruff_python_parser::{Mode, ParseOptions, Parsed};
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};
use std::collections::VecDeque;

/// Why a source did not produce a tree. Python raised these from `ast.parse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseFailure {
    /// `SyntaxError`, including `IndentationError` and `TabError`.
    Syntax(String),
    /// `RecursionError`: the tree is nested more deeply than CPython builds.
    Recursion,
}

/// CPython refuses to build an `ast` object nested deeper than three times its
/// recursion limit minus three per frame the caller already uses. The limit is
/// measured here at the shallowest caller (a script calling `ast.parse` directly),
/// where it counts 2998 levels of statements and expressions. A deeper caller
/// lowers the figure, so inputs close to it are not reproducible.
pub const MAX_TREE_DEPTH: usize = 2998;

/// The tokenizer's bracket limit (`MAXLEVEL`) and indentation limit (`MAXINDENT`).
const MAX_BRACKET_DEPTH: usize = 200;
const MAX_INDENT_LEVELS: usize = 100;

pub struct PyModule {
    parsed: Parsed<ast::ModModule>,
    index: LineIndex,
    source: String,
}

/// Stack for the thread that parses. Ruff's parser and the tree's destructor recurse
/// once per nesting level, and a hostile file can nest deeply before CPython's own
/// limits would reject it, so the whole lifetime of the tree runs on a roomy stack.
const PARSE_STACK_BYTES: usize = 512 * 1024 * 1024;

/// Parse `source` as CPython 3.12 would and hand the tree to `inspect`, or say why
/// CPython would not produce one. The tree never leaves the call: it is built, used
/// and dropped on one large-stack thread.
pub fn with_module<R: Send>(
    source: &str,
    inspect: impl FnOnce(&PyModule) -> R + Send,
) -> Result<R, ParseFailure> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(PARSE_STACK_BYTES)
            .spawn_scoped(scope, move || parse(source).map(|module| inspect(&module)))
            .map_or_else(
                |_| Err(ParseFailure::Recursion),
                |handle| handle.join().unwrap_or(Err(ParseFailure::Recursion)),
            )
    })
}

fn parse(source: &str) -> Result<PyModule, ParseFailure> {
    if source.contains('\0') {
        return Err(ParseFailure::Syntax(
            "source code string cannot contain null bytes".into(),
        ));
    }
    if source.starts_with('\u{feff}') {
        return Err(ParseFailure::Syntax(
            "invalid non-printable character U+FEFF".into(),
        ));
    }
    // CPython translates CRLF and lone CR to LF before tokenizing, which also
    // changes the value of a string literal that spans lines. Ruff keeps the raw text.
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let source = source.as_str();
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY312);
    let parsed = ruff_python_parser::parse_unchecked(source, options)
        .try_into_module()
        .expect("module mode yields a module");
    if let Some(error) = parsed.errors().first() {
        return Err(ParseFailure::Syntax(error.to_string()));
    }
    if let Some(error) = parsed.unsupported_syntax_errors().first() {
        return Err(ParseFailure::Syntax(error.to_string()));
    }
    let module = PyModule {
        index: LineIndex::from_source_text(source),
        parsed,
        source: source.to_owned(),
    };
    module.check_tokenizer_rules()?;
    module.check_calls()?;
    if module.depth() > MAX_TREE_DEPTH {
        return Err(ParseFailure::Recursion);
    }
    Ok(module)
}

/// One node as `ast.walk` yields it: a CPython node type and its line, if it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visit {
    pub kind: &'static str,
    pub line: Option<usize>,
}

/// A node of the CPython-shaped tree. Some are Ruff nodes; the rest are the
/// nodes CPython has and Ruff folds away (`arguments`, `elif` chains, f-string parts).
#[derive(Clone, Copy)]
pub enum Node<'a> {
    Module(&'a ast::ModModule),
    Stmt(&'a Stmt),
    /// The `If` CPython nests inside `orelse` for an `elif`; the slice starts at that `elif`.
    Elif(&'a [ast::ElifElseClause]),
    Expr(&'a Expr),
    /// A bare generator argument, `f(x for x in y)`: CPython places it at the call's `(`.
    Located(&'a Expr, TextSize),
    Leaf(&'static str),
    Arguments(Option<&'a ast::Parameters>),
    Arg(&'a ast::Parameter),
    Keyword(&'a ast::Keyword),
    Alias(&'a ast::Alias),
    WithItem(&'a ast::WithItem),
    Comprehension(&'a ast::Comprehension),
    Handler(&'a ast::ExceptHandlerExceptHandler),
    MatchCase(&'a ast::MatchCase),
    Pattern(&'a Pattern),
    TypeParam(&'a TypeParam),
    /// A run of literal text inside an f-string, starting at this offset.
    Text(TextSize),
    Formatted(&'a ast::InterpolatedElement),
    FormatSpec(&'a ast::InterpolatedStringFormatSpec),
}

enum Piece<'a> {
    Text(TextSize),
    Formatted(&'a ast::InterpolatedElement),
}

/// The pieces of an f-string as CPython builds them: adjacent literal text,
/// including `=` debug text, merges into one constant and empty text vanishes.
fn pieces<'a>(elements: impl Iterator<Item = &'a InterpolatedStringElement>) -> Vec<Piece<'a>> {
    let mut out = Vec::new();
    let mut pending: Option<TextSize> = None;
    for element in elements {
        match element {
            InterpolatedStringElement::Literal(literal) => {
                if !literal.value.is_empty() {
                    pending.get_or_insert(literal.range.start());
                }
            }
            InterpolatedStringElement::Interpolation(interpolation) => {
                if interpolation.debug_text.is_some() {
                    pending.get_or_insert(interpolation.range.start() + TextSize::new(1));
                }
                if let Some(start) = pending.take() {
                    out.push(Piece::Text(start));
                }
                out.push(Piece::Formatted(interpolation));
            }
        }
    }
    if let Some(start) = pending {
        out.push(Piece::Text(start));
    }
    out
}

fn fstring_pieces(value: &ast::FStringValue) -> Vec<Piece<'_>> {
    // A run of literal text starts where its first token does, even when that
    // token is an empty plain string; the run only becomes a constant if it has text.
    let mut out = Vec::new();
    let mut start: Option<TextSize> = None;
    let mut has_text = false;
    for part in value.iter() {
        match part {
            ast::FStringPartRef::Literal(literal) => {
                start.get_or_insert(literal.range.start());
                has_text |= !literal.value.is_empty();
            }
            ast::FStringPartRef::FString(fstring) => {
                for piece in pieces(fstring.elements.iter()) {
                    match piece {
                        Piece::Text(at) => {
                            start.get_or_insert(at);
                            has_text = true;
                        }
                        Piece::Formatted(element) => {
                            if let (Some(at), true) = (start.take(), has_text) {
                                out.push(Piece::Text(at));
                            }
                            has_text = false;
                            start = None;
                            out.push(Piece::Formatted(element));
                        }
                    }
                }
            }
        }
    }
    if let (Some(at), true) = (start, has_text) {
        out.push(Piece::Text(at));
    }
    out
}

fn context(ctx: ExprContext) -> Node<'static> {
    Node::Leaf(match ctx {
        ExprContext::Load => "Load",
        ExprContext::Store => "Store",
        ExprContext::Del => "Del",
        ExprContext::Invalid => "Load",
    })
}

fn operator(op: Operator) -> &'static str {
    match op {
        Operator::Add => "Add",
        Operator::Sub => "Sub",
        Operator::Mult => "Mult",
        Operator::MatMult => "MatMult",
        Operator::Div => "Div",
        Operator::Mod => "Mod",
        Operator::Pow => "Pow",
        Operator::LShift => "LShift",
        Operator::RShift => "RShift",
        Operator::BitOr => "BitOr",
        Operator::BitXor => "BitXor",
        Operator::BitAnd => "BitAnd",
        Operator::FloorDiv => "FloorDiv",
    }
}

fn comparison(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Eq => "Eq",
        CmpOp::NotEq => "NotEq",
        CmpOp::Lt => "Lt",
        CmpOp::LtE => "LtE",
        CmpOp::Gt => "Gt",
        CmpOp::GtE => "GtE",
        CmpOp::Is => "Is",
        CmpOp::IsNot => "IsNot",
        CmpOp::In => "In",
        CmpOp::NotIn => "NotIn",
    }
}

fn stmts<'a>(out: &mut Vec<Node<'a>>, body: &'a [Stmt]) {
    out.extend(body.iter().map(Node::Stmt));
}

fn exprs<'a>(out: &mut Vec<Node<'a>>, list: impl IntoIterator<Item = &'a Expr>) {
    out.extend(list.into_iter().map(Node::Expr));
}

fn optional<'a>(out: &mut Vec<Node<'a>>, value: Option<&'a Expr>) {
    out.extend(value.map(Node::Expr));
}

fn type_params<'a>(out: &mut Vec<Node<'a>>, params: Option<&'a ast::TypeParams>) {
    if let Some(params) = params {
        out.extend(params.type_params.iter().map(Node::TypeParam));
    }
}

fn call_arguments<'a>(out: &mut Vec<Node<'a>>, arguments: &'a ast::Arguments) {
    for argument in &arguments.args {
        out.push(match argument {
            Expr::Generator(generator) if !generator.parenthesized => {
                Node::Located(argument, arguments.range.start())
            }
            _ => Node::Expr(argument),
        });
    }
    out.extend(arguments.keywords.iter().map(Node::Keyword));
}

fn else_body<'a>(out: &mut Vec<Node<'a>>, clauses: &'a [ast::ElifElseClause]) {
    match clauses.first() {
        None => {}
        Some(first) if first.test.is_some() => out.push(Node::Elif(clauses)),
        Some(first) => stmts(out, &first.body),
    }
}

/// The children of `node` in CPython's field order (`ast.iter_child_nodes`).
pub fn children<'a>(node: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    match node {
        Node::Module(module) => stmts(&mut out, &module.body),
        Node::Stmt(stmt) => stmt_children(&mut out, stmt),
        Node::Elif(clauses) => {
            let first = &clauses[0];
            optional(&mut out, first.test.as_ref());
            stmts(&mut out, &first.body);
            else_body(&mut out, &clauses[1..]);
        }
        Node::Expr(expr) | Node::Located(expr, _) => expr_children(&mut out, expr),
        Node::Leaf(_) | Node::Alias(_) | Node::Text(_) => {}
        Node::Arguments(parameters) => {
            if let Some(parameters) = parameters {
                out.extend(
                    parameters
                        .posonlyargs
                        .iter()
                        .map(|p| Node::Arg(&p.parameter)),
                );
                out.extend(parameters.args.iter().map(|p| Node::Arg(&p.parameter)));
                out.extend(parameters.vararg.as_deref().map(Node::Arg));
                out.extend(
                    parameters
                        .kwonlyargs
                        .iter()
                        .map(|p| Node::Arg(&p.parameter)),
                );
                exprs(
                    &mut out,
                    parameters
                        .kwonlyargs
                        .iter()
                        .filter_map(|p| p.default.as_deref()),
                );
                out.extend(parameters.kwarg.as_deref().map(Node::Arg));
                exprs(
                    &mut out,
                    parameters
                        .posonlyargs
                        .iter()
                        .chain(parameters.args.iter())
                        .filter_map(|p| p.default.as_deref()),
                );
            }
        }
        Node::Arg(parameter) => optional(&mut out, parameter.annotation.as_deref()),
        Node::Keyword(keyword) => out.push(Node::Expr(&keyword.value)),
        Node::WithItem(item) => {
            out.push(Node::Expr(&item.context_expr));
            optional(&mut out, item.optional_vars.as_deref());
        }
        Node::Comprehension(comprehension) => {
            out.push(Node::Expr(&comprehension.target));
            out.push(Node::Expr(&comprehension.iter));
            exprs(&mut out, comprehension.ifs.iter());
        }
        Node::Handler(handler) => {
            optional(&mut out, handler.type_.as_deref());
            stmts(&mut out, &handler.body);
        }
        Node::MatchCase(case) => {
            out.push(Node::Pattern(&case.pattern));
            optional(&mut out, case.guard.as_deref());
            stmts(&mut out, &case.body);
        }
        Node::Pattern(pattern) => match pattern {
            Pattern::MatchValue(value) => out.push(Node::Expr(&value.value)),
            Pattern::MatchSingleton(_) | Pattern::MatchStar(_) => {}
            Pattern::MatchSequence(sequence) => {
                out.extend(sequence.patterns.iter().map(Node::Pattern));
            }
            Pattern::MatchMapping(mapping) => {
                exprs(&mut out, mapping.keys.iter());
                out.extend(mapping.patterns.iter().map(Node::Pattern));
            }
            Pattern::MatchClass(class) => {
                out.push(Node::Expr(&class.cls));
                out.extend(class.arguments.patterns.iter().map(Node::Pattern));
                out.extend(
                    class
                        .arguments
                        .keywords
                        .iter()
                        .map(|keyword| Node::Pattern(&keyword.pattern)),
                );
            }
            Pattern::MatchAs(matched) => {
                out.extend(matched.pattern.as_deref().map(Node::Pattern));
            }
            Pattern::MatchOr(alternatives) => {
                out.extend(alternatives.patterns.iter().map(Node::Pattern));
            }
        },
        Node::TypeParam(param) => {
            if let TypeParam::TypeVar(variable) = param {
                optional(&mut out, variable.bound.as_deref());
            }
        }
        Node::Formatted(element) => {
            out.push(Node::Expr(&element.expression));
            out.extend(element.format_spec.as_deref().map(Node::FormatSpec));
        }
        Node::FormatSpec(spec) => {
            for piece in pieces(spec.elements.iter()) {
                out.push(match piece {
                    Piece::Text(start) => Node::Text(start),
                    Piece::Formatted(element) => Node::Formatted(element),
                });
            }
        }
    }
    out
}

fn stmt_children<'a>(out: &mut Vec<Node<'a>>, stmt: &'a Stmt) {
    match stmt {
        Stmt::FunctionDef(function) => {
            out.push(Node::Arguments(Some(&function.parameters)));
            stmts(out, &function.body);
            exprs(out, function.decorator_list.iter().map(|d| &d.expression));
            optional(out, function.returns.as_deref());
            type_params(out, function.type_params.as_deref());
        }
        Stmt::ClassDef(class) => {
            if let Some(arguments) = &class.arguments {
                call_arguments(out, arguments);
            }
            stmts(out, &class.body);
            exprs(out, class.decorator_list.iter().map(|d| &d.expression));
            type_params(out, class.type_params.as_deref());
        }
        Stmt::Return(statement) => optional(out, statement.value.as_deref()),
        Stmt::Delete(statement) => exprs(out, statement.targets.iter()),
        Stmt::TypeAlias(alias) => {
            out.push(Node::Expr(&alias.name));
            type_params(out, alias.type_params.as_deref());
            out.push(Node::Expr(&alias.value));
        }
        Stmt::Assign(assign) => {
            exprs(out, assign.targets.iter());
            out.push(Node::Expr(&assign.value));
        }
        Stmt::AugAssign(assign) => {
            out.push(Node::Expr(&assign.target));
            out.push(Node::Leaf(operator(assign.op)));
            out.push(Node::Expr(&assign.value));
        }
        Stmt::AnnAssign(assign) => {
            out.push(Node::Expr(&assign.target));
            out.push(Node::Expr(&assign.annotation));
            optional(out, assign.value.as_deref());
        }
        Stmt::For(statement) => {
            out.push(Node::Expr(&statement.target));
            out.push(Node::Expr(&statement.iter));
            stmts(out, &statement.body);
            stmts(out, &statement.orelse);
        }
        Stmt::While(statement) => {
            out.push(Node::Expr(&statement.test));
            stmts(out, &statement.body);
            stmts(out, &statement.orelse);
        }
        Stmt::If(statement) => {
            out.push(Node::Expr(&statement.test));
            stmts(out, &statement.body);
            else_body(out, &statement.elif_else_clauses);
        }
        Stmt::With(statement) => {
            out.extend(statement.items.iter().map(Node::WithItem));
            stmts(out, &statement.body);
        }
        Stmt::Match(statement) => {
            out.push(Node::Expr(&statement.subject));
            out.extend(statement.cases.iter().map(Node::MatchCase));
        }
        Stmt::Raise(statement) => {
            optional(out, statement.exc.as_deref());
            optional(out, statement.cause.as_deref());
        }
        Stmt::Try(statement) => {
            stmts(out, &statement.body);
            out.extend(statement.handlers.iter().map(|handler| {
                let ast::ExceptHandler::ExceptHandler(handler) = handler;
                Node::Handler(handler)
            }));
            stmts(out, &statement.orelse);
            stmts(out, &statement.finalbody);
        }
        Stmt::Assert(statement) => {
            out.push(Node::Expr(&statement.test));
            optional(out, statement.msg.as_deref());
        }
        Stmt::Import(statement) => out.extend(statement.names.iter().map(Node::Alias)),
        Stmt::ImportFrom(statement) => out.extend(statement.names.iter().map(Node::Alias)),
        Stmt::Expr(statement) => out.push(Node::Expr(&statement.value)),
        Stmt::Global(_)
        | Stmt::Nonlocal(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::IpyEscapeCommand(_) => {}
    }
}

fn generators<'a>(out: &mut Vec<Node<'a>>, list: &'a [ast::Comprehension]) {
    out.extend(list.iter().map(Node::Comprehension));
}

fn expr_children<'a>(out: &mut Vec<Node<'a>>, expr: &'a Expr) {
    match expr {
        Expr::BoolOp(node) => {
            out.push(Node::Leaf(match node.op {
                BoolOp::And => "And",
                BoolOp::Or => "Or",
            }));
            exprs(out, node.values.iter());
        }
        Expr::Named(node) => {
            out.push(Node::Expr(&node.target));
            out.push(Node::Expr(&node.value));
        }
        Expr::BinOp(node) => {
            out.push(Node::Expr(&node.left));
            out.push(Node::Leaf(operator(node.op)));
            out.push(Node::Expr(&node.right));
        }
        Expr::UnaryOp(node) => {
            out.push(Node::Leaf(match node.op {
                UnaryOp::Invert => "Invert",
                UnaryOp::Not => "Not",
                UnaryOp::UAdd => "UAdd",
                UnaryOp::USub => "USub",
            }));
            out.push(Node::Expr(&node.operand));
        }
        Expr::Lambda(node) => {
            out.push(Node::Arguments(node.parameters.as_deref()));
            out.push(Node::Expr(&node.body));
        }
        Expr::If(node) => {
            out.push(Node::Expr(&node.test));
            out.push(Node::Expr(&node.body));
            out.push(Node::Expr(&node.orelse));
        }
        Expr::Dict(node) => {
            exprs(out, node.items.iter().filter_map(|item| item.key.as_ref()));
            exprs(out, node.items.iter().map(|item| &item.value));
        }
        Expr::Set(node) => exprs(out, node.elts.iter()),
        Expr::ListComp(node) => {
            out.push(Node::Expr(&node.elt));
            generators(out, &node.generators);
        }
        Expr::SetComp(node) => {
            out.push(Node::Expr(&node.elt));
            generators(out, &node.generators);
        }
        Expr::DictComp(node) => {
            optional(out, node.key.as_deref());
            out.push(Node::Expr(&node.value));
            generators(out, &node.generators);
        }
        Expr::Generator(node) => {
            out.push(Node::Expr(&node.elt));
            generators(out, &node.generators);
        }
        Expr::Await(node) => out.push(Node::Expr(&node.value)),
        Expr::Yield(node) => optional(out, node.value.as_deref()),
        Expr::YieldFrom(node) => out.push(Node::Expr(&node.value)),
        Expr::Compare(node) => {
            out.push(Node::Expr(&node.operands[0]));
            out.extend(node.ops.iter().map(|op| Node::Leaf(comparison(*op))));
            exprs(out, node.operands[1..].iter());
        }
        Expr::Call(node) => {
            out.push(Node::Expr(&node.func));
            call_arguments(out, &node.arguments);
        }
        Expr::FString(node) => {
            for piece in fstring_pieces(&node.value) {
                out.push(match piece {
                    Piece::Text(start) => Node::Text(start),
                    Piece::Formatted(element) => Node::Formatted(element),
                });
            }
        }
        Expr::Attribute(node) => {
            out.push(Node::Expr(&node.value));
            out.push(context(node.ctx));
        }
        Expr::Subscript(node) => {
            out.push(Node::Expr(&node.value));
            out.push(Node::Expr(&node.slice));
            out.push(context(node.ctx));
        }
        Expr::Starred(node) => {
            out.push(Node::Expr(&node.value));
            out.push(context(node.ctx));
        }
        Expr::Name(node) => out.push(context(node.ctx)),
        Expr::List(node) => {
            exprs(out, node.elts.iter());
            out.push(context(node.ctx));
        }
        Expr::Tuple(node) => {
            exprs(out, node.elts.iter());
            out.push(context(node.ctx));
        }
        Expr::Slice(node) => {
            optional(out, node.lower.as_deref());
            optional(out, node.upper.as_deref());
            optional(out, node.step.as_deref());
        }
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_)
        | Expr::TString(_)
        | Expr::IpyEscapeCommand(_) => {}
    }
}

fn pattern_kind(pattern: &Pattern) -> &'static str {
    match pattern {
        Pattern::MatchValue(_) => "MatchValue",
        Pattern::MatchSingleton(_) => "MatchSingleton",
        Pattern::MatchSequence(_) => "MatchSequence",
        Pattern::MatchMapping(_) => "MatchMapping",
        Pattern::MatchClass(_) => "MatchClass",
        Pattern::MatchStar(_) => "MatchStar",
        Pattern::MatchAs(_) => "MatchAs",
        Pattern::MatchOr(_) => "MatchOr",
    }
}

fn stmt_kind(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::FunctionDef(function) if function.is_async => "AsyncFunctionDef",
        Stmt::FunctionDef(_) => "FunctionDef",
        Stmt::ClassDef(_) => "ClassDef",
        Stmt::Return(_) => "Return",
        Stmt::Delete(_) => "Delete",
        Stmt::TypeAlias(_) => "TypeAlias",
        Stmt::Assign(_) => "Assign",
        Stmt::AugAssign(_) => "AugAssign",
        Stmt::AnnAssign(_) => "AnnAssign",
        Stmt::For(node) if node.is_async => "AsyncFor",
        Stmt::For(_) => "For",
        Stmt::While(_) => "While",
        Stmt::If(_) => "If",
        Stmt::With(node) if node.is_async => "AsyncWith",
        Stmt::With(_) => "With",
        Stmt::Match(_) => "Match",
        Stmt::Raise(_) => "Raise",
        Stmt::Try(node) if node.is_star => "TryStar",
        Stmt::Try(_) => "Try",
        Stmt::Assert(_) => "Assert",
        Stmt::Import(_) => "Import",
        Stmt::ImportFrom(_) => "ImportFrom",
        Stmt::Global(_) => "Global",
        Stmt::Nonlocal(_) => "Nonlocal",
        Stmt::Expr(_) => "Expr",
        Stmt::Pass(_) => "Pass",
        Stmt::Break(_) => "Break",
        Stmt::Continue(_) => "Continue",
        Stmt::IpyEscapeCommand(_) => "Expr",
    }
}

fn expr_kind(expr: &Expr) -> &'static str {
    match expr {
        Expr::BoolOp(_) => "BoolOp",
        Expr::Named(_) => "NamedExpr",
        Expr::BinOp(_) => "BinOp",
        Expr::UnaryOp(_) => "UnaryOp",
        Expr::Lambda(_) => "Lambda",
        Expr::If(_) => "IfExp",
        Expr::Dict(_) => "Dict",
        Expr::Set(_) => "Set",
        Expr::ListComp(_) => "ListComp",
        Expr::SetComp(_) => "SetComp",
        Expr::DictComp(_) => "DictComp",
        Expr::Generator(_) => "GeneratorExp",
        Expr::Await(_) => "Await",
        Expr::Yield(_) => "Yield",
        Expr::YieldFrom(_) => "YieldFrom",
        Expr::Compare(_) => "Compare",
        Expr::Call(_) => "Call",
        Expr::FString(_) => "JoinedStr",
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => "Constant",
        Expr::Attribute(_) => "Attribute",
        Expr::Subscript(_) => "Subscript",
        Expr::Starred(_) => "Starred",
        Expr::Name(_) => "Name",
        Expr::List(_) => "List",
        Expr::Tuple(_) => "Tuple",
        Expr::Slice(_) => "Slice",
        Expr::TString(_) | Expr::IpyEscapeCommand(_) => "Constant",
    }
}

impl PyModule {
    fn line_at(&self, offset: TextSize) -> usize {
        self.index.line_index(offset).get()
    }

    /// The first source line of the `def`, `async` or `class` keyword. Ruff's range
    /// for a decorated definition starts at its first decorator; CPython's does not.
    fn definition_line(&self, decorators: &[ast::Decorator], fallback: TextSize) -> usize {
        let Some(last) = decorators.last() else {
            return self.line_at(fallback);
        };
        let rest = &self.source[last.range.end().to_usize()..];
        let bytes = rest.as_bytes();
        let mut offset = 0;
        while offset < bytes.len() {
            match bytes[offset] {
                b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' => offset += 1,
                b'#' => {
                    while offset < bytes.len() && !matches!(bytes[offset], b'\n' | b'\r') {
                        offset += 1;
                    }
                }
                b'\\' if matches!(bytes.get(offset + 1), Some(b'\n' | b'\r')) => offset += 2,
                _ => break,
            }
        }
        self.line_at(last.range.end() + TextSize::try_from(offset).unwrap_or_default())
    }

    /// The kind and line of a node, as CPython records them.
    pub fn visit(&self, node: Node<'_>) -> Visit {
        let at = |offset: TextSize| Some(self.line_at(offset));
        let (kind, line) = match node {
            Node::Module(_) => ("Module", None),
            Node::Stmt(stmt) => {
                let line = match stmt {
                    Stmt::FunctionDef(function) => {
                        Some(self.definition_line(&function.decorator_list, function.range.start()))
                    }
                    Stmt::ClassDef(class) => {
                        Some(self.definition_line(&class.decorator_list, class.range.start()))
                    }
                    _ => at(stmt.range().start()),
                };
                (stmt_kind(stmt), line)
            }
            Node::Elif(clauses) => ("If", at(clauses[0].range.start())),
            Node::Expr(expr) => (expr_kind(expr), at(expr.range().start())),
            Node::Located(expr, start) => (expr_kind(expr), at(start)),
            Node::Leaf(name) => (name, None),
            Node::Arguments(_) => ("arguments", None),
            Node::Arg(parameter) => ("arg", at(parameter.range.start())),
            Node::Keyword(keyword) => ("keyword", at(keyword.range.start())),
            Node::Alias(alias) => ("alias", at(alias.range.start())),
            Node::WithItem(_) => ("withitem", None),
            Node::Comprehension(_) => ("comprehension", None),
            Node::Handler(handler) => ("ExceptHandler", at(handler.range.start())),
            Node::MatchCase(_) => ("match_case", None),
            Node::Pattern(pattern) => (pattern_kind(pattern), at(pattern.range().start())),
            Node::TypeParam(param) => (
                match param {
                    TypeParam::TypeVar(_) => "TypeVar",
                    TypeParam::TypeVarTuple(_) => "TypeVarTuple",
                    TypeParam::ParamSpec(_) => "ParamSpec",
                },
                at(param.range().start()),
            ),
            Node::Text(start) => ("Constant", at(start)),
            Node::Formatted(element) => ("FormattedValue", at(element.range.start())),
            Node::FormatSpec(spec) => ("JoinedStr", at(spec.range.start())),
        };
        Visit { kind, line }
    }

    pub fn root(&self) -> Node<'_> {
        Node::Module(self.parsed.syntax())
    }

    /// `ast.walk`: breadth first, children in CPython field order.
    pub fn walk(&self) -> Walk<'_> {
        Walk {
            queue: VecDeque::from([self.root()]),
        }
    }

    /// `ast.walk(node)` for a node of this tree: that node, then its descendants.
    pub fn walk_from<'a>(&self, node: Node<'a>) -> Walk<'a> {
        Walk {
            queue: VecDeque::from([node]),
        }
    }

    /// The deepest nesting of the tree, counting the module as depth one.
    pub fn depth(&self) -> usize {
        let mut deepest = 0;
        let mut stack = vec![(self.root(), 1usize)];
        while let Some((node, depth)) = stack.pop() {
            // Operator and context leaves are never recursed into, so they do not count.
            if matches!(node, Node::Leaf(_)) {
                continue;
            }
            deepest = deepest.max(depth);
            stack.extend(children(node).into_iter().map(|child| (child, depth + 1)));
        }
        deepest
    }

    /// Source line of a node's first token.
    pub fn line(&self, offset: TextSize) -> usize {
        self.line_at(offset)
    }

    /// The text the tree was parsed from, after line endings were normalised.
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn module(&self) -> &ast::ModModule {
        self.parsed.syntax()
    }

    /// `ast.get_docstring(node, clean=True)` for a module, function or class node.
    pub fn docstring(&self, node: Node<'_>) -> Option<String> {
        let body: &[Stmt] = match node {
            Node::Module(module) => &module.body,
            Node::Stmt(Stmt::FunctionDef(function)) => &function.body,
            Node::Stmt(Stmt::ClassDef(class)) => &class.body,
            _ => return None,
        };
        let Some(Stmt::Expr(first)) = body.first() else {
            return None;
        };
        let Expr::StringLiteral(literal) = first.value.as_ref() else {
            return None;
        };
        Some(cleandoc(literal.value.to_str()))
    }

    /// The first statement of a node's body, which is where a docstring lives.
    pub fn first_statement_line(&self, node: Node<'_>) -> Option<usize> {
        let body: &[Stmt] = match node {
            Node::Module(module) => &module.body,
            Node::Stmt(Stmt::FunctionDef(function)) => &function.body,
            Node::Stmt(Stmt::ClassDef(class)) => &class.body,
            _ => return None,
        };
        body.first()
            .map(|statement| self.line_at(statement.range().start()))
    }

    /// CPython's parser rejects some shapes that Ruff's does not: calls whose
    /// positional arguments follow keywords, and type parameter defaults (3.13).
    fn check_calls(&self) -> Result<(), ParseFailure> {
        for visit in self.walk() {
            if let Node::TypeParam(param) = visit {
                let has_default = match param {
                    TypeParam::TypeVar(found) => found.default.is_some(),
                    TypeParam::TypeVarTuple(found) => found.default.is_some(),
                    TypeParam::ParamSpec(found) => found.default.is_some(),
                };
                if has_default {
                    return Err(ParseFailure::Syntax("expected '('".into()));
                }
            }
            let arguments = match visit {
                Node::Expr(Expr::Call(call)) => &call.arguments,
                Node::Stmt(Stmt::ClassDef(class)) => match &class.arguments {
                    Some(arguments) => arguments,
                    None => continue,
                },
                _ => continue,
            };
            let (mut keyword, mut unpacking) = (false, false);
            for item in arguments.iter_source_order() {
                match item {
                    ast::ArgOrKeyword::Keyword(found) if found.arg.is_none() => unpacking = true,
                    ast::ArgOrKeyword::Keyword(_) => keyword = true,
                    ast::ArgOrKeyword::Arg(Expr::Starred(_)) if unpacking => {
                        return Err(ParseFailure::Syntax(
                            "iterable argument unpacking follows keyword argument unpacking".into(),
                        ));
                    }
                    ast::ArgOrKeyword::Arg(Expr::Starred(_)) => {}
                    ast::ArgOrKeyword::Arg(_) if unpacking => {
                        return Err(ParseFailure::Syntax(
                            "positional argument follows keyword argument unpacking".into(),
                        ));
                    }
                    ast::ArgOrKeyword::Arg(_) if keyword => {
                        return Err(ParseFailure::Syntax(
                            "positional argument follows keyword argument".into(),
                        ));
                    }
                    ast::ArgOrKeyword::Arg(_) => {}
                }
            }
        }
        Ok(())
    }

    /// Identifiers and named character escapes are checked against Unicode 15.0,
    /// the version CPython 3.12 uses; Ruff also accepts characters added since.
    fn check_token_text(&self, token: &ruff_python_ast::token::Token) -> Result<(), ParseFailure> {
        use ruff_python_ast::token::TokenKind;
        let range = token.range();
        let slice = &self.source[range.start().to_usize()..range.end().to_usize()];
        match token.kind() {
            TokenKind::Identifier if !slice.is_ascii() => {
                if !text::is_identifier(slice) {
                    return Err(ParseFailure::Syntax(format!(
                        "invalid character in identifier: {slice}"
                    )));
                }
            }
            TokenKind::String | TokenKind::FStringMiddle => {
                let Some(flags) = token.string_flags() else {
                    return Ok(());
                };
                if flags.is_raw_string() || flags.is_byte_string() {
                    return Ok(());
                }
                let backslash = char::from(92);
                let mut chars = slice.char_indices();
                while let Some((index, c)) = chars.next() {
                    if c != backslash {
                        continue;
                    }
                    if let Some((_, 'N')) = chars.next()
                        && slice[index + 2..].starts_with('{')
                        && let Some(end) = slice[index + 3..].find('}')
                    {
                        let name = &slice[index + 3..index + 3 + end];
                        if unicode_names2::character(name)
                            .is_some_and(|found| !text::is_assigned(found))
                        {
                            return Err(ParseFailure::Syntax(format!(
                                "unknown Unicode character name: {name}"
                            )));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The tokenizer's indentation and nesting rules, which Ruff's lexer applies
    /// differently: mixed tabs and spaces that are ambiguous at another tab size
    /// (`TabError`), and the bracket and indentation depth limits.
    fn check_tokenizer_rules(&self) -> Result<(), ParseFailure> {
        use ruff_python_ast::token::TokenKind;
        let mut depth = 0usize;
        let mut at_line_start = true;
        let mut columns = vec![0usize];
        let mut alternates = vec![0usize];
        let mut previous_end = 0usize;
        for token in self.parsed.tokens() {
            let kind = token.kind();
            self.check_token_text(token)?;
            match kind {
                TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace => {
                    depth += 1;
                    if depth > MAX_BRACKET_DEPTH {
                        return Err(ParseFailure::Syntax("too many nested parentheses".into()));
                    }
                }
                TokenKind::Rpar | TokenKind::Rsqb | TokenKind::Rbrace => {
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
            match kind {
                TokenKind::Newline => {
                    at_line_start = true;
                    previous_end = token.end().to_usize();
                    continue;
                }
                TokenKind::NonLogicalNewline
                | TokenKind::Comment
                | TokenKind::Indent
                | TokenKind::Dedent
                | TokenKind::EndOfFile => {
                    continue;
                }
                _ => {}
            }
            if at_line_start {
                at_line_start = false;
                // The first token of a logical line: measure the whitespace before it.
                let start = token.start().to_usize();
                let line_start = self.source[..start]
                    .rfind(['\n', '\r'])
                    .map_or(0, |position| position + 1);
                let _ = previous_end;
                let (mut column, mut alternate) = (0usize, 0usize);
                for byte in self.source[line_start..start].bytes() {
                    match byte {
                        b' ' => {
                            column += 1;
                            alternate += 1;
                        }
                        b'\t' => {
                            column = (column / 8 + 1) * 8;
                            alternate += 1;
                        }
                        b'\x0c' => {
                            column = 0;
                            alternate = 0;
                        }
                        _ => {}
                    }
                }
                let top = columns.len() - 1;
                let tab_error = || {
                    ParseFailure::Syntax(
                        "inconsistent use of tabs and spaces in indentation".into(),
                    )
                };
                if column == columns[top] {
                    if alternate != alternates[top] {
                        return Err(tab_error());
                    }
                } else if column > columns[top] {
                    if alternate <= alternates[top] {
                        return Err(tab_error());
                    }
                    if columns.len() >= MAX_INDENT_LEVELS {
                        return Err(ParseFailure::Syntax(
                            "too many levels of indentation".into(),
                        ));
                    }
                    columns.push(column);
                    alternates.push(alternate);
                } else {
                    while columns.len() > 1 && column < columns[columns.len() - 1] {
                        columns.pop();
                        alternates.pop();
                    }
                    let top = columns.len() - 1;
                    if column != columns[top] {
                        return Err(ParseFailure::Syntax(
                            "unindent does not match any outer indentation level".into(),
                        ));
                    }
                    if alternate != alternates[top] {
                        return Err(tab_error());
                    }
                }
            }
        }
        Ok(())
    }
}

/// `inspect.cleandoc`: expand tabs, strip the common indent from lines after the
/// first, and drop leading and trailing empty lines.
pub fn cleandoc(doc: &str) -> String {
    let expanded = text::expandtabs(doc);
    let mut lines: Vec<String> = expanded.split('\n').map(str::to_owned).collect();
    let mut margin = usize::MAX;
    for line in &lines[1..] {
        let content = text::char_len(text::lstrip(line));
        if content > 0 {
            margin = margin.min(text::char_len(line) - content);
        }
    }
    lines[0] = text::lstrip(&lines[0]).to_owned();
    if margin < usize::MAX {
        for line in &mut lines[1..] {
            *line = text::char_slice(line, margin, None).to_owned();
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    lines.join("\n")
}

/// Iterator behind [`PyModule::walk`].
pub struct Walk<'a> {
    queue: VecDeque<Node<'a>>,
}

impl<'a> Iterator for Walk<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        let node = self.queue.pop_front()?;
        self.queue.extend(children(node));
        Some(node)
    }
}
