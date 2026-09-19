//! The store's vocabulary: inputs, outcomes and errors that mean the same
//! thing on every backend.

use chrono::{DateTime, Utc};
use keryx_core::format_timestamp;
use keryx_core::types::{NotificationEvent, UploadMetadata};

/// One upload, ready to record. The caller resolves the target with
/// [`resolve_upload_target`], mints the version id, builds the object key and
/// writes the blob *before* calling [`record_upload`], so no blob I/O ever
/// happens inside the write transaction.
pub struct NewUpload<'a> {
    pub html: &'a str,
    pub filename: Option<String>,
    pub draft_id: String,
    /// True when `draft_id` was freshly minted and the draft row is inserted
    /// here; false when it names an existing draft.
    pub created: bool,
    pub version_id: String,
    pub object_key: String,
    pub description: Option<String>,
    pub title_from_html: Option<String>,
    pub metadata: &'a UploadMetadata,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub has_inline_script: bool,
    pub external_image_hosts: &'a [String],
}

pub struct UploadOutcome {
    pub draft_id: String,
    pub version_id: String,
    pub version_number: i64,
    pub title: String,
    pub created: bool,
}

#[derive(Debug)]
pub enum UploadError {
    DraftNotFound,
    Other(anyhow::Error),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::DraftNotFound => write!(f, "Draft not found."),
            UploadError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for UploadError {}

pub struct ServedVersion {
    pub draft_id: String,
    pub version_number: i64,
    pub object_key: String,
    pub created_at: String,
}

/// One version's blob as recorded: what `storage migrate` verifies against
/// and what `storage gc` treats as owned.
pub struct BlobRecord {
    pub object_key: String,
    pub content_hash: String,
    pub file_size: i64,
}

#[derive(Debug)]
pub enum AvailabilityError {
    DraftNotFound,
    InvalidWakeTime(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for AvailabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AvailabilityError::DraftNotFound => write!(f, "Draft not found."),
            AvailabilityError::InvalidWakeTime(message) => write!(f, "{message}"),
            AvailabilityError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AvailabilityError {}

impl From<anyhow::Error> for AvailabilityError {
    fn from(e: anyhow::Error) -> Self {
        AvailabilityError::Other(e)
    }
}

/// Accept any RFC 3339 wake time, store it as UTC with milliseconds, and
/// reject anything that is not strictly in the future.
pub fn normalize_wake_time(value: &str, now: DateTime<Utc>) -> Result<String, AvailabilityError> {
    let until = DateTime::parse_from_rfc3339(value.trim())
        .map_err(|_| {
            AvailabilityError::InvalidWakeTime(
                "Wake time must be an RFC 3339 timestamp, e.g. 2026-08-28T08:00:00Z.".into(),
            )
        })?
        .with_timezone(&Utc);
    if until <= now {
        return Err(AvailabilityError::InvalidWakeTime(
            "Wake time must be in the future.".into(),
        ));
    }
    Ok(format_timestamp(until))
}

pub const DEFAULT_DISABLE_REASON: &str = "Disabled by owner.";

/// One event addressed to one subscription, with the keys needed to send it.
#[derive(Debug, Clone)]
pub struct PendingDelivery {
    pub event: NotificationEvent,
    pub subscription_id: String,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    pub attempts: i64,
}
