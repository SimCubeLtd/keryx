//! What a shared draft says about itself: manifest annotations that generic
//! tooling can read, and a config blob that carries everything.

use std::collections::BTreeMap;

use keryx_core::types::UploadMetadata;
use oci_client::manifest::OciImageManifest;
use serde::{Deserialize, Serialize};

// Standard OCI keys first, so generic tooling reads something useful.
pub const ANNOT_TITLE: &str = "org.opencontainers.image.title";
pub const ANNOT_DESCRIPTION: &str = "org.opencontainers.image.description";
pub const ANNOT_CREATED: &str = "org.opencontainers.image.created";
pub const ANNOT_VERSION: &str = "org.opencontainers.image.version";
pub const ANNOT_REVISION: &str = "org.opencontainers.image.revision";
pub const ANNOT_SOURCE: &str = "org.opencontainers.image.source";
// Namespaced keys for the fields OCI has no slot for.
pub const ANNOT_DRAFT_ID: &str = "com.simcube.keryx.draft-id";
pub const ANNOT_VERSION_NUMBER: &str = "com.simcube.keryx.version-number";
pub const ANNOT_CONTENT_SHA256: &str = "com.simcube.keryx.content-sha256";
pub const ANNOT_GIT_BRANCH: &str = "com.simcube.keryx.git-branch";
pub const ANNOT_GIT_DIRTY: &str = "com.simcube.keryx.git-dirty";

const CONFIG_SCHEMA_VERSION: u32 = 1;

/// Metadata for one shared draft version. Every field is optional so a
/// partially annotated artifact still parses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DraftArtifactMeta {
    pub draft_id: Option<String>,
    pub version_number: Option<i64>,
    pub title: Option<String>,
    pub description: Option<String>,
    /// The version's created_at, RFC 3339.
    pub created_at: Option<String>,
    /// sha256 of the HTML, verified on pull.
    pub content_sha256: Option<String>,
    pub repo_host: Option<String>,
    pub repo_org: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_commit_subject: Option<String>,
    pub git_dirty: Option<bool>,
    /// The Keryx version that produced the artifact.
    pub keryx_version: Option<String>,
}

/// The config blob: the metadata plus a schema version.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigBlob {
    schema_version: u32,
    #[serde(flatten)]
    meta: DraftArtifactMeta,
}

impl DraftArtifactMeta {
    /// Manifest annotations, empty fields omitted.
    pub fn to_annotations(&self) -> BTreeMap<String, String> {
        let mut annotations = BTreeMap::new();
        let mut put = |key: &str, value: Option<String>| {
            if let Some(value) = value.filter(|value| !value.is_empty()) {
                annotations.insert(key.to_string(), value);
            }
        };
        put(ANNOT_TITLE, self.title.clone());
        put(ANNOT_DESCRIPTION, self.description.clone());
        put(ANNOT_CREATED, self.created_at.clone());
        put(ANNOT_VERSION, self.version_number.map(|n| format!("v{n}")));
        put(ANNOT_REVISION, self.git_commit_sha.clone());
        put(ANNOT_SOURCE, self.source_url());
        put(ANNOT_DRAFT_ID, self.draft_id.clone());
        put(
            ANNOT_VERSION_NUMBER,
            self.version_number.map(|n| n.to_string()),
        );
        put(ANNOT_CONTENT_SHA256, self.content_sha256.clone());
        put(ANNOT_GIT_BRANCH, self.git_branch.clone());
        put(
            ANNOT_GIT_DIRTY,
            self.git_dirty.map(|dirty| dirty.to_string()),
        );
        annotations
    }

    /// Parse what the annotations can express. The commit subject and the
    /// producing Keryx version live only in the config blob.
    pub fn from_annotations(annotations: &BTreeMap<String, String>) -> Self {
        let get = |key: &str| annotations.get(key).filter(|v| !v.is_empty()).cloned();
        let (repo_host, repo_org, repo_name) = get(ANNOT_SOURCE)
            .and_then(|source| split_source_url(&source))
            .map_or((None, None, None), |(h, o, n)| (Some(h), Some(o), Some(n)));
        DraftArtifactMeta {
            draft_id: get(ANNOT_DRAFT_ID),
            version_number: get(ANNOT_VERSION_NUMBER).and_then(|n| n.parse().ok()),
            title: get(ANNOT_TITLE),
            description: get(ANNOT_DESCRIPTION),
            created_at: get(ANNOT_CREATED),
            content_sha256: get(ANNOT_CONTENT_SHA256),
            repo_host,
            repo_org,
            repo_name,
            git_branch: get(ANNOT_GIT_BRANCH),
            git_commit_sha: get(ANNOT_REVISION),
            git_commit_subject: None,
            git_dirty: get(ANNOT_GIT_DIRTY).and_then(|dirty| dirty.parse().ok()),
            keryx_version: None,
        }
    }

    /// Prefer the config blob, which carries everything; fall back to the
    /// annotations for an artifact whose config does not parse.
    pub(crate) fn from_artifact(manifest: &OciImageManifest, config_json: &str) -> Self {
        if let Ok(blob) = serde_json::from_str::<ConfigBlob>(config_json) {
            if blob.meta.draft_id.is_some() {
                return blob.meta;
            }
        }
        manifest
            .annotations
            .as_ref()
            .map(Self::from_annotations)
            .unwrap_or_default()
    }

    /// The self-describing JSON config blob. Field order is fixed, so the same
    /// version always produces the same bytes and the same manifest digest.
    pub fn to_config_blob(&self) -> Vec<u8> {
        serde_json::to_vec(&ConfigBlob {
            schema_version: CONFIG_SCHEMA_VERSION,
            meta: self.clone(),
        })
        .unwrap_or_else(|_| b"{}".to_vec())
    }

    /// `org.opencontainers.image.title` on the layer is the annotation ORAS
    /// uses to name the file it writes on pull.
    pub(crate) fn layer_annotations(&self) -> BTreeMap<String, String> {
        BTreeMap::from([(
            ANNOT_TITLE.to_string(),
            artifact_filename(
                self.title.as_deref().unwrap_or_default(),
                self.version_number.unwrap_or_default(),
            ),
        )])
    }

    /// The git provenance to replay through the upload endpoint on pull.
    pub fn upload_metadata(&self) -> UploadMetadata {
        UploadMetadata {
            repo_org: self.repo_org.clone(),
            repo_name: self.repo_name.clone(),
            repo_host: self.repo_host.clone(),
            git_branch: self.git_branch.clone(),
            git_commit_sha: self.git_commit_sha.clone(),
            git_commit_subject: self.git_commit_subject.clone(),
            git_dirty: self.git_dirty,
            cli_version: self.keryx_version.clone(),
        }
    }

    fn source_url(&self) -> Option<String> {
        match (&self.repo_host, &self.repo_org, &self.repo_name) {
            (Some(host), Some(org), Some(name)) => Some(format!("https://{host}/{org}/{name}")),
            _ => None,
        }
    }
}

fn split_source_url(source: &str) -> Option<(String, String, String)> {
    let mut parts = source.strip_prefix("https://")?.splitn(3, '/');
    let (host, org, name) = (parts.next()?, parts.next()?, parts.next()?);
    if host.is_empty() || org.is_empty() || name.is_empty() {
        return None;
    }
    Some((host.to_string(), org.to_string(), name.to_string()))
}

/// `<title-slug>-v<n>.html`: the file a plain `oras pull` writes. Only ASCII
/// letters and digits survive, so no title can produce a path.
pub fn artifact_filename(title: &str, version_number: i64) -> String {
    const MAX_SLUG: usize = 80;
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
        if slug.len() >= MAX_SLUG {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    let slug = if slug.is_empty() { "draft" } else { slug };
    format!("{slug}-v{version_number}.html")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> DraftArtifactMeta {
        DraftArtifactMeta {
            draft_id: Some("ab12cd34ef56".into()),
            version_number: Some(3),
            title: Some("Q3 Migration Plan".into()),
            description: Some("Moving the fleet".into()),
            created_at: Some("2026-09-01T10:00:00.000Z".into()),
            content_sha256: Some("4f9c1e".into()),
            repo_host: Some("github.com".into()),
            repo_org: Some("SimCubeLtd".into()),
            repo_name: Some("keryx".into()),
            git_branch: Some("main".into()),
            git_commit_sha: Some("9fc6eb1".into()),
            git_commit_subject: Some("feat: a thing".into()),
            git_dirty: Some(false),
            keryx_version: Some("0.5.1".into()),
        }
    }

    #[test]
    fn annotations_round_trip_and_omit_empty_fields() {
        let annotations = full().to_annotations();
        assert_eq!(annotations[ANNOT_VERSION], "v3");
        assert_eq!(
            annotations[ANNOT_SOURCE],
            "https://github.com/SimCubeLtd/keryx"
        );
        assert_eq!(annotations[ANNOT_GIT_DIRTY], "false");

        // Everything the annotations can express survives the round trip.
        let expected = DraftArtifactMeta {
            git_commit_subject: None,
            keryx_version: None,
            ..full()
        };
        assert_eq!(DraftArtifactMeta::from_annotations(&annotations), expected);

        let sparse = DraftArtifactMeta {
            draft_id: Some("ab12cd34ef56".into()),
            description: Some(String::new()),
            repo_host: Some("github.com".into()),
            ..DraftArtifactMeta::default()
        };
        let annotations = sparse.to_annotations();
        assert_eq!(annotations.len(), 1, "empty and absent fields are omitted");
        assert!(annotations.contains_key(ANNOT_DRAFT_ID));
    }

    #[test]
    fn config_blob_round_trips_everything_and_is_deterministic() {
        let blob = full().to_config_blob();
        assert_eq!(blob, full().to_config_blob());
        let json = String::from_utf8(blob).unwrap();
        assert!(json.starts_with(r#"{"schemaVersion":1,"draftId":"ab12cd34ef56""#));

        let manifest = OciImageManifest::default();
        assert_eq!(DraftArtifactMeta::from_artifact(&manifest, &json), full());
        assert_eq!(
            full().upload_metadata().git_commit_subject.as_deref(),
            Some("feat: a thing")
        );

        // A foreign config falls back to the annotations.
        let annotated = OciImageManifest {
            annotations: Some(full().to_annotations()),
            ..OciImageManifest::default()
        };
        let parsed = DraftArtifactMeta::from_artifact(&annotated, "{}");
        assert_eq!(parsed.draft_id.as_deref(), Some("ab12cd34ef56"));
    }

    #[test]
    fn filenames_are_safe_for_any_title() {
        assert_eq!(
            artifact_filename("Q3 Migration Plan", 3),
            "q3-migration-plan-v3.html"
        );
        assert_eq!(
            artifact_filename("  --Hello,   World!!  ", 1),
            "hello-world-v1.html"
        );
        assert_eq!(artifact_filename("", 2), "draft-v2.html");
        assert_eq!(artifact_filename("计划", 2), "draft-v2.html");
        assert_eq!(artifact_filename("Café plan", 1), "caf-plan-v1.html");
        assert_eq!(
            artifact_filename("../../etc/passwd", 1),
            "etc-passwd-v1.html"
        );
        assert_eq!(
            artifact_filename("C:\\Windows\\system32", 1),
            "c-windows-system32-v1.html"
        );
        let long = artifact_filename(&"a".repeat(500), 1);
        assert!(long.len() <= 80 + "-v1.html".len());
    }
}
