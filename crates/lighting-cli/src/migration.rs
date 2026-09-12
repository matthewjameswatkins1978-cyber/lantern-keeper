use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::Path};

#[derive(Debug, Deserialize)]
pub struct BasicMemorySnapshot {
    pub project: String,
    pub project_id: String,
    pub exported_at: String,
    pub records: Vec<BasicMemoryRecord>,
}

#[derive(Debug, Deserialize)]
pub struct BasicMemoryRecord {
    pub path: String,
    pub title: Option<String>,
    pub note_type: Option<String>,
    pub permalink: Option<String>,
    pub external_id: Option<String>,
    pub updated_at: Option<String>,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct MigrationManifest {
    pub source_project: String,
    pub source_project_id: String,
    pub export_date: String,
    pub file_count: usize,
    pub directory_count: usize,
    pub byte_total: usize,
    pub observation_count: usize,
    pub relation_count: usize,
    pub unresolved_relation_count: usize,
    pub manifest_sha256: String,
    pub records: Vec<ManifestRecord>,
}

#[derive(Debug, Serialize)]
pub struct ManifestRecord {
    pub path: String,
    pub title: Option<String>,
    pub note_type: Option<String>,
    pub permalink: Option<String>,
    pub external_id: Option<String>,
    pub updated_at: Option<String>,
    pub byte_len: usize,
    pub sha256: String,
    pub observation_count: usize,
    pub relation_count: usize,
}

pub fn load_snapshot(path: &Path) -> Result<BasicMemorySnapshot> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read snapshot {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to decode snapshot {}", path.display()))
}

pub fn manifest(snapshot: &BasicMemorySnapshot) -> MigrationManifest {
    let known_refs: HashSet<String> = snapshot
        .records
        .iter()
        .flat_map(|record| {
            [
                record.path.clone(),
                record
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches(".md")
                    .to_lowercase(),
                record.title.clone().unwrap_or_default(),
                record.permalink.clone().unwrap_or_default(),
            ]
        })
        .filter(|value| !value.is_empty())
        .map(|value| value.to_lowercase())
        .collect();
    let records: Vec<ManifestRecord> = snapshot
        .records
        .iter()
        .map(|record| ManifestRecord {
            path: record.path.clone(),
            title: record.title.clone(),
            note_type: record.note_type.clone(),
            permalink: record.permalink.clone(),
            external_id: record.external_id.clone(),
            updated_at: record.updated_at.clone(),
            byte_len: record.content.len(),
            sha256: hex_hash(record.content.as_bytes()),
            observation_count: parse_observations(&record.content).len(),
            relation_count: parse_relations(&record.content).len(),
        })
        .collect();
    let canonical = serde_json::to_vec(&records).expect("manifest records serialize");
    let directories: HashSet<&str> = records
        .iter()
        .filter_map(|record| record.path.rsplit_once('/').map(|(directory, _)| directory))
        .collect();
    MigrationManifest {
        source_project: snapshot.project.clone(),
        source_project_id: snapshot.project_id.clone(),
        export_date: snapshot.exported_at.clone(),
        file_count: records.len(),
        directory_count: directories.len(),
        byte_total: records.iter().map(|r| r.byte_len).sum(),
        observation_count: records.iter().map(|r| r.observation_count).sum(),
        relation_count: records.iter().map(|r| r.relation_count).sum(),
        unresolved_relation_count: snapshot
            .records
            .iter()
            .flat_map(|record| parse_relations(&record.content))
            .filter(|relation| !known_refs.contains(&relation.target_ref.to_lowercase()))
            .count(),
        manifest_sha256: hex_hash(&canonical),
        records,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedObservation {
    pub category: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedRelation {
    pub relation_type: String,
    pub target_ref: String,
}

/// Extracts Basic Memory's useful `- [category] proposition` convention while
/// leaving unknown categories intact for the importer to preserve.
pub fn parse_observations(content: &str) -> Vec<ParsedObservation> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("- [")?;
            let end = rest.find("] ")?;
            let category = rest[..end].trim();
            let text = rest[end + 2..].trim();
            if category.is_empty() || text.is_empty() {
                return None;
            }
            Some(ParsedObservation {
                category: category.to_owned(),
                text: text.to_owned(),
            })
        })
        .collect()
}

/// Extracts typed wiki-link relations, including links whose target is not
/// present in the snapshot. Unresolved targets remain durable import data.
pub fn parse_relations(content: &str) -> Vec<ParsedRelation> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("- ")?;
            let start = rest.find("[[")?;
            let end = rest[start + 2..]
                .find("]] ")
                .or_else(|| rest[start + 2..].find("]]"))?;
            let target = rest[start + 2..start + 2 + end].trim();
            let relation_type = rest[..start].trim();
            if relation_type.is_empty() || target.is_empty() {
                return None;
            }
            let target_ref = target.split('|').next().unwrap_or(target).trim();
            Some(ParsedRelation {
                relation_type: relation_type.to_owned(),
                target_ref: target_ref.to_owned(),
            })
        })
        .collect()
}

/// Reads the small status vocabulary used by Basic Memory frontmatter. Unknown
/// statuses remain normal candidates; only explicit historical/archive values
/// are kept out of Lantern's current view.
pub fn is_historical(content: &str) -> bool {
    let Some(frontmatter) = content.strip_prefix("---") else {
        return false;
    };
    let Some((frontmatter, _)) = frontmatter.split_once("\n---") else {
        return false;
    };
    frontmatter.lines().any(|line| {
        let Some((key, value)) = line.split_once(':') else {
            return false;
        };
        if key.trim().eq_ignore_ascii_case("status") {
            let status = value.trim().trim_matches(['\'', '"']).to_lowercase();
            return status == "historical"
                || status == "archive"
                || status == "archived"
                || status == "superseded"
                || status.starts_with("historical-");
        }
        false
    })
}

pub fn validate(snapshot: &BasicMemorySnapshot) -> Result<MigrationManifest> {
    if snapshot.project != "Lantern" {
        anyhow::bail!("snapshot project is {}, expected Lantern", snapshot.project);
    }
    if snapshot
        .records
        .iter()
        .any(|r| r.path.trim().is_empty() || r.content.is_empty())
    {
        anyhow::bail!("snapshot contains an empty path or empty source content");
    }
    Ok(manifest(snapshot))
}

fn hex_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(path: &str, content: &str) -> BasicMemoryRecord {
        BasicMemoryRecord {
            path: path.to_owned(),
            title: Some(path.to_owned()),
            note_type: Some("fact".to_owned()),
            permalink: Some(format!("memory://{path}")),
            external_id: Some(format!("id-{path}")),
            updated_at: Some("2026-09-12T00:00:00Z".to_owned()),
            content: content.to_owned(),
        }
    }

    #[test]
    fn manifest_is_deterministic_and_counts_unique_directories() {
        let snapshot = BasicMemorySnapshot {
            project: "Lantern".to_owned(),
            project_id: "project-1".to_owned(),
            exported_at: "2026-09-12T00:00:00Z".to_owned(),
            records: vec![
                record("projects/lantern.md", "one"),
                record("projects/tethers.md", "two"),
                record("reference/rules.md", "three"),
            ],
        };

        let first = manifest(&snapshot);
        let second = manifest(&snapshot);
        assert_eq!(first.file_count, 3);
        assert_eq!(first.directory_count, 2);
        assert_eq!(first.byte_total, 11);
        assert_eq!(first.manifest_sha256, second.manifest_sha256);
    }

    #[test]
    fn parses_unknown_observations_and_typed_relations() {
        let content = "- [current] Threadmoth is 1.9.1\n- governed_by [[Lantern Constitution]]\n- related_to [[Tethers|Tethers project]]";
        assert_eq!(parse_observations(content).len(), 1);
        assert_eq!(parse_observations(content)[0].category, "current");
        let relations = parse_relations(content);
        assert_eq!(relations.len(), 2);
        assert_eq!(relations[1].target_ref, "Tethers");
    }

    #[test]
    fn validation_rejects_wrong_project_and_empty_sources() {
        let mut snapshot = BasicMemorySnapshot {
            project: "Other".to_owned(),
            project_id: "project-1".to_owned(),
            exported_at: "2026-09-12T00:00:00Z".to_owned(),
            records: vec![record("projects/lantern.md", "one")],
        };
        assert!(validate(&snapshot).is_err());

        snapshot.project = "Lantern".to_owned();
        snapshot.records[0].content.clear();
        assert!(validate(&snapshot).is_err());
    }

    #[test]
    fn recognizes_explicit_historical_frontmatter_only() {
        assert!(is_historical(
            "---\nstatus: historical-price-snapshot\n---\ntext"
        ));
        assert!(is_historical("---\nstatus: archive\n---\ntext"));
        assert!(!is_historical("---\nstatus: active\n---\ntext"));
        assert!(!is_historical("plain text"));
    }
}
