//! The config file's schema. Sections by concern, keys named after the flags
//! they stand in for. Every key is optional, and unknown keys are rejected.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub client: ClientConfig,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
}

/// Values for the CLI and TUI, which talk to a Keryx server over HTTP.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClientConfig {
    /// `--api-url` / `KERYX_API_URL`.
    pub api_url: Option<String>,
    /// The base repository `keryx share` pushes under: `--to`.
    pub share_to: Option<String>,
}

/// Values for `keryx serve`.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub data_dir: Option<PathBuf>,
    pub public_base_url: Option<String>,
    /// A secret. Keep the file owner-readable only if you set it here.
    pub api_key: Option<String>,
    pub max_html_bytes: Option<usize>,
    pub allow_font_links: Option<bool>,
    pub allow_safe_handlers: Option<bool>,
    pub allow_inline_scripts: Option<bool>,
    pub push_contact: Option<String>,
}

/// Shared by `keryx serve` and the offline `keryx storage` commands.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    /// SQLite path: `--db`.
    pub path: Option<PathBuf>,
    /// `--database-url`. Can carry a password; keep the file owner-readable
    /// only if you set it here.
    pub url: Option<String>,
    pub pool_size: Option<u32>,
    pub no_backup: Option<bool>,
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// `--storage`.
    pub kind: Option<StorageKind>,
    pub s3: S3Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageKind {
    Disk,
    S3,
}

impl StorageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StorageKind::Disk => "disk",
            StorageKind::S3 => "s3",
        }
    }
}

/// S3 credentials are deliberately not here: they resolve through the
/// standard AWS chain, never through Keryx.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct S3Config {
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub endpoint: Option<String>,
    pub prefix: Option<String>,
    pub profile: Option<String>,
}

/// A clap argument id and the value the config file gives it.
pub type ArgDefault = (&'static str, String);

impl FileConfig {
    /// A leading `~/` in a path is the home directory, as people write it.
    pub(crate) fn expand_home(&mut self) {
        for path in [&mut self.server.data_dir, &mut self.database.path]
            .into_iter()
            .flatten()
        {
            if let (Ok(rest), Some(home)) = (path.strip_prefix("~"), dirs::home_dir()) {
                *path = home.join(rest);
            }
        }
    }

    /// Defaults for the database flags (`DatabaseArgs`).
    pub fn database_arg_defaults(&self) -> Vec<ArgDefault> {
        let database = &self.database;
        let mut defaults = Vec::new();
        push(
            &mut defaults,
            "db",
            database.path.as_ref().map(|p| p.display().to_string()),
        );
        push(&mut defaults, "database_url", database.url.clone());
        push(
            &mut defaults,
            "db_pool_size",
            database.pool_size.map(|n| n.to_string()),
        );
        push(
            &mut defaults,
            "no_backup",
            database.no_backup.map(|b| b.to_string()),
        );
        defaults
    }

    /// Defaults for the S3 flags (`S3Args`).
    pub fn s3_arg_defaults(&self) -> Vec<ArgDefault> {
        let s3 = &self.storage.s3;
        let mut defaults = Vec::new();
        push(&mut defaults, "s3_bucket", s3.bucket.clone());
        push(&mut defaults, "s3_region", s3.region.clone());
        push(&mut defaults, "s3_endpoint", s3.endpoint.clone());
        push(&mut defaults, "s3_prefix", s3.prefix.clone());
        push(&mut defaults, "s3_profile", s3.profile.clone());
        defaults
    }

    /// `--storage`, for the commands that take one.
    pub fn storage_kind_arg_default(&self) -> Vec<ArgDefault> {
        let mut defaults = Vec::new();
        push(
            &mut defaults,
            "storage",
            self.storage.kind.map(|k| k.as_str().to_string()),
        );
        defaults
    }

    /// `--data-dir`, shared by `serve` and the storage commands.
    pub fn data_dir_arg_default(&self) -> Vec<ArgDefault> {
        let mut defaults = Vec::new();
        push(
            &mut defaults,
            "data_dir",
            self.server
                .data_dir
                .as_ref()
                .map(|p| p.display().to_string()),
        );
        defaults
    }

    /// Defaults for the flags only `keryx serve` has.
    pub fn serve_arg_defaults(&self) -> Vec<ArgDefault> {
        let server = &self.server;
        let mut defaults = Vec::new();
        push(&mut defaults, "host", server.host.clone());
        push(&mut defaults, "port", server.port.map(|n| n.to_string()));
        push(
            &mut defaults,
            "public_base_url",
            server.public_base_url.clone(),
        );
        push(&mut defaults, "api_key", server.api_key.clone());
        push(
            &mut defaults,
            "max_html_bytes",
            server.max_html_bytes.map(|n| n.to_string()),
        );
        push(
            &mut defaults,
            "allow_font_links",
            server.allow_font_links.map(|b| b.to_string()),
        );
        push(
            &mut defaults,
            "allow_safe_handlers",
            server.allow_safe_handlers.map(|b| b.to_string()),
        );
        push(
            &mut defaults,
            "allow_inline_scripts",
            server.allow_inline_scripts.map(|b| b.to_string()),
        );
        push(&mut defaults, "push_contact", server.push_contact.clone());
        defaults
    }

    /// `keryx share --to`.
    pub fn share_arg_defaults(&self) -> Vec<ArgDefault> {
        let mut defaults = Vec::new();
        push(&mut defaults, "to", self.client.share_to.clone());
        defaults
    }
}

fn push(defaults: &mut Vec<ArgDefault>, id: &'static str, value: Option<String>) {
    if let Some(value) = value {
        defaults.push((id, value));
    }
}
