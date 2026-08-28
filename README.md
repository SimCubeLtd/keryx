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
  controls. The dashboard defaults to the system color scheme and supports
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
  and back up. No external database, no object storage, no OAuth. A single
  optional API key covers the private bits.

## Build

```sh
cargo build --release   # produces target/release/keryx
```

## Server

```sh
keryx serve
```

| Flag / env | Default | Purpose |
| --- | --- | --- |
| `--port` / `KERYX_PORT` | `7812` | Listen port |
| `--host` / `KERYX_HOST` | `127.0.0.1` | Bind address |
| `--db` / `KERYX_DB` | `~/.keryx/keryx.db` | SQLite path (metadata index) |
| `--data-dir` / `KERYX_DATA_DIR` | `~/.keryx` | Root for stored HTML files (written under `drafts/`) |
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
