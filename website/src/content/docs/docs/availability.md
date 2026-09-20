---
title: "Availability and notifications"
description: "Snooze, disable or restore drafts and configure dashboard notifications."
---

Every live draft is in exactly one of three states:

| State | Dashboard | Public, raw, versioned, and PDF routes |
| --- | --- | --- |
| Active | Default tab | Serve |
| Snoozed | Snoozed tab until the wake time | Serve |
| Disabled | Disabled tab | 404 |

Snooze affects attention, not access. `keryx snooze <id> --for 45m|2h|3d|1w`
(units combine, e.g. `1h30m`) or `--until <RFC 3339>` hides the draft from
`keryx list` and the Active tab until the wake time; the server stores the
time as UTC with milliseconds and rejects anything not in the future. A draft
wakes by the clock: once `snoozedUntil` has passed it is active again with no
database write and no cleanup job. `keryx unsnooze` wakes it now, `disable`
stops serving (and clears any snooze), and `enable` serves it again. One
mutation owns every transition, so a draft is never both snoozed and disabled.
Uploading a new version never changes availability.

`keryx list` hides snoozed drafts by default; `--include-snoozed` shows every
live draft and `--snoozed` shows only the sleeping ones. `DraftSummary` on the
wire carries `disabled` and an optional `snoozedUntil`; clients derive the
state from those two fields and the current time.

The dashboard opens on Active. `/?draft=<id>&view=snoozed` deep-links to a tab
and draft. The selected pane offers Snooze (with presets or a custom wake
time), Unsnooze, Disable, or Enable; with an API key set those controls are
absent and the authenticated CLI is the management path.

While the dashboard is open, a Server-Sent Events connection reports that its
current view may be stale. The browser then fetches one server-rendered
snapshot and reconciles the rows, counts, selected draft, version history, and
repository filter without reloading the page. The event carries no draft data.
A reconnect immediately receives the latest revision, so changes made while
the connection was down are recovered. Protected deployments use the same
redacted rendering path as the initial dashboard response.

## Installable app and notifications

Keryx serves a web app manifest, a service worker, and icons on every
deployment. What activates is decided by the browser's real origin:

- On an HTTPS origin (for example a Tailscale Serve hostname proxying to the
  local server, or a reverse proxy with a certificate) a supported browser
  offers **Install Keryx** in the top bar and the **Notifications** control
  can subscribe the device to Web Push.
- On plain HTTP the dashboard works as before and those controls stay hidden.

The service worker handles push display and notification clicks only. It
never intercepts requests and keeps no cache, so drafts are always served
live. A notification click accepts only a same-origin path and focuses and
navigates an existing Keryx window, or opens one.

Notification types are **Plan published** (first upload, opens `/d/:id`),
**Plan revised** (later upload, opens the immutable `/d/:id/v/:n`), **Plan
woke** (a snooze expired, opens the draft), **Plan enabled**, and **Plan
disabled** (open the matching dashboard tab). Snoozing and unsnoozing are the
owner's own attention management and produce no event. PDF publication and
download never create an event. Each device chooses which types it receives
from the Notifications control; preferences are stored per subscription.

Delivery is store-first: an event is written in the same SQLite transaction
as the draft change and queued once per opted-in subscription, then a
background dispatcher sends it, retries temporary push-service failures with
doubling delays, and removes subscriptions the service reports as expired.
Subscription endpoints must be public `https` hosts: private, loopback,
link-local, and other reserved addresses are refused when subscribing, again
after DNS resolution on every connection, and the dispatcher never follows
redirects.
A wake is keyed by its snooze timestamp, so it is sent exactly once even if
the server restarts around the wake time; the dispatcher rebuilds its
schedule on startup. The server's VAPID key pair is created on first run at
`<data-dir>/vapid.json` (owner-readable only) and reused thereafter; changing
it invalidates every subscription. Payload encryption and VAPID signing come
from the `web-push-native` crate, never from Keryx itself, and payloads carry
only display text and a same-origin path.

With `KERYX_API_KEY` set the dashboard cannot subscribe (it is read-only), so
push stays unavailable on protected deployments until Keryx has browser
authentication. Denied or unsupported notification permission never blocks
snoozing: the dashboard moves an expired snooze back to Active on its own and
shows an in-page toast while it is open.
