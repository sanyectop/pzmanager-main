import assert from "node:assert/strict";
import test from "node:test";
import {
  analyzeJarModConflicts,
  jarScanStatusLabel,
  toggleJarModExpansion,
} from "../src/jarModUi.ts";

test("formats the four-frame jar scan status animation", () => {
  assert.equal(jarScanStatusLabel(0, 1, 4), "后台扫描中：1 / 4");
  assert.equal(jarScanStatusLabel(1, 1, 4), "后台扫描中.：1 / 4");
  assert.equal(jarScanStatusLabel(2, 1, 4), "后台扫描中..：1 / 4");
  assert.equal(jarScanStatusLabel(3, 1, 4), "后台扫描中...：1 / 4");
  assert.equal(jarScanStatusLabel(4, 1, 4), "后台扫描中：1 / 4");
});

test("toggles jar mod expansion without changing other rows", () => {
  const expanded = new Set(["first"]);

  const opened = toggleJarModExpansion(expanded, "second");
  assert.deepEqual([...opened].sort(), ["first", "second"]);
  assert.deepEqual([...expanded], ["first"]);

  const closed = toggleJarModExpansion(opened, "first");
  assert.deepEqual([...closed], ["second"]);
});

test("analyzes approved jar mods by shared patch method risk", () => {
  const conflicts = analyzeJarModConflicts([
    {
      id: "Overwrite Mod",
      decision: true,
      patchMethods: [{ annotation: "Overwrite", methodName: "render" }],
    },
    {
      id: "Second Overwrite Mod",
      decision: true,
      patchMethods: [{ annotation: "Overwrite", methodName: "render" }],
    },
    {
      id: "Redirect Mod",
      decision: true,
      patchMethods: [{ annotation: "Redirect", methodName: "render" }],
    },
    {
      id: "Modify Constant Mod",
      decision: true,
      patchMethods: [{ annotation: "ModifyConstant", methodName: "tick" }],
    },
    {
      id: "Second Modify Constant Mod",
      decision: true,
      patchMethods: [{ annotation: "ModifyConstant", methodName: "tick" }],
    },
    {
      id: "Second Redirect Mod",
      decision: true,
      patchMethods: [{ annotation: "Redirect", methodName: "render" }],
    },
    {
      id: "Inject Mod",
      decision: true,
      patchMethods: [{ annotation: "Inject", methodName: "tick" }],
    },
    {
      id: "Second Inject Mod",
      decision: true,
      patchMethods: [{ annotation: "Inject", methodName: "tick" }],
    },
    {
      id: "Unapproved Mod",
      decision: false,
      patchMethods: [{ annotation: "Overwrite", methodName: "render" }],
    },
    {
      id: "Pending Mod",
      decision: null,
      patchMethods: [{ annotation: "Inject", methodName: "tick" }],
    },
  ]);

  assert.deepEqual(conflicts, [
    {
      risk: "high",
      annotation: "Overwrite",
      methodName: "render",
      modIds: ["Overwrite Mod", "Second Overwrite Mod"],
    },
    {
      risk: "mediumHigh",
      annotation: "Redirect",
      methodName: "render",
      modIds: ["Redirect Mod", "Second Redirect Mod"],
    },
    {
      risk: "mediumHigh",
      annotation: "ModifyConstant",
      methodName: "tick",
      modIds: ["Modify Constant Mod", "Second Modify Constant Mod"],
    },
    {
      risk: "mediumLow",
      annotation: "Inject",
      methodName: "tick",
      modIds: ["Inject Mod", "Second Inject Mod"],
    },
  ]);
});
