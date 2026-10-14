---
title: "Web dashboard"
description: "Find drafts, manage individual documents and apply bulk actions."
---

Open your server's root URL to browse drafts and their version history. The dashboard is built into the Rust executable and needs JavaScript in the browser. Serving it requires no Node.js process or separate frontend server. Published document links continue to serve the uploaded HTML directly.

## Find drafts

Use the **Active**, **Snoozed** and **Disabled** tabs to choose an availability state. Search matches draft titles, descriptions, IDs, repository names, branches and tags. Press `/` to focus search when you are not typing in another control.

The repository popover groups repositories by owner. Search for an owner or repository, then select one repository. **All repositories** clears that filter. Repository selection is single-select; [tag filters](../tagging/#filter-the-dashboard) can match several tags at once.

The sort menu offers recently updated, oldest update, title, most versions and tag order. The dashboard URL retains the selected draft, availability tab, search, repository, tag filters and sort order, so you can bookmark a view.

Use the sun or moon button in the top-right corner to choose **Light**, **Dark** or **System**. The browser remembers your choice; System follows your device's colour scheme.

## Manage one draft

Select a draft to see its details, tags and version history. Its row's **…** menu offers document links and downloads, tag assignment, availability controls, history reset and removal. Actions apply to that row's draft even when another draft is open in the detail pane.

- **Add tags…** opens the [multi-select tag popover](../tagging/#add-and-remove-tags).
- **Snooze…**, **Unsnooze**, **Disable** and **Enable** depend on the draft's current [availability](../availability/).
- **Reset version history…** keeps the latest content and [restarts history at v1](../versions/#reset-and-prune-version-history).
- **Prune draft…** stops serving the draft and retains its stored versions.
- **Delete permanently…** removes the draft and every stored version.

With `KERYX_API_KEY` set, the dashboard is read-only: management controls, tags and private provenance are hidden. Use the authenticated CLI for supported management operations.

## Bulk actions

1. Set your availability tab and filters.
2. Choose **Bulk Actions** beside **Tags**.
3. Tick individual draft checkboxes, or use the header checkbox to select all filtered drafts.
4. Choose **Add tags…**, **Reset & prune history…** or **Delete permanently…** in the selection bar.

The workspace turns purple and individual row menus disappear while bulk mode is active. The entry button becomes **Cancel bulk actions**, which exits selection mode and clears the selection.

Changing the availability tab, search, repository or tag filters clears the selection. Changing sort order keeps it. New drafts arriving through live updates are not automatically selected. Opening or cancelling an action leaves the selection intact; confirming an action exits bulk mode and keeps its progress and results visible.

### Add tags

Choose several existing tags or stage new names, then apply them together. Existing assignments are kept. See [bulk tagging](../tagging/#tag-several-drafts) for shared and partially assigned tags.

### Reset and prune history

Review the selected drafts and version counts before confirming. This action permanently removes older versions, keeps each draft's latest content as v1, and lets the next upload create v2. It preserves the draft itself, its main link, tags and availability. Drafts already containing only v1 are left unchanged.

:::caution[Versioned links change]
History reset reuses version numbers. Old versioned HTML, raw HTML, PDF and notification links may stop working or show different content. See [Versions and links](../versions/#reset-and-prune-version-history) before confirming.
:::

### Delete permanently

Review the selected drafts and all stored versions, then type `DELETE` to enable confirmation. This removes each selected draft and every version. Its public and versioned links stop working. There is no undo.

Bulk deletion checks the current version against the reviewed version. If a new upload arrives, that draft is not silently deleted. Refresh the review before confirmation, or inspect a reported conflict and select the draft again.

### Progress and failures

Bulk saves run one draft at a time against the confirmed selection. **Stop after current draft** allows the current request to finish and leaves the remaining drafts unattempted. Completed changes are not rolled back if another draft fails.

Results identify drafts that completed, were unchanged, failed, changed since review or were not attempted. Authentication failures and uncertain network outcomes stop the queue. If an outcome is unknown, refresh and inspect the draft before trying again; Keryx does not automatically retry it.

If history reset or deletion reports that storage cleanup is pending, metadata changes have already committed. Use `keryx storage gc` to inspect orphan files, then `keryx storage gc --delete` to remove them.
