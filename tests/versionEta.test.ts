import assert from "node:assert/strict";
import test from "node:test";

import {
  createVersionEtaTracker,
  formatVersionElapsed,
  formatVersionEta,
  updateVersionEta,
  type VersionOperationUpdate,
} from "../src/versioning.ts";

function progress(percent: number, step = 1): VersionOperationUpdate {
  return {
    operationId: 1,
    operation: "snapshot",
    status: "progress",
    phase: "copying",
    processedFiles: percent,
    totalFiles: 100,
    processedBytes: percent,
    totalBytes: 100,
    currentPath: null,
    message: null,
    error: null,
    step,
    steps: 3,
  };
}

test("eta accumulates a smoothed rate and reports remaining seconds", async () => {
  const { setTimeout: sleep } = await import("node:timers/promises");
  const tracker = createVersionEtaTracker();
  assert.equal(updateVersionEta(tracker, { ...progress(0), status: "started" }), null);

  // 每步之间隔 ~450ms、前进 10%：速率约 10%/450ms，全程应估在秒级而不是 null。
  assert.equal(updateVersionEta(tracker, progress(0)), null);
  await sleep(450);
  const eta1 = updateVersionEta(tracker, progress(10));
  assert.ok(eta1 !== null && eta1 > 0 && eta1 < 6 * 3600, `eta1=${eta1}`);
  await sleep(450);
  const eta2 = updateVersionEta(tracker, progress(20));
  assert.ok(eta2 !== null && eta2 > 0, `eta2=${eta2}`);
  // 稳定速率下第二次估算不会离谱地翻倍。
  assert.ok(eta2 < eta1! * 3, `eta2=${eta2} eta1=${eta1}`);
});

test("eta resets when the step changes or progress goes backwards", () => {
  const tracker = createVersionEtaTracker();
  tracker.startedAt = Date.now();
  tracker.lastAt = Date.now();
  tracker.lastPercent = 100;
  tracker.lastStep = 1;
  tracker.ratePerMs = 0.1;
  // 进入第 2 步：旧速率清零，先返回 null 重新积累样本。
  assert.equal(updateVersionEta(tracker, progress(5, 2)), null);
  assert.equal(tracker.ratePerMs, null);
  // 进度回退同理。
  tracker.lastStep = 2;
  tracker.lastPercent = 50;
  tracker.ratePerMs = 0.1;
  assert.equal(updateVersionEta(tracker, progress(10, 2)), null);
  assert.equal(tracker.ratePerMs, null);
});

test("displayed eta is averaged with previous estimates to damp jitter", () => {
  const tracker = createVersionEtaTracker();
  tracker.startedAt = Date.now();
  tracker.lastStep = 1;
  tracker.smoothedEtaSeconds = 300;

  // 稳定速率 20%/ms·千（0.02%/ms）下，剩余 40% 理论上只要 2 秒：
  // 但上一轮展示值是 300 秒，混合后应落到两者之间、靠近旧值。
  tracker.lastPercent = 50;
  tracker.lastAt = Date.now() - 500;
  const first = updateVersionEta(tracker, progress(60));
  assert.ok(first !== null && first > 2 && first < 300, `first=${first}`);

  // 连续同速率：展示值继续向新水平收敛，不会停在旧值也不会瞬间跳到底。
  tracker.lastPercent = 60;
  tracker.lastAt = Date.now() - 500;
  const second = updateVersionEta(tracker, progress(70));
  assert.ok(second !== null && second < first!, `second=${second} first=${first}`);
  assert.ok(second! > 2, `second=${second}`);
});

test("eta formatting covers seconds, minutes and the long tail", () => {
  assert.equal(formatVersionEta(null), null);
  assert.equal(formatVersionEta(0), null);
  assert.equal(formatVersionEta(30), "约 30 秒");
  assert.equal(formatVersionEta(120), "约 2 分钟");
  assert.equal(formatVersionEta(7200), "超过 1 小时");
});

test("elapsed formatting needs a start time", () => {
  assert.equal(formatVersionElapsed(null), null);
  const started = Date.now() - 65_000;
  const text = formatVersionElapsed(started)!;
  assert.match(text, /^1 分 \d+ 秒$/);
});
