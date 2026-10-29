import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const cssCode = cssSource.replace(/\/\*[\s\S]*?\*\//g, "");

function ruleBlock(selector: string): string {
  const start = cssCode.indexOf(selector);
  assert.ok(start >= 0, `missing ${selector}`);
  const open = cssCode.indexOf("{", start);
  assert.ok(open >= 0, `missing opening brace for ${selector}`);
  let depth = 0;
  for (let index = open; index < cssCode.length; index += 1) {
    if (cssCode[index] === "{") depth += 1;
    else if (cssCode[index] === "}") {
      depth -= 1;
      if (depth === 0) return cssCode.slice(open + 1, index);
    }
  }
  throw new Error(`unclosed block for ${selector}`);
}

test("version settings page stays a flat settings page instead of a floating card", () => {
  const version = ruleBlock(".pz-version-settings-panel");
  assert.match(version, /max-width:\s*64rem/);
  // 设置子页和「通用」一样平铺：不再有卡面底色、圆角、阴影和背景模糊。
  assert.doesNotMatch(version, /background(-color)?:/);
  assert.doesNotMatch(version, /border(-radius)?:/);
  assert.doesNotMatch(version, /box-shadow:/);
  assert.doesNotMatch(version, /backdrop-filter:/);
  assert.equal(
    cssCode.includes(".pz-version-settings-panel::after"),
    false,
    "版本页不该再有噪点浮层",
  );
});

test("toasts use the menu acrylic recipe", () => {
  // 注意先出现的是 .pz-toast-viewport，必须用带空格的选择器精确命中 .pz-toast 本身。
  const toast = ruleBlock(".pz-toast {");
  assert.match(toast, /background-color:\s*var\(--pz-menu-tint\)/);
  assert.match(toast, /background-image:\s*var\(--pz-menu-noise-image\)/);
  assert.match(
    toast,
    /backdrop-filter:\s*blur\(var\(--pz-menu-blur\)\)\s*saturate\(var\(--pz-menu-saturate\)\)/,
  );
  assert.match(toast, /var\(--pz-menu-highlight\)/);
  assert.match(toast, /inset 0 1px 0/);
});
