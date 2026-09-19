//! Shared vocabulary for every Keryx crate: wire types, id generation, the
//! content hash and the one timestamp format. Depends on no other Keryx crate.

pub mod ids;
pub mod types;

use chrono::{DateTime, SecondsFormat, Utc};
use sha2::{Digest, Sha256};

pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn now() -> String {
    format_timestamp(Utc::now())
}

/// Every stored timestamp uses this one shape, so string comparison in SQL
/// orders correctly and equal instants compare equal.
pub fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}
