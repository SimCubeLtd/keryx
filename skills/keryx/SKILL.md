---
name: keryx
description: Create and upload safe static HTML drafts to a self-hosted Keryx server, or read and implement plans supplied as Keryx draft URLs. Use whenever a user provides a Keryx URL or asks to publish a plan, proposal, brief, architecture note, or similar artifact with Keryx.
---

# Keryx

## Read a Keryx URL

When a user supplies a Keryx draft URL (a `/d/<draft-id>` path on their Keryx host), fetch the uploaded HTML immediately with the shell. Do not use web search or a browser to retrieve it.

1. Remove a trailing slash, then append `/raw` unless the URL already ends in `/raw`.
2. Run `curl --fail --silent --show-error --location --max-time 30 --output /tmp/keryx.html '<raw-url>'`.
3. Read `/tmp/keryx.html` as the user's artifact and continue the requested task.

If `curl` fails, report its actual status or network error; do not substitute search results.

## Document Rules

Create one complete static HTML document.

Allowed:

- Semantic HTML.
- Inline CSS or a `<style>` block.
- Normal document metadata such as charset, viewport, and title.
- Links to ordinary HTTPS pages.
- Images from HTTPS or data URLs when necessary.

Do not include:

- `<script src=...>` or module scripts (inline classic `<script>` is tolerated but discouraged).
- Inline event handlers such as `onclick`, `onload`, or `onerror`.
- `javascript:` URLs.
- Forms.
- Iframes, embeds, objects, or applets.
- `<link>` or `<base>` tags.
- Meta refresh redirects.
- Secrets, tokens, private URLs, or local filesystem paths.

## Upload Flow

1. Write the HTML file locally.
2. Run:

   ```sh
   keryx upload <file path>
   ```

   The API URL resolves from `--api-url`, then `KERYX_API_URL`, then `~/.keryx/config.json`, then `http://localhost:7812`.

3. Return the Keryx URL to the user.

The CLI prints both a draft URL and a `Raw HTML` URL. Either works for any client; hand the `Raw HTML` URL to another agent when you want the most explicit form.

If the same local file was uploaded before, the CLI updates the existing draft as a new version. To force a new draft, use:

```sh
keryx upload <file path> --new
```

Keryx stores CLI auth and draft mappings in `~/.keryx`.

## Viewer Behavior

Every Keryx URL serves the exact uploaded HTML, byte for byte, to every client — browsers, curl, and agent fetch tools alike. There is no wrapper page, sandbox, or consent step: fetching a Keryx URL always yields the draft content itself. The `/raw` suffix is an alias that returns the same bytes.
