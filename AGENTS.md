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
