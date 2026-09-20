---
title: "Installation"
description: "Install the Keryx binary and learn how its agent skills fit into your workflow."
---

## What you install

Keryx has two parts you can install:

| Part | What it does | Where it goes |
| --- | --- | --- |
| The `keryx` executable | Runs the server, CLI and terminal UI. | On the server machine, and on machines where you or your agent use the CLI. |
| Agent skills | Teach an agent the steps for publishing, reading, exporting and retiring documents. | In your agent's user or repository skill directory. |

Skills are Markdown instructions in `SKILL.md` files, not server plugins. The binary works without them, but they make publishing part of an agent's normal workflow.

The release includes four skills: `html-communication` for HTML deliverables, `keryx-read` for reading drafts, `keryx-publish` for PDFs, and `keryx-archive` for explicitly requested retirement. The [skills guide](../skills/) explains each one and where to install it.

## Download a release

Open [Keryx releases on GitHub](https://github.com/SimCubeLtd/keryx/releases) and choose an archive for your platform. The release workflow packages these targets:

| Platform | Target | Archive |
| --- | --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin` | `.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc` | `.zip` |

Extract the archive. It includes the executable in `bin/`, agent skills in `skills/`, the README and license. Put the executable in a directory on your `PATH`, or run it by its full path. On macOS and Linux, preserve its executable permission.

Check that your shell can find it:

```sh
keryx --version
keryx --help
```

If an archive for your platform is unavailable, build from source.

## Build from source

Install Git and [Rust through rustup](https://rustup.rs/), then clone the repository:

```sh
git clone https://github.com/SimCubeLtd/keryx.git
cd keryx
cargo build --release --locked
```

The repository pins its Rust nightly in `rust-toolchain.toml`. The resulting executable is `target/release/keryx`, or `target/release/keryx.exe` on Windows. Copy it to a directory on your `PATH`.

S3 storage and OCI sharing are included by default. A smaller build without those features uses:

```sh
cargo build --release --locked --no-default-features
```

## Set up your publishing workflow

1. [Start a server and publish your first document](../quickstart/) to confirm the binary and connection work.
2. [Install the agent skills](../skills/#install-the-skills) from the release's `skills/` directory or the source checkout.
3. [Configure your main `AGENTS.md` or `CLAUDE.md`](../agents/#set-up-agentsmd-or-claudemd) so your agent knows when to use them.

The agent guide includes a complete, copyable example of the maintainer's instructions for mockups, written deliverables and document styling.
