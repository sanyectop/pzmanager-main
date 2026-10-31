import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("task center panel plays an exit animation before unmounting", () => {
  // 三条关闭路径（× / 点外部 / Esc）都要走带动画的 closeTaskPanel，不能直接卸载。
  assert.match(appSource, /function closeTaskPanel\(\)/);
  assert.match(appSource, /setTaskPanelClosing\(true\)/);
  assert.match(appSource, /onClose=\{closeTaskPanel\}/);
  assert.match(appSource, /closing=\{taskPanelClosing\}/);
  assert.doesNotMatch(appSource, /onClose=\{\(\) => setTaskPanelOpen\(false\)\}/);

  // 面板播放打开动画的反向：上滑淡出，时长用统一的退出档 --pz-dur-exit。
  assert.match(cssSource, /\.pz-task-panel\.is-closing \{[^}]*animation: pz-task-panel-out var\(--pz-dur-exit\)/);
  assert.match(
    cssSource,
    /@keyframes pz-task-panel-out \{\s*from \{\s*opacity: 1;\s*transform: none;\s*\}\s*to \{\s*opacity: 0;\s*transform: translateY\(-0\.3rem\);\s*\}\s*\}/,
  );
  // 动画期间面板不响应指针，避免还能点到里面的按钮。
  assert.match(cssSource, /\.pz-task-panel\.is-closing \{[^}]*pointer-events: none;/);
});
