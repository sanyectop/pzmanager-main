import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const guideSource = readFileSync(
  fileURLToPath(new URL("../docs/ui-style-guide.md", import.meta.url)),
  "utf8",
);

test("the unified design doc matches the CSS token source", () => {
  // 标题栏与控件高度：文档和令牌必须同步，避免再出现 32px / 30px 的旧值。
  assert.match(cssSource, /--pz-titlebar-height:\s*50px;/);
  assert.match(guideSource, /自绘标题栏高度 \| `50px`/);
  assert.match(cssSource, /--pz-control-height:\s*26px;/);
  assert.match(guideSource, /`26px`，令牌 `--pz-control-height`/);

  const radii: Array<[string, string]> = [
    ["xs", "0.25rem"],
    ["sm", "0.4rem"],
    ["md", "0.45rem"],
    ["lg", "0.7rem"],
    ["pill", "999px"],
  ];
  for (const [name, value] of radii) {
    const escaped = value.replace(".", "\\.");
    assert.match(cssSource, new RegExp(`--pz-radius-${name}:\\s*${escaped};`));
    assert.match(guideSource, new RegExp(`--pz-radius-${name}: ${escaped}`));
  }

  const durations: Array<[string, string]> = [
    ["tap", "120ms"],
    ["fast", "150ms"],
    ["base", "180ms"],
    ["slow", "260ms"],
    ["slower", "360ms"],
    ["exit", "170ms"],
  ];
  for (const [name, value] of durations) {
    assert.match(cssSource, new RegExp(`--pz-dur-${name}:\\s*${value};`));
    assert.match(guideSource, new RegExp(`--pz-dur-${name}: ${value}`));
  }
});

test("the doc records the agreed design decisions", () => {
  assert.match(guideSource, /2026-10-30 统一裁决/);
  assert.match(guideSource, /百宝箱 \| \*\*单列全宽\*\*卡片/);
  assert.match(guideSource, /独立主操作 \/ 工具栏按钮保持 `36px`/);
  assert.match(guideSource, /与输入框 \/ 下拉同一行的操作按钮统一 `26px`/);
});
