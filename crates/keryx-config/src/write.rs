//! Updating the config file in place. Keryx writes one key, `client.api_url`
//! (from `keryx auth set --api-url`), and must not disturb the rest of a
//! file the user wrote by hand, comments included.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::{value, DocumentMut, Item, Table};

/// Set `client.api_url` in `path`, or in the file Keryx already reads, or in
/// a new file at the default location. Answers the file written.
pub fn set_client_api_url(path: Option<&Path>, api_url: &str) -> Result<PathBuf> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => match crate::resolve_path(None)? {
            Some(existing) => existing,
            None => crate::default_write_path()?,
        },
    };
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    let created = existing.is_empty();
    let mut document: DocumentMut = existing
        .parse()
        .with_context(|| format!("{} is not valid TOML", path.display()))?;

    let client = document
        .entry("client")
        .or_insert_with(|| Item::Table(Table::new()));
    let client = client
        .as_table_like_mut()
        .with_context(|| format!("`client` in {} is not a table", path.display()))?;
    // Replace the value in place when the key exists: inserting afresh would
    // drop a comment the user wrote above it.
    match client.get_mut("api_url") {
        Some(existing) => *existing = value(api_url),
        None => {
            client.insert("api_url", value(api_url));
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, document.to_string())
        .with_context(|| format!("writing {}", path.display()))?;
    // The file may come to hold a server API key or a database password.
    #[cfg(unix)]
    if created {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = created;
    Ok(path)
}
