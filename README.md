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
- **Browse** — a server-rendered dashboard at `/`, a `keryx list` command, and
  a full TUI (`keryx tui`) for browsing, opening, and deleting drafts.
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

With `KERYX_API_KEY` set, uploads, listings, deletes, and PDF publication
require the key as a Bearer token; draft serving stays public. With no key, everything is open —
fine on a trusted LAN. The dashboard at `/` is public either way (it is meant
for your own machine).

Routes: `POST /api/uploads`, `GET/DELETE /api/drafts[/:id]`,
`GET /api/drafts/:id/pdf[?version=n]`
(`DELETE ...?purge=true` for a hard delete), `POST /api/drafts/:id/disable`,
`POST /api/purge`, `GET /d/:id[/raw]`, `GET /d/:id/v/:n[/raw]`,
`GET /healthz`.

## CLI

```sh
keryx upload ./plan.html --description "Q3 migration plan"
keryx list [--json]
keryx raw <draft-id> [-v N]        # exact HTML to stdout
keryx publish --id <draft-id> --output ./report.pdf [--version N]
keryx open <draft-id>              # open in browser
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
`--new` forces a fresh draft, `--draft <id>` targets a specific one. Uploads
also record best-effort git provenance (branch, commit, dirty state, repo)
for display in listings — never for authorization.

`publish` is deliberately Keryx-specific: the endpoint accepts a draft ID and
optional version, never arbitrary HTML. The server resolves that stored HTML,
adds a title/version header and publication-date/page footer to a render-only
copy, and returns the PDF without creating a new Keryx version or writing a PDF
on the server. The CLI refuses to overwrite an existing output file.

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
