//! Configured deployment topology: port of `architecture_topology.py`.
//! It is never evidence of a running deployment.

use super::PyError;
use super::contract::digest;
use super::evidence::evidence;
use super::util::{array, get, lookup, lower_name, text_of};
use super::yaml::{ConfigurationError, Node, configuration_documents};
use crate::py_regex::{regex, search};
use fancy_regex::Regex;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

pub const VERSION: &str = "deployment-1";

const NOTICE: &str = "Configured only. No cloud resources, containers or endpoints were contacted. HCL modules and Helm templates are not evaluated; runtime and OTLP observations must remain separate evidence.";

static FROM_PRESENT: LazyLock<Regex> = LazyLock::new(|| regex(r"(?im)^\s*FROM\s+"));
static FROM_LINE: LazyLock<Regex> =
    LazyLock::new(|| regex(r"(?im)^\s*FROM\s+(?:--\S+\s+)*([^\s]+)(?:\s+AS\s+(\w+))?"));
static RESOURCE: LazyLock<Regex> =
    LazyLock::new(|| regex(r#"(?m)^\s*resource\s+"([a-z0-9_]+)"\s+"([A-Za-z0-9_-]+)"\s*\{"#));
static YAML_LIKE: LazyLock<Regex> = LazyLock::new(|| {
    regex(r"(?m)^\s*(?:apiVersion|services|jobs|kind|steps|openapi|asyncapi)\s*:")
});

/// `_name`: a short, plain identifier, or `unresolved`.
fn name(value: &str) -> String {
    let fits = (1..=128).contains(&value.chars().count())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.:/-".contains(c));
    if fits { value } else { "unresolved" }.to_owned()
}

/// The scalar-keyed mapping of a node; a repeated key is an error.
fn mapping(node: Option<&Node>) -> Result<Vec<(&str, &Node)>, ConfigurationError> {
    let Some(Node::Mapping { pairs, .. }) = node else {
        return Ok(Vec::new());
    };
    let mut found: Vec<(&str, &Node)> = Vec::new();
    let mut scalar_pairs = 0;
    for (key, value) in pairs {
        if let Node::Scalar { value: text, .. } = key {
            scalar_pairs += 1;
            match found.iter_mut().find(|(existing, _)| existing == text) {
                Some(slot) => slot.1 = value,
                None => found.push((text, value)),
            }
        }
    }
    if found.len() != scalar_pairs {
        return Err(ConfigurationError("Duplicate configuration key".into()));
    }
    Ok(found)
}

fn find<'a>(values: &[(&str, &'a Node)], key: &str) -> Option<&'a Node> {
    values
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, node)| *node)
}

/// `_scalar`: a scalar's text, else empty.
fn scalar(node: Option<&Node>) -> &str {
    match node {
        Some(Node::Scalar { value, .. }) => value,
        _ => "",
    }
}

struct Facts {
    resources: Vec<Value>,
    links: Vec<Value>,
    partial: bool,
}

impl Facts {
    /// `add(kind, name, line, environment, **values)`.
    fn add(
        &mut self,
        kind: &str,
        resource: &str,
        line: usize,
        environment: &str,
        values: &[(&str, Value)],
    ) {
        let mut entry = Map::new();
        entry.insert("kind".into(), json!(kind));
        entry.insert("name".into(), json!(name(resource)));
        entry.insert("line".into(), json!(line));
        entry.insert("environment".into(), json!(name(environment)));
        for (key, value) in values {
            entry.insert((*key).to_owned(), value.clone());
        }
        self.resources.push(Value::Object(entry));
    }
}

/// Line of a match's start: newlines before it, plus one.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

fn dockerfile_facts(facts: &mut Facts, source: &str) {
    for (index, found) in FROM_LINE.captures_iter(source).flatten().enumerate() {
        let whole = found.get(0).expect("group zero");
        let image = found.get(1).map_or("", |group| group.as_str());
        let stage = found.get(2).map(|group| group.as_str());
        let base = image.split(':').next().unwrap_or("");
        let leaf = base.rsplit('/').next().unwrap_or("");
        let family = ["python", "node", "golang", "alpine", "debian"]
            .into_iter()
            .find(|candidate| leaf == *candidate)
            .unwrap_or("unspecified");
        let stage_name = match stage {
            Some(found) if !found.is_empty() => found.to_owned(),
            _ => format!("stage-{index}"),
        };
        facts.add(
            "Container",
            &stage_name,
            line_of(source, whole.start()),
            "unspecified",
            &[
                ("image_digest", json!(digest(&json!(image)))),
                ("runtime_family", json!(family)),
            ],
        );
    }
    facts.add("BuildArtifact", "container-image", 1, "unspecified", &[]);
}

fn terraform_facts(facts: &mut Facts, source: &str) {
    for found in RESOURCE.captures_iter(source).flatten() {
        let whole = found.get(0).expect("group zero");
        let resource_type = found.get(1).map_or("", |group| group.as_str());
        let label = found.get(2).map_or("", |group| group.as_str());
        let kind = match resource_type {
            "google_cloud_run_v2_service"
            | "google_cloud_run_service"
            | "aws_ecs_service"
            | "azurerm_linux_web_app" => "Application",
            "google_sql_database_instance" | "aws_db_instance" => "Database",
            "google_pubsub_topic" | "aws_sqs_queue" => "Queue",
            "google_storage_bucket" | "aws_s3_bucket" => "Storage",
            _ => "DeploymentTarget",
        };
        facts.add(
            kind,
            label,
            line_of(source, whole.start()),
            "unspecified",
            &[("configured_type", json!(resource_type))],
        );
        // HCL references, modules, interpolation and workspaces are not evaluated.
        facts.partial = true;
    }
}

fn yaml_facts(facts: &mut Facts, source: &str) -> Result<(), ConfigurationError> {
    for document in &configuration_documents(source)? {
        let top = mapping(Some(document))?;
        let line = document.line();
        for api in ["openapi", "asyncapi"] {
            if !scalar(find(&top, api)).is_empty() {
                let info = mapping(find(&top, "info"))?;
                if !scalar(find(&info, "version")).is_empty() {
                    facts.add(
                        "ExternalAPI",
                        &format!("{api}-contract"),
                        line,
                        "unspecified",
                        &[("configured_type", json!(api))],
                    );
                }
            }
        }
        let kind = scalar(find(&top, "kind"));
        if !kind.is_empty() {
            let meta = mapping(find(&top, "metadata"))?;
            let resource = Some(scalar(find(&meta, "name")))
                .filter(|found| !found.is_empty())
                .unwrap_or("unnamed");
            let environment = Some(scalar(find(&meta, "namespace")))
                .filter(|found| !found.is_empty())
                .unwrap_or("unspecified");
            let mapped = match kind {
                "Deployment" | "StatefulSet" | "Job" => Some("Application"),
                "Service" => Some("Service"),
                "PersistentVolumeClaim" => Some("Storage"),
                _ => None,
            };
            match mapped {
                Some(mapped) => facts.add(
                    mapped,
                    resource,
                    line,
                    environment,
                    &[("configured_type", json!(kind))],
                ),
                None => facts.partial = true,
            }
        }
        let services = mapping(find(&top, "services"))?;
        for (service, node) in &services {
            let config = mapping(Some(node))?;
            let image = scalar(find(&config, "image"));
            let family = image
                .rsplit('/')
                .next()
                .unwrap_or("")
                .split(':')
                .next()
                .unwrap_or("");
            let resource_kind = match family {
                "postgres" | "mysql" | "mariadb" | "mongo" => "Database",
                "rabbitmq" | "kafka" => "Queue",
                "redis" | "valkey" | "minio" => "Storage",
                _ => "Application",
            };
            facts.add(
                resource_kind,
                service,
                node.line(),
                "unspecified",
                &[
                    (
                        "image_digest",
                        if image.is_empty() {
                            Value::Null
                        } else {
                            json!(digest(&json!(image)))
                        },
                    ),
                    ("configured_type", json!("compose-service")),
                ],
            );
            let depends = find(&config, "depends_on");
            let names: Vec<String> = match depends {
                Some(Node::Sequence { items, .. }) => items
                    .iter()
                    .map(|item| scalar(Some(item)).to_owned())
                    .collect(),
                other => mapping(other)?
                    .into_iter()
                    .map(|(key, _)| key.to_owned())
                    .collect(),
            };
            for target in names {
                if services.iter().any(|(existing, _)| *existing == target) {
                    facts.links.push(json!({
                        "source": name(service),
                        "target": name(&target),
                        "kind": "DEPENDS_ON",
                    }));
                } else {
                    facts.partial = true;
                }
            }
        }
        for (job_name, node) in mapping(find(&top, "jobs"))? {
            let job = mapping(Some(node))?;
            let env = find(&job, "environment");
            let environment = match scalar(env) {
                "" => scalar(find(&mapping(env)?, "name")).to_owned(),
                found => found.to_owned(),
            };
            if !environment.is_empty() {
                facts.add(
                    "DeploymentEnvironment",
                    &environment,
                    node.line(),
                    &environment,
                    &[],
                );
            }
            let shown = if environment.is_empty() {
                "unspecified"
            } else {
                &environment
            };
            facts.add(
                "BuildArtifact",
                job_name,
                node.line(),
                shown,
                &[("configured_type", json!("ci-job"))],
            );
        }
    }
    Ok(())
}

/// `deployment_facts`: the resources and links one configuration file declares.
pub fn deployment_facts(source: &str) -> Value {
    let mut facts = Facts {
        resources: Vec::new(),
        links: Vec::new(),
        partial: false,
    };
    if search(&FROM_PRESENT, source) {
        dockerfile_facts(&mut facts, source);
    }
    terraform_facts(&mut facts, source);
    if search(&YAML_LIKE, source) && yaml_facts(&mut facts, source).is_err() {
        facts.partial = true;
    }
    let partial = facts.partial || facts.resources.len() > 128 || facts.links.len() > 256;
    json!({
        "version": VERSION,
        "resources": facts.resources.iter().take(128).collect::<Vec<_>>(),
        "links": facts.links.iter().take(256).collect::<Vec<_>>(),
        "partial": partial,
    })
}

fn is_deployment_file(path: &str) -> bool {
    let name = lower_name(path);
    name.starts_with("dockerfile")
        || name.ends_with(".yaml")
        || name.ends_with(".yml")
        || name.ends_with(".tf")
}

/// `topology(graph)`: every configured entity and link in a snapshot, and where two files disagree.
pub fn topology(graph: &Value) -> Result<Value, PyError> {
    let mut entities: Vec<Value> = Vec::new();
    let mut relationships: Vec<Value> = Vec::new();
    let mut partial = false;
    for file in array(get(graph, "files")?) {
        let path = text_of(get(file, "path")?);
        if !is_deployment_file(path) {
            continue;
        }
        let file_facts = get(file, "facts")?;
        let empty = json!({});
        let facts = lookup(file_facts, "deployment").unwrap_or(&empty);
        let resources = lookup(facts, "resources").map(array).unwrap_or_default();
        partial |= lookup(facts, "partial").is_some_and(super::util::truthy);
        if resources.is_empty() {
            continue;
        }
        let mut ids: BTreeMap<String, String> = BTreeMap::new();
        for resource in resources {
            let key = digest(&json!([
                get(graph, "repository")?,
                get(file, "path")?,
                get(resource, "kind")?,
                get(resource, "name")?,
                get(resource, "environment")?,
            ]))[..24]
                .to_owned();
            ids.insert(text_of(get(resource, "name")?).to_owned(), key.clone());
            let mut entity = resource.as_object().cloned().unwrap_or_default();
            entity.insert("id".into(), json!(key));
            entity.insert("path".into(), get(file, "path")?.clone());
            entity.insert("evidence_class".into(), json!("configured"));
            entity.insert("runtime_observed".into(), json!(false));
            entity.insert(
                "configuration_digest".into(),
                lookup(file_facts, "sha256").cloned().unwrap_or(Value::Null),
            );
            entity.insert(
                "evidence".into(),
                evidence(graph, file, get(resource, "line")?, VERSION)?,
            );
            entities.push(Value::Object(entity));
        }
        for link in lookup(facts, "links").map(array).unwrap_or_default() {
            let (source, target) = (text_of(get(link, "source")?), text_of(get(link, "target")?));
            if let (Some(from), Some(to)) = (ids.get(source), ids.get(target)) {
                let mut relationship = link.as_object().cloned().unwrap_or_default();
                relationship.insert("source".into(), json!(from));
                relationship.insert("target".into(), json!(to));
                relationship.insert("evidence_class".into(), json!("configured"));
                relationship.insert(
                    "evidence".into(),
                    evidence(graph, file, &json!(1), VERSION)?,
                );
                relationships.push(Value::Object(relationship));
            }
        }
    }
    let mut by_identity: BTreeMap<(String, String, String), Vec<Value>> = BTreeMap::new();
    for entity in &entities {
        let key = (
            text_of(get(entity, "kind")?).to_owned(),
            text_of(get(entity, "name")?).to_owned(),
            text_of(get(entity, "environment")?).to_owned(),
        );
        let digests = by_identity.entry(key).or_default();
        let current = get(entity, "configuration_digest")?;
        if !digests.contains(current) {
            digests.push(current.clone());
        }
    }
    let conflicts: Vec<Value> = by_identity
        .iter()
        .filter(|(_, digests)| digests.len() > 1)
        .map(|((kind, resource, environment), _)| {
            json!({
                "kind": kind,
                "name": resource,
                "environment": environment,
                "status": "review-configurations",
            })
        })
        .collect();
    Ok(json!({
        "version": VERSION,
        "entities": entities.iter().take(256).collect::<Vec<_>>(),
        "relationships": relationships.iter().take(256).collect::<Vec<_>>(),
        "configuration_conflicts": conflicts.iter().take(64).collect::<Vec<_>>(),
        "partial": partial || entities.len() > 256 || relationships.len() > 256,
        "notice": NOTICE,
    }))
}

/// `compare_topology(before, after)`: which configured entities appeared, went or changed.
pub fn compare_topology(before: &Value, after: &Value) -> Result<Value, PyError> {
    let index = |side: &Value| -> Result<BTreeMap<String, Value>, PyError> {
        let mut found = BTreeMap::new();
        for entity in array(get(side, "entities")?) {
            found.insert(text_of(get(entity, "id")?).to_owned(), entity.clone());
        }
        Ok(found)
    };
    let (old, new) = (index(before)?, index(after)?);
    let keys: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut changes: Vec<Value> = Vec::new();
    for key in keys {
        let (left, right) = (old.get(key), new.get(key));
        let change = match (left, right) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            (Some(left), Some(right))
                if get(left, "configuration_digest")? != get(right, "configuration_digest")? =>
            {
                "configuration-changed"
            }
            _ => continue,
        };
        let path = get(right.or(left).expect("one side exists"), "path")?;
        changes.push(json!({"id": key, "change": change, "path": path}));
    }
    let partial = super::util::truthy(get(before, "partial")?)
        || super::util::truthy(get(after, "partial")?)
        || changes.len() > 128;
    Ok(json!({
        "deployment_changes": changes.iter().take(128).collect::<Vec<_>>(),
        "partial": partial,
    }))
}
