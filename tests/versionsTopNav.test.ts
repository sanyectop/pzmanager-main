import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";

const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const cubeIconPath = fileURLToPath(new URL("../public/icons/cube.svg", import.meta.url));

test("版本 is a top-level nav page with a cube icon", () => {
  assert.match(
    appSource,
    /type ActivePage = "launch" \| "overview" \| "shots" \| "shaderpacks" \| "versions" \| "workshop" \| "mods" \| "settings";/,
  );
  // 从设置侧栏移除，不再有设置内切换。
  assert.match(
    appSource,
    /export type SettingsPage =\s*\| "about"\s*\| "general"\s*\| "toolbox"\s*\| "logs"\s*\| "modLoader"\s*\| "startup"\s*\| "developer";/,
  );
  assert.doesNotMatch(appSource, /switchSettingsPage\("versions"\)/);
  // 顶栏导航项：标签「版本」、图标 cube。
  assert.match(
    appSource,
    /\{ value: "versions", label: "游戏管理", icon: "cube", description: "游戏版本、支持库、光影包与截图管理" \}/,
  );
  assert.match(cssSource, /\.pz-side-nav-icon-cube\s*\{[\s\S]*?url\("\/icons\/cube\.svg"\)/);
  assert.ok(existsSync(cubeIconPath), "缺少 public/icons/cube.svg");
  const cubeSvg = readFileSync(cubeIconPath, "utf8");
  assert.match(cubeSvg, /viewBox="0 0 24 24"/);
  assert.match(cubeSvg, /<path d="M21 16V8/);
});

test("the versions page renders from the top-level page switch", () => {
  assert.match(appSource, /\{activePage === "versions" && versionSettingsContent\}/);
  // 去换盘提示也应该跳到顶层「版本」页。
  assert.match(
    appSource,
    /function goChangeVersionStoreDrive\(\) \{[\s\S]*?navigateToPage\("versions"\)/,
  );
  // 版本数据加载按顶层页面触发，而不是 settingsPage。
  assert.match(appSource, /if \(activePage !== "versions"\) return;/);
  assert.match(appSource, /\}, \[activePage, versionTab\]\);/);
});
