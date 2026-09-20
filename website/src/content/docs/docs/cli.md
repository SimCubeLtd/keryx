---
title: "CLI reference"
description: "Commands for publishing, browsing, sharing and managing Keryx documents."
---

Every command below starts with `keryx`. Replace values such as `<draft-id>` with your own; do not type the angle brackets. Run `keryx <command> --help` for the full option list.

## Publish documents

| Command | What it does | Key options |
| --- | --- | --- |
| `upload <file>` | Publish HTML or add a version to an existing draft. | `--draft <id>`, `--new`, `--description <text>` |
| `publish` | Export a stored draft version as an A4 PDF. | `--id <id>` and `--output <file>` required; `--version <n>` optional. |

Revise an existing draft:

```sh
keryx upload ./plan.html --draft <draft-id>
```

Re-uploading the same file path adds a version to the same draft. `--draft` targets a specific draft regardless of the file path; `--new` creates a separate draft. Run uploads from your project directory to record repository, branch, commit and dirty state. Git provenance is metadata, not authorization.

Export a specific version:

```sh
keryx publish --id <draft-id> --version 2 --output ./report-v2.pdf
```

Publication reads stored HTML, adds headers and footers to a render-only copy, and returns the PDF without creating a new draft version or storing a PDF on the server. The CLI refuses to overwrite an existing output file. See [PDF publishing](../pdf/).

## Browse and read

| Command | What it does | Key options |
| --- | --- | --- |
| `list` | List drafts. | `--json`, `--include-snoozed` or `--snoozed` |
| `open <draft-id>` | Open the document in your browser. | No additional options needed. |
| `raw <draft-id>` | Write the exact HTML to stdout. | `--version <n>` or `-v <n>` |
| `tui` | Browse drafts and version history in the terminal. | `--api-url <url>` |

Save the HTML from a specific version:

```sh
keryx raw <draft-id> --version 2 > ./plan-v2.html
```

## Availability and deletion

Snoozing hides a draft from the active list until its wake time. Its links keep working. Disabling stops the draft and its versions from serving. See [availability](../availability/).

| Command | Effect | Key options |
| --- | --- | --- |
| `snooze <draft-id>` | Hide from the active list until its wake time. | `--for 2h` or `--until <timestamp>` |
| `unsnooze <draft-id>` | Return a snoozed draft to the active list. | No additional options needed. |
| `disable <draft-id>` | Stop serving the draft and its versions. | `--reason <text>` |
| `enable <draft-id>` | Serve a disabled draft again. | No additional options needed. |
| `delete <draft-id>` | Stop serving; keep the stored data. | `--yes` skips confirmation. |
| `delete <draft-id> --purge` | **Permanently remove** this draft and its stored files. | `--yes` skips confirmation. No undo. |
| `purge` | **Permanently remove** all already-deleted drafts. | `--yes` skips confirmation. No undo. |

Park a draft for two hours:

```sh
keryx snooze <draft-id> --for 2h
```

## Share through a registry

| Command | What it does | Key options |
| --- | --- | --- |
| `share <draft-id>` | Push a draft version to an OCI registry. | `--to <registry/path>`, `--version <n>` |
| `inspect <reference>` | Read artifact metadata without downloading HTML. | Use an explicit version or digest. |
| `pull <reference>` | Import a shared document or save its HTML. | `--draft <id>` or `--output <file>` |

Share a fixed version:

```sh
keryx share <draft-id> --to ghcr.io/acme/plans --version 3
```

Fetch its HTML without importing it into a Keryx server:

```sh
keryx pull ghcr.io/acme/plans/<draft-id>:v3 --output ./plan.html
```

Registry references require an explicit `:v<n>` tag or `@sha256:` digest. See [registry sharing](../sharing/) for credentials and artifact details.

## Authentication and connection

| Command | What it does | Key options |
| --- | --- | --- |
| `auth set <api-key>` | Verify and store the client API key. | `--api-url <url>` also persists the server URL. |
| `auth clear` | Remove the stored client API key. | No additional options needed. |

Configure a connection to another server:

```sh
keryx auth set <api-key> --api-url https://plans.example.com
```

The API URL resolves in this order: `--api-url` flag, `KERYX_API_URL` environment variable, `client.api_url` in [the configuration file](../configuration/), then `http://localhost:7812`.

## Server and storage

| Command | What it does | Key options |
| --- | --- | --- |
| `serve` | Start the Keryx server. | `--host`, `--port`, `--public-base-url`, `--storage` |
| `storage migrate` | Copy and verify documents between disk and S3. | `--from <store>` and `--to <store>` required; `--dry-run` previews the operation. |
| `storage gc` | Report stored objects that no draft version owns. | `--storage <store>`; `--delete` permanently removes eligible orphans. |

Preview a storage migration with the server stopped:

```sh
keryx storage migrate --from disk --to s3 --dry-run
```

Storage operations use the same database, data-directory and S3 connection options as the server. See [storage and upgrades](../storage/) before migrating or removing objects, and [server configuration](../configuration/) for server options and defaults.

## Terminal keyboard reference

Start the terminal interface with `keryx tui`.

| Key | Action |
| --- | --- |
| `j` / `k` | Move through drafts. |
| `Enter` | View version history. |
| `o` | Open the draft in a browser. |
| `y` | Show the raw HTML URL. |
| `d` | Soft-delete the draft, keeping stored data. |
| `D` | Permanently purge the draft. |
| `r` | Refresh. |
| `q` | Quit. |

Both deletion keys ask for confirmation.
