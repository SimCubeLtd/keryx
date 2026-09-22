Run from the repository root after installing the website's locked development dependencies:

```sh
cargo build --offline
pnpm --dir website exec playwright test --config ../tests/dashboard/playwright.config.mjs
```

The runner starts its own server on port 17823 with a temporary SQLite database, blob directory and empty config. It rejects an already-running server and removes its temporary data on shutdown. Playwright Chromium must already be installed.
