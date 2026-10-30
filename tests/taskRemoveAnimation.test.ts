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

test("task removal waits for the exit animation before dropping the task", () => {
  // 与 .pz-task-slot.is-closing 的动画时长（--pz-dur-base）一致
  assert.match(appSource, /const TASK_REMOVE_EXIT_MS = 180;/);
  assert.match(appSource, /function scheduleTaskRemoval\(ids: string\[\]\)/);
  assert.match(
    appSource,
    /setTaskClosingIds\(\(current\) => \[\.\.\.new Set\(\[\.\.\.current, \.\.\.ids\]\)\]\);/,
  );
  // 先挂 closing 标记，定时器到点才真正删任务
  const scheduleBlock = appSource.slice(
    appSource.indexOf("function scheduleTaskRemoval("),
    appSource.indexOf("function removeTask("),
  );
  assert.match(scheduleBlock, /window\.setTimeout\(\(\) => \{/);
  assert.match(scheduleBlock, /TASK_REMOVE_EXIT_MS\);/);
  assert.match(scheduleBlock, /setTasks\(\(current\) => current\.filter\(\(task\) => !ids\.includes\(task\.id\)\)\);/);
  assert.ok(
    scheduleBlock.indexOf("setTaskClosingIds") < scheduleBlock.indexOf("setTasks"),
    "closing flag must be set before the task leaves the list",
  );

  // 单条移除和「清除已完成」共用同一套退场流程
  assert.match(appSource, /function removeTask\(id: string\) \{\s*scheduleTaskRemoval\(\[id\]\);/);
  assert.match(
    appSource,
    /function clearFinishedTasks\(\) \{\s*scheduleTaskRemoval\(\s*tasks\.filter\(\(task\) => task\.status !== "running"\)\.map\(\(task\) => task\.id\)\s*\);/,
  );
});

test("task rows are wrapped in a closing slot", () => {
  assert.match(appSource, /const \[taskClosingIds, setTaskClosingIds\] = useState<string\[\]>\(\[\]\);/);
  assert.match(appSource, /closingTaskIds=\{taskClosingIds\}/);
  assert.match(appSource, /closingTaskIds: string\[\];/);
  assert.match(appSource, /const closing = closingTaskIds\.includes\(task\.id\);/);
  assert.match(
    appSource,
    /className=\{\["pz-task-slot", closing \? "is-closing" : ""\]\.filter\(Boolean\)\.join\(" "\)\}/,
  );
  // 动画期间移除按钮不可重复点
  assert.match(
    appSource,
    /<Button disabled=\{closing\} onClick=\{\(\) => onRemove\(task\.id\)\} size="xs" variant="ghost">移除<\/Button>/,
  );
});

test("task removal reuses the notification hide animation", () => {
  // 复用通知退场用的 keyframes，并同步收拢外层高度
  const slotBlock = cssSource.slice(
    cssSource.indexOf(".pz-task-slot {"),
    cssSource.indexOf(".pz-task-running {"),
  );
  assert.match(slotBlock, /grid-template-rows: 1fr;/);
  assert.match(slotBlock, /transition: grid-template-rows var\(--pz-dur-base\) ease-in;/);
  assert.match(slotBlock, /\.pz-task-slot\.is-closing \{\s*grid-template-rows: 0fr;/);
  assert.match(
    slotBlock,
    /\.pz-task-slot\.is-closing > \.pz-task-row \{[^}]*animation: pz-toast-exit var\(--pz-dur-base\) ease-in both;/,
  );
  assert.match(slotBlock, /pointer-events: none;/);
  // 行的上下 padding/描边会把 0fr 轨道顶在约 20px，退场时一起收到 0 才能真正压扁
  assert.match(slotBlock, /\.pz-task-slot\.is-closing > \.pz-task-row \{[^}]*padding-block: 0;/);
  assert.match(slotBlock, /\.pz-task-slot\.is-closing > \.pz-task-row \{[^}]*border-block-width: 0;/);
  // 通知的退场动画确实存在，任务行只是复用而不是复制一份 keyframes
  assert.match(cssSource, /@keyframes pz-toast-exit \{/);
  assert.match(cssSource, /\.pz-toast-closing \{\s*animation: pz-toast-exit var\(--pz-dur-base\) ease-in both;/);
});
