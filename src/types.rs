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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Snoozed => "snoozed",
            Self::Disabled => "disabled",
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

/// Draft activity worth telling an installed Keryx app about. PDF
/// publication and download never produce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationKind {
    Published,
    Revised,
    Woke,
    Enabled,
    Disabled,
}

impl NotificationKind {
    pub const ALL: [Self; 5] = [
        Self::Published,
        Self::Revised,
        Self::Woke,
        Self::Enabled,
        Self::Disabled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Published => "published",
            Self::Revised => "revised",
            Self::Woke => "woke",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Published => "Plan published",
            Self::Revised => "Plan revised",
            Self::Woke => "Plan woke",
            Self::Enabled => "Plan enabled",
            Self::Disabled => "Plan disabled",
        }
    }
}

/// One stored notification event. `key` is unique per real-world
/// occurrence so the same event is never delivered twice, and `target` is a
/// same-origin path the service worker opens on click.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationEvent {
    pub key: String,
    pub kind: NotificationKind,
    pub draft_id: String,
    pub title: String,
    pub body: String,
    pub target: String,
    pub created_at: String,
}

impl NotificationEvent {
    fn new(
        kind: NotificationKind,
        draft_id: &str,
        key: String,
        body: String,
        target: String,
        created_at: &str,
    ) -> Self {
        Self {
            key,
            kind,
            draft_id: draft_id.to_string(),
            title: kind.title().to_string(),
            body,
            target,
            created_at: created_at.to_string(),
        }
    }

    pub fn published(draft_id: &str, draft_title: &str, version_id: &str, at: &str) -> Self {
        Self::new(
            NotificationKind::Published,
            draft_id,
            format!("published:{draft_id}:{version_id}"),
            draft_title.to_string(),
            format!("/d/{draft_id}"),
            at,
        )
    }

    pub fn revised(
        draft_id: &str,
        draft_title: &str,
        version_id: &str,
        version_number: i64,
        at: &str,
    ) -> Self {
        Self::new(
            NotificationKind::Revised,
            draft_id,
            format!("revised:{draft_id}:{version_id}"),
            format!("{draft_title} · v{version_number}"),
            format!("/d/{draft_id}/v/{version_number}"),
            at,
        )
    }

    /// Keyed by the snooze timestamp, so one snooze wakes exactly once even
    /// if the server restarts around the wake time.
    pub fn woke(draft_id: &str, draft_title: &str, snoozed_until: &str, at: &str) -> Self {
        Self::new(
            NotificationKind::Woke,
            draft_id,
            format!("woke:{draft_id}:{snoozed_until}"),
            draft_title.to_string(),
            format!("/d/{draft_id}"),
            at,
        )
    }

    pub fn enabled(draft_id: &str, draft_title: &str, at: &str) -> Self {
        Self::new(
            NotificationKind::Enabled,
            draft_id,
            format!("enabled:{draft_id}:{at}"),
            draft_title.to_string(),
            format!("/?draft={draft_id}&view=active"),
            at,
        )
    }

    pub fn disabled(draft_id: &str, draft_title: &str, at: &str) -> Self {
        Self::new(
            NotificationKind::Disabled,
            draft_id,
            format!("disabled:{draft_id}:{at}"),
            draft_title.to_string(),
            format!("/?draft={draft_id}&view=disabled"),
            at,
        )
    }
}

/// A browser's push subscription as the dashboard sends it. `events` of
/// None keeps the stored preferences (or, for a new subscription, opts in
/// to everything).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionInput {
    pub endpoint: String,
    pub keys: PushKeys,
    #[serde(default)]
    pub events: Option<Vec<NotificationKind>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PushKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushSubscriptionSummary {
    pub id: String,
    pub endpoint: String,
    pub events: Vec<NotificationKind>,
    pub updated_at: String,
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
