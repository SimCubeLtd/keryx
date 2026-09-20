---
title: "Server configuration"
description: "Configure your client, server, public links and API key."
---

Every setting resolves in the same order:

**command-line flag > environment variable > `config.toml` > built-in default**

The config file is optional and is looked for at
`$XDG_CONFIG_HOME/keryx/config.toml`, then `~/.config/keryx/config.toml`
(on every platform), then the platform's own config directory. `--config
<path>` or `KERYX_CONFIG` names a different file. `keryx <command> --help`
shows the defaults in effect, including those from the file.

```toml
[client]
api_url = "http://plans.internal:7812"
share_to = "ghcr.io/acme/plans"     # default for `keryx share --to`

[server]
host = "0.0.0.0"
port = 7812
data_dir = "~/keryx-data"
public_base_url = "https://plans.example.com"
max_html_bytes = 10485760
allow_font_links = true
# also: api_key, allow_safe_handlers, allow_inline_scripts, push_contact

[database]
path = "~/keryx-data/keryx.db"      # SQLite, or instead:
# url = "postgres://keryx@db-rw.internal:5432/keryx?sslmode=verify-full"
# also: pool_size, no_backup

[storage]
kind = "s3"                          # or "disk"

[storage.s3]
bucket = "keryx-plans"
endpoint = "http://rustfs:9000"
prefix = "prod"
# also: region, profile
```

Keys are named after the flags they stand in for, and each is documented
with its flag in the tables below. The file is validated before any command
runs: an unknown key, a wrong type or an unknown storage kind stops Keryx
with the file and the entry named, so a typo can never silently do nothing.
A leading `~/` in a path is your home directory.

`server.api_key` and a `database.url` with a password are secrets. If you
put them in the file, keep it readable by you alone (`chmod 600`); a file
Keryx creates is made that way. `--help` never prints them. S3 and registry
credentials never go in this file: they resolve through the AWS chain and
Docker credentials. The client's API key is not config either and stays in
`~/.keryx/credentials.json`, written by `keryx auth set`.

A boolean switched on in the file has no flag to switch it off; override it
with the environment variable, for example `KERYX_ALLOW_FONT_LINKS=false`.

Older clients kept the API URL in `~/.keryx/config.json`. The first command
that finds that file moves the URL into `config.toml` and deletes it.

## Server

```sh
keryx serve
```

| Flag / env | Default | Purpose |
| --- | --- | --- |
| `--port` / `KERYX_PORT` | `7812` | Listen port |
| `--host` / `KERYX_HOST` | `127.0.0.1` | Bind address |
| `--db` / `KERYX_DB` | `~/.keryx/keryx.db` | SQLite path (metadata index). Ignored when a database URL is set |
| `--database-url` / `KERYX_DATABASE_URL` | unset (SQLite) | A `postgres://` URL selects Postgres. See [Postgres](../storage/#postgres) |
| `--db-pool-size` / `KERYX_DB_POOL_SIZE` | `1` on SQLite, `4` on Postgres | Database connections in the pool |
| `--no-backup` / `KERYX_NO_BACKUP` | off | Skip the snapshot taken before a database from an older Keryx is first adopted. See [Upgrading](../storage/#upgrading) |
| `--data-dir` / `KERYX_DATA_DIR` | `~/.keryx` | Local state: the push identity, the `.staging` write area, and the HTML files (under `drafts/`) when storage is `disk` |
| `--storage` / `KERYX_STORAGE` | `disk` | Where draft HTML lives: `disk` or `s3`. See [Storage](../storage/) |
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

## Share through a reachable address

The default bind address is loopback. Run behind an HTTPS reverse proxy or a private-network proxy for access from other machines. Set `KERYX_PUBLIC_BASE_URL` to the URL readers should use. Bind to another interface only when your network setup requires it.

Configure an API key before exposing management operations beyond a trusted network. This protects mutations and listings; it does not make draft URLs private. Anyone who can reach the server and has a draft URL can read it.
