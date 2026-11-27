//! `PurePosixPath` behaviour the relationship resolvers rely on.

/// Split a path as `PurePosixPath` does: empty and `.` segments vanish, and exactly
/// two leading slashes form a distinct root.
pub fn posix(path: &str) -> (&'static str, Vec<&str>) {
    let slashes = path.len() - path.trim_start_matches('/').len();
    let root = match slashes {
        0 => "",
        2 => "//",
        _ => "/",
    };
    let tail = path[slashes..]
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    (root, tail)
}

/// `PurePosixPath(path).parts`: the root, when there is one, comes first.
pub fn parts(path: &str) -> Vec<String> {
    let (root, tail) = posix(path);
    let mut found: Vec<String> = Vec::new();
    if !root.is_empty() {
        found.push(root.to_owned());
    }
    found.extend(tail.into_iter().map(str::to_owned));
    found
}

/// `PurePosixPath(path).stem`: the final part without its last suffix.
pub fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 && dot < name.len() - 1 => &name[..dot],
        _ => name,
    }
}
