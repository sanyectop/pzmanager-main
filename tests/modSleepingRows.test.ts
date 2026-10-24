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

test("sleeps mod rows outside the scroll viewport", () => {
  assert.match(appSource, /function SleepingModRow\(/);
  assert.match(appSource, /new IntersectionObserver\(/);
  assert.match(appSource, /rootMargin:\s*"480px 0px"/);
  assert.match(appSource, /awake \? render\(\) : <Box className="pz-mod-sleep-placeholder" \/>/);
  assert.equal((appSource.match(/<SleepingModRow/g) ?? []).length, 2);
});

test("keeps a measured spacer while a row is sleeping", () => {
  assert.match(appSource, /setSleepHeight\(shellRef\.current\.offsetHeight\)/);
  assert.match(
    appSource,
    /style=\{!awake && sleepHeight !== null \? \{ height: `\$\{sleepHeight\}px` \} : undefined\}/,
  );
  assert.match(cssSource, /\.pz-mod-sleep-placeholder\s*\{[^}]*min-height:\s*2\.35rem/s);
});
