import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

test("shows only the Project Zomboid Workshop title", () => {
  assert.match(appSource, /Project Zomboid 创意工坊/);
  assert.doesNotMatch(appSource, /浏览 Steam 创意工坊模组，点击资源行打开详情页。/);
});
