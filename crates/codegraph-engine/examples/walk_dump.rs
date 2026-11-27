//! Developer tool: print `ast.walk`-shaped dumps for Python files, one JSON line each.
//!
//! The engine itself does no I/O; this example reads the files it is given so a
//! sweep over a large corpus (for instance the standard library) can be compared
//! against CPython. Usage: `walk_dump <list-file>` where the list has one path per line.

#![allow(clippy::disallowed_methods)]

use codegraph_engine::pyast;

fn main() {
    let list = std::env::args().nth(1).expect("path to a list of files");
    let paths = std::fs::read_to_string(list).expect("readable list");
    for path in paths.lines().filter(|line| !line.is_empty()) {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let entry = match pyast::with_module(&text, |module| {
            let walk = module
                .walk()
                .map(|node| {
                    let visit = module.visit(node);
                    serde_json::json!([visit.kind, visit.line])
                })
                .collect::<Vec<serde_json::Value>>();
            (walk, module.depth())
        }) {
            Ok((walk, depth)) => {
                serde_json::json!({"path": path, "ok": true, "walk": walk, "depth": depth})
            }
            Err(failure) => {
                serde_json::json!({"path": path, "ok": false, "error": format!("{failure:?}")})
            }
        };
        println!("{entry}");
    }
}
