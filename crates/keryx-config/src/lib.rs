//! The Keryx config file, the lowest layer of configuration:
//!
//! ```text
//! command-line flag  >  environment variable  >  config.toml  >  built-in default
//! ```
//!
//! This crate owns the file only: where it lives, its typed schema, strict
//! validation, and updating it in place. Flags and environment variables stay
//! with clap; the binary feeds this file's values to clap as defaults, so
//! clap resolves the whole order and `--help` shows what is in effect.

mod schema;
mod write;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};

pub use schema::{
    ClientConfig, DatabaseConfig, FileConfig, S3Config, ServerConfig, StorageConfig, StorageKind,
};
pub use write::set_client_api_url;

/// Overrides the search below. Also available as the global `--config` flag.
pub const CONFIG_ENV: &str = "KERYX_CONFIG";

/// The config file that was loaded, and where it came from.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Loaded {
    pub config: FileConfig,
    /// None when no config file exists, which is fine: every key is optional.
    pub path: Option<PathBuf>,
}

static LOADED: OnceLock<Loaded> = OnceLock::new();

/// Load the config file once for this process. `explicit` is `--config` or
/// `KERYX_CONFIG`; an explicit path that does not exist is an error, a
/// missing file at the default locations is not.
pub fn init(explicit: Option<&Path>) -> Result<&'static Loaded> {
    if let Some(loaded) = LOADED.get() {
        return Ok(loaded);
    }
    let loaded = load(explicit)?;
    Ok(LOADED.get_or_init(|| loaded))
}

/// What [`init`] loaded, or an empty config if nothing called it (tests, and
/// library users that never touch a config file).
pub fn get() -> &'static Loaded {
    static EMPTY: OnceLock<Loaded> = OnceLock::new();
    LOADED
        .get()
        .unwrap_or_else(|| EMPTY.get_or_init(Loaded::default))
}

/// Where Keryx looks, in order. The first file that exists wins.
///
/// 1. `$XDG_CONFIG_HOME/keryx/config.toml`
/// 2. `~/.config/keryx/config.toml`, on every platform, because that is where
///    people put CLI config even on macOS
/// 3. the platform's own config directory, which differs from the above only
///    on macOS and Windows
pub fn default_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut push = |dir: Option<PathBuf>| {
        if let Some(dir) = dir {
            let path = dir.join("keryx").join("config.toml");
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    };
    push(
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from),
    );
    push(dirs::home_dir().map(|home| home.join(".config")));
    push(dirs::config_dir());
    paths
}

/// The file [`load`] would read: the explicit path, or the first default
/// location that exists.
pub fn resolve_path(explicit: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = explicit {
        if !path.is_file() {
            anyhow::bail!("config file {} does not exist", path.display());
        }
        return Ok(Some(path.to_path_buf()));
    }
    Ok(default_paths().into_iter().find(|path| path.is_file()))
}

/// Where a new config file is created when none exists yet.
pub fn default_write_path() -> Result<PathBuf> {
    default_paths()
        .into_iter()
        .next()
        .context("cannot find a home directory to keep config.toml in")
}

/// Read and validate the config file. Validation is strict: an unknown key,
/// a wrong type or an unknown storage kind is an error naming the file, so a
/// typo can never silently do nothing.
pub fn load(explicit: Option<&Path>) -> Result<Loaded> {
    let Some(path) = resolve_path(explicit)? else {
        return Ok(Loaded::default());
    };
    let config = load_file(&path)?;
    Ok(Loaded {
        config,
        path: Some(path),
    })
}

/// Read and validate one file.
pub fn load_file(path: &Path) -> Result<FileConfig> {
    let source = config::File::from(path).format(config::FileFormat::Toml);
    let mut parsed: FileConfig = config::Config::builder()
        .add_source(source)
        .build()
        .and_then(config::Config::try_deserialize)
        .with_context(|| format!("invalid config file {}", path.display()))?;
    parsed.expand_home();
    Ok(parsed)
}
