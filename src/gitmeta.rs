//! Best-effort git provenance for uploads. All values are self-reported
//! metadata for display and audit only — never used for authorization.

use std::path::Path;
use std::process::Command;

use crate::types::UploadMetadata;

pub fn collect(cwd: &Path) -> UploadMetadata {
    let repo_root = git(&["rev-parse", "--show-toplevel"], cwd);
    let remote = git(&["config", "--get", "remote.origin.url"], cwd);
    let parsed = remote.as_deref().map(parse_remote).unwrap_or_default();
    let status = git(&["status", "--porcelain"], cwd);

    UploadMetadata {
        repo_org: parsed
            .org
            .or_else(|| infer_org_from_root(repo_root.as_deref())),
        repo_name: parsed.name.or_else(|| {
            repo_root
                .as_deref()
                .and_then(|root| Path::new(root).file_name())
                .map(|name| name.to_string_lossy().to_string())
        }),
        repo_host: parsed.host,
        git_branch: git(&["rev-parse", "--abbrev-ref", "HEAD"], cwd),
        git_commit_sha: git(&["rev-parse", "HEAD"], cwd),
        git_commit_subject: git(&["log", "-1", "--format=%s"], cwd),
        // None when not a git repo; Some(bool) when a working tree is present.
        git_dirty: status.map(|s| !s.is_empty()),
        cli_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    }
}

fn git(args: &[&str], cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(text)
}

#[derive(Default)]
struct ParsedRemote {
    host: Option<String>,
    org: Option<String>,
    name: Option<String>,
}

fn parse_remote(remote: &str) -> ParsedRemote {
    let cleaned = remote.trim().trim_end_matches(".git");
    if cleaned.is_empty() {
        return ParsedRemote::default();
    }

    // SSH form: user@host:org/name
    if let Some((user_host, path)) = cleaned.split_once(':') {
        if user_host.contains('@') && !path.starts_with("//") {
            let host = user_host.split('@').next_back().map(str::to_string);
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            if parts.len() >= 2 {
                return ParsedRemote {
                    host,
                    org: Some(parts[0].to_string()),
                    name: parts.last().map(|s| s.to_string()),
                };
            }
        }
    }

    // URL form: https://host/org/name
    if let Ok(url) = url::Url::parse(cleaned) {
        let parts: Vec<&str> = url.path().split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            return ParsedRemote {
                host: url.host_str().map(str::to_string),
                org: Some(parts[0].to_string()),
                name: parts.last().map(|s| s.to_string()),
            };
        }
    }

    // Plain path fallback: .../org/name
    let parts: Vec<&str> = cleaned.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        return ParsedRemote {
            host: None,
            org: Some(parts[parts.len() - 2].to_string()),
            name: parts.last().map(|s| s.to_string()),
        };
    }

    ParsedRemote::default()
}

fn infer_org_from_root(repo_root: Option<&str>) -> Option<String> {
    let root = Path::new(repo_root?);
    root.parent()?
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_remote() {
        let parsed = parse_remote("git@github.com:acme/widgets.git");
        assert_eq!(parsed.host.as_deref(), Some("github.com"));
        assert_eq!(parsed.org.as_deref(), Some("acme"));
        assert_eq!(parsed.name.as_deref(), Some("widgets"));
    }

    #[test]
    fn parses_https_remote() {
        let parsed = parse_remote("https://gitlab.com/acme/tools/widgets.git");
        assert_eq!(parsed.host.as_deref(), Some("gitlab.com"));
        assert_eq!(parsed.org.as_deref(), Some("acme"));
        assert_eq!(parsed.name.as_deref(), Some("widgets"));
    }
}
