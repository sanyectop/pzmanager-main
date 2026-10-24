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

test("uses a controlled jar mod expansion class instead of native details state", () => {
  assert.match(appSource, /expandedJarModKeys/);
  assert.doesNotMatch(appSource, /<details className="pz-jar-mod-row"/);
  assert.doesNotMatch(cssSource, /\.pz-jar-mod-row\[open\]/);
  assert.match(cssSource, /\.pz-jar-mod-row-expanded \.pz-jar-mod-details-shell/);
});

test("aligns jar mod approval and workshop badges on the summary right", () => {
  assert.match(appSource, /className="pz-jar-mod-summary-content"/);
  assert.match(appSource, /className="pz-jar-mod-badges"/);
  assert.match(
    cssSource,
    /\.pz-jar-mod-badges\s*\{[\s\S]*?justify-content:\s*flex-end;[\s\S]*?text-align:\s*right;/,
  );
});
