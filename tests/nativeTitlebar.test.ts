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

test("native titlebar toggle lives in the appearance section with a reason", () => {
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

test("titlebar and overlay color sit in 外观, not in 通用", () => {
  const start = appSource.indexOf("const generalSettingsContent = (");
  assert.ok(start >= 0);
  const page = appSource.slice(start, appSource.indexOf("const translatorSettingsContent = (", start));
  const generalStart = page.indexOf('<SettingsSection title="通用">');
  const appearanceStart = page.indexOf('<SettingsSection title="外观">');
  const translationStart = page.indexOf('<SettingsSection title="翻译">');
  assert.ok(generalStart >= 0 && appearanceStart > generalStart && translationStart > appearanceStart);

  const general = page.slice(generalStart, appearanceStart);
  const appearance = page.slice(appearanceStart, translationStart);
  // 这两项都是外观类设置：从「通用」搬到「外观」子分区。
  assert.doesNotMatch(general, /原生窗口标题栏/);
  assert.doesNotMatch(general, /覆盖层背景色/);
  assert.match(appearance, /label="原生窗口标题栏"/);
  assert.match(appearance, /label="覆盖层背景色"/);
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

test("drag region leaves dragging and double-click maximize to Tauri's script", () => {
  // Tauri 注入的 drag.js 已处理：mousedown detail=1 拖动、detail=2 双击最大化。
  // 自己再在 onMouseDown 里 startDragging 会让第二击也触发拖动，双击最大化被吃掉。
  assert.match(
    shellSource,
    /<Box aria-label="拖动窗口" className="pz-window-drag-region" data-tauri-drag-region \/>/,
  );
  assert.doesNotMatch(shellSource, /pz-window-drag-region[^>]*onMouseDown/);
  assert.doesNotMatch(shellSource, /pz-window-drag-region[^>]*onDoubleClick/);
});

test("restore icon is drawn as two overlapping squares inside the 11px box", () => {
  // 重绘版：::before 后框贴右上、::after 前框贴左下，等大 8×8、对角错开 3px；
  // 前框带不透明底色盖住重叠边框，悬停时底色跟着按钮变。
  assert.match(
    cssSource,
    /\.pz-window-icon-restore::before \{\s*position: absolute;\s*top: 0;\s*right: 0;\s*width: 8px;\s*height: 8px;\s*border: 1px solid currentColor;/,
  );
  assert.match(
    cssSource,
    /\.pz-window-icon-restore::after \{\s*position: absolute;\s*bottom: 0;\s*left: 0;\s*width: 8px;\s*height: 8px;\s*border: 1px solid currentColor;/,
  );
  assert.match(
    cssSource,
    /\.pz-window-control:hover \.pz-window-icon-restore::after \{\s*background: var\(--pz-window-icon-bg-hover\);/,
  );
  // 旧画法（主体带边框 + before 顶出图标盒）不应残留。
  assert.doesNotMatch(cssSource, /top: -3px;\s*left: 3px;/);
});
