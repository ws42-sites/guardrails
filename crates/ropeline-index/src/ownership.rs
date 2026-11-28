//! Reading CODEOWNERS from a checkout as inert text.

use crate::capture::{open_regular, read_limited};
use ropeline_engine::codeowners::{self, CANDIDATES, MAX_BYTES, Ownership};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::Path;

/// The first CODEOWNERS file, parsed; unreadable means unknown.
///
/// A link or a file that is not a plain regular file is refused, wherever it sits on the
/// way to the file, so a checkout cannot redirect the read. `listed`, when given, limits the
/// candidates to paths the caller already knows exist.
pub fn read_codeowners(root: &Path, listed: Option<&HashSet<String>>) -> Option<Ownership> {
    for candidate in CANDIDATES {
        if listed.is_some_and(|names| !names.contains(candidate)) {
            continue;
        }
        let mut location = root.to_path_buf();
        let mut seen = None;
        let mut found = true;
        for part in candidate.split('/') {
            location.push(part);
            match std::fs::symlink_metadata(&location) {
                Ok(metadata) if metadata.file_type().is_symlink() => return None,
                Ok(metadata) => seen = Some(metadata),
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    found = false;
                    break;
                }
                Err(_) => return None,
            }
        }
        if !found {
            continue;
        }
        let seen = seen?;
        if !seen.is_file() {
            return None;
        }
        let file = open_regular(&location, &seen)?;
        let data = read_limited(file, MAX_BYTES as u64 + 1)?;
        return codeowners::parse(&data, candidate);
    }
    None
}
