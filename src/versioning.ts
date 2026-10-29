/** 与后端 `src-tauri/src/versioning.rs` 对应的类型和纯展示函数。 */

export type VersionMatchKind = "hash" | "metadata" | "unknown" | "none" | "missing";

export type VersionAddedFile = {
  path: string;
  size: number;
  sha256: string;
  delta: string;
};

export type VersionModifiedFile = {
  path: string;
  oldSize: number;
  newSize: number;
  oldSha256: string;
  newSha256: string;
  delta: string;
};

export type VersionChangeLog = {
  from: string;
  to: string;
  fromBuildId: string | null;
  toBuildId: string | null;
  added: VersionAddedFile[];
  removed: string[];
  modified: VersionModifiedFile[];
};

export type VersionRecord = {
  id: string;
  parentId: string | null;
  steamBuildId: string | null;
  branch: string;
  /** `full` = 完整快照（母文件/检查点）；`delta` = 增量版本。 */
  baseType: "full" | "delta" | string;
  createdAt: number;
  rootHash: string;
  metadataHash: string;
  fileCount: number;
  totalBytes: number;
  label: string;
  changeLog: VersionChangeLog | null;
};

export type VersionCurrentMatch = {
  kind: VersionMatchKind;
  versionId: string | null;
  candidates: string[];
  checkedHashes: boolean;
  message: string;
};

export type VersionPendingChanges = {
  added: number;
  removed: number;
  modified: number;
};

export type VersionStatus = {
  gamePath: string | null;
  storePath: string;
  storeExists: boolean;
  steamBuildId: string | null;
  branch: string;
  lastCapturedBuildId: string | null;
  steamUpdatePending: boolean;
  versions: VersionRecord[];
  currentMatch: VersionCurrentMatch;
  pendingChanges: VersionPendingChanges | null;
  activeVersionId: string | null;
  operationRunning: boolean;
};

export type VersionOperationUpdate = {
  operationId: number;
  operation: string;
  /** `started` / `progress` / `completed` / `error` */
  status: string;
  phase: string;
  processedFiles: number;
  totalFiles: number;
  processedBytes: number;
  totalBytes: number;
  currentPath: string | null;
  message: string | null;
  error: string | null;
};

/** 「校验当前目录」完成后额外推送的结构化结果。 */
export type VersionVerifyResult = {
  matchedVersionId: string | null;
  rootHash: string;
  message: string;
};

export function formatVersionBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index + 1 < units.length) {
    value /= 1024;
    index += 1;
  }
  if (index === 0) {
    return `${Math.round(value)} B`;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

export function formatVersionTime(unixSeconds: number): string {
  if (!Number.isFinite(unixSeconds) || unixSeconds <= 0) {
    return "未知时间";
  }
  return new Date(unixSeconds * 1000).toLocaleString();
}

export function shortVersionHash(hash: string): string {
  const trimmed = hash.startsWith("sha256:") ? hash.slice("sha256:".length) : hash;
  return trimmed ? `sha256:${trimmed.slice(0, 12)}…` : "无根哈希";
}

export function versionBaseTypeLabel(baseType: string, parentId: string | null): string {
  if (baseType === "full") {
    return parentId ? "检查点" : "基准";
  }
  return "增量";
}

export function versionLabelById(versions: VersionRecord[], id: string | null): string {
  if (!id) {
    return "未知版本";
  }
  return versions.find((version) => version.id === id)?.label ?? id;
}

export function versionMatchLabel(
  match: VersionCurrentMatch | null,
  versions: VersionRecord[],
): string {
  if (!match) {
    return "尚未读取";
  }
  switch (match.kind) {
    case "hash":
      return `已确认：${versionLabelById(versions, match.versionId)}`;
    case "metadata":
      return `疑似：${versionLabelById(versions, match.versionId)}`;
    case "unknown":
      return "未知 / 已修改";
    case "none":
      return "尚未创建版本";
    case "missing":
      return "未检测到游戏目录";
    default:
      return match.message;
  }
}

export function versionPhaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    preparing: "准备",
    scanning: "扫描",
    hashing: "计算文件哈希",
    copying: "复制文件",
    delta: "生成增量包",
    restoring: "还原文件",
    pzopt: "处理 PZ Optimization",
    cleaning: "清理多余文件",
    done: "完成",
    error: "失败",
  };
  return labels[phase] ?? phase;
}

export function versionProgressPercent(update: VersionOperationUpdate | null): number {
  if (!update) {
    return 0;
  }
  if (update.status === "completed") {
    return 100;
  }
  const total = update.totalBytes > 0 ? update.totalBytes : update.totalFiles;
  const done = update.totalBytes > 0 ? update.processedBytes : update.processedFiles;
  if (!Number.isFinite(total) || total <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, Math.round((done / total) * 100)));
}

export function versionChangeSummary(changeLog: VersionChangeLog | null): string {
  if (!changeLog) {
    return "完整快照";
  }
  const parts: string[] = [];
  if (changeLog.added.length > 0) {
    parts.push(`新增 ${changeLog.added.length}`);
  }
  if (changeLog.removed.length > 0) {
    parts.push(`删除 ${changeLog.removed.length}`);
  }
  if (changeLog.modified.length > 0) {
    parts.push(`修改 ${changeLog.modified.length}`);
  }
  return parts.length > 0 ? parts.join(" · ") : "无文件变化";
}

/** 从最新版本往前数，最近一个完整快照之后有多少个增量版本。 */
export function versionDeltaChainDepth(versions: VersionRecord[]): number {
  let depth = 0;
  for (let index = versions.length - 1; index >= 0; index -= 1) {
    if (versions[index].baseType === "full") {
      return depth;
    }
    depth += 1;
  }
  return depth;
}
