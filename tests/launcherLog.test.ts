import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
import { rustSource } from "./sourceText.ts";
const feedbackSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/feedback.rs", import.meta.url)),
  "utf8",
);
const logSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/launcher_log.rs", import.meta.url)),
  "utf8",
);

test("settings › 日志 is wired to the launcher log commands", () => {
  assert.match(appSource, /settingsPage === "logs"/);
  assert.match(appSource, /启动器日志/);
  assert.match(appSource, /read_launcher_log/);
  assert.match(appSource, /clear_launcher_log/);
  assert.match(appSource, /open_launcher_log_folder/);
  assert.match(appSource, /log_frontend_message/);
  assert.match(appSource, /launcherLogSettingsContent/);
});

test("backend writes and exposes the launcher log", () => {
  assert.match(rustSource, /mod launcher_log;/);
  assert.match(rustSource, /fn read_launcher_log/);
  assert.match(rustSource, /fn clear_launcher_log/);
  assert.match(rustSource, /fn open_launcher_log_folder/);
  assert.match(rustSource, /fn log_frontend_message/);
  assert.match(rustSource, /read_launcher_log,/);
  assert.match(rustSource, /launcher_log::start_session\(\)/);
  assert.match(rustSource, /std::panic::set_hook/);
});

test("launcher log rotates and is included in feedback uploads", () => {
  assert.match(logSource, /MAX_LOG_BYTES/);
  assert.match(logSource, /SESSION_MARKER/);
  assert.match(logSource, /rotate_if_needed_at/);
  // 反馈带整份启动器日志的尾部（重启过启动器也能带上出问题的那次会话）。
  assert.match(logSource, /pub\(crate\) fn full_log_tail\(max_bytes: usize\)/);
  assert.match(feedbackSource, /full_log_tail\(MAX_LAUNCHER_LOG_BYTES\)/);
  assert.match(feedbackSource, /launcher\.log/);
});
