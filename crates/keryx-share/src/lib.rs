//! Share a draft version as a single-layer OCI artifact in any registry that
//! speaks the distribution spec. The design target is interop: someone with no
//! Keryx at all gets a usable HTML file from one `oras pull`.
//!
//! This is the ONLY crate that depends on `oci-client` and `docker_credential`.
//! The public surface is synchronous; network calls run on a short-lived
//! current-thread runtime (see `block_on`) so the CLI never touches async.

mod meta;
mod reference;

use std::future::Future;

use anyhow::{anyhow, bail, Context, Result};
use oci_client::client::{ClientConfig, ClientProtocol, Config as OciConfig, ImageLayer};
use oci_client::errors::{OciDistributionError, OciErrorCode};
use oci_client::manifest::OciImageManifest;
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};

pub use meta::{artifact_filename, DraftArtifactMeta};
pub use reference::{parse_pull_reference, ShareTarget};

/// The layer is a real media type, not a vendor one: the exact stored bytes,
/// the same as `/raw` serves, so generic tooling knows what it pulled.
pub const LAYER_MEDIA_TYPE: &str = "text/html";
/// Media type of the self-describing JSON config blob.
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.simcube.keryx.draft.config.v1+json";
/// Lets `oras discover` and registry UIs identify a Keryx plan.
pub const ARTIFACT_TYPE: &str = "application/vnd.simcube.keryx.draft.v1";

#[derive(Debug, Clone, Copy, Default)]
pub struct RegistryOptions {
    /// Talk plain HTTP, for a local zot or registry. Off by default.
    pub plain_http: bool,
}

#[derive(Debug)]
pub struct PushOutcome {
    /// `registry/repository:v<n>`.
    pub reference: String,
    /// Manifest digest now at that tag.
    pub digest: String,
    /// The identical artifact was already there; nothing was pushed.
    pub already_shared: bool,
}

#[derive(Debug)]
pub struct InspectedDraft {
    pub meta: DraftArtifactMeta,
    /// The resolved manifest digest, exact even if the tag is later moved.
    pub digest: String,
}

#[derive(Debug)]
pub struct PulledDraft {
    pub html: String,
    pub meta: DraftArtifactMeta,
    pub digest: String,
}

/// The manifest for one draft version: a `text/html` layer named for ORAS, the
/// config blob, and the annotations.
fn assemble(
    html: &str,
    meta: &DraftArtifactMeta,
) -> (Vec<ImageLayer>, OciConfig, OciImageManifest) {
    let layers = vec![ImageLayer::new(
        html.as_bytes().to_vec(),
        LAYER_MEDIA_TYPE.to_string(),
        Some(meta.layer_annotations()),
    )];
    let config = OciConfig::new(meta.to_config_blob(), CONFIG_MEDIA_TYPE.to_string(), None);
    let mut manifest = OciImageManifest::build(&layers, &config, Some(meta.to_annotations()));
    manifest.artifact_type = Some(ARTIFACT_TYPE.to_string());
    (layers, config, manifest)
}

/// Push one version under its immutable `:v<n>` tag, and nothing else.
///
/// Keryx versions are immutable, so the tag must be too, and most registries
/// happily overwrite one. If the tag already holds this exact artifact the
/// push is a no-op; if it holds anything else it is refused unless `force`.
pub fn push_artifact(
    target: &ShareTarget,
    html: &str,
    meta: &DraftArtifactMeta,
    force: bool,
    options: RegistryOptions,
) -> Result<PushOutcome> {
    let client = build_client(options);
    let reference = target.reference();
    let auth = resolve_auth(reference.resolve_registry());
    let (layers, config, manifest) = assemble(html, meta);

    block_on(async {
        match client.pull_image_manifest(&reference, &auth).await {
            Ok((existing, digest)) => {
                if same_manifest(&existing, &manifest) {
                    return Ok(PushOutcome {
                        reference: target.display(),
                        digest,
                        already_shared: true,
                    });
                }
                if !force {
                    bail!(
                        "{} already holds a different artifact ({digest}). Versions are \
                         immutable; pass --force to overwrite the tag.",
                        target.display()
                    );
                }
            }
            Err(error) if is_not_found(&error) => {}
            Err(error) => return Err(registry_error("checking", &target.display(), error)),
        }

        client
            .push(&reference, &layers, config, &auth, Some(manifest))
            .await
            .map_err(|error| registry_error("pushing", &target.display(), error))?;
        let digest = client
            .fetch_manifest_digest(&reference, &auth)
            .await
            .map_err(|error| registry_error("resolving", &target.display(), error))?;
        Ok(PushOutcome {
            reference: target.display(),
            digest,
            already_shared: false,
        })
    })?
}

/// Read what a reference contains from its manifest and config blob alone:
/// one cheap request pair and no document download.
pub fn fetch_meta(reference: &Reference, options: RegistryOptions) -> Result<InspectedDraft> {
    let client = build_client(options);
    let auth = resolve_auth(reference.resolve_registry());
    let (manifest, digest, config) =
        block_on(async { client.pull_manifest_and_config(reference, &auth).await })?
            .map_err(|error| registry_error("inspecting", &reference.whole(), error))?;
    Ok(InspectedDraft {
        meta: DraftArtifactMeta::from_artifact(&manifest, &config),
        digest,
    })
}

/// Pull the document and verify it. A sha256 mismatch against the artifact's
/// recorded content hash is a hard error: corruption or tampering.
pub fn pull_artifact(reference: &Reference, options: RegistryOptions) -> Result<PulledDraft> {
    let client = build_client(options);
    let auth = resolve_auth(reference.resolve_registry());
    let image = block_on(async { client.pull(reference, &auth, vec![LAYER_MEDIA_TYPE]).await })?
        .map_err(|error| registry_error("pulling", &reference.whole(), error))?;

    let manifest = image
        .manifest
        .as_ref()
        .context("the registry returned no image manifest")?;
    let config = String::from_utf8_lossy(&image.config.data).into_owned();
    let meta = DraftArtifactMeta::from_artifact(manifest, &config);
    let digest = image
        .digest
        .clone()
        .context("the registry returned no manifest digest")?;
    let layer = image
        .layers
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("pulled artifact has no layers"))?;
    let html = verified_html(layer.data.to_vec(), &meta)?;
    Ok(PulledDraft { html, meta, digest })
}

fn verified_html(bytes: Vec<u8>, meta: &DraftArtifactMeta) -> Result<String> {
    let expected = meta
        .content_sha256
        .as_deref()
        .context("artifact carries no com.simcube.keryx.content-sha256; not a Keryx draft")?;
    let html = String::from_utf8(bytes).context("pulled document is not valid UTF-8")?;
    let actual = keryx_core::sha256_hex(&html);
    if actual != expected {
        bail!("integrity check failed: the artifact records sha256 {expected}, the pulled document hashes to {actual}");
    }
    Ok(html)
}

/// Equal manifests serialise to equal canonical JSON, hence equal digests.
fn same_manifest(a: &OciImageManifest, b: &OciImageManifest) -> bool {
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn is_not_found(error: &OciDistributionError) -> bool {
    match error {
        OciDistributionError::ImageManifestNotFoundError(_) => true,
        OciDistributionError::ServerError { code: 404, .. } => true,
        OciDistributionError::RegistryError { envelope, .. } => envelope.errors.iter().any(|e| {
            matches!(
                e.code,
                OciErrorCode::ManifestUnknown | OciErrorCode::NameUnknown | OciErrorCode::NotFound
            )
        }),
        _ => false,
    }
}

/// oci-client errors carry URLs and registry messages, never credentials.
fn registry_error(action: &str, reference: &str, error: OciDistributionError) -> anyhow::Error {
    anyhow!("{action} {reference}: {error}")
}

/// Run a future to completion on a short-lived current-thread runtime, keeping
/// async confined to this crate.
fn block_on<F: Future>(future: F) -> Result<F::Output> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting async runtime")?;
    Ok(runtime.block_on(future))
}

fn build_client(options: RegistryOptions) -> Client {
    Client::new(ClientConfig {
        protocol: if options.plain_http {
            ClientProtocol::Http
        } else {
            ClientProtocol::Https
        },
        ..Default::default()
    })
}

/// Resolution order: `KERYX_REGISTRY_TOKEN`, then `KERYX_REGISTRY_USER` plus
/// `KERYX_REGISTRY_PASS`, then docker credentials (`~/.docker/config.json` and
/// its helpers), then anonymous. Never read from Keryx's own config file.
fn resolve_auth(registry: &str) -> RegistryAuth {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    if let Some(token) = env("KERYX_REGISTRY_TOKEN") {
        return RegistryAuth::Bearer(token);
    }
    if let (Some(user), Some(pass)) = (env("KERYX_REGISTRY_USER"), env("KERYX_REGISTRY_PASS")) {
        return RegistryAuth::Basic(user, pass);
    }
    match docker_credential::get_credential(registry) {
        Ok(docker_credential::DockerCredential::UsernamePassword(user, pass)) => {
            RegistryAuth::Basic(user, pass)
        }
        Ok(docker_credential::DockerCredential::IdentityToken(token)) => {
            RegistryAuth::Bearer(token)
        }
        Err(_) => RegistryAuth::Anonymous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> DraftArtifactMeta {
        DraftArtifactMeta {
            draft_id: Some("ab12cd34ef56".into()),
            version_number: Some(3),
            title: Some("Q3 Migration Plan".into()),
            content_sha256: Some(keryx_core::sha256_hex("<p>plan</p>")),
            ..DraftArtifactMeta::default()
        }
    }

    /// Proves oci-client compiles and links with no TLS feature of its own,
    /// on the ring provider the binary installs.
    #[test]
    fn oci_client_builds_on_the_ring_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = Client::try_from(ClientConfig::default());
        assert!(
            client.is_ok(),
            "oci-client failed to build its HTTPS client"
        );
    }

    #[test]
    fn manifest_is_an_html_layer_named_for_oras_with_an_artifact_type() {
        let (layers, config, manifest) = assemble("<p>plan</p>", &meta());

        assert_eq!(manifest.artifact_type.as_deref(), Some(ARTIFACT_TYPE));
        assert_eq!(manifest.config.media_type, CONFIG_MEDIA_TYPE);
        assert_eq!(config.media_type, CONFIG_MEDIA_TYPE);
        assert_eq!(manifest.layers.len(), 1);
        assert_eq!(manifest.layers[0].media_type, "text/html");
        assert_eq!(
            manifest.layers[0].annotations.as_ref().unwrap()["org.opencontainers.image.title"],
            "q3-migration-plan-v3.html"
        );
        // The layer is the exact stored bytes.
        assert_eq!(&layers[0].data[..], b"<p>plan</p>");
        assert!(same_manifest(
            &manifest,
            &assemble("<p>plan</p>", &meta()).2
        ));
        assert!(!same_manifest(
            &manifest,
            &assemble("<p>other</p>", &meta()).2
        ));
    }

    #[test]
    fn a_sha256_mismatch_on_pull_is_a_hard_error() {
        assert_eq!(
            verified_html(b"<p>plan</p>".to_vec(), &meta()).unwrap(),
            "<p>plan</p>"
        );

        let error = verified_html(b"<p>tampered</p>".to_vec(), &meta()).unwrap_err();
        assert!(error.to_string().contains("integrity check failed"));

        let unverifiable = DraftArtifactMeta {
            content_sha256: None,
            ..meta()
        };
        assert!(verified_html(b"<p>plan</p>".to_vec(), &unverifiable).is_err());
    }
}
