//! Persistence for draft and version metadata, availability, and the
//! notification outbox. The HTML bytes themselves live in the blob store
//! (keryx-store); each version row records the blob's object key.
//!
//! [`DraftStore`] is the seam and [`SeaOrmStore`] its one implementation,
//! which runs on SQLite (the default) or Postgres. An existing SQLite
//! database from an older Keryx is adopted in place; see [`adopt`].

pub mod adopt;
pub mod connect;
pub mod entity;
pub mod migration;
mod store;
mod types;

pub use store::{DraftStore, SeaOrmStore};
pub use types::{
    normalize_wake_time, AvailabilityError, BlobRecord, NewUpload, PendingDelivery, ServedVersion,
    UploadError, UploadOutcome, DEFAULT_DISABLE_REASON,
};
