---
title: "Versions and links"
description: "Keep one draft through revisions and link to specific versions."
---

A draft has a stable ID and a sequence of versions. Re-uploading the same file path adds a version to the same draft. To remove ambiguity across file moves or sessions, target its ID:

```sh
keryx upload ./revised-plan.html --draft <draft-id>
```

Use `--new` only when you want a separate document.

## Link to the right version

| Path | Meaning |
| --- | --- |
| `/d/<id>` | Latest HTML |
| `/d/<id>/raw` | Latest raw HTML |
| `/d/<id>/v/<n>` | A specific version |
| `/d/<id>/v/<n>/raw` | Raw HTML for a specific version |

Both public and raw routes return the stored HTML bytes. Disabling or deleting a draft can make its links unavailable. Resetting its version history reuses version numbers, so an old versioned link may stop working or show different content.

## Inspect a draft

```sh
keryx list
keryx open <draft-id>
keryx raw <draft-id> --version 2
```

Use `keryx tui` for a terminal browser and version history. The web dashboard at your server's root also shows drafts and their versions.

## Reset and prune version history

Use **Reset and prune version history** beside the dashboard's Version history
heading, or **Reset version history…** in a draft's context menu. Both open the same confirmation showing
how many older versions will be deleted and which version will be kept.

For example, a draft with v1 through v12 keeps v12's content, permanently deletes
the 11 older versions and their stored HTML, and renumbers the kept version as
v1. The next upload creates v2. The draft ID, main draft link, tags and
availability stay unchanged. Later uploads build version history again.

This cannot be undone. The old v12 link returns 404 and the old v1 link now
serves the retained content. Future uploads reuse v2 and higher numbers. This
also applies to versioned raw HTML and PDF requests and old notification links.
PDFs you have already downloaded are unchanged.

The action requires the dashboard's existing management access and works for
active, snoozed and disabled drafts. It is hidden when the draft already has
only v1. If a new version arrives before you confirm, the dashboard reloads
the history and asks you to review the updated confirmation.

If stored file cleanup fails, the history reset still completes and the
dashboard reports incomplete cleanup. Run `keryx storage gc` to inspect orphan
files, then `keryx storage gc --delete` to remove them.

To reset several histories together, choose **Bulk Actions**, select drafts and
choose **Reset & prune history…**. This keeps the drafts and their latest content;
it does not prune the drafts from the system. **Delete permanently…** removes
the selected drafts and every version instead. See [Bulk actions](../dashboard/#bulk-actions)
for review, confirmation and per-draft results.

## Provenance

Uploads record best-effort Git metadata from the directory where you run the command: repository, branch, commit and dirty state. Run uploads from the project checkout when this context matters.

## Export a fixed version

```sh
keryx publish --id <draft-id> --version 2 --output ./plan-v2.pdf
```

See [PDF publishing](../pdf/) for formatting requirements.
