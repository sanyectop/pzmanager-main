import assert from "node:assert/strict";
import { setTimeout as sleep } from "node:timers/promises";
import test from "node:test";

import {
  createVersionEtaTracker,
  formatVersionElapsed,
  formatVersionEta,
  updateVersionEta,
  versionProgressFraction,
  type VersionEtaTracker,
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

/** 按新实现需要的形状播种跟踪器：本步骤 500ms 前从 percent 起步。 */
function seededTracker(percent: number, step = 1): VersionEtaTracker {
  const tracker = createVersionEtaTracker();
  const at = Date.now() - 500;
  tracker.startedAt = at;
  tracker.stepStartAt = at;
  tracker.stepStartPercent = percent;
  tracker.lastPercent = percent;
  tracker.lastStep = step;
  return tracker;
}

test("估算每 0.5s 才结算一次，两次结算之间展示值保持不变", async () => {
  const tracker = createVersionEtaTracker();
  assert.equal(updateVersionEta(tracker, { ...progress(0), status: "started" }), null);

  // 第一个结算点：跨度刚过下限，能给出粗估算。
  await sleep(600);
  const first = updateVersionEta(tracker, progress(10));
  assert.ok(first !== null && first > 0, `首个结算应有估算 first=${first}`);

  // 结算后 100ms 的新事件：百分比动了也不重算，展示值原样保持。
  await sleep(100);
  const cached = updateVersionEta(tracker, progress(12));
  assert.equal(cached, first, "未到结算点应原样返回上次展示值");

  // 到下一个结算点：正常更新。
  await sleep(600);
  const second = updateVersionEta(tracker, progress(20));
  assert.ok(second !== null && second > 0);
});

test("速率是窗口真平均：稳定推进下估算贴住真值", async () => {
  const tracker = createVersionEtaTracker();
  updateVersionEta(tracker, { ...progress(0), status: "started" });
  // 以约 500ms/10% 的稳定速度推进 6 个结算点：真值 = 剩余% / (20%/s)。
  for (let index = 1; index <= 6; index += 1) {
    await sleep(500);
    const eta = updateVersionEta(tracker, progress(index * 10));
    if (index >= 2) {
      const trueEtaSeconds = (100 - index * 10) / 20;
      assert.ok(
        eta !== null && Math.abs(eta - trueEtaSeconds) <= 1.5,
        `第 ${index} 个结算点估算 ${eta} 偏离真值 ${trueEtaSeconds}`,
      );
    }
  }
});

test("停滞如实计入：整体平均下剩余时间持续变长且不会乱跳", async () => {
  const tracker = createVersionEtaTracker();
  updateVersionEta(tracker, { ...progress(0), status: "started" });
  await sleep(600);
  assert.ok(updateVersionEta(tracker, progress(50)) !== null);
  const movingEta = tracker.etaSeconds;

  // 停滞窗口（进度不动）：分母（经过时间）继续涨、分子不动，估算单调变长。
  let previousEta = movingEta ?? 0;
  for (let index = 0; index < 4; index += 1) {
    await sleep(520);
    const stalledEta = updateVersionEta(tracker, progress(50));
    assert.ok(stalledEta !== null, `停滞窗口 ${index + 1} 不应失去估算`);
    assert.ok(stalledEta >= previousEta, `停滞不应让估算变短: ${stalledEta} < ${previousEta}`);
    previousEta = stalledEta;
  }
  // 长期停滞也不会崩：估算持续变长（整体平均的诚实行为），直到被 6h 上限挡住。
  await sleep(2000);
  const later = updateVersionEta(tracker, progress(50));
  assert.ok(later !== null && later >= previousEta, `长停滞应继续变长: ${later} < ${previousEta}`);
});

test("估算用未取整百分比：整数进度不动也能看到细微推进", () => {
  // 12% → 12.4%：进度条纹丝不动，估算仍能算出剩余时间。
  const tracker = seededTracker(12);
  const eta = updateVersionEta(tracker, {
    ...progress(12),
    processedBytes: 1_240,
    totalBytes: 10_000,
  });
  assert.ok(eta !== null && eta > 0, "未取整的推进应当产出估算");
});

test("换步骤或进度回退时重置，从头累计", () => {
  const tracker = seededTracker(50);
  assert.ok(updateVersionEta(tracker, progress(55)) !== null);
  // 进入下一步：旧平均作废，先返回 null，起点重置为本步骤当前位置。
  assert.equal(updateVersionEta(tracker, progress(5, 2)), null);
  assert.equal(tracker.etaSeconds, null);
  assert.equal(tracker.stepStartPercent, 5);
  // 进度回退同理：整体平均从头累计。
  tracker.stepStartAt = Date.now() - 500;
  tracker.stepStartPercent = 50;
  tracker.lastPercent = 50;
  assert.equal(updateVersionEta(tracker, progress(10)), null);
  assert.equal(tracker.stepStartPercent, 10);
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

test("fraction helper keeps the raw percent for the eta", () => {
  assert.equal(versionProgressFraction(progress(50)), 50);
  // 字节进度不取整：5001/10000 → 50.01。
  const raw = versionProgressFraction({ ...progress(0), processedBytes: 5001, totalBytes: 10_000 });
  assert.ok(Math.abs(raw - 50.01) < 1e-9, `raw=${raw}`);
  assert.equal(versionProgressFraction(null), 0);
});
