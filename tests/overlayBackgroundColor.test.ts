import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const rustSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);
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

test("settings › 通用 exposes the overlay background color and persists it", () => {
  // 默认值必须与 DLL 的 DEFAULT_BACKGROUND(255,0,0) 一致
  assert.match(appSource, /const DEFAULT_OVERLAY_BACKGROUND_COLOR = "#FF0000";/);
  assert.match(appSource, /覆\u76d6\u5c42\u80cc\u666f\u8272/);
  // 系统取色器 + 预设按钮 + 恢复默认
  assert.match(appSource, /type="color"/);
  assert.match(appSource, /OVERLAY_BACKGROUND_PRESETS/);
  assert.match(
    appSource,
    /invoke\("save_overlay_background_color", \{ value: nextValue \}\)/,
  );
  // 老 settings.json 没这个字段时回落到默认纯红
  assert.match(
    appSource,
    /overlayBackgroundColor \?\? DEFAULT_OVERLAY_BACKGROUND_COLOR/,
  );
  assert.match(appSource, /overlayBackgroundColor: string \| null;/);
});

test("backend normalizes the color and registers the save command", () => {
  assert.match(rustSource, /fn save_overlay_background_color\(value: String\)/);
  assert.match(rustSource, /save_overlay_background_color,/);
  assert.match(rustSource, /overlay_background_color: Option<String>/);
  assert.match(
    rustSource,
    /fn normalize_overlay_background_color\(value: &str\) -> Option<String>/,
  );
  // 归一化统一成大写 #RRGGBB，坏值返回 None（覆盖层继续用默认红）
  assert.match(rustSource, /to_ascii_uppercase\(\)/);
  assert.match(rustSource, /const DEFAULT_OVERLAY_BACKGROUND_COLOR: &str = "#FF0000";/);
});

test("launcher writes bgcolor into the status file for the DLL", () => {
  assert.match(sessionSource, /background_color: Option<String>/);
  assert.match(sessionSource, /text\.push_str\(&format!\("bgcolor=\{color\}\\n"\)\)/);
  // 状态文件是从 settings.json 读取的（DLL 读不到启动器设置）
  assert.match(sessionSource, /overlay_background_color/);
});

test("overlay DLL parses bgcolor, tints both paths and picks readable ink", () => {
  // 状态文件协议：bgcolor=#RRGGBB
  assert.match(overlayUi, /line\.strip_prefix\("bgcolor="\)/);
  assert.match(overlayUi, /pub const DEFAULT_BACKGROUND: \(u8, u8, u8\) = \(255, 0, 0\);/);
  // DIB 底色与 GL 清屏必须同色，否则纹理就绪前后会有一次跳色
  assert.match(overlayUi, /createSolidBrush|CreateSolidBrush\(colorref\(background_rgb\)\)/);
  assert.match(overlayLib, /let \(red, green, blue\) = crate::ui::background_rgb\(\);/);
  assert.match(overlayLib, /glClearColor\(/);
  // 浅色背景要自动换深色字
  assert.match(overlayUi, /fn ink_for\(rgb: \(u8, u8, u8\)\) -> \(u8, u8, u8\)/);
  // 颜色变化要触发纹理重传
  assert.match(overlayUi, /background_changed/);
});
