import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("settings sub-pages replay a directional slide on switch", () => {
  // 切分页时正文必须重挂（key），否则 React 原地复用 DOM 节点、动画不会重播。
  assert.match(appSource, /key=\{settingsPage\}/);
  assert.match(appSource, /data-direction=\{settingsTransitionDirection\}/);
  assert.match(appSource, /function switchSettingsPage\(nextPage: SettingsPage\)/);
  // 方向按侧栏顺序判断，6 个分页点击都走 switchSettingsPage。
  for (const page of ["about", "general", "toolbox", "logs", "modLoader", "startup"]) {
    assert.match(appSource, new RegExp(`switchSettingsPage\\("${page}"\\)`));
  }

  // 动画复用主页面切换的方向性滑入关键帧。
  assert.match(
    cssSource,
    /\.pz-settings-center > \.pz-resource-center-main > \.pz-panel \{\s*animation: pz-page-content-in-from-right/,
  );
  assert.match(
    cssSource,
    /\.pz-settings-center > \.pz-resource-center-main\[data-direction="backward"\] > \.pz-panel \{\s*animation-name: pz-page-content-in-from-left;/,
  );
  assert.match(
    cssSource,
    /@media \(prefers-reduced-motion: reduce\)\s*\{\s*\.pz-settings-center > \.pz-resource-center-main > \.pz-panel \{\s*animation: none;/,
  );
});
