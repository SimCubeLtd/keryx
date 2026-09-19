//! Registry coordinates. Keryx never pushes, reads or records a `:latest` tag
//! and offers no custom tag names: a reference is only useful as provenance if
//! it always returns the same version.

use anyhow::{bail, Context, Result};
use oci_client::Reference;

/// Where `keryx share` pushes one version: `<base>/<draft-id>:v<n>`, one
/// repository per draft so its history is browsable in any registry UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareTarget {
    reference: Reference,
}

impl ShareTarget {
    pub fn new(base_repository: &str, draft_id: &str, version_number: i64) -> Result<Self> {
        let base = base_repository.trim().trim_end_matches('/');
        if base.is_empty() {
            bail!("the base repository is empty");
        }
        if base.contains('@')
            || base
                .rsplit('/')
                .next()
                .is_some_and(|last| last.contains(':'))
        {
            bail!("the base repository {base:?} must not carry a tag or digest");
        }
        let reference: Reference = format!("{base}/{draft_id}:v{version_number}")
            .parse()
            .with_context(|| format!("{base:?} is not a valid registry repository"))?;
        Ok(Self { reference })
    }

    pub(crate) fn reference(&self) -> Reference {
        self.reference.clone()
    }

    /// `registry/repository:v<n>`.
    pub fn display(&self) -> String {
        self.reference.whole()
    }
}

/// Parse a reference for `pull` or `inspect`: an explicit `:v<n>` tag or an
/// `@sha256:` digest, nothing else.
///
/// The check runs after parsing, because `Reference::parse` silently rewrites
/// a bare reference to `:latest`, so a bare reference and an explicit
/// `:latest` look identical.
pub fn parse_pull_reference(input: &str) -> Result<Reference> {
    let reference: Reference = input
        .trim()
        .parse()
        .with_context(|| format!("{input:?} is not a valid OCI reference"))?;
    if reference.digest().is_some() {
        return Ok(reference);
    }
    let versioned = reference.tag().is_some_and(|tag| {
        tag.strip_prefix('v')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    });
    if !versioned {
        bail!(
            "{input:?} must name an explicit version: a :v<n> tag or an @sha256: digest. \
             Keryx never reads :latest or any other moving tag."
        );
    }
    Ok(reference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_target_is_one_repository_per_draft_with_exactly_a_version_tag() {
        let target = ShareTarget::new("ghcr.io/simcubeltd/plans/", "ab12cd34ef56", 3).unwrap();
        assert_eq!(target.display(), "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3");
        assert_eq!(target.reference().tag(), Some("v3"));
        assert_eq!(
            target.reference().repository(),
            "simcubeltd/plans/ab12cd34ef56"
        );

        let local = ShareTarget::new("localhost:5000/plans", "ab12cd34ef56", 1).unwrap();
        assert_eq!(local.display(), "localhost:5000/plans/ab12cd34ef56:v1");

        assert!(ShareTarget::new("ghcr.io/simcubeltd/plans:latest", "ab12cd34ef56", 1).is_err());
        assert!(ShareTarget::new("ghcr.io/plans@sha256:abc", "ab12cd34ef56", 1).is_err());
        assert!(ShareTarget::new("", "ab12cd34ef56", 1).is_err());
    }

    #[test]
    fn pull_accepts_only_a_version_tag_or_a_digest() {
        let digest = "sha256:".to_string() + &"a".repeat(64);
        for accepted in [
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3".to_string(),
            "localhost:5000/plans/ab12cd34ef56:v12".to_string(),
            format!("ghcr.io/simcubeltd/plans/ab12cd34ef56@{digest}"),
            format!("ghcr.io/simcubeltd/plans/ab12cd34ef56:v3@{digest}"),
        ] {
            assert!(parse_pull_reference(&accepted).is_ok(), "{accepted}");
        }
        for rejected in [
            "ghcr.io/simcubeltd/plans/ab12cd34ef56",
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:latest",
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v",
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:v3-rc1",
            "ghcr.io/simcubeltd/plans/ab12cd34ef56:stable",
        ] {
            let error = parse_pull_reference(rejected).unwrap_err().to_string();
            assert!(error.contains("explicit version"), "{rejected}: {error}");
        }
    }
}
