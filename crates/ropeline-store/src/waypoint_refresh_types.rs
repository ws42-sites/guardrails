//! What a waypoint refresh is handed and what it reports.

use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap};

/// One indexed file as the refresh sees it. `facts` is the analysis dictionary the
/// indexer stored; its `waypoint` entry holds what the engine derived from the source.
pub struct RefreshFile<'a> {
    pub path: &'a str,
    pub analysis_id: &'a str,
    pub content_hash: &'a str,
    pub facts: &'a Value,
}

/// A resolved link: the target path, the target's analysis id and the relationship kind.
pub type Link = (String, String, String);

pub struct Refresh<'a> {
    pub snapshot_id: i64,
    pub files: &'a [RefreshFile<'a>],
    pub dependencies: &'a HashMap<String, Vec<Link>>,
    pub revision: &'a str,
    pub repository: &'a str,
    pub configuration: &'a BTreeMap<String, String>,
    pub policy_digest: &'a str,
    pub summaries_computed: i64,
}

fn stat_names() -> [&'static str; 9] {
    [
        "summariesComputed",
        "summariesReused",
        "validationsComputed",
        "validationsReused",
        "areasComputed",
        "areasReused",
        "areaValidationsComputed",
        "areaValidationsReused",
        "unavailableFiles",
    ]
}

/// The counters Python returns, in its order, with `partial` last.
pub(crate) struct Stats(Vec<(&'static str, i64)>, bool);

impl Stats {
    pub(crate) fn new(computed: i64, total: i64) -> Self {
        let mut counters: Vec<(&'static str, i64)> =
            stat_names().iter().map(|name| (*name, 0)).collect();
        counters[0].1 = computed;
        counters[1].1 = total - computed;
        Self(counters, false)
    }

    /// Files with no displayable path are counted, and make the snapshot partial.
    pub(crate) fn unavailable(&mut self, count: usize) {
        self.0[8].1 = i64::try_from(count).unwrap_or(i64::MAX);
        self.1 = count > 0;
    }

    pub(crate) fn add(&mut self, name: &str) {
        for item in &mut self.0 {
            if item.0 == name {
                item.1 += 1;
            }
        }
    }

    pub(crate) fn value(&self) -> Value {
        let mut map = Map::new();
        for (name, count) in &self.0 {
            map.insert((*name).to_owned(), json!(count));
        }
        map.insert("partial".into(), json!(self.1));
        Value::Object(map)
    }
}

pub(crate) type Identity = (String, String, String, String, String);
