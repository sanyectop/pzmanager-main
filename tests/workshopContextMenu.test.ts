import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
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

test("workshop rows open the in-window detail by default", () => {
  const rowStart = appSource.indexOf('className="pz-workshop-list-item"');
  assert.ok(rowStart >= 0);
  const rowBlock = appSource.slice(rowStart, appSource.indexOf("</Menu.ContextTrigger>", rowStart));
  assert.match(rowBlock, /onClick=\{\(\) => void openWorkshopItemInWindow\(item\)\}/);
  assert.match(
    rowBlock,
    /if \(event\.key === "Enter" \|\| event\.key === " "\) \{[\s\S]*?void openWorkshopItemInWindow\(item\);/,
  );
  // 行本身不再走 Steam，只有按钮和右键菜单保留外部打开。
  assert.doesNotMatch(rowBlock.split("<Button")[0], /void openWorkshopItem\(item\.id/);
});

test("a square share button on the workshop row opens Steam", () => {
  const buttonStart = appSource.indexOf('className="pz-workshop-share-button"');
  assert.ok(buttonStart >= 0);
  const buttonBlock = appSource.slice(
    appSource.lastIndexOf("<Button", buttonStart),
    appSource.indexOf("</Button>", buttonStart),
  );
  assert.match(buttonBlock, /event\.stopPropagation\(\)/);
  assert.match(buttonBlock, /void openWorkshopItem\(item\.id, item\.fileUrl\)/);
  // 图标是斜向右上的箭头（↗），不是三点连线的分享图标，也不是先上后右的折线。
  assert.match(buttonBlock, /className="pz-workshop-share-icon"/);
  assert.match(buttonBlock, /d="M3\.4 12\.6 12\.8 3\.2"/);
  assert.match(buttonBlock, /d="M6\.8 3\.2H12\.8V9\.2"/);
  assert.doesNotMatch(buttonBlock, /d="M4\.2 13\.4V3\.6H12"/);
  assert.doesNotMatch(buttonBlock, /default-share-icon|faShareNodes/);
  assert.match(buttonBlock, /aria-label=\{`在 Steam 中打开 \$\{item\.title \|\| "未命名模组"\}`\}/);

  const rule = cssSource.slice(
    cssSource.indexOf(".pz-workshop-share-button {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-workshop-share-button {")) + 1,
  );
  assert.match(rule, /width:\s*var\(--pz-control-height\)\s*!important/);
  assert.match(rule, /height:\s*var\(--pz-control-height\)\s*!important/);
  assert.match(rule, /border-radius:\s*var\(--pz-radius-sm\)\s*!important/);

  const iconRule = cssSource.slice(
    cssSource.indexOf(".pz-workshop-share-icon {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-workshop-share-icon {")) + 1,
  );
  assert.match(iconRule, /width:\s*0\.95rem/);
  assert.match(iconRule, /height:\s*0\.95rem/);
});
