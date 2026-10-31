import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const css = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
).replace(/\/\*[\s\S]*?\*\//g, "");

test("slider thumb and fill glide between detents instead of jumping", () => {
  // zag 把位置写进 root 上的自定义属性，过渡必须挂在消费这些变量的属性上。
  const range = css.match(/\.pz-memory-slider \[data-part="range"\]\s*\{([^}]*)\}/);
  assert.ok(range, "缺少滑块填充条规则");
  assert.match(range[1], /transition:[^;}]*left\s+var\(--pz-dur-fast\)/);
  assert.match(range[1], /transition:[^;}]*right\s+var\(--pz-dur-fast\)/);

  const thumb = css.match(/\.pz-memory-slider \[data-part="thumb"\]\s*\{([^}]*)\}/);
  assert.ok(thumb, "缺少滑块滑块头规则");
  assert.match(thumb[1], /transition:\s*inset-inline-start\s+var\(--pz-dur-fast\)/);

  // 减少动态效果时直接跳到目标档位。
  assert.match(
    css,
    /@media \(prefers-reduced-motion: reduce\)\s*\{[^@]*?\.pz-memory-slider \[data-part="thumb"\][^}]*?transition:\s*none;/,
  );
});
