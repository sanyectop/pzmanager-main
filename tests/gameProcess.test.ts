import assert from "node:assert/strict";
import test from "node:test";
import { shouldResetGameWindow } from "../src/gameProcess.ts";

test("hides the stop button when the game window disappears", () => {
  assert.equal(shouldResetGameWindow(true, { running: false, windowReady: false }), true);
  assert.equal(shouldResetGameWindow(true, { running: true, windowReady: false }), true);
});

test("keeps the stop button while the game window is present", () => {
  assert.equal(shouldResetGameWindow(true, { running: true, windowReady: true }), false);
  assert.equal(shouldResetGameWindow(false, { running: true, windowReady: false }), false);
});
