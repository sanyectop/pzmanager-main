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

test("keeps the launch workshop details and toggle in one continuous control", () => {
  const panelStart = appSource.indexOf("className={`pz-launch-workshop-info");
  const panelEnd = appSource.indexOf("</Box>", appSource.indexOf("</Button>", panelStart));
  const panel = appSource.slice(panelStart, panelEnd);

  assert.ok(panelStart >= 0);
  assert.match(panel, /pz-launch-workshop-info-details/);
  assert.match(panel, /pz-launch-workshop-info-toggle/);
  assert.ok(
    panel.indexOf("pz-launch-workshop-info-details") <
      panel.indexOf("pz-launch-workshop-info-toggle"),
  );
});

test("styles the launch workshop panel and toggle as one capsule", () => {
  assert.match(
    cssSource,
    /\.pz-launch-workshop-info\s*\{[^}]*display:\s*flex;[^}]*flex-direction:\s*row;[^}]*gap:\s*0;/s,
  );
  assert.match(
    cssSource,
    /\.pz-launch-workshop-info-toggle\s*\{[^}]*order:\s*1;/s,
  );
  assert.match(
    cssSource,
    /\.pz-launch-workshop-info-details\s*\{[^}]*order:\s*2;[^}]*max-width:\s*0;/s,
  );
  assert.match(
    cssSource,
    /\.pz-launch-workshop-info\.is-open \.pz-launch-workshop-info-toggle\s*\{[^}]*border-right:\s*0 !important;[^}]*border-radius:\s*0 !important;/s,
  );
  assert.match(
    cssSource,
    /\.pz-launch-workshop-info\.is-open \.pz-launch-workshop-info-details\s*\{[^}]*max-width:\s*min\(14rem/s,
  );
});
