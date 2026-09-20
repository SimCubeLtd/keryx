---
title: "HTML policy"
description: "Understand accepted HTML and the content security policy for served documents."
---

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
  but `;`-separated assignments of literals or dotted property paths , the
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
