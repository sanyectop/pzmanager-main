import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
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

  // 两条进度通道都同步到任务行
  const syncCalls = appSource.match(/applyVersionTaskProgress\(taskId, update\)/g) ?? [];
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
  assert.match(appSource, /taskCount=\{tasks\.filter\(\(task\) => task\.status === "running"\)\.length\}/);
  assert.match(appSource, /暂无任务。下载光影、保存\/还原版本、清理内存等后台操作会出现在这里。/);
});
