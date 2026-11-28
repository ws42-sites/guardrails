//! The Rust-only parts of symbol extraction, kept out of `parsing` so that module
//! stays about the shared walk. Port of `_impl_owner` and `RUST_IMPORT_TYPES`.

use crate::text;
use tree_sitter::Node;

/// `use` and `extern crate` lines are imports, not definitions.
pub const IMPORT_TYPES: [&str; 2] = ["use_declaration", "extern_crate_declaration"];

/// The type an `impl` block is written for, which owns the methods inside it.
///
/// Only a plain, generic-applied or path-qualified type name counts; anything else
/// (`impl<T> Trait for &T`, tuples, arrays) leaves the enclosing owner in place.
pub fn impl_owner(node: Node<'_>, content: &[u8]) -> Option<String> {
    let mut target = node.child_by_field_name("type");
    if target.is_some_and(|found| found.kind() == "generic_type") {
        target = target.and_then(|found| found.child_by_field_name("type"));
    }
    if target.is_some_and(|found| found.kind() == "scoped_type_identifier") {
        target = target.and_then(|found| found.child_by_field_name("name"));
    }
    let target =
        target.filter(|found| matches!(found.kind(), "type_identifier" | "primitive_type"))?;
    Some(text::decode_replace(
        &content[target.start_byte()..target.end_byte()],
    ))
}
