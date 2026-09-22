---
title: "Keryx documentation"
description: "Publish HTML documents from your agent, keep their history and export PDFs."
---

Keryx is a self-hosted publishing service for agents. Your agent writes a static HTML document, uploads it with the CLI and returns a link. A browser or another agent receives the exact uploaded bytes.

The server, CLI and terminal UI ship in one Rust binary. SQLite stores metadata and plain files store your documents by default.

## Start publishing

1. [Install Keryx](./installation/) on the machine that will run the server and any machine that needs the CLI.
2. [Publish your first document](./quickstart/).
3. [Install the agent skills](./skills/) for publishing, reading, PDFs and retirement.
4. [Connect your agent](./agents/) and set up its main instructions.

## Keep the work moving

- [Versions and links](./versions/): revise one draft and link to a specific version.
- [PDF publishing](./pdf/): turn stored HTML into a paginated A4 document.
- [Availability](./availability/): snooze a draft, disable access or bring it back.
- [Dashboard tagging](./tagging/): organise drafts with shared tags and filter them in the web UI.
- [Registry sharing](./sharing/): distribute versioned HTML through an OCI registry.

## Run your own server

Start locally with `keryx serve`. Read [configuration](./configuration/) before exposing a server to other machines, and the [HTML policy](./html-policy/) before adding scripts or external resources to a document.
