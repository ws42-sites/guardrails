//! Which files are indexed, and which are counted as unsupported source.

use codegraph_engine::posix_path::parts;
use codegraph_engine::text;

/// Directories never walked into (`dist` only when the caller opts in).
pub const SKIP: [&str; 9] = [
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    "coverage",
    "vendor",
];

const UNSUPPORTED: [&str; 16] = [
    ".go", ".java", ".kt", ".kts", ".c", ".h", ".cpp", ".hpp", ".cc", ".cs", ".rb", ".php",
    ".swift", ".scala", ".vue", ".svelte",
];

/// `PurePosixPath(path).name`.
pub fn name_of(path: &str) -> String {
    parts(path).pop().unwrap_or_default()
}

/// `PurePosixPath(path).suffix`: from the last dot, unless the dot starts or ends the name.
pub fn suffix_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => &name[dot..],
        _ => "",
    }
}

fn extension_language(suffix: &str) -> Option<&'static str> {
    Some(match suffix {
        ".py" => "python",
        ".ts" => "typescript",
        ".tsx" => "tsx",
        ".js" | ".jsx" | ".cjs" | ".mjs" => "javascript",
        ".rs" => "rust",
        ".md" | ".json" | ".toml" | ".yaml" | ".yml" | ".tf" | ".sql" => "text",
        _ => return None,
    })
}

/// The language a repository path is indexed as, or `None` when it is not indexed.
pub fn source_language(path: &str) -> Option<&'static str> {
    let name = name_of(path);
    if name == "Dockerfile" || name == "Jenkinsfile" {
        return Some("text");
    }
    extension_language(suffix_of(&name))
}

/// Languages a stored analysis may carry.
pub fn is_language(language: &str) -> bool {
    matches!(
        language,
        "python" | "typescript" | "tsx" | "javascript" | "rust" | "text"
    )
}

/// Source in a language CodeGraph does not read yet; it is counted, never indexed. Hidden
/// and generated folders are excluded from the count, as they are from the walk.
pub fn unsupported_source(relative: &str, include_dist: bool) -> bool {
    let found = parts(relative);
    let Some((name, folders)) = found.split_last() else {
        return false;
    };
    let suffix = text::lower(suffix_of(name));
    UNSUPPORTED.contains(&suffix.as_str())
        && !folders.iter().any(|part| {
            part.starts_with('.')
                || (SKIP.contains(&part.as_str()) && !(include_dist && part == "dist"))
        })
}
