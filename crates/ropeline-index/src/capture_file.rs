//! Capturing one file into a snapshot: check its path and limits, find or make its analysis,
//! and add it to the manifest. Source bytes are only ever parsed.

use crate::analysis::{Record, analyse, from_stored};
use crate::edges::Known;
use crate::error::{Result, SourceError};
use crate::identity::{analysis_key, sha256_hex};
use crate::language::{name_of, source_language};
use crate::options::{IndexOptions, SourceFile};
use ropeline_engine::posix_path::parts;
use ropeline_engine::text;
use ropeline_store::analysis;
use rusqlite::Connection;
use std::collections::HashMap;
use std::rc::Rc;

const MAX_ANALYSIS_TOTAL: i64 = 64_000_000;

/// A repository path the graph will accept: relative, inside the repository, one
/// separator, no drive and no longer than the contract allows.
fn valid_path(relative: &str) -> bool {
    !(relative.is_empty()
        || relative.starts_with('/')
        || parts(relative).iter().any(|part| part == "..")
        || relative.contains('\\')
        || relative.contains(':')
        || text::char_len(relative) > 4096)
}

/// Running totals the limits are judged against.
pub(crate) struct Totals {
    pub(crate) parsed: usize,
    pub(crate) symbols: i64,
    pub(crate) bytes: i64,
    pub(crate) analysis_bytes: i64,
}

fn truthy_text(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|text| !text.is_empty())
}

/// The analysis for one file: found by id, found by content, or made now.
fn record_for(
    connection: &Connection,
    item: &SourceFile,
    language: &str,
    options: &IndexOptions,
    signature: &str,
    used: &mut HashMap<String, Rc<Record>>,
    totals: &mut Totals,
) -> Result<Rc<Record>> {
    if let Some(id) = truthy_text(&item.analysis_id) {
        let record = match used.get(id) {
            Some(found) => Some(Rc::clone(found)),
            None => analysis(connection, id)?
                .map(from_stored)
                .transpose()?
                .map(Rc::new),
        };
        return match record {
            Some(found) if found.version == signature && found.language == language => Ok(found),
            _ => Err(SourceError::value(
                "Cached analysis is missing or incompatible",
            )),
        };
    }
    let data = item
        .data
        .as_deref()
        .ok_or_else(|| SourceError::new("KeyError", "'data'"))?;
    let size = i64::try_from(data.len()).unwrap_or(i64::MAX);
    if totals.bytes.saturating_add(size) > options.max_bytes {
        return Err(SourceError::value(
            "Repository exceeds indexing file/byte limits",
        ));
    }
    let content_hash = sha256_hex(data);
    let key = analysis_key(language, signature, &content_hash);
    if let Some(found) = used.get(&key) {
        return Ok(Rc::clone(found));
    }
    if let Some(stored) = analysis(connection, &key)? {
        return Ok(Rc::new(from_stored(stored)?));
    }
    let fresh = analyse(language, data, signature, &key)?;
    if let Some(claimed) = truthy_text(&item.git_oid)
        && claimed != fresh.record.git_oid
    {
        return Err(SourceError::value(
            "Git object does not match captured content",
        ));
    }
    connection.execute(
        "INSERT INTO analyses VALUES (?,?,?,?,?,?,?,?)",
        (
            &key,
            &fresh.record.content_hash,
            &fresh.record.git_oid,
            language,
            signature,
            fresh.record.size_bytes,
            &fresh.encoded_symbols,
            &fresh.encoded_facts,
        ),
    )?;
    totals.parsed += 1;
    Ok(Rc::new(fresh.record))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn capture_file(
    connection: &Connection,
    snapshot: i64,
    item: SourceFile,
    options: &IndexOptions,
    signature: &str,
    known: &mut Known,
    used: &mut HashMap<String, Rc<Record>>,
    totals: &mut Totals,
) -> Result<()> {
    let relative = item.path.clone();
    if !valid_path(&relative) {
        return Err(SourceError::value("Invalid source path"));
    }
    let language = source_language(&relative);
    let Some(language) = language.filter(|_| !known.contains(&relative)) else {
        return Err(SourceError::value("Unsupported or duplicate source path"));
    };
    if i64::try_from(known.len()).unwrap_or(i64::MAX) >= options.max_files {
        return Err(SourceError::value(
            "Repository exceeds indexing file/byte limits",
        ));
    }
    let record = record_for(
        connection, &item, language, options, signature, used, totals,
    )?;
    if let Some(claimed) = truthy_text(&item.git_oid)
        && claimed != record.git_oid
    {
        return Err(SourceError::value(
            "Git object does not match captured content",
        ));
    }
    if !used.contains_key(&record.id) {
        totals.analysis_bytes += record.encoded_bytes;
        if totals.analysis_bytes > MAX_ANALYSIS_TOTAL {
            return Err(SourceError::value(
                "Repository exceeds analysis output limit",
            ));
        }
    }
    totals.bytes += record.size_bytes;
    totals.symbols += i64::try_from(record.symbols.len()).unwrap_or(i64::MAX);
    if totals.bytes > options.max_bytes || totals.symbols > options.max_symbols {
        return Err(SourceError::value(
            "Repository exceeds indexing byte/symbol limits",
        ));
    }
    used.insert(record.id.clone(), Rc::clone(&record));
    let file_id = sha256_hex(format!("{}\0{relative}", record.id).as_bytes());
    connection.execute(
        "INSERT INTO snapshot_files VALUES (?,?,?,?,?)",
        (
            snapshot,
            &relative,
            &file_id,
            name_of(&relative),
            &record.id,
        ),
    )?;
    known.push(relative, file_id, record);
    Ok(())
}
