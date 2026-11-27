//! Walking a local folder and reading its source files as inert bytes.
//!
//! The walk is the Python indexer's, step for step: folders are visited in sorted order, hidden
//! and generated folders are skipped, a link is never followed, a file larger than the cap or
//! not valid UTF-8 is counted and left out. Nothing read here is executed; the bytes only go
//! to the parser.

use crate::clock::Clock;
use crate::error::{Result, SourceError};
use crate::language::{SKIP, source_language, unsupported_source};
use crate::options::SourceFile;
use std::cell::Cell;
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Folders that start with a dot but are still read: workflow files and CodeGraph policy.
const VISIBLE_HIDDEN: [&str; 2] = [".github", ".codegraph"];

/// The one hidden file that is read: a GitLab pipeline definition.
const VISIBLE_FILE: &str = ".gitlab-ci.yml";

/// Whether an opened file is the one a path check just looked at. A path can be swapped for
/// a link between the check and the open; where the platform names files by device and
/// inode the two must agree.
#[cfg(unix)]
fn same_file(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file(_left: &std::fs::Metadata, _right: &std::fs::Metadata) -> bool {
    true
}

/// Open a regular file that is neither a link nor swapped since `seen` was taken.
pub(crate) fn open_regular(path: &Path, seen: &std::fs::Metadata) -> Option<std::fs::File> {
    let file = std::fs::File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    (opened.is_file() && same_file(seen, &opened)).then_some(file)
}

/// Read at most `limit` bytes.
pub(crate) fn read_limited(file: std::fs::File, limit: u64) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    file.take(limit).read_to_end(&mut data).ok()?;
    Some(data)
}

struct Folder {
    path: PathBuf,
    relative: String,
}

/// The files `index_repo` offers the indexer, one at a time, in Python's order.
pub struct Capture<'a> {
    root: PathBuf,
    include_dist: bool,
    max_file_bytes: u64,
    clock: &'a dyn Clock,
    deadline: f64,
    skipped: &'a Cell<usize>,
    folders: Vec<Folder>,
    current: Option<Folder>,
    names: VecDeque<String>,
}

impl<'a> Capture<'a> {
    pub fn new(
        root: PathBuf,
        include_dist: bool,
        max_file_bytes: u64,
        clock: &'a dyn Clock,
        deadline: f64,
        skipped: &'a Cell<usize>,
    ) -> Self {
        Self {
            folders: vec![Folder {
                path: root.clone(),
                relative: String::new(),
            }],
            root,
            include_dist,
            max_file_bytes,
            clock,
            deadline,
            skipped,
            current: None,
            names: VecDeque::new(),
        }
    }

    fn skip(&self) {
        self.skipped.set(self.skipped.get() + 1);
    }

    /// Sort a folder's entries into the files to consider and the folders to descend into.
    fn list(&self, folder: &Folder) -> (Vec<String>, Vec<String>) {
        let mut files = Vec::new();
        let mut folders = Vec::new();
        // A folder that cannot be listed is walked as empty, as `os.walk` does.
        let Ok(entries) = std::fs::read_dir(&folder.path) else {
            return (files, folders);
        };
        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(raw) => {
                    // A name that is not text cannot be stored; it is counted, not read.
                    let lossy = raw.to_string_lossy();
                    if source_language(&lossy).is_some()
                        || unsupported_source(&join(&folder.relative, &lossy), self.include_dist)
                    {
                        self.skip();
                    }
                    continue;
                }
            };
            let kind = entry.file_type();
            let link = kind.as_ref().is_ok_and(|kind| kind.is_symlink());
            // `os.walk` follows a link to decide whether an entry is a folder.
            let directory = if link {
                std::fs::metadata(entry.path()).is_ok_and(|found| found.is_dir())
            } else {
                kind.as_ref().is_ok_and(|kind| kind.is_dir())
            };
            if directory {
                let wanted = (!SKIP.contains(&name.as_str())
                    || (self.include_dist && name == "dist"))
                    && (!name.starts_with('.') || VISIBLE_HIDDEN.contains(&name.as_str()))
                    && !link;
                if wanted {
                    folders.push(name);
                }
            } else {
                files.push(name);
            }
        }
        files.sort();
        folders.sort();
        (files, folders)
    }

    /// Decide about one file name: left out silently, counted as skipped, or read.
    fn consider(&self, folder: &Folder, name: &str) -> Result<Option<SourceFile>> {
        if name.starts_with('.') && name != VISIBLE_FILE {
            return Ok(None);
        }
        if self.clock.now() >= self.deadline {
            return Err(SourceError::timeout());
        }
        let relative = join(&folder.relative, name);
        if source_language(name).is_none() {
            if unsupported_source(&relative, self.include_dist) {
                self.skip();
            }
            return Ok(None);
        }
        let path = folder.path.join(name);
        let Some(data) = self.read(&path) else {
            self.skip();
            return Ok(None);
        };
        Ok(Some(SourceFile {
            path: relative,
            data: Some(data),
            ..SourceFile::default()
        }))
    }

    /// The file's bytes, or `None` for anything that is not a plain, readable, in-repository,
    /// UTF-8 regular file within the size cap.
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        let seen = std::fs::symlink_metadata(path).ok()?;
        // Never open anything that is not a plain file: opening a named pipe would wait forever.
        if !seen.is_file() {
            return None;
        }
        let resolved = std::fs::canonicalize(path).ok()?;
        if !resolved.starts_with(&self.root) {
            return None;
        }
        let file = open_regular(path, &seen)?;
        let data = read_limited(file, self.max_file_bytes.saturating_add(1))?;
        if data.len() as u64 > self.max_file_bytes || std::str::from_utf8(&data).is_err() {
            return None;
        }
        Some(data)
    }
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

impl Iterator for Capture<'_> {
    type Item = Result<SourceFile>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(name) = self.names.pop_front() {
                let folder = self.current.take()?;
                let outcome = self.consider(&folder, &name);
                self.current = Some(folder);
                match outcome {
                    Ok(Some(file)) => return Some(Ok(file)),
                    Ok(None) => continue,
                    Err(error) => {
                        self.folders.clear();
                        self.names.clear();
                        return Some(Err(error));
                    }
                }
            }
            let folder = self.folders.pop()?;
            let (files, folders) = self.list(&folder);
            // Pushed in reverse so the first folder is walked next, as a pre-order walk does.
            for name in folders.into_iter().rev() {
                self.folders.push(Folder {
                    path: folder.path.join(&name),
                    relative: join(&folder.relative, &name),
                });
            }
            self.names = files.into();
            self.current = Some(folder);
        }
    }
}

/// The folder to index, with links resolved. It must exist and be a folder.
pub(crate) fn resolve_root(root: &Path) -> Result<PathBuf> {
    let resolved = std::fs::canonicalize(root).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            SourceError::new("FileNotFoundError", "The repository folder does not exist")
        }
        _ => SourceError::new("OSError", error.to_string()),
    })?;
    if !resolved.is_dir() {
        return Err(SourceError::value("Repository must be a directory"));
    }
    Ok(resolved)
}
