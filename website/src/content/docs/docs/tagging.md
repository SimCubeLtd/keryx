---
title: "Dashboard tagging"
description: "Organise drafts with shared tags and filter them in the web dashboard."
---

Tags help you find related drafts in the web dashboard. Use labels such as `planning`, `needs-review` or `release-notes` to organise work across repositories.

Tagging is **dashboard-only**: there are no CLI or terminal UI commands, upload flags or automatic tagging. Tags are saved in the server's database and shared across browsers, so everyone using the dashboard sees the same assignments.

:::note[Protected dashboards]
With `KERYX_API_KEY` set, the dashboard is read-only and hides tags and their controls. Tagging is unavailable through the dashboard on these deployments; there is no CLI tagging alternative.
:::

## Add and remove tags

1. Select a draft in the dashboard and choose **+ Add tags** in its detail pane, or **Add tags…** in its row menu.
2. Search for existing tags and select as many as you need. To stage a new name, choose **Create**. You can keep searching and stage more names before saving.
3. Review the pending tag chips, then choose **Apply** to save all additions together.

The popover stays open while you select tags. Tags already on the draft are marked **Already assigned** and cannot be selected again. Remove a pending chip to unstage that choice, or use **Clear** to discard all pending choices. Neither removes existing assignments.

**Cancel**, Escape or clicking outside discards pending choices without saving. New tags are created only when you apply. Use arrow keys to navigate suggestions and Enter to toggle the highlighted choice; Enter does not save. Suggestions update after a short typing pause and longer lists scroll.

After a successful save the popover closes. If saving fails, your choices and the error stay visible so you can retry. Live updates preserve the search text and pending choices; if the target draft disappears, the popover closes with a notice.

Remove a tag using the **×** on its chip in the detail pane. This removes only that draft's assignment. The tag remains available to reuse, even when no drafts currently use it.

Names accept ASCII letters, digits, spaces and hyphens, with a maximum of **32 characters**. Keryx lowercases names, trims surrounding spaces and collapses repeated spaces: `Planning` and ` planning ` resolve to the same tag. Each draft can have up to **20 tags**.

## Tag several drafts

Choose **Bulk Actions** beside **Tags**, select drafts, then choose **Add tags…** in the selection bar. The same popover lets you select several existing tags and stage new ones before applying once to the selected drafts.

Tags already on every selected draft are disabled and marked **Already on all**. Partially assigned tags show a count such as **On 2 of 5**; selecting one fills in the missing assignments. Existing tags are always kept. Clear and Cancel discard pending choices; Cancel also preserves the draft selection.

Applying exits bulk mode and shows progress and a result for each draft inside the popover. The confirmed draft list stays fixed while saving. Each draft's additions are atomic: if the whole addition would exceed its 20-tag limit, none of those additions are saved to that draft. Other drafts can still succeed. Bulk tagging does not remove or replace tags.

See [Bulk actions](../dashboard/#bulk-actions) for selection rules, stopping a queue and handling failures.

## Filter the dashboard

Open **Tags** in the toolbar and tick one or more tags. The list filters immediately while the dropdown stays open. Search within the dropdown to find a tag; longer lists scroll.

- Multiple selected tags match **all** of them. Selecting `planning` and `needs-review` shows drafts carrying both labels.
- Repository, availability and text search narrow that result further. Text search also matches tag names.
- **Untagged only** shows drafts with no tags and clears selected tag filters. Selecting a tag leaves Untagged mode.
- Remove a selected filter chip to clear that filter, or choose **Clear all** to clear the tag filters.

Counts beside tags reflect the current availability tab, repository and text search before tag filtering. The availability tab badges still show their totals.

Rows show up to two tags, followed by `+N` for any others. The detail pane shows every tag. Clicking a row's tag applies it as a filter. **Tag A–Z** sorts by each draft's first alphabetical tag, then title; untagged drafts come last.

Filters are retained in the dashboard URL, so you can bookmark or share a filtered view. Live dashboard updates preserve filters and pending choices in an open tag-assignment popover.

## Tags stay separate from documents

Tags belong to the draft and survive new uploads, snoozing and disabling. Adding or removing a tag does not change its updated time, version history, content, public links or availability, and sends no notification. Tags do not appear in exported HTML or PDFs.

Resetting and pruning version history keeps tag assignments. Pruning the draft itself hides it from the dashboard; permanently deleting it removes its assignments. Existing drafts start untagged.

See [Versions and links](../versions/) for document revisions and [Availability and notifications](../availability/) for snoozing and access controls.
