# Project Workspace Rules

## Sandbox and temporary-user policy

**Do not create any sandbox-related directory, user, profile, cache, temporary runtime, or test workspace inside the project folder.**

This includes, without limitation:

- `.tmp-*`, `.sandbox-*`, `.codex-sandbox*`, `sandbox-*`, `webview-test-*`
- local sandbox users or Windows accounts
- WebView2, browser, crashpad, or runtime data directories
- temporary build/runtime directories whose only purpose is sandbox testing

If isolation or temporary runtime storage is required, use an explicitly approved
location outside `F:\pzm` and clean it up afterward.

The project directory must contain only source, tests, documentation, build
configuration, and intentional project outputs.

## Official website (官网)

The public website (https://pzm.sanyec.top) lives in this repo at `docs/promo/release-page/`:

- `index.html` — landing page: top nav, hero with download buttons (GitHub Releases / 123 云盘), feature grid, footer. Shows the current release version in `.dl-note` — bump it together with `package.json` / `tauri.conf.json` on every release.
- `docs.html` — user documentation page.
- `pzm-logo.png` — site logo; `shots/` — launcher screenshots used by the hero.
- `developer.png` — the author avatar (叁葉), same file as the launcher's `public/avatars/developer.png`; keep both in sync when the avatar changes.
- Author support link: 爱发电 https://ifdian.net/a/sanyee (username 叁葉). Present in the top nav, as a `.support-strip` banner under the hero downloads, and in the footer — keep it prominent but out of the body content.
- The Steam API relay (`steam-relay(服务端请勿上传)/server.js`, deployed separately) links back to this site and mimics its aurora style; the relay is NOT part of the website.
- `?wall=true` query parameter (all three pages): embedded/chrome-less view — hides the top nav, TOC and footer, renders the content single-column. Used when the launcher displays site pages in its own webview. The flag is read in a head script before first paint (`<html data-wall="true">`) so nothing flashes.
- Deployment is manual (static files). Local preview: open `index.html` directly in a browser.
- `policy.html` — privacy policy (隐私协议): effective date at the top, linked from the footers of index/docs. Update it together with the release notes when data behavior changes (update check, Steam relay, feedback upload).
