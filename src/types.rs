//! JSON shapes shared by the server, CLI, and TUI. Field names are camelCase
//! on the wire so agent tooling in any language reads naturally.

use chrono::{DateTime, Utc};
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
    #[serde(default)]
    pub latest_git_branch: Option<String>,
    #[serde(default)]
    pub latest_git_commit_sha: Option<String>,
    #[serde(default)]
    pub latest_git_commit_subject: Option<String>,
    #[serde(default)]
    pub latest_git_dirty: Option<bool>,
    pub disabled: bool,
    /// RFC 3339 UTC wake time. A value in the past means the draft is active
    /// again; nothing rewrites the row when a snooze expires.
    #[serde(default)]
    pub snoozed_until: Option<String>,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub raw_url: String,
}

impl DraftSummary {
    pub fn availability(&self) -> Availability {
        Availability::derive(self.disabled, self.snoozed_until.as_deref(), Utc::now())
    }
}

/// The three exclusive availability states. Snoozed drafts still serve;
/// disabled drafts do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Availability {
    Active,
    Snoozed,
    Disabled,
}

impl Availability {
    /// `disabled` wins; otherwise a future `snoozed_until` means snoozed and
    /// everything else is active. An unparseable wake time never hides a
    /// draft.
    pub fn derive(disabled: bool, snoozed_until: Option<&str>, now: DateTime<Utc>) -> Self {
        if disabled {
            return Self::Disabled;
        }
        let snoozed = snoozed_until
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_some_and(|until| until.with_timezone(&Utc) > now);
        if snoozed {
            Self::Snoozed
        } else {
            Self::Active
        }
    }
}

/// A requested availability transition. On the wire:
/// `{"state":"active"}`, `{"state":"snoozed","until":"2026-08-28T08:00:00Z"}`,
/// or `{"state":"disabled","reason":"Superseded"}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum AvailabilityUpdate {
    Active,
    Snoozed {
        until: String,
    },
    Disabled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub version_number: i64,
    pub created_at: String,
    #[serde(default)]
    pub repo_org: Option<String>,
    #[serde(default)]
    pub repo_name: Option<String>,
    #[serde(default)]
    pub repo_host: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_is_derived_from_time_without_a_write() {
        let now = DateTime::parse_from_rfc3339("2026-08-28T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        assert_eq!(Availability::derive(false, None, now), Availability::Active);
        assert_eq!(
            Availability::derive(false, Some("2026-08-28T12:00:00.001Z"), now),
            Availability::Snoozed
        );
        assert_eq!(
            Availability::derive(false, Some("2026-08-28T12:00:00.000Z"), now),
            Availability::Active
        );
        assert_eq!(
            Availability::derive(false, Some("not a time"), now),
            Availability::Active
        );
        assert_eq!(
            Availability::derive(true, Some("2099-01-01T00:00:00.000Z"), now),
            Availability::Disabled
        );
    }

    #[test]
    fn availability_update_uses_the_documented_wire_shape() {
        let snoozed: AvailabilityUpdate =
            serde_json::from_str(r#"{"state":"snoozed","until":"2026-08-28T08:00:00.000Z"}"#)
                .unwrap();
        assert!(
            matches!(snoozed, AvailabilityUpdate::Snoozed { ref until } if until == "2026-08-28T08:00:00.000Z")
        );

        let disabled: AvailabilityUpdate = serde_json::from_str(r#"{"state":"disabled"}"#).unwrap();
        assert!(matches!(
            disabled,
            AvailabilityUpdate::Disabled { reason: None }
        ));

        assert_eq!(
            serde_json::to_string(&AvailabilityUpdate::Active).unwrap(),
            r#"{"state":"active"}"#
        );
        assert!(serde_json::from_str::<AvailabilityUpdate>(r#"{"state":"snoozed"}"#).is_err());
        assert!(serde_json::from_str::<AvailabilityUpdate>(r#"{"state":"paused"}"#).is_err());
    }
}
