---
name: html-communication
description: Create and publish self-contained HTML documents through the local Keryx server. Use when the user wants a plan, spec, write-up, findings, summary, report, comparison, or UI mocks, or mentions "HTML" with no additional context. This is the default delivery format for those, whether or not the user says "HTML". Do not use for HTML that ships as part of a product.
---

# HTML Communication

## Document

Create one self-contained HTML file, capped at 512 KB.

Structure and safety, for every document:

- Make it mobile-readable with a responsive viewport and no fixed-width layout.
- Use semantic HTML, inline CSS, inline SVG, and HTTPS or data-URL images.
- Use an inline classic script only when interactivity materially helps. Keep
  scripted pages useful without JavaScript; the sandbox blocks storage, fetch,
  workers, frames, forms, and popups.
- In script-free files, give external links `target="_blank"` and
  `rel="noopener noreferrer"`. If any script exists, omit `target="_blank"`.

Never include external or module scripts, inline event handlers, `javascript:`
URLs, forms, frames, embeds, objects, applets, meta refresh, linked stylesheets,
secrets, private URLs, or local filesystem paths.

Style, for documents about the work (plans, specs, reviews, findings, reports):

- Write it like a spec, not a landing page: dense, scannable, no hero,
  decorative chrome, marketing voice, or em dashes.
- True black (`#000`), white primary text, and dark gray only for secondary
  surfaces or accents.

UI mocks are exempt from those style rules. A mock follows the design system of
the product being mocked and the look the feature needs; if the app is
light-themed, mock it light.

## UI Mocks

When the user asks for variants:

- Render real styled variants, not descriptions.
- Label them `A`, `B`, `C`... for easy selection.
- Lay them out for direct comparison.

## Publish

The user has given standing permission to upload every artifact created or
updated with this skill. Upload is required, including in auto mode. Do not ask
for separate permission or stop at the local file.

1. Write the HTML file locally to tmp.
2. For a new document, run `keryx upload <absolute-file-path>`. For a revision
   of an existing one, follow Versioning below.
3. Report the returned Keryx public and raw HTML URLs, and the version number.

Use Keryx's default local API at `http://localhost:7812`. Do not configure
authentication or pass an API URL override. If validation fails, fix the
markup and retry. If the local Keryx server is unavailable, report that
clearly and retain the local artifact.

Never open a browser or claim the document is hosted before upload succeeds.
Do not verify in a browser unless the user asks.

## Versioning

A document under iteration keeps one draft URL for its whole life. Every
revision is a new version of that draft, never a second draft.

- Update by draft ID: `keryx upload <file> --draft <draft-id>`. The ID is the
  segment after `/d/` in any URL the user pastes back. `keryx list` recovers it
  otherwise.
- Re-uploading the same absolute file path also adds a version, but that only
  holds within a session, since tmp is cleared and a later session does not know
  the earlier filename. Prefer `--draft` whenever the ID is available, and keep
  one absolute file path across iterations so the fallback works.
- If a document is clearly a revision but the draft ID cannot be determined,
  ask. Do not create a second draft for the same document. Silently doing so
  leaves two half-current copies at two URLs, which is worse than stopping.
- Use `--new` only when the document is genuinely a different one.

A plan stays live through implementation. Keep revising the same draft as
reality changes it; do not open a second draft for implementation notes. Report
the version number on every upload, and label the draft with `--description`
when the user signs off (for example `Approved v3`). Retiring a finished draft
belongs to the 'keryx-archive' skill, not this one.
