import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("long version-page explanations live in hint dots", () => {
  assert.match(appSource, /function HintDot\(\{ label, text \}/);
  assert.match(
    appSource,
    /<Tooltip\.Root[\s\S]*?positioning=\{\{ placement: "top", flip: true, shift: 8, overflowPadding: 8 \}\}/,
  );
  assert.match(appSource, /<Tooltip\.Trigger asChild>/);
  assert.match(appSource, /className="pz-hint-tooltip"/);
  // 游戏页 5 处 + 模组页 2 处 + 「版本记录」模态框 1 处
  const usages = appSource.match(/<HintDot\b/g) ?? [];
  assert.ok(usages.length >= 8, `expected at least 8 HintDot usages, got ${usages.length}`);
});

test("long descriptions are no longer rendered as visible paragraphs", () => {
  assert.doesNotMatch(appSource, /<Text[^>]*>\s*同时读写多个文件/);
  assert.doesNotMatch(appSource, /<Text[^>]*>\s*保存游戏版本时，把当前启用的模组也各存一份/);
  assert.doesNotMatch(appSource, /<Text[^>]*>\s*「完整保存」把当前文件全部复制一份/);
  assert.doesNotMatch(appSource, /<Text[^>]*>\s*把整个游戏目录存成一个版本/);
  // 原文案完整保存在提示内容里
  assert.match(appSource, /text="同时读写多个文件，保存和还原会快一些/);
  assert.match(appSource, /text=\{"「完整保存」把当前文件全部复制一份/);
  assert.match(appSource, /text=\{"把整个游戏目录存成一个版本/);
});

test("hint dots are 1rem circles and tooltips stay inside the window", () => {
  const dot = cssSource.slice(
    cssSource.indexOf(".pz-hint-dot {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-hint-dot {")) + 1,
  );
  assert.match(dot, /width:\s*1rem/);
  assert.match(dot, /height:\s*1rem/);
  assert.match(dot, /border-radius:\s*var\(--pz-radius-pill\)/);

  const tooltip = cssSource.slice(
    cssSource.indexOf(".pz-hint-tooltip {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-hint-tooltip {")) + 1,
  );
  // 宽度夹在窗口内；换行保留，长路径不会撑出屏幕。
  assert.match(tooltip, /max-width:\s*min\(22rem, calc\(100vw - 2rem\)\)/);
  assert.match(tooltip, /white-space:\s*pre-line/);
  assert.match(tooltip, /overflow-wrap:\s*anywhere/);
});

test("hint tooltip never intercepts the pointer (no show/hide loop)", () => {
  // 顶部空间不足时 flip 会让提示压住问号；提示层若截走指针会陷入显示/隐藏循环。
  // 纯文本提示直接关闭指针事件。
  assert.match(
    cssSource,
    /\.pz-hint-positioner,\s*\.pz-hint-tooltip \{\s*pointer-events: none !important;/,
  );
  // HintDot 的定位器不再复用下拉菜单的共享类（那个类需要响应指针）。
  assert.match(appSource, /className="pz-hint-positioner"/);
  assert.doesNotMatch(
    appSource,
    /<Tooltip\.Positioner className="pz-menu-positioner">\s*<Tooltip\.Content className="pz-hint-tooltip">/,
  );
});
