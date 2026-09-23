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

1. Select a draft in the dashboard.
2. In its detail pane, choose **+ Add tag**.
3. Type to find an existing tag, then select it. To add a new name, choose **Create**.

The modal closes after the tag is saved. If saving fails, your input and the error stay visible so you can retry. Use the arrow keys to navigate suggestions, Enter to select, or Escape to cancel. Suggestions update after a short typing pause and show at most eight options in a scrollable list. Tags already on the draft are marked **Added**.

Remove a tag using the **×** on its chip in the detail pane. This removes only that draft's assignment. The tag remains available to reuse, even when no drafts currently use it.

Names accept ASCII letters, digits, spaces and hyphens, with a maximum of **32 characters**. Keryx lowercases names, trims surrounding spaces and collapses repeated spaces: `Planning` and ` planning ` resolve to the same tag. Each draft can have up to **20 tags**.

## Filter the dashboard

Open **Tags** in the toolbar and tick one or more tags. The list filters immediately while the dropdown stays open. Search within the dropdown to find a tag; longer lists scroll.

- Multiple selected tags match **all** of them. Selecting `planning` and `needs-review` shows drafts carrying both labels.
- Repository, availability and text search narrow that result further. Text search also matches tag names.
- **Untagged only** shows drafts with no tags and clears selected tag filters. Selecting a tag leaves Untagged mode.
- Remove a selected filter chip to clear that filter, or choose **Clear all** to clear the tag filters.

Counts beside tags reflect the current availability tab, repository and text search before tag filtering. The availability tab badges still show their totals.

Rows show up to two tags, followed by `+N` for any others. The detail pane shows every tag. Clicking a row's tag applies it as a filter. **Tag A–Z** sorts by each draft's first alphabetical tag, then title; untagged drafts come last.

Filters are retained in the dashboard URL, so you can bookmark or share a filtered view. Live dashboard updates preserve filters and an open Add tag modal.

## Tags stay separate from documents

Tags belong to the draft and survive new uploads, snoozing and disabling. Adding or removing a tag does not change its updated time, version history, content, public links or availability, and sends no notification. Tags do not appear in exported HTML or PDFs.

Pruning a draft hides it from the dashboard; permanently purging it removes its tag assignments. Existing drafts start untagged.

See [Versions and links](../versions/) for document revisions and [Availability and notifications](../availability/) for snoozing and access controls.
