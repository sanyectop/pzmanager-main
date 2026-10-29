import assert from "node:assert/strict";
import test from "node:test";
import {
  formatVersionBytes,
  shortVersionHash,
  versionBaseTypeLabel,
  versionChangeSummary,
  versionDeltaChainDepth,
  versionMatchLabel,
  versionPhaseLabel,
  versionProgressPercent,
  versionLabelById,
  type VersionChangeLog,
  type VersionOperationUpdate,
  type VersionRecord,
} from "../src/versioning.ts";

function record(overrides: Partial<VersionRecord> = {}): VersionRecord {
  return {
    id: "v1",
    parentId: null,
    steamBuildId: "100",
    branch: "stable",
    baseType: "full",
    createdAt: 1_700_000_000,
    rootHash: "sha256:abcdef0123456789",
    metadataHash: "sha256:1111",
    fileCount: 10,
    totalBytes: 1024,
    label: "基准版本",
    changeLog: null,
    ...overrides,
  };
}

function changeLog(overrides: Partial<VersionChangeLog> = {}): VersionChangeLog {
  return {
    from: "v1",
    to: "v2",
    fromBuildId: "100",
    toBuildId: "200",
    added: [],
    removed: [],
    modified: [],
    ...overrides,
  };
}

function progress(overrides: Partial<VersionOperationUpdate> = {}): VersionOperationUpdate {
  return {
    operationId: 1,
    operation: "snapshot",
    status: "progress",
    phase: "copying",
    processedFiles: 5,
    totalFiles: 10,
    processedBytes: 50,
    totalBytes: 100,
    currentPath: null,
    message: null,
    error: null,
    ...overrides,
  };
}

test("formats version sizes in binary units", () => {
  assert.equal(formatVersionBytes(0), "0 B");
  assert.equal(formatVersionBytes(512), "512 B");
  assert.equal(formatVersionBytes(1024), "1.0 KB");
  assert.equal(formatVersionBytes(1536), "1.5 KB");
  assert.equal(formatVersionBytes(5 * 1024 * 1024), "5.0 MB");
  assert.equal(formatVersionBytes(2 * 1024 * 1024 * 1024), "2.0 GB");
});

test("shortens root hashes and labels snapshot types", () => {
  assert.equal(shortVersionHash("sha256:abcdef0123456789"), "sha256:abcdef012345…");
  assert.equal(shortVersionHash(""), "无根哈希");
  assert.equal(versionBaseTypeLabel("full", null), "基准");
  assert.equal(versionBaseTypeLabel("full", "v1"), "检查点");
  assert.equal(versionBaseTypeLabel("delta", "v1"), "增量");
});

test("labels the current match using the version list", () => {
  const versions = [record({ id: "v1", label: "旧版" }), record({ id: "v2", label: "新版" })];
  assert.equal(versionLabelById(versions, "v2"), "新版");
  assert.equal(versionLabelById(versions, "missing"), "missing");
  assert.equal(versionLabelById(versions, null), "未知版本");

  assert.equal(
    versionMatchLabel(
      { kind: "metadata", versionId: "v2", candidates: ["v2"], checkedHashes: false, message: "" },
      versions,
    ),
    "疑似：新版",
  );
  assert.equal(
    versionMatchLabel(
      { kind: "hash", versionId: "v1", candidates: ["v1"], checkedHashes: true, message: "" },
      versions,
    ),
    "已确认：旧版",
  );
  assert.equal(
    versionMatchLabel(
      { kind: "unknown", versionId: null, candidates: [], checkedHashes: false, message: "" },
      versions,
    ),
    "未知 / 已修改",
  );
  assert.equal(versionMatchLabel(null, versions), "尚未读取");
});

test("summarizes change logs", () => {
  assert.equal(versionChangeSummary(null), "完整快照");
  assert.equal(versionChangeSummary(changeLog()), "无文件变化");
  assert.equal(
    versionChangeSummary(
      changeLog({
        added: [{ path: "a", size: 1, sha256: "aa", delta: "d" }],
        removed: ["b"],
        modified: [
          { path: "c", oldSize: 1, newSize: 2, oldSha256: "aa", newSha256: "bb", delta: "d" },
        ],
      }),
    ),
    "新增 1 · 删除 1 · 修改 1",
  );
});

test("computes progress from bytes first, then file counts", () => {
  assert.equal(versionProgressPercent(null), 0);
  assert.equal(versionProgressPercent(progress()), 50);
  assert.equal(
    versionProgressPercent(
      progress({ totalBytes: 0, processedBytes: 999, processedFiles: 3, totalFiles: 10 }),
    ),
    30,
  );
  assert.equal(
    versionProgressPercent(progress({ totalBytes: 0, processedBytes: 0, processedFiles: 7, totalFiles: 10 })),
    70,
  );
  assert.equal(versionProgressPercent(progress({ status: "completed" })), 100);
  assert.equal(versionProgressPercent(progress({ totalBytes: 0, totalFiles: 0 })), 0);
});

test("maps backend phases to Chinese labels", () => {
  assert.equal(versionPhaseLabel("hashing"), "计算文件哈希");
  assert.equal(versionPhaseLabel("copying"), "复制文件");
  assert.equal(versionPhaseLabel("restoring"), "还原文件");
  assert.equal(versionPhaseLabel("custom"), "custom");
});

test("counts deltas since the most recent full snapshot", () => {
  assert.equal(versionDeltaChainDepth([]), 0);
  assert.equal(
    versionDeltaChainDepth([record({ id: "v1", baseType: "full", parentId: null })]),
    0,
  );
  assert.equal(
    versionDeltaChainDepth([
      record({ id: "v1", baseType: "full", parentId: null }),
      record({ id: "v2", baseType: "delta", parentId: "v1" }),
      record({ id: "v3", baseType: "delta", parentId: "v2" }),
    ]),
    2,
  );
  assert.equal(
    versionDeltaChainDepth([
      record({ id: "v1", baseType: "full", parentId: null }),
      record({ id: "v2", baseType: "delta", parentId: "v1" }),
      record({ id: "v3", baseType: "full", parentId: "v2" }),
      record({ id: "v4", baseType: "delta", parentId: "v3" }),
    ]),
    1,
  );
});
