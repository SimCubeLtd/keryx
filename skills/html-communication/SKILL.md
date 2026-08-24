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

- Write it like a spec, not a landing page: dense, scannable, no marketing
  hero, decorative chrome, marketing voice, or em dashes. A typeset document
  header (title, one-line lede, meta block) is not a hero.
- Typeset it, do not render markdown: display-scale page title with tight
  letter-spacing, uppercase letter-spaced kicker labels, hairline-rule grid
  layouts instead of stacked prose, and a sticky jump nav on long documents.
- True black (`#000`), white primary text, and dark gray only for secondary
  surfaces or accents.
- One recurring identity accent color per document (kickers, badges, nav
  highlights, scores) is welcome alongside semantic amber for risk and green
  for verified.

UI mocks are exempt from those style rules. A mock follows the design system of
the product being mocked and the look the feature needs; if the app is
light-themed, mock it light.

## PDF-safe documents

Documents are also source material for `keryx publish`. Keep their semantic DOM
useful when browser grids are flattened onto A4:

- Use one body-level `header`, followed by `main` with top-level `section`
  elements. PDF publication treats the header as a cover and begins each
  section on a new page.
- Keep reading order correct in the HTML itself. Multi-column browser layouts
  may become single-column PDF flow.
- Use semantic tables without fixed pixel widths. Allow headings, cells, code,
  URLs, images, and SVGs to wrap or shrink within their container.
- Mark a custom multi-column container with `data-keryx-print="stack"` when it
  must become single-column in the PDF.
- Avoid fixed heights, viewport-sized panels, horizontal scrolling, and large
  components that cannot split across a page.

Keryx supplies the A4 normalization, title/version publication header, and
date/page footer in a render-only copy. Do not duplicate those in the stored
HTML.

## UI Mocks

When the user asks for variants:

- Render real styled variants, not descriptions.
- Label them `A`, `B`, `C`... for easy selection.
- Lay them out for direct comparison.

## Publish

The user has given standing permission to upload every artifact created or
updated with this skill. Upload is required, including in auto mode. Do not ask
for separate permission or stop at the local file.

1. Write the HTML file under `/tmp/keryx/` (`mkdir -p /tmp/keryx` first): a new
   document as `/tmp/keryx/<slug>.html`, a revision as `/tmp/keryx/<draft-id>.html`.
   That directory is the working area for every Keryx document and survives the
   session (on Linux agent hosts it is bound into the agent sandbox, unlike the
   rest of `/tmp`), so a later session finds the file instead of rebuilding it.
2. For a new document, run `keryx upload '<absolute-file-path>'`. For a revision
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

- The draft ID is the segment after `/d/` in any URL the user pastes back;
  `keryx list` recovers it otherwise. Before using it in a path or command,
  require exactly 12 ASCII lowercase letters or digits (`[a-z0-9]{12}`). Stop
  if it does not match.
- Update by draft ID: `keryx upload '<file>' --draft '<draft-id>'`. Quote the
  validated ID and complete file path in every shell command.
- Before revising, look for `/tmp/keryx/<draft-id>.html`. If it is there, that is
  the working copy: edit it in place. If not, fetch the current version with
  `keryx raw '<draft-id>' > '/tmp/keryx/<draft-id>.html'` and work from that.
  Never rebuild a document from memory when either is available.
- Re-uploading the same absolute file path also adds a version, but only within a
  session (`/tmp/keryx` persists until reboot, not forever). Prefer `--draft`
  whenever the ID is available, and keep the one `/tmp/keryx/<draft-id>.html` path
  across iterations so the fallback works too.
- If a document is clearly a revision but the draft ID cannot be determined,
  ask. Do not create a second draft for the same document. Silently doing so
  leaves two half-current copies at two URLs, which is worse than stopping.
- Use `--new` only when the document is genuinely a different one.

A plan stays live through implementation. Keep revising the same draft as
reality changes it; do not open a second draft for implementation notes. Report
the version number on every upload, and label the draft with `--description`
when the user signs off (for example `Approved v3`). Retiring a finished draft
belongs to the 'keryx-archive' skill, not this one.
