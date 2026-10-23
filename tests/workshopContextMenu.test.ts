import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

test("workshop item context menu is portaled out of the scrolling result list", () => {
  const contextTriggerStart = appSource.indexOf("<Menu.ContextTrigger asChild>");
  const contextMenuStart = appSource.indexOf(
    'className="pz-workshop-context-menu"',
    contextTriggerStart,
  );
  const portalStart = appSource.lastIndexOf("<Portal>", contextMenuStart);
  const positionerStart = appSource.lastIndexOf(
    '<Menu.Positioner className="pz-menu-positioner">',
    contextMenuStart,
  );
  const portalEnd = appSource.indexOf("</Portal>", contextMenuStart);

  assert.ok(contextTriggerStart >= 0);
  assert.ok(contextMenuStart > contextTriggerStart);
  assert.ok(portalStart > contextTriggerStart, "Portal should wrap the context menu positioner");
  assert.ok(positionerStart > portalStart, "positioner should sit inside the Portal");
  assert.ok(portalEnd > contextMenuStart, "Portal should close after the context menu content");
});

test("workshop item context menu uses fixed positioning", () => {
  assert.match(
    appSource,
    /<Menu\.Root\s+key=\{item\.id\}\s+positioning=\{\{[^}]*strategy:\s*"fixed"/,
  );
});
