Run from the repository root with Vite Plus and its pinned pnpm toolchain:

```sh
cd frontend
vp install --frozen-lockfile
vp run build
cd ..
CARGO_TARGET_DIR=/tmp/keryx/ui-target cargo build --offline --locked
cd frontend
KERYX_TEST_BINARY=/tmp/keryx/ui-target/debug/keryx vp run test:browser
```

The runner starts its own server on `127.0.0.1:17823`, with a temporary SQLite
database, blob directory, empty config and working directory. It strips all
inherited `KERYX_*` deployment settings, rejects an already-running server and
removes its temporary data on shutdown. It never uses the host Keryx service.
`KERYX_TEST_PORT` overrides the fixture port when another preview is running.
`KERYX_TEST_BINARY` selects the executable; CI defaults to `target/debug/keryx`.

Playwright Chromium must already be installed. For an existing compatible
Chromium, set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to its absolute path. To install
the matching browser, run `vp exec playwright install chromium` from `frontend/`.

The browser suite enables service workers and covers the actual worker's
registration, root scope and lifecycle. It does not send real push subscriptions
or notifications to external push services. SSE coalescing and in-flight
invalidation are covered by `vp test run` in `frontend/`.

Bulk actions are exercised against the isolated fixture: filtered selection,
version-history reset, confirmed permanent deletion, concurrent revisions,
partial failures, stopping the queue, authorization and uncertain network outcomes.
The tests never run bulk mutations against the host service.

Tag assignment uses an anchored multi-select popover for individual and bulk
drafts. Tests cover staging and creating several tags before Apply, discarding
pending choices, IME input, SSE updates, retrying a failed save, frozen bulk
targets, and atomic per-draft tag-limit failures.
