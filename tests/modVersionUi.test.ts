import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  filterModVersionTargets,
  formatVersionDate,
  sortVersionsNewestFirst,
  versionDotSize,
  versionOrdinal,
  type ModVersionTarget,
  type VersionRecord,
} from "../src/versioning.ts";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
import { rustSource as backendSource } from "./sourceText.ts";

function target(overrides: Partial<ModVersionTarget> = {}): ModVersionTarget {
  return {
    key: "local-alpha",
    rootPath: "C:\\Zomboid\\mods\\Alpha",
    name: "Alpha",
    modId: "Alpha",
    source: "local",
    workshopId: null,
    hasStore: false,
    versionCount: 0,
    latestVersionId: null,
    latestLabel: null,
    latestCreatedAt: null,
    ...overrides,
  };
}

function record(overrides: Partial<VersionRecord> = {}): VersionRecord {
  return {
    id: "v1",
    parentId: null,
    steamBuildId: null,
    branch: "mods",
    baseType: "full",
    createdAt: 1_700_000_000,
    rootHash: "sha256:abcdef0123456789",
    metadataHash: "sha256:1111",
    fileCount: 3,
    totalBytes: 1024,
    label: "基准版本",
    changeLog: null,
    ...overrides,
  };
}

test("filters mod version targets by source, isolation and keyword", () => {
  const targets = [
    target({ key: "a", name: "Alpha", modId: "Alpha", source: "local", versionCount: 2 }),
    target({
      key: "b",
      name: "Bravo",
      modId: "bravo",
      rootPath: "H:\\SteamLibrary\\content\\Bravo",
      source: "workshop",
      workshopId: "1",
    }),
    target({
      key: "c",
      name: "Charlie",
      modId: "charlie",
      rootPath: "H:\\SteamLibrary\\content\\Charlie",
      source: "workshop",
      workshopId: "2",
      versionCount: 1,
    }),
  ];

  assert.equal(filterModVersionTargets(targets, "", "all").length, 3);
  assert.deepEqual(
    filterModVersionTargets(targets, "", "isolated").map((item) => item.key),
    ["a", "c"],
  );
  assert.deepEqual(
    filterModVersionTargets(targets, "", "workshop").map((item) => item.key),
    ["b", "c"],
  );
  assert.deepEqual(
    filterModVersionTargets(targets, "", "local").map((item) => item.key),
    ["a"],
  );
  assert.deepEqual(
    filterModVersionTargets(targets, "brav", "all").map((item) => item.key),
    ["b"],
  );
  assert.deepEqual(
    filterModVersionTargets(targets, "C:\\Zomboid", "all").map((item) => item.key),
    ["a"],
  );
});

test("timeline shows the newest version first with 1-based ordinals", () => {
  const versions = [
    record({ id: "v1", createdAt: 1 }),
    record({ id: "v2", createdAt: 2 }),
    record({ id: "v3", createdAt: 3 }),
  ];
  assert.deepEqual(
    sortVersionsNewestFirst(versions).map((item) => item.id),
    ["v3", "v2", "v1"],
  );
  // 不改入参
  assert.deepEqual(versions.map((item) => item.id), ["v1", "v2", "v3"]);
  assert.deepEqual([0, 1, 2].map(versionOrdinal), [1, 2, 3]);
});

test("timeline dots shrink with age but never below 10px", () => {
  assert.equal(versionDotSize(0), 18);
  assert.equal(versionDotSize(1), 17);
  assert.equal(versionDotSize(8), 10);
  assert.equal(versionDotSize(20), 10);
});

test("formats the timeline date box as year/month/day", () => {
  const stamp = Math.floor(new Date(2026, 9, 29, 13, 5).getTime() / 1000);
  assert.equal(formatVersionDate(stamp), "2026/10/29");
  assert.equal(formatVersionDate(0), "未知日期");
});

test("版本 page lists its sections in a left sidebar wired to versionTab", () => {
  assert.match(appSource, /const \[versionTab, setVersionTab\] = useState<VersionTab>\("game"\);/);
  // 六个分区：概览 / 模组版本 / 历史版本 / 支持库 / 光影包 / 截图。
  for (const [value, label] of [
    ["game", "概览"],
    ["mods", "模组版本"],
    ["history", "历史版本"],
    ["support", "支持库"],
    ["shaderpacks", "光影包"],
    ["shots", "截图"],
  ]) {
    assert.match(
      appSource,
      new RegExp(`\\{ value: "${value}", label: "${label}", icon: fa\\w+ \\}`),
    );
  }
  assert.match(appSource, /aria-current=\{versionTab === section\.value \? "page" : undefined\}/);
  assert.match(appSource, /className="pz-version-hub-sidebar"/);
  // 支持库 / 光影包 / 截图 复用原页面内容。
  assert.match(appSource, /versionTab === "support" \? \(\s*overviewContent/);
  assert.match(appSource, /versionTab === "shaderpacks" \? \(\s*shaderpacksContent/);
  assert.match(appSource, /:\s*\(\s*shotsContent\s*\)/);
});

test("game version history lives on its own tab", () => {
  const gameStart = appSource.indexOf("const gameVersionSettingsContent");
  const historyStart = appSource.indexOf("const versionHistorySettingsContent");
  const modStart = appSource.indexOf("const modVersionBusy");
  assert.ok(gameStart >= 0 && historyStart > gameStart && modStart > historyStart);

  const gameBlock = appSource.slice(gameStart, historyStart);
  const historyBlock = appSource.slice(historyStart, modStart);
  // 历史版本列表、逐个还原/删除和改动统计都在历史分页里，不再堆在游戏版本页底部。
  assert.doesNotMatch(gameBlock, /className="pz-version-list"/);
  assert.doesNotMatch(gameBlock, />历史版本</);
  assert.match(historyBlock, /className="pz-version-list"/);
  assert.match(historyBlock, /还原到这个版本/);
  assert.match(historyBlock, /pz-version-diff-button/);
  // 从历史页还原也要能看到后台进度。
  assert.match(historyBlock, /\{versionOperationProgressContent\}/);
});

test("game captures can optionally record mod versions (default on)", () => {
  assert.match(appSource, /const \[versionIncludeMods, setVersionIncludeMods\] = useState\(true\);/);
  assert.match(appSource, /aria-label="保存时同时备份模组"/);
  assert.match(appSource, /invoke\("save_version_include_mods", \{ enabled \}\)/);
  assert.match(appSource, /includeMods: versionIncludeMods/);
  assert.match(appSource, /restoreMods: true/);
  assert.match(backendSource, /version_include_mods/);
  assert.match(backendSource, /save_version_include_mods/);
  assert.match(backendSource, /fn record_mod_versions_for_game_version/);
  assert.match(backendSource, /fn restore_associated_mod_versions/);
  assert.match(backendSource, /versioning::attach_mod_versions\(game_store, game_version_id/);
});

test("mod version tab lists mods, expands inline and reports progress", () => {
  assert.match(appSource, /className="pz-version-mod-list"/);
  assert.match(appSource, /className=\{`pz-version-mod-row/);
  assert.match(appSource, />模组版本</);
  assert.match(appSource, /保存变化/);
  assert.match(appSource, /完整保存/);
  assert.match(appSource, /get_mod_version_status/);
  assert.match(appSource, /list_mod_version_targets/);
  assert.match(appSource, /delete_mod_version/);
  assert.match(appSource, /start_mod_version_snapshot/);
  assert.match(appSource, /start_mod_version_capture/);
  assert.match(appSource, /start_mod_version_restore/);
  assert.match(appSource, /start_mod_version_verify/);
});

test("mod progress events are handled on their own channel", () => {
  assert.match(appSource, /listen<ModVersionOperationUpdate>\("mod-version-progress"/);
  assert.match(appSource, /const rootPath = update\.rootPath \?\? null;/);
  assert.match(backendSource, /"mod-version-progress"/);
  assert.match(backendSource, /modSnapshot|modCapture|modRestore|modVerify/);
});

test("capture progress shows the operation counter after the percentage", () => {
  assert.match(
    appSource,
    /versionProgressPercent\(versionOperation\)\}%\{versionStepLabel\(versionOperation\)\}/,
  );
  assert.match(
    appSource,
    /versionProgressPercent\(modVersionOperation\)\}%\{versionStepLabel\(modVersionOperation\)\}/,
  );
  // 模组步骤里的计数是「第几个模组」，不是文件数，所以不显示「N / M 个文件」。
  assert.match(appSource, /versionOperation\.phase !== "mods" && versionOperation\.totalFiles > 0/);
  assert.match(backendSource, /fn version_step_for\(operation: &str, phase: &str\) -> u32/);
  assert.match(backendSource, /step: u32,\n\s+steps: u32,/);
});

test("mod rows expose 版本记录 in the context menu for both Lua and JAR", () => {
  assert.match(appSource, /<Text>版本记录<\/Text>/);
  assert.match(appSource, /onOpenVersionHistory=\{\(\) => openModVersionDialog\(mod\.path, mod\.name\)\}/);
  assert.match(appSource, /onOpenVersionHistory=\{\(\) => openModVersionDialog\(mod\.modRoot, mod\.id\)\}/);
  assert.match(backendSource, /mod_root: mod_root_for_info_path\(info_path\)/);
});

test("the timeline and dialog reuse the flat version page pieces", () => {
  assert.match(appSource, /function ModVersionTimeline\(/);
  assert.match(appSource, /className="pz-version-timeline"/);
  assert.match(appSource, /pz-version-timeline-dot/);
  assert.match(appSource, /className="pz-version-timeline-line"/);
  assert.match(appSource, /className="pz-version-timeline-date"/);
  assert.match(appSource, /function ModVersionDialog\(/);
  assert.match(appSource, /className="pz-resource-file-dialog pz-mod-version-dialog"/);
  assert.match(cssSource, /\.pz-version-timeline-date \{/);
  assert.match(cssSource, /\.pz-version-timeline-line \{/);
  assert.match(cssSource, /\.pz-mod-version-dialog \{/);
});

test("backend registers every mod version command", () => {
  for (const command of [
    "list_mod_version_targets",
    "get_mod_version_status",
    "start_mod_version_snapshot",
    "start_mod_version_capture",
    "start_mod_version_restore",
    "start_mod_version_verify",
    "delete_mod_version",
    "save_version_include_mods",
  ]) {
    assert.ok(
      backendSource.includes(`            ${command},`),
      `missing handler registration: ${command}`,
    );
  }
});

test("support / shaderpacks / shots are folded into the versions page", () => {
  // 顶栏不再有这三个独立入口。
  assert.doesNotMatch(appSource, /value: "overview", label: "支持库"/);
  assert.doesNotMatch(appSource, /value: "shots", label: "截图管理"/);
  assert.doesNotMatch(appSource, /value: "shaderpacks", label: "光影管理"/);
  // 旧入口统一重定向到版本页对应分区。
  assert.match(
    appSource,
    /if \(nextPage === "overview" \|\| nextPage === "shots" \|\| nextPage === "shaderpacks"\) \{\s*setVersionTab\(nextPage === "overview" \? "support" : nextPage\);\s*nextPage = "versions";/,
  );
  // 光影包拖放同时接受「版本 › 光影包」视图。
  assert.match(appSource, /activePage === "versions" && versionTab === "shaderpacks"/);
  // 媒体分区不触发版本状态刷新。
  assert.match(
    appSource,
    /if \(versionTab === "support" \|\| versionTab === "shaderpacks" \|\| versionTab === "shots"\) \{\s*return;\s*\}/,
  );
});
