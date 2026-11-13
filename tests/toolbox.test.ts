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
    /export type SettingsPage =\s*\| "about"\s*\| "general"\s*\| "gameOptions"\s*\| "toolbox"\s*\| "logs"\s*\| "modLoader"\s*\| "startup"\s*\| "experimental"\s*\| "developer";/,
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

test("设置 › 通用 的分区卡片与百宝箱同一种亚克力材质", () => {
  const toolboxCard = rule(".pz-toolbox-card");
  const settingsCard = rule(".pz-settings-card");
  // 半透明底牌、细描边、轻阴影三者与百宝箱卡片一致；纯白实心块是旧样式。
  assert.match(settingsCard, /background:\s*var\(--pz-surface-card\)/);
  assert.match(settingsCard, /border:\s*1px solid var\(--pz-line\)/);
  assert.match(settingsCard, /box-shadow:\s*var\(--pz-shadow-sm\)/);
  assert.doesNotMatch(settingsCard, /background:\s*#fff/);
  assert.match(toolboxCard, /background:\s*var\(--pz-surface-card\)/);
  // 圆角与留白保持设置卡片自己的节奏，只换材质；标题到卡片的间距压到 0.45rem（紧凑版）。
  assert.match(settingsCard, /border-radius:\s*0\.5rem/);
  assert.match(settingsCard, /margin-top:\s*0\.45rem/);
});

test("设置行紧凑：收窄内边距、压紧标签与说明的行距", () => {
  // 组件侧挂上可定位的类名，CSS 才能真正覆盖 Chakra 的 p="4" / mt="1"。
  assert.match(appSource, /className="pz-settings-row-hint"/);
  assert.match(appSource, /className="pz-settings-row-label"/);
  assert.match(appSource, /className="pz-settings-section-title"/);

  // 注意 `.pz-settings-row + .pz-settings-row` 里也含 ".pz-settings-row {"，
  // 这里直接用正则匹配单条规则，避免 rule() 取到相邻兄弟选择器。
  assert.match(cssSource, /\.pz-settings-row \{\s*padding:\s*0\.55rem 0\.9rem !important/);

  const hint = rule(".pz-settings-row-hint");
  assert.match(hint, /margin-top:\s*0\.125rem !important/);

  // 标题、说明、分区标题共用一档更紧的行高。
  assert.match(
    cssSource,
    /\.pz-settings-row-label,\s*\.pz-settings-row-hint,\s*\.pz-settings-section-title\s*\{\s*line-height:\s*1\.35 !important/,
  );
});
