//! The SQL text of the graph snapshot schema, byte for byte as `graph_store.py` holds it.
//!
//! The statements are stored in `sqlite_master` verbatim, so a stray space here would
//! show up as a schema difference against databases Python made. The view definitions
//! are assembled by the same replacements Python performs.

/// `PRAGMA user_version` of the current layout.
pub const VERSION: i64 = 3;

/// Longest `length(symbols)+length(facts)` that `analysis` will return.
pub const MAX_ANALYSIS_BYTES: i64 = 16_000_000;
pub const TABLES: [&str; 12] = [
    r####"CREATE TABLE IF NOT EXISTS snapshots(id INTEGER PRIMARY KEY,repo TEXT NOT NULL,revision TEXT NOT NULL,indexed_at TEXT DEFAULT CURRENT_TIMESTAMP)"####,
    r####"CREATE TABLE IF NOT EXISTS legacy_nodes(id TEXT PRIMARY KEY,snapshot_id INTEGER NOT NULL,kind TEXT NOT NULL,name TEXT NOT NULL,path TEXT NOT NULL,line INTEGER,metadata TEXT DEFAULT '{}')"####,
    r####"CREATE TABLE IF NOT EXISTS legacy_edges(snapshot_id INTEGER NOT NULL,source TEXT NOT NULL,target TEXT NOT NULL,kind TEXT NOT NULL,evidence TEXT,PRIMARY KEY(snapshot_id,source,target,kind))"####,
    r####"CREATE INDEX IF NOT EXISTS idx_nodes_lookup ON legacy_nodes(snapshot_id,name,kind)"####,
    r####"CREATE INDEX IF NOT EXISTS idx_edges_source ON legacy_edges(snapshot_id,source)"####,
    r####"CREATE INDEX IF NOT EXISTS idx_edges_target ON legacy_edges(snapshot_id,target)"####,
    r####"CREATE TABLE IF NOT EXISTS analyses(
        id TEXT PRIMARY KEY,content_hash TEXT NOT NULL,git_oid TEXT NOT NULL,
        language TEXT NOT NULL,version TEXT NOT NULL,size_bytes INTEGER NOT NULL,
        symbols TEXT NOT NULL CHECK(json_valid(symbols)),facts TEXT NOT NULL CHECK(json_valid(facts)))"####,
    r####"CREATE INDEX IF NOT EXISTS analysis_object ON analyses(git_oid,language,version)"####,
    r####"CREATE TABLE IF NOT EXISTS snapshot_files(
        snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
        path TEXT NOT NULL,file_id TEXT NOT NULL,name TEXT NOT NULL,
        analysis_id TEXT NOT NULL REFERENCES analyses(id),PRIMARY KEY(snapshot_id,path))"####,
    r####"CREATE INDEX IF NOT EXISTS manifest_analysis ON snapshot_files(analysis_id)"####,
    r####"CREATE TABLE IF NOT EXISTS resolved_edges(
        snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
        source TEXT NOT NULL,target TEXT NOT NULL,kind TEXT NOT NULL,evidence TEXT,
        PRIMARY KEY(snapshot_id,source,target,kind))"####,
    r####"CREATE INDEX IF NOT EXISTS resolved_inbound ON resolved_edges(snapshot_id,target)"####,
];

pub const WAYPOINT_TABLES: [&str; 4] = [
    r####"CREATE TABLE IF NOT EXISTS waypoint_cache(
        id TEXT PRIMARY KEY,kind TEXT NOT NULL CHECK(kind IN ('file','area','validation')),
        version TEXT NOT NULL,payload TEXT NOT NULL CHECK(json_valid(payload)),
        payload_hash TEXT NOT NULL)"####,
    r####"CREATE TABLE IF NOT EXISTS snapshot_waypoints(
        snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
        path TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('file','area')),
        summary_key TEXT NOT NULL REFERENCES waypoint_cache(id),
        validation_key TEXT NOT NULL REFERENCES waypoint_cache(id),
        PRIMARY KEY(snapshot_id,path))"####,
    r####"CREATE INDEX IF NOT EXISTS waypoint_summary_bindings ON snapshot_waypoints(summary_key)"####,
    r####"CREATE INDEX IF NOT EXISTS waypoint_validation_bindings ON snapshot_waypoints(validation_key)"####,
];

pub const V2_NODE_VIEW: &str = r####"CREATE VIEW nodes AS
 SELECT id,snapshot_id,kind,name,path,line,metadata FROM legacy_nodes
 UNION ALL
 SELECT 'r:'||node_prefix,id,'repository',repo,'.',NULL,metadata
 FROM snapshots WHERE node_prefix IS NOT NULL
 UNION ALL
 SELECT 'f:'||s.node_prefix||':'||f.file_id,f.snapshot_id,'file',f.name,f.path,NULL,
 json_object('language',a.language,'sha256',a.content_hash)
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id JOIN analyses a ON a.id=f.analysis_id
 UNION ALL
 SELECT 's:'||s.node_prefix||':'||f.file_id||':'||j.key,f.snapshot_id,
 json_extract(j.value,'$[0]'),json_extract(j.value,'$[1]'),f.path,json_extract(j.value,'$[2]'),
 json_object('parent',json_extract(j.value,'$[3]'))
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id
 JOIN analyses a ON a.id=f.analysis_id JOIN json_each(a.symbols) j
"####;

pub const EDGE_VIEW: &str = r####"CREATE VIEW edges AS
 SELECT snapshot_id,source,target,kind,evidence FROM legacy_edges
 UNION ALL SELECT snapshot_id,source,target,kind,evidence FROM resolved_edges
 UNION ALL
 SELECT f.snapshot_id,'r:'||s.node_prefix,'f:'||s.node_prefix||':'||f.file_id,'CONTAINS',f.path
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id
 UNION ALL
 SELECT f.snapshot_id,'f:'||s.node_prefix||':'||f.file_id,
 's:'||s.node_prefix||':'||f.file_id||':'||j.key,'DEFINES',
 f.path||':'||json_extract(j.value,'$[2]')
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id
 JOIN analyses a ON a.id=f.analysis_id JOIN json_each(a.symbols) j
"####;

pub const STAGING_EDGE_TRIGGER: &str = r####"CREATE TRIGGER staging_edge_insert INSTEAD OF INSERT ON edges
        BEGIN INSERT INTO resolved_edges(snapshot_id,source,target,kind,evidence)
        VALUES (new.snapshot_id,new.source,new.target,new.kind,new.evidence); END"####;

/// `rollback_waypoints` recreates the trigger from a more deeply indented string than
/// `_install_views` does; the whitespace is stored in the schema, so it is kept apart.
pub const STAGING_EDGE_TRIGGER_DOWNGRADE: &str = r####"CREATE TRIGGER staging_edge_insert INSTEAD OF INSERT ON edges
            BEGIN INSERT INTO resolved_edges(snapshot_id,source,target,kind,evidence)
            VALUES (new.snapshot_id,new.source,new.target,new.kind,new.evidence); END"####;

pub const OLD_SNAPSHOTS: &str = r####"CREATE TABLE old_snapshots(id INTEGER PRIMARY KEY,repo TEXT NOT NULL,revision TEXT NOT NULL,indexed_at TEXT DEFAULT CURRENT_TIMESTAMP)"####;

/// Revision-bearing evidence is rebound from the snapshot manifest, so cached
/// descriptions never carry a branch identity of their own.
const BOUND_WAYPOINT: &str = r####"json_set(wc.payload,'$.validation',json(vc.payload),'$.evidence',
    json((SELECT json_group_array(json_set(we.value,'$.revision',s.revision))
          FROM json_each(wc.payload,'$.evidence') we)))"####;

const REPOSITORY_NEEDLE: &str = r####"'repository',repo,'.',NULL,metadata
 FROM snapshots"####;

const REPOSITORY_BEFORE: &str = r####"'repository',repo,'.',NULL,
 json_set(metadata,'$.areaWaypoints',json((
   SELECT json_group_array(json(waypoint)) FROM (
     SELECT "####;

const REPOSITORY_AFTER: &str = r####" AS waypoint
     FROM snapshot_waypoints sw JOIN waypoint_cache wc ON wc.id=sw.summary_key
     JOIN waypoint_cache vc ON vc.id=sw.validation_key
     WHERE sw.snapshot_id=s.id AND sw.kind='area' ORDER BY sw.path
   ))))
 FROM snapshots s"####;

const FILE_NEEDLE: &str = r####"json_object('language',a.language,'sha256',a.content_hash)
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id JOIN analyses a ON a.id=f.analysis_id"####;

const FILE_BEFORE: &str = r####"CASE WHEN sw.path IS NULL THEN json_object('language',a.language,'sha256',a.content_hash)
 ELSE json_object('language',a.language,'sha256',a.content_hash,'waypoint',json("####;

const FILE_AFTER: &str = r####")) END
 FROM snapshot_files f JOIN snapshots s ON s.id=f.snapshot_id JOIN analyses a ON a.id=f.analysis_id
 LEFT JOIN snapshot_waypoints sw ON sw.snapshot_id=f.snapshot_id AND sw.path=f.path AND sw.kind='file'
 LEFT JOIN waypoint_cache wc ON wc.id=sw.summary_key
 LEFT JOIN waypoint_cache vc ON vc.id=sw.validation_key"####;

/// The version-3 `nodes` view: the version-2 view with waypoints bound in.
pub fn node_view() -> String {
    let repository = format!("{REPOSITORY_BEFORE}{BOUND_WAYPOINT}{REPOSITORY_AFTER}");
    let file = format!("{FILE_BEFORE}{BOUND_WAYPOINT}{FILE_AFTER}");
    V2_NODE_VIEW
        .replace(REPOSITORY_NEEDLE, &repository)
        .replace(FILE_NEEDLE, &file)
}
