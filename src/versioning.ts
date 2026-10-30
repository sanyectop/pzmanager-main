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

/** 游戏大版本里关联的单个模组版本（后端 `ModVersionRef`）。 */
export type ModVersionRef = {
  /** 模组版本库的目录名：`workshop-<id>-<modId>` / `local-<modId>`。 */
  key: string;
  modId: string;
  name: string;
  /** `local` / `workshop` */
  source: string;
  workshopId: string | null;
  versionId: string;
  rootPath: string;
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
  /** 只有游戏大版本会填；模组自己的链里为空/缺失。 */
  modVersions?: ModVersionRef[];
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
  /** 当前第几个步骤（1 起）与总步骤数；见 `versionStepLabel`。 */
  step?: number;
  steps?: number;
};

/** 「校验当前目录」完成后额外推送的结构化结果。 */
export type VersionVerifyResult = {
  matchedVersionId: string | null;
  rootHash: string;
  message: string;
};

/** 单个模组的版本库摘要（后端 `ModVersionTarget`），列表页只看这个。 */
export type ModVersionTarget = {
  key: string;
  rootPath: string;
  name: string;
  modId: string;
  /** `local` / `workshop` */
  source: string;
  workshopId: string | null;
  hasStore: boolean;
  versionCount: number;
  latestVersionId: string | null;
  latestLabel: string | null;
  latestCreatedAt: number | null;
};

/** 单个模组的完整版本状态（展开行 / 模态框用）。 */
export type ModVersionStatus = {
  target: ModVersionTarget;
  storePath: string;
  versions: VersionRecord[];
  currentMatch: VersionCurrentMatch;
  pendingChanges: VersionPendingChanges | null;
  operationRunning: boolean;
};

/** `mod-version-progress` 事件；比游戏事件多 target 定位字段。 */
export type ModVersionOperationUpdate = VersionOperationUpdate & {
  rootKey?: string | null;
  rootPath?: string | null;
};

/** 一条「整合包」：某个时刻所有模组各自版本的集合。 */
export type ModPackRecord = {
  id: string;
  label: string;
  createdAt: number;
  mods: ModVersionRef[];
};

/** 备份目录是否在系统盘（C:）；UNC 路径和其它盘都不算。 */
export function versionStoreOnSystemDrive(storePath: string | null | undefined): boolean {
  const trimmed = (storePath ?? "").trim();
  // 用户手填的扩展长度路径（\\?\C:\...）也要认出来。
  const normalized = trimmed.startsWith("\\\\?\\") ? trimmed.slice(4) : trimmed;
  return /^c:/i.test(normalized);
}

/** 备份在 C 盘、且用户没有勾过「不再弹出」时，每次操作前都要先提醒换盘。 */
export function shouldWarnVersionStoreOnSystemDrive(
  storePath: string | null | undefined,
  noticeDismissed: boolean,
): boolean {
  return !noticeDismissed && versionStoreOnSystemDrive(storePath);
}

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

/** 时间线上的日期矩形：只要 `YYYY/MM/DD`，时间放在下面的说明行。 */
export function formatVersionDate(unixSeconds: number): string {
  if (!Number.isFinite(unixSeconds) || unixSeconds <= 0) {
    return "未知日期";
  }
  const date = new Date(unixSeconds * 1000);
  const pad = (value: number) => `${value}`.padStart(2, "0");
  return `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())}`;
}

/** 时间线按「最新在上」展示；不修改入参。 */
export function sortVersionsNewestFirst(versions: VersionRecord[]): VersionRecord[] {
  return versions.slice().reverse();
}

/** 时间线序号：最新的一条是 1，越旧越大。 */
export function versionOrdinal(index: number): number {
  return index + 1;
}

/** 时间线圆形胶囊直径：越新越大，最小 10px，前 8 条逐级递减。 */
export function versionDotSize(index: number): number {
  return Math.max(10, 18 - index);
}

export function modSourceLabel(source: string): string {
  return source === "workshop" ? "创意工坊" : "本地";
}

export type ModVersionFilter = "all" | "isolated" | "workshop" | "local";

/** 模组版本列表的搜索 + 来源/已隔离筛选。 */
export function filterModVersionTargets(
  targets: ModVersionTarget[],
  query: string,
  filter: ModVersionFilter,
): ModVersionTarget[] {
  const keyword = query.trim().toLowerCase();
  return targets.filter((target) => {
    if (filter === "isolated" && target.versionCount === 0) {
      return false;
    }
    if (filter === "workshop" && target.source !== "workshop") {
      return false;
    }
    if (filter === "local" && target.source !== "local") {
      return false;
    }
    if (!keyword) {
      return true;
    }
    return (
      target.name.toLowerCase().includes(keyword) ||
      target.modId.toLowerCase().includes(keyword) ||
      target.rootPath.toLowerCase().includes(keyword)
    );
  });
}

export function shortVersionHash(hash: string): string {
  const trimmed = hash.startsWith("sha256:") ? hash.slice("sha256:".length) : hash;
  return trimmed ? `sha256:${trimmed.slice(0, 12)}…` : "无根哈希";
}

export function versionBaseTypeLabel(baseType: string, _parentId: string | null): string {
  // 用户只需要知道这次是完整备份还是只存了变化；基准/检查点的区别留在开发者视角。
  return baseType === "full" ? "完整备份" : "只存变化";
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
    return "正在读取…";
  }
  switch (match.kind) {
    case "hash":
      return `与「${versionLabelById(versions, match.versionId)}」一致`;
    case "metadata":
      return `可能是「${versionLabelById(versions, match.versionId)}」`;
    case "unknown":
      return "和保存过的版本都不一样";
    case "none":
      return "还没保存过";
    case "missing":
      return "没找到游戏目录";
    default:
      return match.message;
  }
}

export function versionPhaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    preparing: "准备",
    scanning: "正在检查文件",
    hashing: "正在核对文件",
    copying: "正在复制文件",
    delta: "正在保存变化",
    restoring: "正在还原文件",
    pzopt: "正在处理 PZ Optimization",
    /** 大版本保存时顺带记录模组版本。 */
    mods: "正在保存模组版本",
    cleaning: "正在清理多余文件",
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

/** 进度后面的操作计数，例如 `10% 1/3`（计算哈希 → 复制/保存变化 → 保存模组）。
    只有一步的操作（校验）不显示计数。 */
export function versionStepLabel(update: VersionOperationUpdate | null): string {
  const step = update?.step;
  const steps = update?.steps;
  if (!step || !steps || steps <= 1) {
    return "";
  }
  return ` ${Math.min(Math.max(1, Math.round(step)), steps)}/${steps}`;
}

export function versionChangeSummary(changeLog: VersionChangeLog | null): string {
  if (!changeLog) {
    return "完整备份";
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
