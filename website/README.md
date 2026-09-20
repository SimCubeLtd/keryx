# Keryx website

One static Astro project: a custom landing page at `/` and Starlight documentation at `/docs/`.

## Development

Use Node.js 24 and the pnpm version pinned in `package.json`.

```sh
cd website
pnpm install --frozen-lockfile
pnpm dev
```

The development server normally listens on `http://localhost:4321`. Starlight search is indexed during production builds, so test search with `pnpm preview` after building.

## Build and verify

```sh
pnpm build
pnpm exec playwright install chromium
pnpm test
```

`build` runs Astro's type checker and creates the static site in `dist/`. Browser tests start a separate preview on port 4329 and check colour modes, persistence between the landing page and docs, blocked storage, mobile layout and search. Set `PLAYWRIGHT_CHROMIUM_EXECUTABLE` to reuse an existing compatible Chromium installation.

## Production URL

Set `SITE_URL` to the final public HTTPS origin when building:

```sh
SITE_URL=https://your-domain.example pnpm build
```

This sets canonical URLs and enables Starlight's sitemap. Without it, local builds still work, but the sitemap is skipped. Serve `dist/` with a static host that supports directory index files and uses `404.html` for missing routes. No backend or deployment credentials are required to build.

## GitHub Pages

`.github/workflows/deploy-docs.yml` publishes both the landing page and docs.
It only runs via **workflow_dispatch**. Pushes, pull requests and releases do
not deploy. The separate `website.yml` workflow only builds and tests.

1. Make the website files, lockfile and workflow available on the default
   branch through your normal review process. GitHub requires a dispatch
   workflow on the default branch before it appears in the Actions UI.
   Adding it does not trigger deployment.
2. Open repository **Settings → Pages → Build and deployment**. Set
   **Source** to **GitHub Actions**.
3. Check **Settings → Environments → github-pages**. Its deployment branch
   rules must allow the branch you intend to publish. Keep any required
   reviewers you want.
4. Open **Actions → deploy-docs → Run workflow**. Select the branch you want
   to publish, then run it. This replaces the published website with that
   branch's build. The deployment job reports the resulting URL.

The workflow uses `withastro/action@v6` with `path: website`, Node 24 and
the pnpm version pinned in `package.json`. It builds and uploads the static
artifact, then `actions/deploy-pages@v5` publishes it. No PAT, extra secrets,
repository variables, or `gh-pages` branch are needed.

`actions/configure-pages` reads the configured Pages origin and base path.
The build passes these as `SITE_URL` and `SITE_BASE`, so the GitHub project
URL and a later custom domain both work. To reproduce the project URL locally:

```sh
SITE_URL=https://simcubeltd.github.io SITE_BASE=/keryx pnpm build
SITE_URL=https://simcubeltd.github.io SITE_BASE=/keryx pnpm test
```

Keep handwritten Markdown links relative, and use `import.meta.env.BASE_URL`
for landing-page and custom component links and assets. Starlight handles
its generated navigation and assets.

### Optional custom domain later

In **Settings → Pages**, add the chosen custom domain. For
`keryx.simcube.co.uk`, add a DNS CNAME record for `keryx` pointing to
`simcubeltd.github.io`, with no repository path. Wait for GitHub's DNS and
certificate checks, then enable **Enforce HTTPS** when available. Manually
run `deploy-docs` again so canonical links and the sitemap use the new
domain. For an Actions deployment, GitHub manages the domain through Pages
settings; a committed `public/CNAME` file is not required.

References: [Astro action](https://github.com/withastro/action),
[GitHub Pages workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages),
[manual workflow runs](https://docs.github.com/en/actions/managing-workflow-runs-and-deployments/managing-workflow-runs/manually-running-a-workflow),
[custom domains](https://docs.github.com/en/pages/configuring-a-custom-domain-for-your-github-pages-site/managing-a-custom-domain-for-your-github-pages-site).

## Content and styling

- `src/pages/index.astro` owns the landing page.
- `src/content/docs/docs/` holds usage documentation. The nested `docs` directory creates the URL prefix.
- `src/styles/theme.css` supplies shared colours and self-hosted fonts.
- `src/styles/landing.css` styles the landing page; `docs.css` maps the theme into Starlight.
- `ThemeProvider.astro` and `ThemeSelect.astro` are shared by both parts of the site. Auto follows the system and is the default. Explicit choices persist in local storage.

Keep usage commands and documented defaults aligned with the repository README and CLI. The logo in `public/favicon.svg` is copied from `crates/keryx-render/assets/keryx-logo.svg`; update the copy when that asset changes.
