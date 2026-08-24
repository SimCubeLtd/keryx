---
name: keryx-publish
description: Publish an immutable Keryx draft version as a shippable PDF. Use when the user asks to export, publish, download, or produce a PDF from a Keryx draft or draft URL. Do not use for arbitrary HTML-to-PDF conversion.
---

# Keryx Publish

Produce a PDF only through Keryx's versioned publication workflow. Do not send
arbitrary HTML to the server, call the PDF endpoint directly, or use a browser,
Chromium, web search, or a separate PDF converter.

## Resolve the source

1. Extract the draft ID from the segment after `/d/` when the user gives a
   Keryx URL. Ignore a trailing `/raw` segment. Before using the ID in a path or
   command, require exactly 12 ASCII lowercase letters or digits
   (`[a-z0-9]{12}`). Stop if it does not match.
2. Use the version from `/v/<number>` or an explicit user request when present.
3. Otherwise, run `keryx list --json` and read `latestVersionNumber` for the
   matching `draftId`. Pass that resolved version explicitly to publication so
   the source cannot move between resolution and rendering. If the draft is
   missing or has no version, report that and stop.

## Choose the output

Use the path the user names. When none is given, create `/tmp/keryx` and use
`/tmp/keryx/<draft-id>.v<version>.pdf`. Never delete or replace an existing
file to make publication succeed; Keryx deliberately refuses to clobber it.

## Publish

Run:

```sh
keryx publish --id '<draft-id>' --version '<version>' --output '<absolute-path.pdf>'
```

Quote the validated draft ID and complete output path in every shell command.

The client requests bytes for that stored version and writes the file locally.
The server does not retain the PDF or create another draft version.

After success, confirm the file exists, is non-empty, and begins with `%PDF-`.
Report the absolute output path, draft ID, resolved version, page count, and
immutable source URL printed by the command.

Use Keryx's default local API at `http://localhost:7812`. Do not configure
authentication or pass an API URL override. If publication fails because the
stored HTML is not PDF-compatible, report Keryx's actual error. Do not revise
or upload the draft unless the user also asks for that change.
