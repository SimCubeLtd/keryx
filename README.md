# Keryx

Keryx is a self-hosted publishing service for agents. Upload a static HTML
document, and Keryx serves its exact bytes to browsers and agents. One Rust
binary includes the server, CLI and terminal UI. SQLite and local files are
the defaults.

[Documentation](https://simcubeltd.github.io/keryx/) ·
[Latest release](https://github.com/SimCubeLtd/keryx/releases/latest)

## Get started

[Install Keryx](https://simcubeltd.github.io/keryx/docs/installation/), then
start the server and upload an HTML file from another terminal:

```sh
keryx serve
keryx upload ./plan.html
```

The [first document guide](https://simcubeltd.github.io/keryx/docs/quickstart/)
walks through creating the file, revising it and sharing it beyond your machine.

## What it does

- [Versions and links](https://simcubeltd.github.io/keryx/docs/versions/):
  revise a draft while keeping earlier versions available.
- [PDF publishing](https://simcubeltd.github.io/keryx/docs/pdf/): export a
  stored version as a paginated PDF.
- [Dashboard tagging](https://simcubeltd.github.io/keryx/docs/tagging/):
  organise and filter drafts in the web UI.
- [Availability and notifications](https://simcubeltd.github.io/keryx/docs/availability/):
  snooze drafts, disable access and receive browser notifications.
- [Registry sharing](https://simcubeltd.github.io/keryx/docs/sharing/):
  distribute immutable versions through an OCI registry.

The [CLI reference](https://simcubeltd.github.io/keryx/docs/cli/) covers
commands, options and terminal UI keys.

## Run Keryx

- [Server configuration](https://simcubeltd.github.io/keryx/docs/configuration/)
  covers the API key, public URLs and server settings.
- [Storage and upgrades](https://simcubeltd.github.io/keryx/docs/storage/)
  covers SQLite, Postgres, disk, S3, migrations and backups.
- [HTML policy](https://simcubeltd.github.io/keryx/docs/html-policy/)
  explains what uploads may contain and what browsers may run.

Draft URLs remain readable by anyone who can reach the server. An API key
protects management operations, not access to published documents. Read the
[configuration guide](https://simcubeltd.github.io/keryx/docs/configuration/)
before exposing a server.

## Connect an agent

The [agent skills guide](https://simcubeltd.github.io/keryx/docs/skills/)
covers publishing, reading, exporting and retiring documents. The
[agent setup guide](https://simcubeltd.github.io/keryx/docs/agents/) shows
how to configure a connection and give your agent instructions for using
those skills.

## Development

The repository pins its Rust toolchain in `rust-toolchain.toml`. From the repository root:

```sh
cargo build --locked
cargo test --locked
```

See [`supply-chain/README.md`](supply-chain/README.md) for dependency review
and update rules.

## Why Keryx?

The ancient Greek `kēryx` was a herald trusted to carry a message faithfully.
Keryx follows that idea by serving uploaded HTML bytes unchanged.

## Attribution

Keryx was inspired by the concept behind
[PostPlan](https://www.npmjs.com/package/postplan/v/0.0.4?activeTab=code):
publishing agent-created documents at shareable URLs. Keryx's implementation
is original code developed by SimCube Ltd.

PostPlan is Copyright (c) 2026 t3dotgg and was distributed under the MIT
License. A copy of its original license is available in
[LICENSES/PostPlan-v0.0.4-MIT.md](LICENSES/PostPlan-v0.0.4-MIT.md).

Keryx is Copyright (c) 2026 SimCube Ltd and is distributed under the MIT
License in the root [LICENSE](LICENSE) file.

Keryx is independently maintained by SimCube Ltd and is not affiliated with
or endorsed by PostPlan or its author, Theo Browne (t3dotgg).
