import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { sanitizeSavedTasks, TASK_HISTORY_CAP } from "../src/taskHistory.ts";
import { appSource, rustSource as backendSource } from "./sourceText.ts";

function validTask(overrides: Record<string, unknown> = {}) {
  return {
    id: "version-task-3",
    kind: "version-operation",
    title: "保存版本",
    detail: "基准版本",
    status: "success",
    downloaded: 0,
    total: null,
    createdAt: 1700000000000,
    finishedAt: 1700000006000,
    error: null,
    savedPath: null,
    request: null,
    ...overrides,
  };
}

test("sanitize keeps valid tasks and drops broken entries", () => {
  const tasks = sanitizeSavedTasks([
    validTask(),
    "not an object",
    null,
    { id: "", kind: "memory-clean" },
    { id: "t2", kind: "unknown-kind", status: "success" },
    { id: "t3", kind: "memory-clean", status: "deleted" },
  ]);
  assert.equal(tasks.length, 1);
  assert.equal(tasks[0].id, "version-task-3");
  assert.equal(tasks[0].status, "success");
  assert.equal(tasks[0].downloaded, 0);
});

test("interrupted running tasks become errors with a clear reason", () => {
  const tasks = sanitizeSavedTasks([
    validTask({ id: "version-task-9", status: "running", finishedAt: null }),
    validTask({
      id: "shader-task-10",
      kind: "shader-download",
      status: "running",
      request: { url: "https://x/z.zip", fallbackUrls: [], fileName: "z.zip", directory: "D:\\dl", threads: 8 },
    }),
  ]);
  assert.equal(tasks.length, 2);
  // 还在 running 的任务在应用退出时必然已被杀：统一标成失败 + 明确原因。
  assert.ok(tasks.every((task) => task.status === "error"));
  assert.ok(tasks.every((task) => task.error === "应用退出时任务被中断。"));
  assert.ok(tasks.every((task) => task.finishedAt !== null));
  // 光影下载的 request 保留：重启后「重试」按钮仍然可用。
  assert.equal(tasks[1].request?.url, "https://x/z.zip");
});

test("sanitize fills defaults and caps the history", () => {
  const many = Array.from({ length: TASK_HISTORY_CAP + 50 }, (_, index) =>
    validTask({ id: `memory-task-${index}`, createdAt: index }),
  );
  const tasks = sanitizeSavedTasks(many);
  assert.equal(tasks.length, TASK_HISTORY_CAP);
  // 缺 title 的条目回落「未命名任务」，缺 detail 回落空串。
  const bare = sanitizeSavedTasks([{ id: "memory-task-1", kind: "memory-clean", status: "success" }]);
  assert.equal(bare[0].title, "未命名任务");
  assert.equal(bare[0].detail, "");
  // 非数字 createdAt 回落到当前时间（只要求是有限数字）。
  const weird = sanitizeSavedTasks([validTask({ createdAt: "oops" })]);
  assert.ok(Number.isFinite(weird[0].createdAt));
  // 非数组输入一律空列表。
  assert.deepEqual(sanitizeSavedTasks(null), []);
  assert.deepEqual(sanitizeSavedTasks("nope"), []);
});

test("app loads history on startup and saves tasks debounced", () => {
  assert.match(appSource, /invoke<unknown\[\]>\("load_saved_tasks"\)/);
  assert.match(appSource, /invoke\("save_tasks", \{ tasks: tasks\.slice\(0, TASK_HISTORY_CAP\) \}\)/);
  // 首次装载完成前禁止落盘（防抖回调里的守卫），防止空列表覆盖历史。
  assert.match(appSource, /if \(!tasksLoadedRef\.current\) \{\s*return;\s*\}/);
  // 装载历史后把任务 id 计数器拨到已见最大号之后，避免新旧任务撞 id。
  assert.match(appSource, /taskIdRef\.current = Math\.max\(taskIdRef\.current, Number\(match\[1\]\)\)/);
});

test("backend persists tasks.json atomically with a history cap", () => {
  assert.match(backendSource, /pub\(crate\) const TASK_HISTORY_CAP: usize = 200;/);
  assert.match(backendSource, /fn load_saved_tasks\(\) -> Result<Vec<serde_json::Value>, String>/);
  assert.match(backendSource, /fn save_tasks\(tasks: Vec<serde_json::Value>\) -> Result<\(\), String>/);
  // 原子写：先临时文件再改名。
  assert.match(backendSource, /with_extension\("json\.tmp"\)/);
  for (const command of ["load_saved_tasks", "save_tasks"]) {
    assert.ok(
      backendSource.includes(`            ${command},`),
      `missing handler registration: ${command}`,
    );
  }
});
