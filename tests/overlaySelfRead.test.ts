import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const sessionSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/overlay_session.rs", import.meta.url)),
  "utf8",
);
const overlayUi = readFileSync(
  fileURLToPath(new URL("../overlay-dll/src/ui.rs", import.meta.url)),
  "utf8",
);
const overlayLib = readFileSync(
  fileURLToPath(new URL("../overlay-dll/src/lib.rs", import.meta.url)),
  "utf8",
);
const consoleReader = readFileSync(
  fileURLToPath(new URL("../overlay-dll/src/console_reader.rs", import.meta.url)),
  "utf8",
);
const coreSource = readFileSync(
  fileURLToPath(new URL("../overlay-core/src/lib.rs", import.meta.url)),
  "utf8",
);

test("launcher status file carries a heartbeat, console path and session id", () => {
  assert.match(sessionSource, /fn heartbeat\(&mut self\)/);
  assert.match(sessionSource, /beat=\{now_ms\}\\n/);
  assert.match(sessionSource, /console=\{\}\\n/);
  assert.match(sessionSource, /launcherpid=\{\}\\n/);
  assert.match(sessionSource, /session=\{\}\\n/);
  assert.match(sessionSource, /StatusFile::new\(console_path\.clone\(\), session_start\)/);
  assert.match(sessionSource, /status\.heartbeat\(\)/);
});

test("DLL switches to self-reading console.txt when the launcher heartbeat is stale", () => {
  assert.match(overlayUi, /const STATUS_STALE_AFTER: Duration/);
  assert.match(overlayUi, /fn read_status_snapshot\(\) -> StatusSnapshot/);
  assert.match(overlayUi, /fn poll_overlay_sources\(/);
  assert.match(overlayUi, /ConsoleReader/);
  assert.match(overlayUi, /启动器状态陈旧\/缺失，改为自读 console\.txt/);
  assert.match(overlayUi, /crate::overlay::request_shutdown\(&reason\)/);
});

test("DLL console reader reuses overlay-core and can stop itself", () => {
  assert.match(overlayLib, /mod console_reader;/);
  assert.match(overlayLib, /fn request_shutdown\(reason: &str\)/);
  assert.match(overlayLib, /SELF_STOP_REQUESTED/);
  assert.match(consoleReader, /pub struct ConsoleReader/);
  assert.match(consoleReader, /default_console_path/);
  assert.match(consoleReader, /on_game_init_complete/);
  assert.match(consoleReader, /completion_marker/);
  assert.match(consoleReader, /self\.complete = Some/);
  assert.match(coreSource, /pub fn zb_phase/);
  assert.match(coreSource, /pub fn progress_for/);
  assert.match(coreSource, /pub fn idle_completion/);
  assert.match(coreSource, /pub fn normalize_console_line/);
});
