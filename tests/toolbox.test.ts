import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

function rule(selector: string): string {
  const start = cssSource.indexOf(`${selector} {`);
  assert.ok(start >= 0, `missing rule: ${selector}`);
  return cssSource.slice(start, cssSource.indexOf("}", start) + 1);
}

test("百宝箱 is a settings category, not a top-level nav page", () => {
  assert.doesNotMatch(appSource, /type ActivePage = [^;]*"toolbox"/);
  assert.doesNotMatch(appSource, /value: "toolbox"/);
  assert.doesNotMatch(appSource, /activePage === "toolbox"/);
  // 设置 › 设置分类里新增「百宝箱」
  assert.match(
    appSource,
    /export type SettingsPage =\s*\| "about"\s*\| "general"\s*\| "toolbox"\s*\| "logs"\s*\| "modLoader"\s*\| "startup"\s*\| "developer";/,
  );
  assert.match(appSource, /onClick=\{\(\) => switchSettingsPage\("toolbox"\)\}/);
  assert.match(appSource, /aria-current=\{settingsPage === "toolbox" \? "page" : undefined\}/);
  assert.match(appSource, /settingsPage === "toolbox"\s*\n\s*\? toolboxContent/);
});

test("translator, downloader and console leave the settings sidebar", () => {
  assert.doesNotMatch(appSource, /setSettingsPage\("translator"\)/);
  assert.doesNotMatch(appSource, /setSettingsPage\("console"\)/);
  assert.doesNotMatch(appSource, /setSettingsPage\("downloader"\)/);
  assert.doesNotMatch(appSource, /settingsPage === "translator"/);
  assert.doesNotMatch(appSource, /settingsPage === "console"/);
  assert.doesNotMatch(appSource, /settingsPage === "downloader"/);
});

test("the toolbox page stacks the three tools as full-width cards", () => {
  const start = appSource.indexOf("const toolboxContent = (");
  assert.ok(start >= 0);
  const block = appSource.slice(start, appSource.indexOf("const settingsContent = (", start));
  assert.match(block, /className="pz-panel pz-toolbox-page"/);
  assert.match(block, /className="pz-toolbox-grid"/);
  assert.match(block, /className="pz-toolbox-card">\{translatorSettingsContent\}/);
  assert.match(block, /className="pz-toolbox-card">\{downloaderSettingsContent\}/);
  assert.match(block, /\{consoleContent\}/);
  // 文案与新布局一致：不再是“瀑布流”。
  assert.match(appSource, /卡片按单列全宽排列/);
  assert.doesNotMatch(appSource, /卡片按瀑布流排列/);

  // 控制台卡片自己带标题和分隔线（原来这一页没有标题）
  assert.match(appSource, /const consoleContent = \(\s*<Box className="pz-toolbox-card">/);
  assert.match(appSource, /className="pz-toolbox-separator"/);
});

test("toolbox cards stack in one full-width column", () => {
  const page = rule(".pz-toolbox-page");
  assert.match(page, /max-width:\s*none/);

  const grid = rule(".pz-toolbox-grid");
  assert.match(grid, /display:\s*flex/);
  assert.match(grid, /flex-direction:\s*column/);
  assert.match(grid, /gap:\s*0\.9rem/);
  assert.doesNotMatch(grid, /columns:/);
  assert.doesNotMatch(grid, /column-gap:/);

  const card = rule(".pz-toolbox-card");
  assert.match(card, /margin:\s*0;/);
  assert.doesNotMatch(card, /break-inside/);
  assert.match(card, /border-radius:\s*var\(--pz-radius-md\)/);
  assert.match(card, /background:\s*var\(--pz-surface-card\)/);

  // 卡片里的设置面板不再自带卡面/内边距，避免卡片套卡片
  const flattened = rule(".pz-toolbox-card .pz-panel");
  assert.match(flattened, /padding:\s*0 !important/);
  assert.match(flattened, /background:\s*transparent !important/);
  assert.match(flattened, /border:\s*0 !important/);
});
