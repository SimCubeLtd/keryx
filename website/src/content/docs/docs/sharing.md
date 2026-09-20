---
title: "Registry sharing"
description: "Share immutable Keryx versions through OCI registries."
---

A draft version can be shared as an OCI artifact in any registry that speaks
the distribution spec: GHCR, ECR, Harbor, zot, Artifactory, Docker Hub. The
person receiving it needs no Keryx, no login to a Keryx server and no VPN.
One [`oras`](https://oras.land) command gives them the HTML file:

```sh
$ oras pull ghcr.io/acme/plans/ab12cd34ef56:v3
Downloaded  4f9c1e... q3-migration-plan-v3.html
$ open q3-migration-plan-v3.html
```

Sharing it in the first place:

```sh
keryx share <draft-id> --to ghcr.io/acme/plans [--version N] [--force]
```

`share` fetches the version's exact bytes from your Keryx server and pushes
them from your machine, with your registry credentials. The Keryx server
holds no registry secrets and makes no outbound connection. Each draft gets
its own repository, `<base>/<draft-id>`, with one immutable tag per version:
`:v1`, `:v2`, `:v3`. If the tag already holds this exact artifact, `share`
reports it and does nothing. If it holds something else, `share` refuses
unless you pass `--force`.

```sh
keryx inspect ghcr.io/acme/plans/ab12cd34ef56:v3     # metadata only, no download
keryx pull ghcr.io/acme/plans/ab12cd34ef56:v3        # into your Keryx, as a new draft
keryx pull ghcr.io/acme/plans/ab12cd34ef56:v4 --draft <local-draft-id>
keryx pull ghcr.io/acme/plans/ab12cd34ef56:v3 --output ./plan.html   # no server involved
```

**Explicit versions only.** Keryx never pushes, reads or records `:latest`,
and has no custom tags. `pull` and `inspect` accept a `:v<n>` tag or an
`@sha256:` digest and reject anything else, so a reference always means the
same document.

**A pulled artifact is untrusted HTML.** `pull` recomputes the document's
sha256 against the hash recorded in the artifact and fails hard on a
mismatch. It then runs the same [HTML policy](../html-policy/) as `keryx upload`
against the destination server, which validates again on receipt. A document
that trips the policy is not uploaded, and there is no flag to skip this.

A pulled version replays the original git provenance and records where it
came from, tag plus resolved manifest digest, as its filename:
`ghcr.io/acme/plans/ab12cd34ef56:v3@sha256:...`.

Registry credentials resolve in this order: `KERYX_REGISTRY_TOKEN`, then
`KERYX_REGISTRY_USER` with `KERYX_REGISTRY_PASS`, then Docker credentials
(`~/.docker/config.json` and its helpers, so `docker login ghcr.io` is all
the setup there is), then anonymous. `--plain-http` exists for a local
registry and is off by default.

The artifact is a single `text/html` layer, a JSON config blob of type
`application/vnd.simcube.keryx.draft.config.v1+json`, and the artifact type
`application/vnd.simcube.keryx.draft.v1`. Standard `org.opencontainers.image.*`
annotations carry the title, description, creation time, version, commit and
source; `com.simcube.keryx.*` annotations carry the draft id, version number,
content sha256, git branch and dirty state.
