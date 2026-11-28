//! Pure file-purpose hints from repository paths, without reading source files.
//!
//! Port of `codegraph.file_categories`.

use crate::text;
use serde::Serialize;
use std::collections::HashSet;

const CODE: &[&str] = &[
    "py", "pyw", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "java", "kt", "kts", "go",
    "rs", "c", "h", "cc", "cpp", "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "ps1",
    "psm1", "sql", "r", "lua", "vue", "svelte", "html", "css", "scss", "sass", "less",
];
const ASSETS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "ico", "bmp", "tiff", "mp3", "wav", "ogg",
    "mp4", "webm", "mov", "woff", "woff2", "ttf", "otf", "eot", "glb", "gltf", "obj", "fbx",
    "blend", "pdf",
];
const CONFIG: &[&str] = &[
    "json",
    "jsonc",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "env",
    "editorconfig",
];
const DOCS: &[&str] = &["md", "mdx", "rst", "txt", "adoc"];
const ARCHIVES: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "whl", "jar", "war", "exe", "dll", "so", "dylib",
    "pyc", "pyo",
];

/// An explicitly inferred annotation, serialised as the Python dictionary is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileCategory {
    pub category: &'static str,
    pub basis: &'static str,
    pub inferred: bool,
}

fn any_directory(directories: &HashSet<&str>, names: &[&str]) -> bool {
    names.iter().any(|name| directories.contains(name))
}

/// `\.(min|generated|g)\.` anywhere in the name.
fn generated_marker(name: &str) -> bool {
    ["min", "generated", "g"]
        .iter()
        .any(|word| name.contains(&format!(".{word}.")))
}

/// `[._-](test|spec)\.` anywhere in the name.
fn test_marker(name: &str) -> bool {
    ["test", "spec"].iter().any(|word| {
        ['.', '_', '-']
            .iter()
            .any(|separator| name.contains(&format!("{separator}{word}.")))
    })
}

/// `(^|\.)config\.` anywhere in the name.
fn config_marker(name: &str) -> bool {
    name.starts_with("config.") || name.contains(".config.")
}

/// Classify a repository path; `None` for folders and for paths that are not a
/// well-formed repository-relative file path. `path` is `None` for a non-string.
pub fn file_category(path: Option<&str>, kind: &str) -> Option<FileCategory> {
    let path = path?;
    if matches!(kind, "repository" | "directory" | "module" | "cluster")
        || !(1..=4096).contains(&text::char_len(path))
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(|c| (c as u32) < 32 || c as u32 == 127)
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    let folded = text::casefold(path);
    let parts: Vec<&str> = folded.split('/').collect();
    let (name, directory_parts) = parts.split_last()?;
    let name = *name;
    let directories: HashSet<&str> = directory_parts.iter().copied().collect();
    let extension = name.rsplit_once('.').map_or("", |(_, tail)| tail);
    let is_in = |list: &[&str]| list.contains(&extension);

    let (category, basis) = if any_directory(
        &directories,
        &[
            "dist",
            "build",
            "coverage",
            "artifacts",
            "artefacts",
            "generated",
            "__snapshots__",
            "__pycache__",
        ],
    ) || is_in(ARCHIVES)
        || matches!(extension, "map" | "snap" | "tsbuildinfo" | "lock")
        || matches!(
            name,
            "package-lock.json" | "npm-shrinkwrap.json" | "pnpm-lock.yaml"
        )
        || generated_marker(name)
    {
        ("artifact", "generated-or-build-path")
    } else if any_directory(
        &directories,
        &[
            "test",
            "tests",
            "__tests__",
            "e2e",
            "spec",
            "specs",
            "fixtures",
            "__fixtures__",
            "__mocks__",
        ],
    ) || name.starts_with("test_")
        || matches!(name, "conftest.py" | "tests.py")
        || test_marker(name)
        || name.ends_with("_test.go")
    {
        ("test", "test-path-or-filename")
    } else if any_directory(
        &directories,
        &[
            ".github",
            ".gitlab",
            "config",
            "configs",
            ".vscode",
            ".devcontainer",
        ],
    ) || is_in(CONFIG)
        || config_marker(name)
        || [".env", ".eslintrc", ".prettierrc", "dockerfile", "compose."]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || matches!(
            name,
            "makefile"
                | "docker-compose.yml"
                | ".gitignore"
                | ".gitattributes"
                | ".dockerignore"
                | ".npmrc"
                | ".nvmrc"
        )
    {
        ("configuration", "configuration-path-or-filename")
    } else if is_in(ASSETS) || any_directory(&directories, &["assets", "images", "fonts", "media"])
    {
        ("asset", "asset-path-or-extension")
    } else if is_in(DOCS)
        || matches!(
            name,
            "license" | "licence" | "readme" | "changelog" | "notice"
        )
    {
        ("documentation", "documentation-filename-or-extension")
    } else if is_in(CODE) {
        ("code", "source-extension")
    } else {
        ("other", "unrecognised-path")
    };
    Some(FileCategory {
        category,
        basis,
        inferred: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(path: &str) -> Option<&'static str> {
        file_category(Some(path), "file").map(|found| found.category)
    }

    #[test]
    fn classifies_by_path_and_name() {
        assert_eq!(category("src/app.py"), Some("code"));
        assert_eq!(category("tests/test_app.py"), Some("test"));
        assert_eq!(category("dist/app.js"), Some("artifact"));
        assert_eq!(category("README.md"), Some("documentation"));
        assert_eq!(category("pyproject.toml"), Some("configuration"));
        assert_eq!(category("logo.png"), Some("asset"));
        assert_eq!(category("src/a.min.js"), Some("artifact"));
        assert_eq!(category("src/a.unknown"), Some("other"));
    }

    #[test]
    fn folders_and_malformed_paths_have_no_category() {
        assert!(file_category(Some("src"), "directory").is_none());
        assert!(file_category(None, "file").is_none());
        for bad in [
            "/abs.py",
            "a\\b.py",
            "a:b.py",
            "a//b.py",
            "a/../b.py",
            "",
            "a/\u{7f}",
        ] {
            assert!(file_category(Some(bad), "file").is_none(), "{bad:?}");
        }
        assert!(file_category(Some(&"a".repeat(4097)), "file").is_none());
    }

    #[test]
    fn case_is_folded_before_matching() {
        assert_eq!(category("TESTS/Foo.PY"), Some("test"));
        assert_eq!(category("Dockerfile"), Some("configuration"));
    }
}
