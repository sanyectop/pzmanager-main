/** 任务中心的落盘模型：历史条数上限 + tasks.json 条目的清洗。
 *
 * 独立模块（不 import 运行时代码），`node --test` 可以直接单测；
 * 类型用 `import type` 引入，构建 / 测试时都会被完整擦除。 */
import type { TaskItem, TaskStatus } from "./app/types";

/** 落盘历史条数上限（与后端 task_store::TASK_HISTORY_CAP 一致）。 */
export const TASK_HISTORY_CAP = 200;

const TASK_KINDS = ["shader-download", "version-operation", "memory-clean"] as const;

/** 启动时把 tasks.json 里的历史条目清洗回 TaskItem：
 * 缺字段 / 类型不对 / kind 或 status 不认识的条目直接丢弃；
 * 还挂着 running 的任务在应用退出时必然已经被杀掉，统一标成
 * 「失败：应用退出时任务被中断」（光影下载保留 request，重试按钮继续可用）。 */
export function sanitizeSavedTasks(raw: unknown): TaskItem[] {
  if (!Array.isArray(raw)) {
    return [];
  }
  const items: TaskItem[] = [];
  for (const entry of raw) {
    if (typeof entry !== "object" || entry === null) {
      continue;
    }
    const record = entry as Record<string, unknown>;
    const id = record.id;
    const kind = record.kind;
    if (typeof id !== "string" || id === "" || typeof kind !== "string") {
      continue;
    }
    if (!TASK_KINDS.includes(kind as (typeof TASK_KINDS)[number])) {
      continue;
    }
    const rawStatus = record.status;
    if (rawStatus !== "running" && rawStatus !== "success" && rawStatus !== "error") {
      continue;
    }
    const numberOrNull = (value: unknown): number | null =>
      typeof value === "number" && Number.isFinite(value) ? value : null;
    const stringOrNull = (value: unknown): string | null =>
      typeof value === "string" ? value : null;
    const interrupted = rawStatus === "running";
    const status: TaskStatus = interrupted ? "error" : rawStatus;
    const now = Date.now();
    const createdAt = numberOrNull(record.createdAt) ?? now;
    const finishedAt = interrupted
      ? (numberOrNull(record.finishedAt) ?? now)
      : numberOrNull(record.finishedAt);
    items.push({
      id,
      kind: kind as TaskItem["kind"],
      title: typeof record.title === "string" ? record.title : "未命名任务",
      detail: typeof record.detail === "string" ? record.detail : "",
      status,
      downloaded: numberOrNull(record.downloaded) ?? 0,
      total: numberOrNull(record.total),
      createdAt,
      finishedAt,
      error: interrupted ? "应用退出时任务被中断。" : stringOrNull(record.error),
      savedPath: stringOrNull(record.savedPath),
      request:
        typeof record.request === "object" && record.request !== null
          ? (record.request as TaskItem["request"])
          : null,
      percent: numberOrNull(record.percent),
      step: numberOrNull(record.step),
      steps: numberOrNull(record.steps),
      doneFiles: numberOrNull(record.doneFiles),
      totalFiles: numberOrNull(record.totalFiles),
      rootPath: stringOrNull(record.rootPath),
      freedMb: numberOrNull(record.freedMb),
      usedPercent: numberOrNull(record.usedPercent),
    });
  }
  return items.slice(0, TASK_HISTORY_CAP);
}
