//! Expectations ported from the Python tests (`test_codeowners.py`, `test_file_categories.py`,
//! `test_relationships.py`) for the pure parts of those modules.

use ropeline_engine::codeowners::{self, MAX_BYTES, MAX_RULES};
use ropeline_engine::file_categories::file_category;
use ropeline_engine::relationships::{extract, relative_target};
use std::collections::HashSet;

const PATH: &str = ".github/CODEOWNERS";

fn known(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("serialises")
}

#[test]
fn gitignore_style_matching() {
    let cases = [
        ("*.js", "a.js", true),
        ("*.js", "deep/a/b.js", true),
        ("/*.js", "deep/b.js", false),
        ("/docs/", "docs/a.md", true),
        ("/docs/", "x/docs/a.md", false),
        ("docs/", "x/docs/a.md", true),
        ("docs/*", "docs/a.md", true),
        ("docs/*", "docs/sub/a.md", false),
        ("docs/**", "docs/sub/a.md", true),
        ("**/logs", "a/logs/x.txt", true),
        ("a/**/b.py", "a/b.py", true),
        ("a/**/b.py", "a/x/y/b.py", true),
        ("/build/logs", "build/logs/x", true),
        ("src/app.py", "src/app.py", true),
        ("src/app.py", "other/src/app.py", false),
        ("app?.py", "app1.py", true),
        ("app?.py", "app/.py", false),
        ("a\\ b.md", "a b.md", true),
    ];
    for (pattern, path, expected) in cases {
        let parsed =
            codeowners::parse(format!("{pattern} @me\n").as_bytes(), PATH).expect("parses");
        assert_eq!(
            parsed.lookup(path).is_some(),
            expected,
            "{pattern} vs {path}"
        );
    }
}

#[test]
fn malformed_lines_are_skipped_not_fatal() {
    let long_pattern = format!("{} @me", "x".repeat(257));
    let many_owners = format!(
        "src/ {}",
        (0..17)
            .map(|i| format!("@u{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let owner_at_cap = format!("src/ @{}", "x".repeat(128));
    let lines = [
        "../secret @me",
        "src/../secret @me",
        "a..b @me",
        "//server/share @me",
        "C:/windows @me",
        "~/home @me",
        "!negated @me",
        "[ab].py @me",
        "src/\u{0}x @me",
        "/ @me",
        &long_pattern,
        "a/**/**/**/**/**/b @me",
        "src/ @bad owner",
        "src/ noatsign",
        "src/ @",
        "src/ @a/",
        "src/ plain@nodot",
        "src/ @me\u{0}",
        &owner_at_cap,
        &many_owners,
    ];
    for line in lines {
        let parsed =
            codeowners::parse(format!("{line}\nok/ @good\n").as_bytes(), PATH).expect("parses");
        let patterns: Vec<&str> = parsed
            .rules
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, ["ok/"], "{line:?}");
        assert_eq!(parsed.rules[0].line, 2);
    }
}

#[test]
fn bounds_and_unknown_ownership() {
    let names = (0..16)
        .map(|i| format!("@u{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let parsed = codeowners::parse(format!("* {names}\n").as_bytes(), PATH).expect("parses");
    assert_eq!(parsed.lookup("a").expect("owned").0.len(), 16);
    let mut big = b"* @me\n".to_vec();
    big.extend(std::iter::repeat_n(b'#', MAX_BYTES));
    assert!(codeowners::parse(&big, PATH).is_none());
    let edge: String = (0..MAX_RULES).map(|i| format!("/d{i}/ @me\n")).collect();
    assert!(codeowners::parse(edge.as_bytes(), PATH).is_some());
    assert!(codeowners::parse(format!("{edge}/extra/ @me\n").as_bytes(), PATH).is_none());
    for data in [&b""[..], b"# only a comment\n", b"\xff\xfe* @me"] {
        assert!(codeowners::parse(data, PATH).is_none());
    }
    assert!(codeowners::parse(b"* @me\n", "../CODEOWNERS").is_none());
    assert!(codeowners::parse(b"* @me\n", "src/CODEOWNERS").is_none());
    let crlf = codeowners::parse(b"*.py @me # why\r\n", PATH).expect("parses");
    assert_eq!(crlf.lookup("a.py"), Some((vec!["@me".to_owned()], 1)));
}

#[test]
fn owner_token_limits_and_forms() {
    for good in ["@user", "@org/team-1", "dev@example.com"] {
        assert!(codeowners::valid_owner(good));
    }
    assert!(codeowners::valid_owner(&format!("@{}", "a".repeat(127))));
    assert!(!codeowners::valid_owner(&format!("@{}", "a".repeat(128))));
    assert!(!codeowners::valid_owner("") && !codeowners::valid_owner("@a/b/c"));
}

#[test]
fn path_purposes() {
    let cases = [
        ("src/workers.ts", "code"),
        ("src/Contest.ts", "code"),
        ("src/latest.py", "code"),
        ("src/server.test.ts", "test"),
        ("src/server.spec.tsx", "test"),
        ("tests/helpers.py", "test"),
        ("src/__tests__/helpers.ts", "test"),
        ("e2e/home.ts", "test"),
        ("spec/worker.rb", "test"),
        ("fixtures/users.json", "test"),
        ("conftest.py", "test"),
        ("service_test.go", "test"),
        ("TESTS/Test_Main.PY", "test"),
        ("vite.config.ts", "configuration"),
        ("eslint.config.mjs", "configuration"),
        ("config/server.py", "configuration"),
        (".github/workflows/ci.yml", "configuration"),
        ("package.json", "configuration"),
        ("pyproject.toml", "configuration"),
        (".env.example", "configuration"),
        ("Dockerfile", "configuration"),
        ("dist/server.test.js", "artifact"),
        ("src/api.generated.ts", "artifact"),
        ("tests/__snapshots__/worker.snap", "artifact"),
        ("package-lock.json", "artifact"),
        ("pnpm-lock.yaml", "artifact"),
        ("client.tgz", "artifact"),
        ("assets/logo.svg", "asset"),
        ("public/font.woff2", "asset"),
        ("manual.pdf", "asset"),
        ("README.md", "documentation"),
        ("docs/guide.rst", "documentation"),
        ("LICENSE", "documentation"),
        ("data/example.unknown", "other"),
    ];
    for (path, category) in cases {
        let found = file_category(Some(path), "file").expect(path);
        assert_eq!(found.category, category, "{path}");
        assert!(found.inferred && !found.basis.is_empty());
    }
    for bad in [
        "",
        ".",
        "../test.py",
        "src/../test.py",
        "/tests/a.py",
        "C:/a.py",
        "src\\test.py",
        "a\u{0}.py",
        "a\u{7f}.py",
        "a//test.py",
    ] {
        assert!(file_category(Some(bad), "file").is_none(), "{bad:?}");
    }
    assert!(file_category(None, "file").is_none());
    assert!(file_category(Some(&"a".repeat(4097)), "file").is_none());
    for kind in ["repository", "directory", "module", "cluster"] {
        assert!(file_category(Some("src/a.py"), kind).is_none());
    }
}

#[test]
fn relationship_extraction_cases() {
    let files = known(&["src/index.ts", "src/services/router.ts"]);
    assert_eq!(
        relative_target("src/index.ts", "./services/router", &files).as_deref(),
        Some("src/services/router.ts")
    );
    let (imports, calls) = extract(
        "src/index.ts",
        "import {route} from './services/router';",
        &files,
    )
    .expect("ok");
    assert_eq!(
        json(&imports),
        r#"[["src/services/router.ts",1,"resolved-relative"]]"#
    );
    assert!(calls.is_empty());
    let source = "def helper():\n    return 1\n\ndef run():\n    return helper()\n";
    let (_, calls) = extract("src/mod.py", source, &known(&["src/mod.py"])).expect("ok");
    assert!(
        calls
            .iter()
            .any(|c| c.0 == "helper" && c.1 == 5 && c.2 == "local-name-candidate")
    );
    let paths = known(&["src/helper.py", "src/main.py"]);
    let (imports, _) = extract("src/main.py", "from src import helper\n", &paths).expect("ok");
    assert_eq!(imports[0].0, "src/helper.py");
    let react = extract(
        "src/index.ts",
        "import React from 'react'",
        &known(&["src/index.ts"]),
    );
    assert!(react.expect("ok").0.is_empty());
    let both = known(&["main.ts", "target.ts"]);
    for text in [
        "// import { route } from \"./target\";",
        "/* export { route } from \"./target\"; */",
        "const text = \"require('./target')\";",
        "const template = `import { route } from \"./target\";`;",
    ] {
        assert!(
            extract("main.ts", text, &both).expect("ok").0.is_empty(),
            "{text}"
        );
    }
    for text in [
        "import { route } from \"./target\";",
        "export { route } from \"./target\";",
        "const module = require(\"./target\");",
        "const module = import(\"./target\");",
    ] {
        let (imports, _) = extract("main.ts", text, &both).expect("ok");
        assert_eq!(
            json(&imports),
            r#"[["target.ts",1,"resolved-relative"]]"#,
            "{text}"
        );
    }
    let pkg = known(&["pkg/__init__.py", "pkg/helper.py", "pkg/other/__init__.py"]);
    let (imports, _) =
        extract("pkg/__init__.py", "from . import helper, other\n", &pkg).expect("ok");
    assert_eq!(
        json(&imports),
        r#"[["pkg/helper.py",1,"resolved-relative"],["pkg/other/__init__.py",1,"resolved-relative"]]"#
    );
    let escape = extract(
        "pkg/main.py",
        "from ... import secret\n",
        &known(&["secret.py"]),
    );
    assert!(escape.expect("ok").0.is_empty());
}
