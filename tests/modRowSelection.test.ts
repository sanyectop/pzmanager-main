import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

test("uses the whole mod row as a checkbox without native checkbox controls", () => {
  assert.equal((appSource.match(/role="checkbox"/g) ?? []).length, 2);
  assert.equal((appSource.match(/aria-checked=\{selected\}/g) ?? []).length, 2);
  assert.doesNotMatch(appSource, /className="pz-mod-select"/);
  assert.match(appSource, /element\.classList\.toggle\("is-selected", nextSelected\)/);
  assert.match(appSource, /startTransition\(\(\) => \{\s*setSelectedLuaModKeys/);
  assert.match(appSource, /startTransition\(\(\) => \{\s*setSelectedJarModKeys/);
});
