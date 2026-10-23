import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("keeps launch settings and details mounted for animated open and close states", () => {
  assert.match(appSource, /pz-launch-popover-shell[^"]*pz-launch-settings-popover/);
  assert.match(appSource, /pz-launch-popover-shell[^"]*pz-launch-details-popover/);
  assert.match(cssSource, /\.pz-launch-popover-shell\s*\{[\s\S]*?grid-template-rows:\s*0fr;[\s\S]*?opacity:\s*0;/);
  assert.match(cssSource, /\.pz-launch-popover-shell\.is-open\s*\{[\s\S]*?grid-template-rows:\s*1fr;[\s\S]*?opacity:\s*1;/);
});
