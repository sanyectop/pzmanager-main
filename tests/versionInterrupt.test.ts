import assert from "node:assert/strict";
import test from "node:test";

import { appSource } from "./sourceText.ts";

test("version page operations expose an interrupt everywhere they are watched", () => {
  // 后端取消按钮统一叫「中断」：游戏版本页、模组版本页进度框、整合包保存按钮排、任务中心。
  // （快速启动弹窗的「取消」是另一回事，不在本断言范围。）
  // 直接挂在按钮上的 3 处（游戏版本页按钮排 / 模组分页页首 / 模组进度框），
  // 第 4 处在任务中心行内，走 onInterruptVersion 回调（下面单独断言）。
  const interruptButtons = appSource.match(/onClick=\{cancelVersionOperation\}/g) ?? [];
  assert.ok(interruptButtons.length >= 3, `中断按钮只有 ${interruptButtons.length} 处`);
  // 任务中心行内的中断只给进行中的版本操作，且由上层传入回调。
  assert.match(appSource, /onInterruptVersion=\{cancelVersionOperation\}/);
  assert.match(
    appSource,
    /task\.kind === "version-operation" && task\.status === "running" && onInterruptVersion/,
  );
});

test("version progress panels show elapsed time and an eta estimate", () => {
  // 两个进度面板（游戏版本 / 模组版本）都挂时间行；估算器在 versioning.ts。
  const timingUsages = appSource.match(/\{versionTimingLabel\((?:version|modVersion)Operation\.status\) \? \(/g) ?? [];
  assert.equal(timingUsages.length, 2, `时间行只挂了 ${timingUsages.length} 个面板`);
  assert.match(appSource, /const etaSeconds = updateVersionEta\(versionEtaTrackerRef\.current, update\);/);
  assert.equal(appSource.match(/updateVersionEta\(versionEtaTrackerRef\.current, update\)/g)?.length, 2);
  // 任务中心那行文案也带估算。
  assert.match(appSource, /预计剩余 \$\{formatVersionEta\(etaSeconds\)/);
});
