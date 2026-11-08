import assert from "node:assert/strict";
import test from "node:test";
import {
  LAUNCH_START_TIMEOUT_MS,
  launchTimeoutMessage,
  shouldBreakLaunchWait,
  shouldResetGameWindow,
} from "../src/gameProcess.ts";

test("hides the stop button when the game window disappears", () => {
  assert.equal(shouldResetGameWindow(true, { running: false, windowReady: false }), true);
  assert.equal(shouldResetGameWindow(true, { running: true, windowReady: false }), true);
});

test("keeps the stop button while the game window is present", () => {
  assert.equal(shouldResetGameWindow(true, { running: true, windowReady: true }), false);
  assert.equal(shouldResetGameWindow(false, { running: true, windowReady: false }), false);
});

test("launch wait breaks only after the timeout with no game process", () => {
  // 15 秒内没见过游戏进程 → 熔断
  assert.equal(shouldBreakLaunchWait({ sawRunning: false, elapsedMs: 15_000 }), true);
  assert.equal(shouldBreakLaunchWait({ sawRunning: false, elapsedMs: 40_000 }), true);
  // 还没到点：继续等
  assert.equal(shouldBreakLaunchWait({ sawRunning: false, elapsedMs: 14_999 }), false);
  assert.equal(shouldBreakLaunchWait({ sawRunning: false, elapsedMs: 0 }), false);
  // 见过进程（哪怕后来退了）不熔断，交给原有的"游戏退出"分支处理
  assert.equal(shouldBreakLaunchWait({ sawRunning: true, elapsedMs: 60_000 }), false);
  // 自定义阈值
  assert.equal(
    shouldBreakLaunchWait({ sawRunning: false, elapsedMs: 5_000, timeoutMs: 5_000 }),
    true,
  );
});

test("launch timeout message points at the right culprit", () => {
  const steamDown = launchTimeoutMessage(false);
  assert.match(steamDown, /没有拉起 Steam/);
  assert.match(steamDown, /15 秒/);
  const steamUp = launchTimeoutMessage(true);
  assert.match(steamUp, /没有拉起游戏进程/);
  assert.match(steamUp, /Steam 正在运行/);
  assert.notEqual(steamDown, steamUp);
});

test("launch timeout default is 15 seconds", () => {
  assert.equal(LAUNCH_START_TIMEOUT_MS, 15_000);
});
