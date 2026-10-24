import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("keeps the startup timing on its own row under the stop button subtitle", () => {
  const shellStart = appSource.indexOf('className="pz-launch-card-shell"');
  const stopButtonStart = appSource.indexOf("pz-launch-stop-button", shellStart);
  const timingStart = appSource.indexOf("pz-launch-startup-timing", shellStart);

  assert.ok(shellStart >= 0);
  assert.ok(stopButtonStart > shellStart);
  // 计时文案在卡片内、停止按钮之后，是独立的一行；塞进按钮的居中栈里就会和副标题同排。
  assert.ok(timingStart > stopButtonStart);
  assert.equal(appSource.indexOf("</Button>", stopButtonStart) < timingStart, true);
});

test("reserves the timing row inside the shared launch button height", () => {
  // 两个按钮共用同一高度，底部多留一行给计时文案，文案出现/消失都不推动按钮。
  assert.match(
    cssSource,
    /\.pz-launch-card-button\s*\{[\s\S]*?min-height:\s*6\.25rem !important;[\s\S]*?padding:\s*0\.85rem 1\.15rem 1\.25rem !important;/,
  );
});

test("centers the timing caption in the reserved row instead of the card corner", () => {
  const rule = cssSource.match(/\.pz-launch-startup-timing\s*\{[\s\S]*?\n\}/)?.[0] ?? "";

  assert.notEqual(rule, "");
  assert.match(rule, /position:\s*absolute;/);
  assert.match(rule, /left:\s*0\.7rem;/);
  assert.match(rule, /right:\s*0\.7rem;/);
  assert.match(rule, /bottom:\s*0\.4rem;/);
  assert.match(rule, /text-align:\s*center;/);
  assert.match(rule, /line-height:\s*1\.1rem;/);
  assert.match(rule, /white-space:\s*nowrap;/);
  // 数字用等宽字形，秒数跳变时文案不会左右抖动。
  assert.match(rule, /font-variant-numeric:\s*tabular-nums;/);
});
