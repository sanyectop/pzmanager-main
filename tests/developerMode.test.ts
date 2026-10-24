import assert from "node:assert/strict";
import test from "node:test";
import { conciseNotification, shouldShowNotification } from "../src/developerMode.ts";

test("keeps detailed notifications in developer mode", () => {
  assert.equal(shouldShowNotification("success", true), true);
  assert.equal(shouldShowNotification("info", true), true);
  assert.equal(shouldShowNotification("error", true), true);
  assert.equal(conciseNotification("first line\nsecond line", true), "first line\nsecond line");
});

test("keeps only errors and shortens them outside developer mode", () => {
  assert.equal(shouldShowNotification("success", false), false);
  assert.equal(shouldShowNotification("info", false), false);
  assert.equal(shouldShowNotification("error", false), true);

  const longMessage = `${"错误详情".repeat(40)}\n更多技术信息`;
  const concise = conciseNotification(longMessage, false);
  assert.equal(concise.endsWith("..."), true);
  assert.equal(concise.length, 140);
  assert.equal(concise.includes("更多技术信息"), false);
});
