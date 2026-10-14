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

#[cfg(test)]
mod bindings {
    fn typescript() -> String {
        let types = specta::Types::default()
            .register::<crate::types::DashboardSnapshot>()
            .register::<crate::types::Availability>()
            .register::<crate::types::DraftDetail>()
            .register::<crate::types::PruneVersionsRequest>()
            .register::<crate::types::PruneVersionsResponse>()
            .register::<crate::types::PurgeDraftResponse>()
            .register::<crate::types::TagAssignmentRequest>()
            .register::<crate::types::TagAssignmentResponse>()
            .register::<crate::types::AvailabilityUpdate>()
            .register::<crate::types::PushSubscriptionInput>()
            .register::<crate::types::PushSubscriptionSummary>();
        specta_typescript::Typescript::default()
            .header("// Generated from Keryx Rust types by `vp run rust:bindings`. Do not edit.\n")
            .export(&types, specta_serde::PhasesFormat)
            .expect("dashboard wire types must export to TypeScript")
    }

    fn path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../frontend/src/lib/generated/bindings.ts")
    }

    #[test]
    #[ignore = "writes generated frontend bindings; run vp run rust:bindings"]
    fn export_bindings() {
        let generated = typescript();
        if std::fs::read_to_string(path()).ok().as_deref() != Some(generated.as_str()) {
            std::fs::write(path(), generated).expect("write frontend bindings");
        }
    }

    #[test]
    fn frontend_bindings_are_current() {
        assert_eq!(
            std::fs::read_to_string(path()).expect("run vp run rust:bindings first"),
            typescript(),
            "Rust wire types changed; run vp run rust:bindings"
        );
    }
}
