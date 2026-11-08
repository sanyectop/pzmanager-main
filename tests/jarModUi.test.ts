import assert from "node:assert/strict";
import test from "node:test";
import {
  analyzeJarModConflicts,
  filterJarModsByInjectionItem,
  jarModMatchesInjectionItem,
  jarScanStatusLabel,
  toggleJarModExpansion,
} from "../src/jarModUi.ts";
import { appSource, cssSource } from "./sourceText.ts";

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

function injectionFixture(id: string, fields: Partial<Parameters<typeof jarModMatchesInjectionItem>[0]>) {
  return {
    id,
    classReferences: [],
    patchClasses: [],
    luaClasses: [],
    patchMethods: [],
    ...fields,
  };
}

test("matches injection items across game class references and mod class names", () => {
  const mod = injectionFixture("DGVViewpointEyes", {
    classReferences: [
      "zombie.util.list.PZArrayList",
      "zombie.network.fields.hit.HitInfo",
      "fmod.fmod.FMODSoundEmitter",
    ],
    patchClasses: ["dgveyes.Patch_Foo"],
    luaClasses: ["dgveyes.DGVEReach"],
    patchMethods: [{ annotation: "Inject", methodName: "onEnter" }],
  });

  // 点分 / 斜杠 / 大小写：三种输入形式都能命中。
  assert.equal(jarModMatchesInjectionItem(mod, "zombie.util.list.PZArrayList"), true);
  assert.equal(jarModMatchesInjectionItem(mod, "zombie/util/list/PZArrayList"), true);
  assert.equal(jarModMatchesInjectionItem(mod, "  PZARRAYLIST  "), true);
  assert.equal(jarModMatchesInjectionItem(mod, "fmod.FMODSoundEmitter"), true);
  // 模组自身类名 / 方法名也在搜索范围（诊断可能点名 dgveyes.DGVEReach）。
  assert.equal(jarModMatchesInjectionItem(mod, "dgveyes.DGVEReach"), true);
  assert.equal(jarModMatchesInjectionItem(mod, "onEnter"), true);
  // 未引用的类不命中。
  assert.equal(jarModMatchesInjectionItem(mod, "zombie.ai.StateMachine"), false);
});

test("empty injection query keeps every mod", () => {
  const mods = [
    injectionFixture("A", { classReferences: ["zombie.util.list.PZArrayList"] }),
    injectionFixture("B", {}),
  ];
  assert.deepEqual(filterJarModsByInjectionItem(mods, ""), mods);
  assert.deepEqual(filterJarModsByInjectionItem(mods, "   "), mods);
  assert.deepEqual(
    filterJarModsByInjectionItem(mods, "PZArrayList").map((mod) => mod.id),
    ["A"],
  );
});

test("injection search box lives inside the conflict panel and filters both views", () => {
  const searchIndex = appSource.indexOf('aria-label="搜索注入项"');
  const panelIndex = appSource.indexOf("pz-jar-conflict-panel${jarConflictExpanded");
  const detailsIndex = appSource.indexOf('className="pz-jar-conflict-details-shell"');
  assert.ok(searchIndex >= 0, "应存在注入项搜索框");
  assert.ok(panelIndex >= 0 && detailsIndex > panelIndex, "应存在冲突分析面板及其折叠区");
  assert.ok(searchIndex > panelIndex, "搜索框应在冲突分析面板内部");
  assert.ok(searchIndex < detailsIndex, "搜索框应在折叠区之前：面板收起时也能用");
  assert.match(appSource, /className="pz-jar-conflict-search"/);
  // 面板按未过滤的模组列表显示：搜索无命中时搜索框不会跟面板一起消失，用户清得掉关键词。
  const panelGate = appSource.slice(Math.max(0, panelIndex - 260), panelIndex);
  assert.ok(panelGate.includes("(jarMods?.length ?? 0) > 0"), panelGate);
  assert.ok(!panelGate.includes("filteredJarMods"), panelGate);
  // 列表与冲突对都要按注入项过滤。
  assert.match(appSource, /filterJarModsByInjectionItem\(/);
  assert.match(appSource, /visibleJarModConflicts/);
  // 详情行展示游戏类引用，命中结果可核对。
  assert.match(appSource, /游戏类引用：/);
});

test("conflict rows keep their wrapped height inside the scroll list", () => {
  // 列表是带 max-height 的 flex 列：行若参与收缩会被压成单行高、换行文字压到下一行。
  const rowRule = cssSource.slice(
    cssSource.indexOf(".pz-jar-conflict-row {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-jar-conflict-row {")),
  );
  assert.match(rowRule, /flex: 0 0 auto/, rowRule);
  // 文字列可缩到 0 宽度才换行，长模组名不会把行撑宽。
  assert.match(cssSource, /\.pz-jar-conflict-row > p \{[^}]*min-width: 0/s);
  // 风险徽标不被压缩截字。
  assert.match(cssSource, /\.pz-jar-conflict-row > span \{[^}]*flex: 0 0 auto/s);
});
