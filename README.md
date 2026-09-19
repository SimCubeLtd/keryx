# Keryx

**Keryx** (Greek **κῆρυξ**, pronounced **KEH-riks** — /ˈkɛrɪks/) is a
self-hosted publishing service for agents: server, CLI, and TUI in a single
Rust binary. An agent hands Keryx a static HTML document — a plan, proposal,
brief, or report — and Keryx proclaims it at a URL that serves the exact
uploaded bytes to every client: browsers, `curl`, and agent fetch tools alike.

## Why "Keryx"?

In the ancient Greek world the *kēryx* was the herald. Under the protection of
Hermes and carrying the *kerykeion* (the herald's staff, better known by its
Latin name, the caduceus), heralds were inviolable: they crossed battle lines
untouched, convened assemblies, and delivered proclamations between kings,
armies, and gods. Homer's heralds — Talthybius for Agamemnon, Eurybates for
Odysseus — were trusted to carry a message faithfully and repeat it exactly as
given, word for word.

That is precisely the contract this tool makes. Your agents compose the
message; Keryx carries it and repeats it byte for byte — no wrapper pages, no
rewriting, no consent interstitials — to whoever holds the URL.

## What it does

- **Publish** — `keryx upload plan.html` returns a public URL and a raw URL.
  Re-uploading the same file adds a new version; old versions stay
  addressable at `/d/<id>/v/<n>`.
- **Serve** — every draft URL returns the exact uploaded HTML with a strict
  Content-Security-Policy and `X-Keryx-Draft-Id` / `X-Keryx-Draft-Version`
  headers. The CSP never alters the bytes; it only constrains what the page
  may do in a browser (no script execution, no network, no form posts).
- **Ship** — `keryx publish --id <id> --output report.pdf` renders the latest
  immutable version as a paginated A4 PDF. `--version <n>` selects an older
  version. The browser-free Fulgur renderer runs on the server, returns bytes
  without storing a PDF, and the client writes the destination atomically.
- **Browse** — a server-rendered dashboard at `/` organised by availability
  (Active, Snoozed, Disabled) with search, repository filtering, a
  selected-draft pane, downloads, snooze and disable controls, and prune
  controls. Open dashboards update when drafts are published, revised, moved,
  or pruned. The dashboard defaults to the system color scheme and supports
  light and dark overrides. The `keryx list` command and `keryx tui` provide
  terminal interfaces.
- **Snooze** — `keryx snooze <id> --for 2h` parks a draft until a wake time
  without touching its links; `keryx disable <id>` is the only state that
  stops serving. See [Availability](#availability).
- **Notify** — installed as an app over HTTPS, Keryx sends a Web Push
  notification when a plan is published, revised, wakes, or is enabled or
  disabled, even while the dashboard is closed. See
  [Installable app and notifications](#installable-app-and-notifications).
- **Stay small** — one binary, a SQLite index for metadata (default
  `~/.keryx/keryx.db`), and the HTML stored as plain files on disk (default
  `~/.keryx/drafts/<draft-id>/<version-id>.html`) — easy to inspect, grep,
  and back up. No OAuth, and no external database or object storage unless
  you opt into [Postgres](#postgres) or [S3](#storage). A single optional API key covers the private
  bits.

## Build

```sh
cargo build --release   # produces target/release/keryx
```

The repository pins its Rust nightly in `rust-toolchain.toml`. CI runs
`cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo build --all-targets`, `cargo test`, `cargo deny check`, and
`cargo vet --locked` on that same toolchain.

S3 storage and OCI sharing are default Cargo features (`s3`, `share`).
`cargo build --release --no-default-features` gives a lean binary with
neither.

`.cargo/config.toml` refuses crates.io releases younger than 14 days while
resolving dependencies. If `cargo update` declines a version you expected,
wait or pin the previous release.

## Server

```sh
keryx serve
```

| Flag / env | Default | Purpose |
| --- | --- | --- |
| `--port` / `KERYX_PORT` | `7812` | Listen port |
| `--host` / `KERYX_HOST` | `127.0.0.1` | Bind address |
| `--db` / `KERYX_DB` | `~/.keryx/keryx.db` | SQLite path (metadata index). Ignored when a database URL is set |
| `--database-url` / `KERYX_DATABASE_URL` | unset (SQLite) | A `postgres://` URL selects Postgres. See [Postgres](#postgres) |
| `--db-pool-size` / `KERYX_DB_POOL_SIZE` | `1` on SQLite, `4` on Postgres | Database connections in the pool |
| `--no-backup` / `KERYX_NO_BACKUP` | off | Skip the snapshot taken before a database from an older Keryx is first adopted. See [Upgrading](#upgrading) |
| `--data-dir` / `KERYX_DATA_DIR` | `~/.keryx` | Local state: the push identity, the `.staging` write area, and the HTML files (under `drafts/`) when storage is `disk` |
| `--storage` / `KERYX_STORAGE` | `disk` | Where draft HTML lives: `disk` or `s3`. See [Storage](#storage) |
| `--public-base-url` / `KERYX_PUBLIC_BASE_URL` | request Host header | Base for returned links |
| `--api-key` / `KERYX_API_KEY` | unset (open) | Require this Bearer key for mutations, listings, and PDFs |
| `--max-html-bytes` / `KERYX_MAX_HTML_BYTES` | `524288` | Upload size cap |
| `--allow-font-links` / `KERYX_ALLOW_FONT_LINKS` | off | Accept `<link>` to Google Fonts, and widen the served CSP to match |
| `--allow-safe-handlers` / `KERYX_ALLOW_SAFE_HANDLERS` | off | Accept assignment-only inline `on*` handlers |
| `--allow-inline-scripts` / `KERYX_ALLOW_INLINE_SCRIPTS` | off | Serve with `script-src 'unsafe-inline'` so inline scripts actually run |
| `--push-contact` / `KERYX_PUSH_CONTACT` | HTTPS public base URL, else `mailto:keryx@localhost` | VAPID contact push services may use about this server's traffic |

With `KERYX_API_KEY` set, uploads, API listings, deletes, availability
changes, push subscriptions, and PDF publication require the key as a Bearer
token; draft serving stays public. The dashboard at `/` remains public, but
redacts Git provenance, hides every mutation control, and directs management
and PDF work to the authenticated CLI. With no key, everything is open,
including the dashboard controls. This is suitable for a trusted LAN.

Routes: `POST /api/uploads`, `GET/DELETE /api/drafts[/:id]`,
`GET /api/drafts/:id/pdf[?version=n]`
(`DELETE ...?purge=true` for a hard delete),
`PUT /api/drafts/:id/availability`, `POST /api/drafts/:id/disable`
(compatibility adapter over the availability route), `POST /api/purge`,
`GET /api/push/vapid`, `PUT/DELETE /api/push/subscriptions`,
`GET /d/:id[/raw]`, `GET /d/:id/v/:n[/raw]`, `GET /manifest.webmanifest`,
`GET /sw.js`, `GET /healthz`.

## Upgrading

A database written by an older Keryx keeps working in place. The first time
a newer server opens it, Keryx takes a consistent snapshot next to it
(`keryx.db.backup-<timestamp>`, written with `VACUUM INTO`, so nothing still
in the write-ahead log is missed), brings the schema to the current shape,
and records that in a `seaql_migrations` table. The startup banner says what
it did. It happens once; `--no-backup` skips the snapshot.

Nothing is moved or rewritten, and `PRAGMA user_version` is left alone, so
the previous Keryx release can still open the same file if you need to go
back.

## Postgres

SQLite on a local disk is the default and stays that way. Postgres is for
running Keryx with no persistent volume, for example as a small tenant on an
existing cluster that already has backups and point-in-time recovery.

```sh
keryx serve --database-url 'postgres://keryx:<password>@db-rw.internal:5432/keryx?sslmode=verify-full&sslrootcert=/etc/keryx/ca.crt'
```

Prefer `KERYX_DATABASE_URL` from a secret over the flag, which shows up in
the process list. Keryx never prints the URL's credentials or query string:
the banner and errors show only `postgres://host:port/database`.

- **TLS** is set in the URL. `sslmode=verify-full` checks the certificate and
  host name. `sslrootcert` adds a private CA, such as a CloudNativePG cluster
  CA, on top of the built-in roots.
- **Privileges.** The database user needs DDL rights on its own database,
  because Keryx creates and migrates its schema at startup. Two pods starting
  together are safe: the migrator runs under an advisory lock.
- **Connect to the read-write service directly**, not through a PgBouncer
  pooler in transaction mode, which breaks prepared statement caching.
- **Pool.** `--db-pool-size` defaults to 4. Keryx is a small tenant; leave
  the cluster's connection budget for everyone else.
- **Failover.** Pooled connections are checked before use, with connect and
  acquire timeouts, so Keryx heals after a failover without a restart.
- **Probes.** `/healthz` queries the database. Use it for readiness only. As
  a liveness probe it would restart a healthy pod on every Postgres failover.

The schema matches SQLite's: timestamps are `TEXT` in RFC 3339 with
milliseconds, which orders lexically, and only the two flag columns differ
(`BOOLEAN`). There is no tool to copy a SQLite database into Postgres; a
draft is a complete HTML document, so re-upload what you want to keep.

**Still one replica.** Dashboard live updates are a channel inside one
process, the push dispatcher sends without claiming rows, and authentication
is one shared key. Do not run two Keryx servers against one database.

**Disaster recovery is manual, by design.** If the database is lost and the
blobs survive, every object is a complete HTML document and re-uploading
with the client rebuilds the drafts. If the database is rewound past a
purge, the only rows that come back without objects belong to drafts someone
deliberately deleted: they serve a clean not-found, and a second purge
removes them. Afterwards, `keryx storage gc` lists any objects left without
a row.

## Storage

Draft HTML is stored as opaque objects, on local disk by default or in any
S3-compatible store: AWS, RustFS, MinIO, Ceph RGW, Cloudflare R2, Backblaze
B2. The flags below are ignored unless `--storage s3`.

| Flag / env | Default | Purpose |
| --- | --- | --- |
| `--s3-bucket` / `KERYX_S3_BUCKET` | unset | Required when storage is `s3` |
| `--s3-region` / `KERYX_S3_REGION` | `us-east-1` | Region, or the placeholder most S3-compatible endpoints accept |
| `--s3-endpoint` / `KERYX_S3_ENDPOINT` | AWS | Custom endpoint, addressed path-style. Falls back to `AWS_ENDPOINT_URL_S3` |
| `--s3-prefix` / `KERYX_S3_PREFIX` | empty | Key prefix inside the bucket. Never stored in the database, so it can change freely |
| `--s3-profile` / `KERYX_S3_PROFILE` | unset | Named AWS profile for credential lookup |

Credentials are never Keryx flags. They resolve through the standard AWS
chain: environment variables, the shared profile, SSO, `credential_process`,
web identity, ECS, then IMDS.

At startup Keryx writes and deletes one probe object and refuses to boot if
that fails, so a wrong bucket or a read-only credential never surfaces as a
500 on the first upload. The credential needs exactly four permissions on
the prefix: `s3:GetObject`, `s3:PutObject`, `s3:DeleteObject` on
`arn:aws:s3:::<bucket>/<prefix>/*`, and `s3:ListBucket` on the bucket.

**One server per database.** Moving blobs to S3 does not make Keryx
multi-node. With SQLite the index is still local and still the single source
of truth, so two servers pointed at one bucket would mint divergent
histories and purge each other's objects. Run exactly one. Pair S3 with
[Postgres](#postgres) and nothing durable is left on local disk except the
push identity, but it is still one replica.

### Moving between stores

Object keys are identical on every backend, so moving is a verified copy and
never rewrites a database row. Stop the server first.

```sh
keryx storage migrate --from disk --to s3 --dry-run
keryx storage migrate --from disk --to s3
keryx serve --storage s3            # check it, then, optionally:
keryx storage migrate --from disk --to s3 --remove-source
```

Each object is read back from the destination and checked against the sha256
recorded at upload. Objects already present with the recorded size are
skipped, so an interrupted run resumes by re-running it. Any failure exits
non-zero and leaves the source untouched. `--from s3 --to disk` works the
same way. `storage` commands take the same `--db`, `--database-url`,
`--data-dir` and `--s3-*` flags and environment variables as `serve`.

### Orphaned objects

Blob removal is best-effort, and an upload writes its object before its
database row, so a failed delete or a failed upload can leave an object no
version owns. `keryx storage gc [--storage s3]` lists them; `--delete`
removes them. Objects younger than one hour are always left alone, so gc can
never take a version that is about to commit.

## CLI

```sh
keryx upload ./plan.html --description "Q3 migration plan"
keryx list [--json] [--include-snoozed | --snoozed]
keryx raw <draft-id> [-v N]        # exact HTML to stdout
keryx publish --id <draft-id> --output ./report.pdf [--version N]
keryx open <draft-id>              # open in browser
keryx snooze <draft-id> --for 2h   # or --until 2026-08-28T08:00:00Z
keryx unsnooze <draft-id>
keryx disable <draft-id> [--reason "Superseded"]
keryx enable <draft-id>
keryx delete <draft-id> [--yes]    # soft delete: stops serving, keeps data
keryx delete <draft-id> --purge    # hard delete: removes rows and files, no undo
keryx purge [--yes]                # hard-delete everything already soft-deleted
keryx auth set <api-key>           # verified against the server, then stored
keryx auth clear
keryx share <draft-id> --to ghcr.io/acme/plans [--version N]   # see Sharing
keryx inspect ghcr.io/acme/plans/<draft-id>:v3
keryx pull ghcr.io/acme/plans/<draft-id>:v3 [--draft <id> | --output ./plan.html]
keryx storage migrate | gc         # offline, see Storage
```

The API URL resolves as: `--api-url` flag > `KERYX_API_URL` env >
`~/.keryx/config.json` > `http://localhost:7812`. Persist a non-default URL
once with `keryx auth set <key> --api-url http://myhost:7812` (or edit
`~/.keryx/config.json`).

Re-uploading the same file path updates the same draft as a new version;
`--new` forces a fresh draft, `--draft <id>` targets a specific one. Each
upload records best-effort Git provenance from the directory where you run
`keryx upload`. Every version stores its repository, branch, commit, and dirty
state for display and audit. Keryx never uses provenance for authorization.

`publish` is deliberately Keryx-specific: the endpoint accepts a draft ID and
optional version, never arbitrary HTML. The server resolves that stored HTML,
adds a title/version header and publication-date/page footer to a render-only
copy, and returns the PDF without creating a new Keryx version or writing a PDF
on the server. The CLI refuses to overwrite an existing output file.

## Sharing

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
mismatch. It then runs the same [HTML policy](#html-policy) as `keryx upload`
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

## Availability

Every live draft is in exactly one of three states:

| State | Dashboard | Public, raw, versioned, and PDF routes |
| --- | --- | --- |
| Active | Default tab | Serve |
| Snoozed | Snoozed tab until the wake time | Serve |
| Disabled | Disabled tab | 404 |

Snooze affects attention, not access. `keryx snooze <id> --for 45m|2h|3d|1w`
(units combine, e.g. `1h30m`) or `--until <RFC 3339>` hides the draft from
`keryx list` and the Active tab until the wake time; the server stores the
time as UTC with milliseconds and rejects anything not in the future. A draft
wakes by the clock: once `snoozedUntil` has passed it is active again with no
database write and no cleanup job. `keryx unsnooze` wakes it now, `disable`
stops serving (and clears any snooze), and `enable` serves it again. One
mutation owns every transition, so a draft is never both snoozed and disabled.
Uploading a new version never changes availability.

`keryx list` hides snoozed drafts by default; `--include-snoozed` shows every
live draft and `--snoozed` shows only the sleeping ones. `DraftSummary` on the
wire carries `disabled` and an optional `snoozedUntil`; clients derive the
state from those two fields and the current time.

The dashboard opens on Active. `/?draft=<id>&view=snoozed` deep-links to a tab
and draft. The selected pane offers Snooze (with presets or a custom wake
time), Unsnooze, Disable, or Enable; with an API key set those controls are
absent and the authenticated CLI is the management path.

While the dashboard is open, a Server-Sent Events connection reports that its
current view may be stale. The browser then fetches one server-rendered
snapshot and reconciles the rows, counts, selected draft, version history, and
repository filter without reloading the page. The event carries no draft data.
A reconnect immediately receives the latest revision, so changes made while
the connection was down are recovered. Protected deployments use the same
redacted rendering path as the initial dashboard response.

## Installable app and notifications

Keryx serves a web app manifest, a service worker, and icons on every
deployment. What activates is decided by the browser's real origin:

- On an HTTPS origin (for example a Tailscale Serve hostname proxying to the
  local server, or a reverse proxy with a certificate) a supported browser
  offers **Install Keryx** in the top bar and the **Notifications** control
  can subscribe the device to Web Push.
- On plain HTTP the dashboard works as before and those controls stay hidden.

The service worker handles push display and notification clicks only. It
never intercepts requests and keeps no cache, so drafts are always served
live. A notification click accepts only a same-origin path and focuses and
navigates an existing Keryx window, or opens one.

Notification types are **Plan published** (first upload, opens `/d/:id`),
**Plan revised** (later upload, opens the immutable `/d/:id/v/:n`), **Plan
woke** (a snooze expired, opens the draft), **Plan enabled**, and **Plan
disabled** (open the matching dashboard tab). Snoozing and unsnoozing are the
owner's own attention management and produce no event. PDF publication and
download never create an event. Each device chooses which types it receives
from the Notifications control; preferences are stored per subscription.

Delivery is store-first: an event is written in the same SQLite transaction
as the draft change and queued once per opted-in subscription, then a
background dispatcher sends it, retries temporary push-service failures with
doubling delays, and removes subscriptions the service reports as expired.
Subscription endpoints must be public `https` hosts: private, loopback,
link-local, and other reserved addresses are refused when subscribing, again
after DNS resolution on every connection, and the dispatcher never follows
redirects.
A wake is keyed by its snooze timestamp, so it is sent exactly once even if
the server restarts around the wake time; the dispatcher rebuilds its
schedule on startup. The server's VAPID key pair is created on first run at
`<data-dir>/vapid.json` (owner-readable only) and reused thereafter; changing
it invalidates every subscription. Payload encryption and VAPID signing come
from the `web-push-native` crate, never from Keryx itself, and payloads carry
only display text and a same-origin path.

With `KERYX_API_KEY` set the dashboard cannot subscribe (it is read-only), so
push stays unavailable on protected deployments until Keryx has browser
authentication. Denied or unsupported notification permission never blocks
snoozing: the dashboard moves an expired snooze back to Active on its own and
shows an in-page toast while it is open.

## TUI

```sh
keryx tui [--api-url http://myhost:7812]
```

`j`/`k` move · `Enter` version history · `o` open in browser · `y` show raw
URL · `d` soft delete · `D` purge (permanent) · `r` refresh · `q` quit.
Both delete keys ask for confirmation.

## HTML policy

Uploads may contain inline classic `<script>` blocks, and inert data blocks
(`<script type="application/json">` / `application/ld+json`, which no browser
executes). Rejected at upload time: external script sources, module scripts,
`importmap`, inline event handlers (`on*`), `javascript:`/`vbscript:`/`file:`
URLs, `<form>`, `<iframe>`/`<object>`/`<embed>`/`<applet>`, `<base>`, `<link>`,
`srcdoc`, meta-refresh, and unsafe inline CSS. Once stored, drafts are served
verbatim.

Two rules relax per-server, off by default:

- `--allow-font-links` accepts a `<link>` whose `rel` is only
  `stylesheet`/`preconnect`/`dns-prefetch`/`preload` and whose `href` host is
  `fonts.googleapis.com` or `fonts.gstatic.com`. `<base>` and every other host
  stay blocked. The flag also adds `style-src https://fonts.googleapis.com` and
  `font-src https://fonts.gstatic.com` to the CSP on served drafts, without
  which an accepted font link would still be blocked in the browser.
- `--allow-safe-handlers` accepts an inline `on*` handler whose body is nothing
  but `;`-separated assignments of literals or dotted property paths — the
  async-CSS idiom `onload="this.media='all'"`. Anything containing `(`, `[`,
  `<`, a template literal, or a blocked scheme is still rejected, so a permitted
  handler can set properties but cannot call anything.

Accepting a script at upload is not the same as letting it run. Drafts serve with
`script-src 'none'` by default, so an inline `<script>` is stored and returned
byte for byte but never executes in a browser, and neither does an `on*` handler
accepted by `--allow-safe-handlers`. `--allow-inline-scripts` switches the served
CSP to `script-src 'unsafe-inline'`, which covers inline scripts, event handlers
and `javascript:` URLs alike; upload validation is what keeps the last two in
check. `connect-src` stays `'none'` regardless: a draft is a document, not a
client for something else.

Uploading a document with inline scripts to a server that does not have the flag
returns a warning saying so, rather than leaving you to find it in the browser
console.

`keryx upload` reads the server's effective policy from `GET /api/me` before
validating locally, so the CLI never rejects a document the server would accept.

PDF publication supports semantic HTML, paginated tables, inline SVG diagrams,
and base64-embedded PNG, JPEG, and GIF `<img>` elements. A body-level `header`
becomes the cover, and top-level sections begin on new pages. Use
`keryx-page-flow` to keep a section in normal flow, `keryx-page-break` to force
another element onto a new page, and `data-keryx-print="stack"` to flatten a
custom multi-column layout for A4. Script-generated content, `<canvas>`, CSS
imports, external images, and CSS image URLs are rejected for deterministic
publication. An empty `data-keryx-version` element is filled with the selected
version in the render copy.

## Agent flow

Write a complete static HTML file, then:

```sh
keryx upload ./plan.html
```

Hand the printed `Raw HTML` URL to other agents — `curl <url>` returns the
document itself. Repo-local agent workflows live in
[`skills/html-communication/SKILL.md`](skills/html-communication/SKILL.md),
[`skills/keryx-read/SKILL.md`](skills/keryx-read/SKILL.md), and
[`skills/keryx-publish/SKILL.md`](skills/keryx-publish/SKILL.md).


# Attribution

This project is a Rust reimplementation derived from [PostPlan](https://www.npmjs.com/package/postplan/v/0.0.4?activeTab=code).

PostPlan is Copyright (c) 2026 t3dotgg and was distributed under the MIT License. A copy of its original license is available in LICENSES/PostPlan-v0.0.4-MIT.md

All new implementation work is Copyright (c) 2026 SimCube Ltd and is distributed under the MIT License in the root LICENSE file.

This project is independently maintained by SimCube Ltd and is not affiliated with or endorsed by the original PostPlan author: Theo Browne (t3dotgg).
