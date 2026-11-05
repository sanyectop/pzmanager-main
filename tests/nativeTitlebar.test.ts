import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";
const shellSource = readFileSync(
  fileURLToPath(new URL("../src/components/shell.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("native titlebar toggle lives in general settings with a reason", () => {
  assert.match(appSource, /const \[nativeTitlebar, setNativeTitlebar\] = useState\(false\);/);
  assert.match(appSource, /setNativeTitlebar\(Boolean\(settingsResult\.value\.nativeTitlebar\)\)/);
  assert.match(appSource, /invoke\("save_native_titlebar", \{ enabled \}\)/);
  assert.match(appSource, /原生窗口标题栏/);
  assert.match(appSource, /无法拖动时用它兜底/);
  assert.match(
    appSource,
    /onCheckedChange=\{\(details\) => void updateNativeTitlebar\(details\.checked\)\}/,
  );
  // 失败回滚开关，界面不撒谎。
  assert.match(appSource, /async function updateNativeTitlebar\(enabled: boolean\)/);
});

test("backend persists the flag and applies decorations at startup", () => {
  assert.match(rustSource, /native_titlebar: bool/);
  assert.match(rustSource, /fn save_native_titlebar\(app: tauri::AppHandle, enabled: bool\)/);
  assert.match(rustSource, /set_decorations\(enabled\)/);
  assert.match(rustSource, /            save_native_titlebar,/);
  // 启动时在窗口内容出现前应用。
  assert.match(rustSource, /启用原生标题栏失败/);
});

test("titlebar hides both sides with slide-fade when native is on", () => {
  // 组件挂上开关类名；中间的导航不受影响。
  assert.match(shellSource, /nativeTitlebar\?: boolean/);
  assert.match(shellSource, /pz-titlebar-native/);
  // 左侧 LOGO+文字向左滑出，右侧窗口按钮向右滑出，均为淡入淡出。
  assert.match(cssSource, /\.pz-window-titlebar\.pz-titlebar-native \.pz-sidebar-brand \{/);
  assert.match(cssSource, /\.pz-window-titlebar\.pz-titlebar-native \.pz-window-controls \{/);
  assert.match(cssSource, /transform: translateX\(-1rem\)/);
  assert.match(cssSource, /transform: translateX\(1rem\)/);
  assert.match(cssSource, /pointer-events: none/);
});

test("task trigger slides to the right following the hidden controls", () => {
  // 原生模式下任务中心按钮平移过窗口按钮让出的空间（3 × 按钮宽 + 自身右边距），
  // 并回收 0.75rem 不贴窗口边缘；与按钮同方向同时长滑动。
  assert.match(cssSource, /--pz-window-control-width: 46px/);
  assert.match(cssSource, /width: var\(--pz-window-control-width\)/);
  assert.match(
    cssSource,
    /\.pz-window-titlebar\.pz-titlebar-native \.pz-task-trigger \{\s*transform: translateX\(calc\(3 \* var\(--pz-window-control-width\) \+ 0\.5rem - 0\.75rem\)\);/,
  );
  assert.match(
    cssSource,
    /\.pz-window-titlebar \.pz-task-trigger \{\s*transition:\s*opacity var\(--pz-dur-base\) ease,/,
  );
});

test("pressing the task trigger keeps its native offset instead of snapping back", () => {
  // 全局 `.pz-app-shell button:active` 会把 transform 整条换成 translateY(1px) scale(0.98)。
  // 原生模式下按钮的位置就靠这条 translateX，若不叠加回去，按住"任务"会先弹回
  // 自绘标题栏的位置、松手再滑回来。
  assert.match(
    cssSource,
    /\.pz-window-titlebar\.pz-titlebar-native\s+\.pz-task-trigger:not\(:disabled\):active\s*\{[\s\S]*?transform:\s*translateX\(calc\(3 \* var\(--pz-window-control-width\) \+ 0\.5rem - 0\.75rem\)\)\s*translateY\(1px\)\s*scale\(0\.98\)/,
  );
});
