import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";

const tasksSource = readFileSync(
  fileURLToPath(new URL("../src/components/tasks.tsx", import.meta.url)),
  "utf8",
);
const shellSource = readFileSync(
  fileURLToPath(new URL("../src/components/shell.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("version operations are recorded in the task center", () => {
  // 任务类型：光影下载 + 版本操作
  assert.match(appSource, /kind: "shader-download" \| "version-operation"/);
  assert.match(appSource, /function startVersionTask\(/);
  assert.match(appSource, /function applyVersionTaskProgress\(/);

  // 游戏与模组操作在发起时各建一条任务
  assert.match(appSource, /startVersionTask\(`游戏版本 · \$\{label\}`, null\)/);
  assert.match(appSource, /startVersionTask\(`\$\{modName\} · \$\{label\}`, rootPath\)/);

  // 两条进度通道都同步到任务行（第三参是时间估算，可以省略）
  const syncCalls = appSource.match(/applyVersionTaskProgress\(taskId, update(?:, etaSeconds)?\)/g) ?? [];
  assert.equal(syncCalls.length, 2, "game and mod listeners must both update their task");
});

test("version task rows show percent, step counter and file counts", () => {
  assert.match(appSource, /task\.kind === "version-operation"/);
  assert.match(appSource, /task\.step != null && task\.steps != null && task\.steps > 1/);
  assert.match(appSource, /task\.doneFiles != null && task\.totalFiles != null/);
  // 已完成/失败时把结果消息写在说明行
  assert.match(appSource, /update\.error \?\? update\.message \?\?/);
});

test("only downloads offer retry and saved-path text", () => {
  assert.match(appSource, /task\.kind === "shader-download" && task\.status === "error"/);
  assert.match(appSource, /task\.kind === "shader-download" && task\.status === "success" && task\.savedPath/);
});

test("the task center badge counts running version operations", () => {
  // 计数提升为派生变量 taskRunningCount（徽标飞入动画的触发源也用它）。
  assert.match(appSource, /const taskRunningCount = tasks\.filter\(\(task\) => task\.status === "running"\)\.length;/);
  assert.match(appSource, /taskCount=\{taskRunningCount\}/);
  // 飞行期间真实徽标以「预留态」挂在按钮上（不占布局），落地转正。
  assert.match(appSource, /badgeCount=\{taskRunningCount\}/);
  assert.match(appSource, /badgeFlightId=\{taskFlight \? taskFlight\.key : null\}/);
  // 兜底：动画意外中断 1.5s 后强制收尾。
  assert.match(appSource, /taskFlightFallbackRef\.current = window\.setTimeout\(\(\) => setTaskFlight\(null\), 1500\)/);
  assert.match(appSource, /暂无任务。下载光影、保存\/还原版本、清理内存等后台操作会出现在这里。/);
});

test("new tasks fly from the pointer to the task center with a parabolic arc", () => {
  // 飞入徽标组件：起点圆形淡出 + 徽标本体。
  assert.match(appSource, /TaskArrivalFlight/);
  // 横纵都是匀加速直线（初速 0，位移 ∝ t²）：两轴同曲线 → 一条带二次缓入的
  // 直线，用合成器动画（element.animate）执行，主线程忙也不会卡顿。
  assert.match(tasksSource, /export const TASK_FLIGHT_DURATION_MS = 500;/);
  assert.match(tasksSource, /easing: "cubic-bezier\(0\.55, 0, 1, 0\.45\)", fill: "both"/);
  assert.match(tasksSource, /animation\.finished/);
  // 飞行期间按钮宽度不变（徽标绝对定位隐藏），结束前 0.2s 渐变到目标宽度。
  assert.match(shellSource, /pz-task-trigger-badge\.is-reserved/);
  assert.match(shellSource, /const TRIGGER_EXPAND_LEAD_MS = 200;/);
  assert.match(shellSource, /TASK_FLIGHT_DURATION_MS - TRIGGER_EXPAND_LEAD_MS/);
  assert.match(shellSource, /button\.style\.transition = "width 0\.2s cubic-bezier\(0\.33, 1, 0\.68, 1\)"/);
  // CSS：起点圆形淡出 + 徽标弹跳 + 任务行入场 + 预留态徽标。
  assert.match(cssSource, /@keyframes pz-task-flight-ripple/);
  assert.match(cssSource, /@keyframes pz-badge-pop/);
  assert.match(cssSource, /@keyframes pz-task-row-in/);
  assert.match(cssSource, /\.pz-task-trigger-badge\.is-reserved \{/);
});
