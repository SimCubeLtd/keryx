//! `keryx share`, `keryx pull` and `keryx inspect`: a draft version as an OCI
//! artifact in any registry. Everything registry-shaped lives in keryx-share;
//! this module fetches from and uploads to a Keryx server around it.
//!
//! Push happens here, on the operator's machine with their own registry
//! credentials. The server keeps no registry secrets and makes no outbound
//! connection.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use serde_json::{json, Value};

use keryx_client::Api;
use keryx_policy::{validate_html, PolicyOptions};
use keryx_share::{
    fetch_meta, parse_pull_reference, pull_artifact, push_artifact, DraftArtifactMeta, PulledDraft,
    RegistryOptions, ShareTarget,
};

#[derive(Args, Debug)]
pub struct ShareArgs {
    /// Draft id
    pub id: String,
    /// Base repository; the draft lands at <base>/<draft-id>:v<n>,
    /// e.g. ghcr.io/simcubeltd/plans
    #[arg(long)]
    pub to: String,
    /// Version to share (default: latest)
    #[arg(long)]
    pub version: Option<i64>,
    /// Overwrite the tag if the registry already holds a different artifact
    #[arg(long)]
    pub force: bool,
    /// Talk plain HTTP to the registry, for a local zot or registry
    #[arg(long)]
    pub plain_http: bool,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct PullArgs {
    /// Reference with an explicit version: <repository>:v<n> or
    /// <repository>@sha256:<digest>
    pub reference: String,
    /// Add the document to this existing draft as a new version
    /// (default: create a new draft)
    #[arg(long, conflicts_with = "output")]
    pub draft: Option<String>,
    /// Write the document to a file instead, touching no Keryx server
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Talk plain HTTP to the registry, for a local zot or registry
    #[arg(long)]
    pub plain_http: bool,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// Reference with an explicit version: <repository>:v<n> or
    /// <repository>@sha256:<digest>
    pub reference: String,
    /// Talk plain HTTP to the registry, for a local zot or registry
    #[arg(long)]
    pub plain_http: bool,
}

pub fn share(args: ShareArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let (draft, version) = api.version(&args.id, args.version)?;
    // The exact stored bytes, as /raw serves them.
    let html = api.raw_html(&args.id, Some(version.version_number))?;

    // Each version carries its own <title>; the draft's is only the latest.
    let title = validate_html(&html, &PolicyOptions::default())
        .title
        .unwrap_or_else(|| draft.title.clone());
    let meta = DraftArtifactMeta {
        draft_id: Some(draft.draft_id.clone()),
        version_number: Some(version.version_number),
        title: Some(title),
        description: draft.description,
        created_at: Some(version.created_at),
        content_sha256: Some(keryx_core::sha256_hex(&html)),
        repo_host: version.repo_host,
        repo_org: version.repo_org,
        repo_name: version.repo_name,
        git_branch: version.git_branch,
        git_commit_sha: version.git_commit_sha,
        git_commit_subject: version.git_commit_subject,
        git_dirty: version.git_dirty,
        keryx_version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    let target = ShareTarget::new(&args.to, &draft.draft_id, version.version_number)?;
    let options = RegistryOptions {
        plain_http: args.plain_http,
    };
    let outcome = push_artifact(&target, &html, &meta, args.force, options)?;

    println!(
        "{}",
        if outcome.already_shared {
            "Already shared"
        } else {
            "Shared draft"
        }
    );
    println!("Reference: {}", outcome.reference);
    println!("Digest: {}", outcome.digest);
    println!("Pull without Keryx: oras pull {}", outcome.reference);
    Ok(())
}

pub fn inspect(args: InspectArgs) -> Result<()> {
    let reference = parse_pull_reference(&args.reference)?;
    let inspected = fetch_meta(
        &reference,
        RegistryOptions {
            plain_http: args.plain_http,
        },
    )?;
    let meta = inspected.meta;
    let show = |label: &str, value: Option<String>| {
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            println!("{label}: {value}");
        }
    };
    println!("Reference: {}", reference.whole());
    println!("Digest: {}", inspected.digest);
    show("Title", meta.title);
    show("Description", meta.description);
    show("Draft ID", meta.draft_id);
    show("Version", meta.version_number.map(|n| n.to_string()));
    show("Created", meta.created_at);
    show("sha256", meta.content_sha256);
    show(
        "Repository",
        match (meta.repo_host, meta.repo_org, meta.repo_name) {
            (Some(host), Some(org), Some(name)) => Some(format!("{host}/{org}/{name}")),
            _ => None,
        },
    );
    show("Branch", meta.git_branch);
    show("Commit", meta.git_commit_sha);
    show("Dirty", meta.git_dirty.map(|dirty| dirty.to_string()));
    show("Keryx", meta.keryx_version);
    Ok(())
}

pub fn pull(args: PullArgs) -> Result<()> {
    let reference = parse_pull_reference(&args.reference)?;
    // Integrity is verified inside pull_artifact before anything is returned.
    let pulled = pull_artifact(
        &reference,
        RegistryOptions {
            plain_http: args.plain_http,
        },
    )?;
    let provenance = provenance(
        &reference.whole(),
        reference.digest().is_some(),
        &pulled.digest,
    );

    if let Some(output) = args.output {
        std::fs::write(&output, &pulled.html)
            .with_context(|| format!("writing {}", output.display()))?;
        println!("Pulled {provenance}");
        println!("Wrote {}", output.display());
        return Ok(());
    }

    let api = Api::from_args(args.api_url.as_deref())?;
    let payload = upload_payload(&pulled, &provenance, args.draft.as_deref(), &api.policy())?;
    let response = api.upload(&payload)?;

    println!("Pulled {provenance}");
    println!("URL: {}", response.public_url);
    println!("Draft ID: {}", response.draft_id);
    println!("Version: {}", response.version_number);
    for warning in &response.warnings {
        eprintln!("Warning: {warning}");
    }
    Ok(())
}

/// What a pulled version records as its upload filename: the reference plus
/// the resolved manifest digest, so it stays exact even if a registry allowed
/// the tag to be overwritten.
fn provenance(reference: &str, has_digest: bool, resolved_digest: &str) -> String {
    if has_digest {
        reference.to_string()
    } else {
        format!("{reference}@{resolved_digest}")
    }
}

/// The upload for a pulled document. An artifact from a registry is untrusted
/// third-party HTML: it passes the same validate_html gate as `keryx upload`,
/// against the destination server's policy, and there is no way to skip it.
/// The server validates again regardless.
fn upload_payload(
    pulled: &PulledDraft,
    provenance: &str,
    draft_id: Option<&str>,
    policy: &PolicyOptions,
) -> Result<Value> {
    let validation = validate_html(&pulled.html, policy);
    if !validation.ok() {
        bail!(
            "pulled HTML failed Keryx validation; nothing was uploaded:\n- {}",
            validation.errors.join("\n- ")
        );
    }
    // Replay the original git provenance; the CLI version is this one's.
    let mut metadata = pulled.meta.upload_metadata();
    metadata.cli_version = Some(env!("CARGO_PKG_VERSION").to_string());
    Ok(json!({
        "html": pulled.html,
        "filename": provenance,
        "draftId": draft_id,
        "description": pulled.meta.description,
        "metadata": metadata,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pulled(html: &str) -> PulledDraft {
        PulledDraft {
            html: html.to_string(),
            meta: DraftArtifactMeta {
                description: Some("Moving the fleet".into()),
                git_branch: Some("main".into()),
                git_commit_sha: Some("9fc6eb1".into()),
                keryx_version: Some("0.4.0".into()),
                ..DraftArtifactMeta::default()
            },
            digest: "sha256:abc".into(),
        }
    }

    #[test]
    fn provenance_carries_the_tag_and_the_resolved_digest() {
        assert_eq!(
            provenance(
                "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3",
                false,
                "sha256:abc"
            ),
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3@sha256:abc"
        );
        // A digest reference is already exact.
        assert_eq!(
            provenance(
                "ghcr.io/simcubeltd/plans/ab12cd34ef56@sha256:abc",
                true,
                "sha256:abc"
            ),
            "ghcr.io/simcubeltd/plans/ab12cd34ef56@sha256:abc"
        );
    }

    #[test]
    fn a_pull_replays_git_provenance_and_records_the_reference_as_the_filename() {
        let payload = upload_payload(
            &pulled("<!doctype html><title>Plan</title><p>ok</p>"),
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3@sha256:abc",
            Some("localdraft01"),
            &PolicyOptions::default(),
        )
        .unwrap();
        assert_eq!(
            payload["filename"],
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3@sha256:abc"
        );
        assert_eq!(payload["draftId"], "localdraft01");
        assert_eq!(payload["description"], "Moving the fleet");
        assert_eq!(payload["metadata"]["gitBranch"], "main");
        assert_eq!(payload["metadata"]["gitCommitSha"], "9fc6eb1");
        assert_eq!(payload["metadata"]["cliVersion"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn a_pull_that_trips_the_policy_uploads_nothing_and_surfaces_the_errors() {
        let hostile =
            "<!doctype html><title>x</title><script src=\"https://evil.test/x.js\"></script>";
        let error = upload_payload(
            &pulled(hostile),
            "r:v1@sha256:abc",
            None,
            &PolicyOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("nothing was uploaded"), "{error}");
        assert!(
            error.lines().count() > 1,
            "validation errors are listed: {error}"
        );
    }
}
