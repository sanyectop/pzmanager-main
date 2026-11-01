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
  versionStepLabel,
  versionLabelById,
  compareVersionStrings,
  parseDevBuildVersion,
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
  // 用户视角只区分“完整保存”和“只存了变化”，不再区分基准/检查点。
  assert.equal(versionBaseTypeLabel("full", null), "完整备份");
  assert.equal(versionBaseTypeLabel("full", "v1"), "完整备份");
  assert.equal(versionBaseTypeLabel("delta", "v1"), "只存变化");
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
    "可能是「新版」",
  );
  assert.equal(
    versionMatchLabel(
      { kind: "hash", versionId: "v1", candidates: ["v1"], checkedHashes: true, message: "" },
      versions,
    ),
    "与「旧版」一致",
  );
  assert.equal(
    versionMatchLabel(
      { kind: "unknown", versionId: null, candidates: [], checkedHashes: false, message: "" },
      versions,
    ),
    "和保存过的版本都不一样",
  );
  assert.equal(versionMatchLabel(null, versions), "正在读取…");
});

test("summarizes change logs", () => {
  assert.equal(versionChangeSummary(null), "完整备份");
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

test("shows the operation counter after the percentage", () => {
  // 例：`10% 1/3`——百分比是当前这一步的进度，后面跟「第几步 / 共几步」。
  assert.equal(versionStepLabel(progress({ step: 1, steps: 3 })), " 1/3");
  assert.equal(versionStepLabel(progress({ step: 2, steps: 3 })), " 2/3");
  assert.equal(
    versionStepLabel(progress({ step: 3, steps: 3, status: "completed" })),
    " 3/3",
  );
  // 只有一步（校验）不显示计数
  assert.equal(versionStepLabel(progress({ step: 1, steps: 1 })), "");
  assert.equal(versionStepLabel(progress()), "");
  assert.equal(versionStepLabel(null), "");
  // 异常值收敛到 [1, steps]
  assert.equal(versionStepLabel(progress({ step: 0, steps: 3 })), "");
  assert.equal(versionStepLabel(progress({ step: 9, steps: 3 })), " 3/3");
});

test("maps backend phases to Chinese labels", () => {
  assert.equal(versionPhaseLabel("hashing"), "正在核对文件");
  assert.equal(versionPhaseLabel("copying"), "正在复制文件");
  assert.equal(versionPhaseLabel("restoring"), "正在还原文件");
  assert.equal(versionPhaseLabel("mods"), "正在保存模组版本");
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

test("内测构建版本号按 base 比较，构建号不影响大小", () => {
  assert.deepEqual(parseDevBuildVersion("1.5.261101-devbuild.1793501619"), {
    base: "1.5.261101",
    buildId: "1793501619",
  });
  assert.deepEqual(parseDevBuildVersion("v1.5.261101-DevBuild.7"), {
    base: "1.5.261101",
    buildId: "7",
  });
  assert.equal(parseDevBuildVersion("1.5.261101"), null);
  assert.equal(parseDevBuildVersion("1.5.261101-beta.7"), null);

  // 同号正式版与内测构建视为同一版：隐私协议不会因为换了构建号反复弹窗。
  assert.equal(compareVersionStrings("1.5.261101-devbuild.1793501619", "1.5.261101"), 0);
  assert.equal(compareVersionStrings("1.5.261101-devbuild.5", "1.5.261101-devbuild.9"), 0);
  // 次版本更高的内测构建仍然更新。
  assert.equal(compareVersionStrings("1.6.0-devbuild.1", "1.5.261101"), 1);
});
