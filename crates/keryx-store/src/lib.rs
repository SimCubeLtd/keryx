//! Blob storage for draft HTML. The database stays the metadata index; the
//! documents themselves live as opaque objects behind [`BlobBackend`], on
//! local disk by default or in any S3-compatible store.

mod backend;
#[cfg(feature = "s3")]
mod s3;

#[cfg(any(test, feature = "test-support"))]
pub use backend::memory_backend;
pub use backend::{
    create_backend, object_key, BackendConfig, BlobBackend, BlobEntry, DiskConfig, OpenDalBackend,
    S3Config,
};
