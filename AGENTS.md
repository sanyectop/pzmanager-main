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
- 不要上传任何带有请勿上传字样的文件夹和文件

## Overlay DLL (pz_overlay) — rebuild after every source change

The loading overlay injected into the game is a separate crate: `overlay-dll/`
(produces `pz_overlay.dll`). **Its build product is only refreshed by an explicit
build — Rust/Tauri builds of `src-tauri` never rebuild it.** A stale copy is the
#1 cause of "改了 DLL 代码但行为没变"（2026-10-31 ~ 11-06 的排查浪费即此因：注入的一直是 11-01 的旧副本）。

Rules:

- After touching anything in `overlay-dll/src/**`, rebuild it:
  `npm run build:overlay`（= `cargo build --release --target x86_64-pc-windows-msvc --manifest-path overlay-dll/Cargo.toml`）。
  `npm run tauri:dev` / `npm run tauri:build` already run it as a prerequisite — do NOT bypass them
  by invoking `cargo build` / `tauri dev` directly.
- Build product (dev 注入取它，workspace 优先): `overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll`.
  `src-tauri/target/debug/` 里那份是 dev 启动时的资源复制，**只在 dev 启动时刷新一次**，不随重新编译更新。
- Verification: launcher.log 会话详情行（`覆盖层会话详情：… 渲染≈…`）——`渲染≈无采样` 通常意味着注入的是旧 DLL
  （新 DLL 每 500ms 回写渲染速率），但非充要：UI 线程 panic、渲染文件写失败或采样过期（>2 秒）时，
  新 DLL 也会显示无采样。DLL 自报版本看 `%TEMP%\overlay.log` 各会话块开头的
  `pz_overlay vX.Y.Z 已加载` 行（log 为 append 模式、跨会话不清空，需按 pid 检索）。
- 相应地：改完 DLL 源码后，"改动没生效"的排查第一步永远是确认注入的 DLL 时间戳 / 版本行，再谈代码。
