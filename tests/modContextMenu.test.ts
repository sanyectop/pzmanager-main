import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { modWorkshopLink } from "../src/modWorkshop.ts";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

function slice(source: string, start: string, end: string): string {
  const from = source.indexOf(start);
  const to = source.indexOf(end, from + start.length);
  assert.ok(from >= 0, `missing start marker: ${start}`);
  assert.ok(to > from, `missing end marker after ${start}: ${end}`);
  return source.slice(from, to);
}

const menuComponent = slice(appSource, "function ModRowContextMenu({", "function SleepingModRow({");
const luaPage = slice(appSource, "const luaModsContent = (", "const jarModsContent = (");
const jarPage = slice(appSource, "const jarModsContent = (", "const modsContent = (");

test("mod workshop links are built from the mod's own workshop id", () => {
  assert.deepEqual(modWorkshopLink("3498201101"), {
    url: "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=3498201101",
    fallbackUrl: "https://steamcommunity.com/sharedfiles/filedetails/?id=3498201101",
  });
  // JAR 模组的 ID 是数字，Lua 模组是字符串，两者拼出的链接必须一致。
  assert.deepEqual(modWorkshopLink(3498201101), modWorkshopLink("3498201101"));
});

test("local mods and unusable ids produce no workshop link", () => {
  assert.equal(modWorkshopLink(null), null);
  assert.equal(modWorkshopLink(undefined), null);
  assert.equal(modWorkshopLink(""), null);
  assert.equal(modWorkshopLink("0"), null);
  assert.equal(modWorkshopLink("not-a-number"), null);
});

test("the mod context menu hangs off the whole row and is portaled", () => {
  assert.match(menuComponent, /<Menu\.Root[^>]*positioning=\{\{[^}]*strategy: "fixed"/);
  assert.match(menuComponent, /<Menu\.ContextTrigger asChild>\{children\}<\/Menu\.ContextTrigger>/);

  const menuContentStart = menuComponent.indexOf('className="pz-mod-context-menu"');
  const portalStart = menuComponent.lastIndexOf("<Portal>", menuContentStart);
  const positionerStart = menuComponent.lastIndexOf('<Menu.Positioner className="pz-menu-positioner">', menuContentStart);
  const portalEnd = menuComponent.indexOf("</Portal>", menuContentStart);

  assert.ok(menuContentStart >= 0);
  assert.ok(portalStart >= 0, "menu content should be portaled");
  assert.ok(positionerStart > portalStart, "positioner should sit inside the Portal");
  assert.ok(portalEnd > menuContentStart, "Portal should close after the menu content");
});

test("the mod context menu offers enable, disable and open-workshop", () => {
  assert.match(menuComponent, /<Text>启用<\/Text>/);
  assert.match(menuComponent, /<Text>禁用<\/Text>/);
  assert.match(menuComponent, /<Text>创意工坊打开<\/Text>/);
});

test("menu entries reflect the row's own state instead of toggling blindly", () => {
  assert.match(menuComponent, /disabled=\{busy \|\| enabled \|\| !canEnable\}/);
  assert.match(menuComponent, /disabled=\{busy \|\| !enabled\}/);
  // 本地模组没有创意工坊页面，菜单项置灰而不是打开一个死链接。
  assert.match(menuComponent, /disabled=\{workshopLink === null\}/);
});

test("Lua mod rows wire the menu to their own toggle and workshop id", () => {
  assert.match(luaPage, /<ModRowContextMenu/);
  assert.match(luaPage, /workshopId=\{mod\.workshopId\}/);
  assert.match(luaPage, /onEnable=\{\(\) => void setLuaModsEnabled\(\[mod\], true\)\}/);
  assert.match(luaPage, /onDisable=\{\(\) => void setLuaModsEnabled\(\[mod\], false\)\}/);
  // 非 B42 的 Lua 模组启动器不能启用，菜单必须跟开关保持同一条限制。
  assert.match(luaPage, /canEnable=\{mod\.hasB42\}/);
});

test("JAR mod rows wire the menu to their own decision and workshop id", () => {
  assert.match(jarPage, /<ModRowContextMenu/);
  assert.match(jarPage, /workshopId=\{mod\.workshopId\}/);
  assert.match(jarPage, /onEnable=\{\(\) => void setJarModDecisions\(\[mod\], true\)\}/);
  assert.match(jarPage, /onDisable=\{\(\) => void setJarModDecisions\(\[mod\], false\)\}/);
});

test("opening a workshop page still falls back to https when Steam is unavailable", () => {
  const opener = slice(appSource, "async function openWorkshopLink(", "async function openWorkshopItem(");
  assert.match(opener, /await openUrl\(link\.url\)/);
  assert.match(opener, /await openUrl\(link\.fallbackUrl\)/);
});

test("the mod context menu reuses the acrylic menu surface", () => {
  assert.match(cssSource, /\.pz-mod-context-menu \{\s*min-width: 170px;/);
  assert.match(cssSource, /\.pz-workshop-tag-menu-content,\s*\.pz-workshop-context-menu,\s*\.pz-mod-context-menu \{/);
});
