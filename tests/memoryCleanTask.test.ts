import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { appSource } from "./sourceText.ts";
import {
  MEMORY_CLEAN_NO_DECREASE_GRACE_MS,
  MEMORY_CLEAN_POLL_MS,
  MEMORY_CLEAN_PLATEAU_GRACE_MS,
  applyMemoryCleanSample,
  createMemoryCleanTracker,
  memoryCleanFinishedDetail,
  memoryCleanProgressDetail,
  shouldFinishMemoryClean,
} from "../src/memoryClean.ts";

function sample(usedMb: number) {
  return { usedMb, usedPercent: 50 };
}

test("内存占用按每秒一次的节奏检测", () => {
  assert.equal(MEMORY_CLEAN_POLL_MS, 1000);
});

test("已清理量按点击前基准计算，并保留观察到的峰值", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(7500));
  assert.equal(tracker.peakFreedMb, 500);
  assert.equal(tracker.sawDecrease, true);
  assert.equal(tracker.done, false);

  tracker = applyMemoryCleanSample(tracker, sample(7000));
  assert.equal(tracker.peakFreedMb, 1000);
  assert.equal(tracker.done, false);

  // 物理内存不再减少、开始回升：任务完成，"已清理"仍记峰值。
  tracker = applyMemoryCleanSample(tracker, sample(7050));
  assert.equal(tracker.done, true);
  assert.equal(tracker.peakFreedMb, 1000);
});

test("内存没有先下降时不会因为波动判定完成", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(8100));
  assert.equal(tracker.sawDecrease, false);
  assert.equal(tracker.done, false);
});

test("后端命令返回前不按回升收尾", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(7000));
  tracker = applyMemoryCleanSample(tracker, sample(7100));
  assert.equal(tracker.done, true);

  assert.equal(
    shouldFinishMemoryClean(tracker, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: 300,
      cleanReturned: false,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(tracker, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: 300,
      cleanReturned: true,
    }),
    true,
  );
});

test("观察窗口兜底：一直没下降或下降后平稳都会收尾", () => {
  const flat = createMemoryCleanTracker(8000);
  assert.equal(
    shouldFinishMemoryClean(flat, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: MEMORY_CLEAN_NO_DECREASE_GRACE_MS - 1,
      cleanReturned: true,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(flat, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: MEMORY_CLEAN_NO_DECREASE_GRACE_MS,
      cleanReturned: true,
    }),
    true,
  );

  let dropped = createMemoryCleanTracker(8000);
  dropped = applyMemoryCleanSample(dropped, sample(7000));
  assert.equal(
    shouldFinishMemoryClean(dropped, {
      startedAt: 0,
      lastDecreaseAt: 1000,
      now: 1000 + MEMORY_CLEAN_PLATEAU_GRACE_MS - 1,
      cleanReturned: true,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(dropped, {
      startedAt: 0,
      lastDecreaseAt: 1000,
      now: 1000 + MEMORY_CLEAN_PLATEAU_GRACE_MS,
      cleanReturned: true,
    }),
    true,
  );
});

test("任务行文案带上当前已清理的 MB", () => {
  assert.equal(memoryCleanProgressDetail(0), "正在清理…");
  assert.equal(memoryCleanProgressDetail(128), "当前已清理 128 MB");

  assert.equal(memoryCleanFinishedDetail(0, 10), "清理完成，内存占用没有明显下降");
  assert.equal(memoryCleanFinishedDetail(256, 12), "清理完成，共释放 256 MB（12 个进程）");
  assert.equal(memoryCleanFinishedDetail(256, 0), "清理完成，共释放 256 MB");
});

test("设置里的清理按钮改为登记任务中心任务并采样物理内存", () => {
  assert.match(appSource, /kind: "memory-clean"/);
  assert.match(appSource, /const taskId = "memory-task-" \+ \(\+\+taskIdRef\.current\)/);
  assert.match(appSource, /applyMemoryCleanSample\(tracker/);
  assert.match(appSource, /setTaskPanelOpen\(true\)/);
  assert.match(appSource, /memoryCleanProgressDetail\(tracker\.peakFreedMb\)/);
  assert.match(appSource, /memoryCleanFinishedDetail\(freedMb, report\.trimmed\)/);
});
