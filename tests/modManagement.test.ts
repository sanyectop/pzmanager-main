import assert from "node:assert/strict";
import test from "node:test";
import {
  filterJarMods,
  filterLuaMods,
  invertSelection,
  mergeItemsPreservingOrder,
  setSelectionForKeys,
  sortLuaModsForDisplay,
  toggleSelection,
  type LuaModFilter,
} from "../src/modManagement.ts";

test("sorts enabled Lua mods first without disturbing load order", () => {
  const mods = [
    { name: "Disabled A", enabled: false, loadOrder: null, hasB42: true },
    { name: "Enabled B", enabled: true, loadOrder: 2, hasB42: true },
    { name: "Enabled A", enabled: true, loadOrder: 1, hasB42: true },
  ];

  assert.deepEqual(
    sortLuaModsForDisplay(mods).map((mod) => mod.name),
    ["Enabled A", "Enabled B", "Disabled A"],
  );
});

test("updates Lua mod state while preserving the current visible order", () => {
  const current = [
    { key: "a", enabled: false },
    { key: "b", enabled: true },
    { key: "c", enabled: false },
  ];
  const latest = [
    { key: "b", enabled: true },
    { key: "a", enabled: true },
    { key: "d", enabled: true },
    { key: "c", enabled: false },
  ];

  assert.deepEqual(
    mergeItemsPreservingOrder(current, latest, (item) => item.key),
    [
      { key: "a", enabled: true },
      { key: "b", enabled: true },
      { key: "c", enabled: false },
      { key: "d", enabled: true },
    ],
  );
});

test("combines status and B42 Lua mod filters", () => {
  const mods = [
    { name: "Enabled B42", enabled: true, hasB42: true },
    { name: "Enabled Legacy", enabled: true, hasB42: false },
    { name: "Disabled B42", enabled: false, hasB42: true },
  ];
  const filters = new Set<LuaModFilter>(["enabled", "b42"]);

  assert.deepEqual(
    filterLuaMods(mods, filters).map((mod) => mod.name),
    ["Enabled B42"],
  );
});

test("filters JAR mods by enabled state only", () => {
  const mods = [
    { id: "enabled", decision: true, disabled: false },
    { id: "disabled", decision: false, disabled: false },
    { id: "file-disabled", decision: true, disabled: true },
    { id: "pending", decision: null, disabled: false },
  ];

  assert.deepEqual(
    filterJarMods(mods, "enabled").map((mod) => mod.id),
    ["enabled"],
  );
  assert.deepEqual(
    filterJarMods(mods, "disabled").map((mod) => mod.id),
    ["disabled", "file-disabled", "pending"],
  );
  assert.deepEqual(
    filterJarMods(mods, "all").map((mod) => mod.id),
    ["enabled", "disabled", "file-disabled", "pending"],
  );
});

test("supports PCL2 style select, clear and invert operations", () => {
  const selected = new Set<string>(["a"]);

  assert.deepEqual([...toggleSelection(selected, "b")], ["a", "b"]);
  assert.deepEqual([...setSelectionForKeys(selected, ["a", "b"], true)], ["a", "b"]);
  assert.deepEqual([...setSelectionForKeys(selected, ["a"], false)], []);
  assert.deepEqual([...invertSelection(selected, ["a", "b"])], ["b"]);
});
