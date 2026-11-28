//! Identifiers: node ids, analysis cache keys, Git blob ids and the analyser signature.

use sha1::Sha1;
use sha2::{Digest, Sha256};

/// What this runtime stamps on every analysis it writes.
///
/// Python embeds the installed `tree-sitter` and `tree-sitter-language-pack` versions.
/// Rust cannot report those, and must never serve a Python-written analysis as its own or
/// the reverse, so its signature names the engine (`"rust"`) and the grammar crates it was
/// built with. Cache keys hash the signature, so another runtime's rows are never found: a
/// miss, which is correct, where a false hit would be a bug. `format` moves with Python's
/// `format` whenever extraction semantics change. `tests/signature.rs` ties the grammar
/// versions to `Cargo.lock` and the text to the committed signature file.
pub const ANALYSIS_VERSION: &str = r#"{"engine": "rust", "format": 15, "grammars": {"javascript": "0.25.0", "python": "0.25.0", "rust": "0.24.2", "typescript": "0.23.2"}, "tree-sitter": "0.25.10", "waypoints": "waypoint-v2"}"#;

pub fn analysis_version() -> &'static str {
    ANALYSIS_VERSION
}

pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// A short stable id for a node: a hash of where it came from, not of anything that varies.
pub fn stable_id(snapshot: &str, kind: &str, path: &str, name: &str, line: Option<i64>) -> String {
    let value = format!("{snapshot}:{kind}:{path}:{name}:{}", line.unwrap_or(0));
    sha256_hex(value.as_bytes())[..24].to_owned()
}

/// The key an analysis is stored and found under: language, signature and content.
pub fn analysis_key(language: &str, signature: &str, content_hash: &str) -> String {
    sha256_hex(format!("{language}\0{signature}\0{content_hash}").as_bytes())
}

/// Git's blob object id. It is a lookup key for a cache, never a trust boundary: the
/// analysis key above is derived from the SHA-256 of the content.
pub fn git_blob_oid(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(b"blob ");
    hasher.update(data.len().to_string().as_bytes());
    hasher.update([0]);
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}
