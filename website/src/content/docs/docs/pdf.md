---
title: "Publish a PDF"
description: "Export a stored Keryx version as a paginated A4 PDF."
---

`keryx publish` converts a stored draft version to PDF. It takes a draft ID, not a path to arbitrary HTML.

```sh
keryx publish --id <draft-id> --output ./report.pdf
```

Without `--version`, Keryx uses the latest version. Pin a version for a fixed deliverable:

```sh
keryx publish --id <draft-id> --version 2 --output ./report-v2.pdf
```

The server renders the PDF and returns its bytes without storing a PDF or creating a new document version. The CLI writes the result atomically and refuses to overwrite an existing output file.

## Structure the HTML for print

- Use one body-level `header` for the cover.
- Put the document's main sections in top-level `section` elements. Each begins on a new page.
- Add `class="keryx-page-flow"` to a section that should continue in normal flow.
- Use `class="keryx-page-break"` to start another element on a new page.
- Add `data-keryx-print="stack"` to a custom multi-column layout that should become one column on A4.

Keryx adds a title/version header and publication-date/page footer to a render-only copy. An empty element with `data-keryx-version` is filled with the selected version in that copy. The original HTML stays unchanged.

## Supported content

Semantic HTML, paginated tables, inline SVG diagrams and base64-embedded PNG, JPEG and GIF images are supported. Script-generated content, canvas, CSS imports, external images and CSS image URLs are rejected for deterministic output.

A document that works in a browser may need changes for PDF publication. Keep reading order meaningful and embed images rather than depending on remote resources.
