import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
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

test("startup polling survives the launch gap and only stops after the game was seen", () => {
  // 11-06 回归：点下启动的第一拍 Steam 还没拉起游戏（running=false），轮询被
  // 「游戏退出」分支误停 —— 窗口永远检测不到，启动计时不会弹出。守卫必须基于
  // 「本轮启动真的见过游戏进程」，且没见过时要有超时兜底而不是无限空转。
  assert.match(appSource, /let sawRunning = false;/);
  assert.match(appSource, /sawRunning = true;/);
  assert.match(
    appSource,
    /if \(!status\.running && sawRunning && !gameWindowReady\) \{/,
  );
  // 「还没见过进程」的正常启动间隙里不得停轮询（旧的误停分支不应存在）。
  assert.doesNotMatch(appSource, /if \(!status\.running && !gameWindowReady\) \{/);
  // Steam 启动失败/被拦时的兜底：15 秒没见过进程就熔断（中断等待并提示，可立刻重试）。
  assert.match(appSource, /shouldBreakLaunchWait\(\{/);
  assert.match(appSource, /sawRunning,/);
  assert.match(appSource, /elapsedMs: Date\.now\(\) - watchStart,/);
  // 熔断后要能重试：停掉轮询并弹错误提示（带 Steam 是否在跑的差异化文案）。
  assert.match(appSource, /launchTimeoutMessage\(steamRunning\)/);
  assert.match(appSource, /steam_process_running/);
});

test("the overlay session event is an authoritative window-detected trigger", () => {
  // 11-06 用户反馈"进程出来了但计时没弹"的第二层保险：后端覆盖层会话检测到窗口
  // （injecting / loading 阶段）直接起表 —— 它驱动真正的 DLL 注入，比前端轮询可靠。
  assert.match(
    appSource,
    /update\.phase === "injecting" \|\| update\.phase === "loading"/,
  );
  assert.match(appSource, /!gameWindowReadyRef\.current/);
  // 轮询的 false→true 跳变同样走 ref，避免与会话事件重复起表把起点往后推。
  assert.match(appSource, /if \(status\.windowReady && !gameWindowReadyRef\.current\) \{/);
  // 所有的 gameWindowReady 重置点都要同步 ref，否则第二次启动计时失效。
  assert.match(appSource, /gameWindowReadyRef\.current = false;/);
});
