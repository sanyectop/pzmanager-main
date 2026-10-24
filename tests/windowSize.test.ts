import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const config = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("../src-tauri/tauri.conf.json", import.meta.url)),
    "utf8",
  ),
) as {
  app: {
    windows: Array<{
      minWidth: number;
    }>;
  };
};

test("sets the minimum window width to 840 DIP", () => {
  assert.equal(config.app.windows[0]?.minWidth, 840);
});
