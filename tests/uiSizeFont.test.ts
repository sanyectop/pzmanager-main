import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const rustSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);

/** 注释里会写历史值，比较前先去掉。 */
const cssCode = cssSource.replace(/\/\*[\s\S]*?\*\//g, "");

test("root font size and family scale via css variables with fallbacks", () => {
  // 根字号必须吃 --pz-font-scale，且 var() 自带兜底（JS 还没跑起来时也有默认值）
  assert.match(cssCode, /font-size:\s*calc\(16px \* var\(--pz-font-scale, 1\)\);/);
  assert.match(cssCode, /line-height:\s*calc\(22px \* var\(--pz-font-scale, 1\)\);/);
  // :root 里要有倍率兜底声明（与 --pz-blur-scale 同一套约定）
  assert.match(cssCode, /--pz-font-scale:\s*1;/);
  // 字体栈必须包在 var(--pz-font-family, <默认栈>) 里，才能被设置项覆写
  assert.match(cssCode, /font-family:\s*var\(\s*--pz-font-family,/);
});

test("settings › 通用 exposes 字号/字体/窗口大小 and applies them", () => {
  assert.match(appSource, /const MIN_FONT_SCALE = 80;/);
  assert.match(appSource, /const MAX_FONT_SCALE = 130;/);
  assert.match(appSource, /const DEFAULT_FONT_SCALE = 100;/);
  // 变量写在 documentElement 上：Portal 到 body 的菜单 / 弹窗 / Toast 也吃得到
  assert.match(
    appSource,
    /document\.documentElement\.style\.setProperty\("--pz-font-scale", String\(fontScale \/ 100\)\)/,
  );
  assert.match(
    appSource,
    /document\.documentElement\.style\.setProperty\("--pz-font-family", trimmed\)/,
  );
  assert.match(appSource, /界面字号/);
  assert.match(appSource, /界面字体/);
  assert.match(appSource, /窗口大小/);
  assert.match(appSource, /max=\{MAX_FONT_SCALE\}/);
  assert.match(appSource, /invoke\("save_font_scale", \{ value: clamped \}\)/);
  assert.match(appSource, /invoke\("save_font_family", \{ value: nextValue \}\)/);
  // 老 settings.json 没有这些字段时要回落默认值
  assert.match(appSource, /setFontScale\(settingsResult\.value\.fontScale \?\? DEFAULT_FONT_SCALE\)/);
  assert.match(appSource, /settingsResult\.value\.fontFamily \?\? ""/);
  // 窗口尺寸记忆：拖动结束防抖落盘 + 恢复默认按钮
  assert.match(appSource, /invoke<\[number, number\]>\("save_window_size"\)/);
  assert.match(appSource, /invoke\("restore_default_window_size"\)/);
  assert.match(appSource, /const DEFAULT_WINDOW_WIDTH = 840;/);
  assert.match(appSource, /const DEFAULT_WINDOW_HEIGHT = 560;/);
});

test("backend commands and ranges match the frontend", () => {
  // invoke 里的命令名必须和后端函数同名，否则运行时直接 404
  assert.match(rustSource, /fn save_font_scale\(value: u8\)/);
  assert.match(rustSource, /fn save_font_family\(value: String\)/);
  assert.match(rustSource, /fn save_window_size\(app: tauri::AppHandle\)/);
  assert.match(rustSource, /fn restore_default_window_size\(app: tauri::AppHandle\)/);
  for (const command of [
    "save_font_scale",
    "save_font_family",
    "save_window_size",
    "restore_default_window_size",
  ]) {
    assert.match(rustSource, new RegExp(`${command},`), `handler 列表缺少 ${command}`);
  }
  assert.match(rustSource, /font_family: Option<String>/);
  assert.match(rustSource, /font_scale: Option<u8>/);
  assert.match(rustSource, /window_width: Option<u32>/);
  assert.match(rustSource, /window_height: Option<u32>/);
  assert.match(rustSource, /const FONT_SCALE_MAX: u8 = 130;/);
  assert.match(rustSource, /const FONT_SCALE_DEFAULT: u8 = 100;/);
  assert.match(rustSource, /fn normalize_font_scale\(value: u8\) -> u8/);
  // 窗口下限与两份 tauri conf 的 minWidth/minHeight 保持一致
  assert.match(rustSource, /const WINDOW_WIDTH_MIN: u32 = 840;/);
  assert.match(rustSource, /const WINDOW_HEIGHT_MIN: u32 = 560;/);
  // 启动时在 setup 里恢复记住的窗口尺寸
  assert.match(rustSource, /window\.set_size\(tauri::LogicalSize::new\(width, height\)\)/);
});
