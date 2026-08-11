//! JSON shapes shared by the server, CLI, and TUI. Field names are camelCase
//! on the wire so agent tooling in any language reads naturally.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummary {
    pub draft_id: String,
    pub title: String,
    pub description: Option<String>,
    pub repo_org: Option<String>,
    pub repo_name: Option<String>,
    pub repo_host: Option<String>,
    pub latest_version_number: Option<i64>,
    pub version_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub latest_version_at: Option<String>,
    pub disabled: bool,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub raw_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub version_number: i64,
    pub created_at: String,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_commit_subject: Option<String>,
    pub git_dirty: Option<bool>,
    pub file_size: i64,
    pub original_filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftDetail {
    pub draft: DraftSummary,
    pub versions: Vec<VersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    pub draft_id: String,
    pub version_id: String,
    pub version_number: i64,
    pub title: String,
    pub public_url: String,
    pub raw_url: String,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UploadMetadata {
    pub repo_org: Option<String>,
    pub repo_name: Option<String>,
    pub repo_host: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_commit_subject: Option<String>,
    pub git_dirty: Option<bool>,
    pub cli_version: Option<String>,
}
