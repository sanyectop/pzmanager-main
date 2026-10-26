import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { MIN_MEMORY_MB, defaultMemoryLimitMb } from "../src/memorySettings.ts";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

test("defaults to half of physical memory rounded to whole GB", () => {
  // 用户给的实际例子:23.8 GB 物理 → 12 GB 默认。
  assert.equal(defaultMemoryLimitMb(Math.round(23.8 * 1024)), 12 * 1024);
  assert.equal(defaultMemoryLimitMb(16 * 1024), 8 * 1024);
  assert.equal(defaultMemoryLimitMb(8 * 1024), 4 * 1024);
  assert.equal(defaultMemoryLimitMb(4 * 1024), 2 * 1024);
});

test("rounds half-way values up and never goes below the minimum", () => {
  // 15 GB 物理:一半 7.5 GB,四舍五入到 8 GB。
  assert.equal(defaultMemoryLimitMb(15 * 1024), 8 * 1024);
  assert.equal(defaultMemoryLimitMb(2 * 1024), MIN_MEMORY_MB);
  assert.equal(defaultMemoryLimitMb(1024), MIN_MEMORY_MB);
});

test("applies the half-physical default in the app", () => {
  assert.match(appSource, /defaultMemoryLimitMb\(totalMemoryMb\)/);
  // 旧的 8 GB 推荐默认(含 UI 提示)应彻底退场。
  assert.doesNotMatch(appSource, /RECOMMENDED_MEMORY_MB/);
});
