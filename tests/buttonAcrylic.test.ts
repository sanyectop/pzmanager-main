import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const themeSource = readFileSync(
  fileURLToPath(new URL("../src/theme.ts", import.meta.url)),
  "utf8",
);
const mainSource = readFileSync(
  fileURLToPath(new URL("../src/main.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const guideSource = readFileSync(
  fileURLToPath(new URL("../docs/ui-style-guide.md", import.meta.url)),
  "utf8",
);

test("ghost and outline buttons get the shared acrylic surface", () => {
  assert.match(themeSource, /createSystem\(\s*defaultConfig/);
  assert.match(themeSource, /recipes:\s*\{\s*button: acrylicButton/);
  // 两种透明底变体都走同一套亚克力令牌
  assert.equal((themeSource.match(/bg: "var\(--pz-btn-tint\)"/g) ?? []).length, 2);
  assert.equal(
    (themeSource.match(/backdropFilter: "blur\(var\(--pz-menu-blur\)\) saturate\(var\(--pz-menu-saturate\)\)"/g) ?? [])
      .length,
    2,
  );
  assert.match(themeSource, /boxShadow: "inset 0 1px 0 var\(--pz-btn-highlight\)"/);
  // solid 主色按钮保持实色，不参与亚克力
  assert.doesNotMatch(themeSource, /solid:\s*\{/);
  assert.match(mainSource, /import \{ system \} from "\.\/theme";/);
  assert.match(mainSource, /<ChakraProvider value=\{system\}>/);
});

test("button acrylic tokens exist and follow the global blur setting", () => {
  assert.match(cssSource, /--pz-btn-tint:\s*rgba\(255, 255, 255, 0\.44\);/);
  assert.match(cssSource, /--pz-btn-tint-hover:\s*rgba\(255, 255, 255, 0\.68\);/);
  assert.match(cssSource, /--pz-btn-highlight:/);
});

test("navigation buttons stay flat instead of becoming white pills", () => {
  const start = cssSource.indexOf(".pz-side-nav-item:not(.pz-side-nav-item-active)");
  assert.ok(start >= 0, "missing navigation exception rule");
  const block = cssSource.slice(start, cssSource.indexOf("}", start) + 1);
  assert.match(block, /\.pz-resource-center-sidebar-item:not\(\.pz-resource-center-sidebar-item-active\)/);
  assert.match(block, /\.pz-version-tab:not\(\.pz-version-tab-active\)/);
  assert.match(block, /\.pz-version-filter:not\(\.is-active\)/);
  assert.match(block, /background:\s*transparent !important/);
  assert.match(block, /backdrop-filter:\s*none !important/);
});

test("the button acrylic rule is written into the style guide", () => {
  assert.match(guideSource, /统一的\*\*亚克力底\*\*|统一带\*\*亚克力底\*\*/);
  assert.match(guideSource, /src\/theme\.ts/);
  assert.match(guideSource, /导航类按钮保持平铺/);
});

test("buttons sharing a row with inputs or dropdowns follow the 26px control height", () => {
  // 只匹配直接包含 input / 下拉触发器的容器；独立工具栏按钮保持 36px。
  assert.match(cssSource, /\.pz-app-shell :has\(> input:not\(\[type="checkbox"\]\)/);
  assert.match(cssSource, /:not\(\[type="color"\]\)\) > \.chakra-button,/);
  assert.match(cssSource, /\.pz-app-shell :has\(> \.chakra-menu__trigger\) > \.chakra-button\s*\{/s);

  const start = cssSource.search(/\.pz-app-shell :has\(> input/);
  const block = cssSource.slice(start, cssSource.indexOf("}", start) + 1);
  assert.match(block, /height:\s*var\(--pz-control-height\) !important/);
  assert.match(block, /min-height:\s*var\(--pz-control-height\) !important/);
  assert.match(block, /padding-block:\s*0 !important/);

  // 规范文字同步锁住，避免以后又回去写“普通按钮 30px”或逐行覆盖。
  assert.match(guideSource, /独立主操作 \/ 工具栏按钮保持 Chakra `size="sm"` 的 `36px`/);
  assert.match(guideSource, /与输入框 \/ 下拉同一行的操作按钮统一 `26px`/);
  assert.doesNotMatch(guideSource, /普通按钮默认高度为 `30px`/);
});
