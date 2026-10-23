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
const backendSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);

test("mod presets are registered as backend commands and invoked from the mods sidebar", () => {
  for (const command of ["list_mod_presets", "save_mod_preset", "apply_mod_preset", "delete_mod_preset"]) {
    assert.match(backendSource, new RegExp(`fn ${command}\\(`));
    assert.match(backendSource, new RegExp(`^\\s+${command},$`, "m"));
    assert.match(appSource, new RegExp(`invoke(<[^>]*>)?\\("${command}"`));
  }
  assert.match(appSource, /className="pz-mod-sidebar-section-title">预设</);
});

test("mod presets store the Lua load order and JAR enabled state", () => {
  assert.match(backendSource, /let lua_mod_ids = read_default_mod_ids\(&default_path\)\?;/);
  assert.match(backendSource, /write_default_mod_ids\(&default_path, &lua_ids\)\?;/);
  assert.match(backendSource, /let jar_changed = apply_jar_mod_state_to_roots\(&roots, &state\)\?;/);
});

test("mod rows never select text and support drag-to-select", () => {
  assert.match(cssSource, /\.pz-mod-row\[role="checkbox"\]\s*\{[^}]*user-select:\s*none;/s);
  // 拖动勾选把行的当前选中状态传进去，回移时用于恢复拖动前的状态。
  assert.match(appSource, /extendRowDrag\("lua", key, selected\)/);
  assert.match(appSource, /extendRowDrag\("jar", modKey, selected\)/);
});

test("batch enable and disable buttons stay visible without a selection", () => {
  assert.doesNotMatch(appSource, /\{selectedCount > 0 && \(\s*<>\s*<Button\s*colorPalette="green"/s);
  assert.match(appSource, /disabled=\{busy \|\| selectedCount === 0\}/);
});

test("preset box scrolls vertically instead of growing the sidebar", () => {
  assert.match(cssSource, /\.pz-mod-preset-list\s*\{[^}]*max-height:\s*16rem;[^}]*overflow-y:\s*auto;/s);
  assert.match(appSource, /className="pz-mod-preset-list"/);
});

test("preset box stays a vertical scroll list on narrow screens", () => {
  assert.match(cssSource, /\.pz-resource-center-sidebar \.pz-mod-preset-list\s*\{[^}]*flex-direction:\s*column;[^}]*overflow-x:\s*visible;/s);
});

test("settings sub-pages scroll inside their own content area", () => {
  assert.match(cssSource, /\.pz-settings-center > \.pz-resource-center-main\s*\{[^}]*overflow-y:\s*auto;/s);
  assert.match(cssSource, /\.pz-settings-center > \.pz-resource-center-main\s*\{\s*overflow-y:\s*visible;\s*\}/);
  assert.match(appSource, /<Box className="pz-resource-center pz-settings-center">/);
});

test("mod management sidebar scrolls its filter and preset list on desktop", () => {
  assert.match(cssSource, /\.pz-mod-management \.pz-resource-center-sidebar\s*\{[^}]*min-height:\s*0;[^}]*overflow-y:\s*auto;/s);
  assert.match(cssSource, /\.pz-mod-management \.pz-resource-center-sidebar\s*\{\s*flex-basis:\s*auto;\s*width:\s*auto;\s*overflow-y:\s*visible;\s*\}/s);
});
