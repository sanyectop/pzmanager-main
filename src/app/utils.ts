/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  BUILTIN_BACKGROUNDS,
  BUILTIN_BACKGROUND_PREFIX,
  BUILTIN_SEASONAL_ID,
  CUSTOM_TRANSLATION_CHUNK_CHARS,
  FONT_FAMILY_PRESET_CSS,
  LEGACY_DEFAULT_BUILTIN_ID,
  TRANSLATION_BATCH_SIZE,
  newsEngineLabels,
} from "./constants";
import {
  BuiltinBackground,
  CustomTranslationLine,
  DetectionState,
  DeveloperNewsTranslationResult,
  ProjectZomboidPaths,
  StartupTiming,
  TaskStatus,
  ToastKind,
} from "./types";
import { invoke } from "@tauri-apps/api/core";

export function builtinBackgroundFromPath(path: string | null | undefined): BuiltinBackground | null {
  if (!path || !path.startsWith(BUILTIN_BACKGROUND_PREFIX)) {
    return null;
  }
  const rawId = path.slice(BUILTIN_BACKGROUND_PREFIX.length);
  const id = rawId === LEGACY_DEFAULT_BUILTIN_ID ? BUILTIN_SEASONAL_ID : rawId;
  return BUILTIN_BACKGROUNDS.find((background) => background.id === id) ?? null;
}

export function fontFamilyChoiceFromValue(value: string): string {
  if (!value.trim()) {
    return "default";
  }
  for (const [key, css] of Object.entries(FONT_FAMILY_PRESET_CSS)) {
    if (css === value) {
      return key;
    }
  }
  return "custom";
}

export function detectionStateFromPaths(paths: ProjectZomboidPaths): DetectionState {
  return paths.profile || paths.installPath || paths.workshopPath || paths.zombieBuddy.available
    ? { status: "found", paths }
    : { status: "notFound" };
}

export function mimeTypeFor(name: string): string {
  const extension = name.split(".").pop()?.toLowerCase();
  return extension === "jpg" || extension === "jpeg"
    ? "image/jpeg"
    : extension === "webp"
      ? "image/webp"
      : extension === "bmp"
        ? "image/bmp"
        : extension === "gif"
          ? "image/gif"
          : "image/png";
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatMemoryMb(memoryMb: number): string {
  return memoryMb % 1024 === 0
    ? `${memoryMb / 1024} GB`
    : `${(memoryMb / 1024).toFixed(1)} GB`;
}

export function overlayPhaseLabel(phase: string): string {
  switch (phase) {
    case "waitingProcess":
      return "正在启动游戏";
    case "waitingWindow":
      return "正在等待游戏窗口";
    case "injecting":
      return "正在连接游戏";
    case "loading":
      return "正在读取加载进度";
    case "finished":
      return "已完成";
    case "game-exited":
      return "游戏已退出";
    case "error":
      return "失败";
    default:
      return phase;
  }
}

export function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return minutes > 0 ? `${minutes} 分 ${seconds} 秒` : `${seconds} 秒`;
}

export function startupTimingText(
  result: StartupTiming | null,
  startedAt: number | null,
  now: number,
): string {
  if (result && result.lastMs !== null) {
    const base = `启动用时 ${formatElapsed(result.lastMs)}`;
    const previous = result.previousMs;
    if (previous === null) {
      return base;
    }
    const delta = result.lastMs - previous;
    if (Math.abs(delta) < 500) {
      return `${base} · 与上次持平`;
    }
    return `${base} · 比上次${delta < 0 ? "快" : "慢"} ${formatElapsed(Math.abs(delta))}`;
  }
  if (startedAt !== null) {
    return `启动计时中 ${formatElapsed(Math.max(0, now - startedAt))}`;
  }
  return "";
}

export function formatWorkshopSize(value: string): string {
  let size = Number.parseInt(value, 10) || 0;
  if (size <= 0) {
    return "-";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size.toFixed(1)} ${units[index]}`;
}

export function formatWorkshopDate(timestamp: number): string {
  if (!timestamp) {
    return "-";
  }
  return new Date(timestamp * 1000).toLocaleDateString("zh-CN");
}

export function notificationKindLabel(kind: ToastKind): string {
  return kind === "success" ? "成功" : kind === "error" ? "错误" : "信息";
}

export function notificationKindPalette(kind: ToastKind): "green" | "red" | "blue" {
  return kind === "success" ? "green" : kind === "error" ? "red" : "blue";
}

export function taskStatusLabel(status: TaskStatus): string {
  return status === "success" ? "已完成" : status === "error" ? "失败" : "进行中";
}

export function taskStatusPalette(status: TaskStatus): "green" | "red" | "blue" {
  return status === "success" ? "green" : status === "error" ? "red" : "blue";
}

export function formatDownloadSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function isModRowInteractiveTarget(target: EventTarget) {
  return (target as HTMLElement).closest(
    "button, input, textarea, select, a, [data-pz-mod-switch], .pz-jar-mod-details-shell",
  ) !== null;
}

export function isTauriRuntime() {
  return Boolean(
    typeof window !== "undefined" &&
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  );
}

export function escapeNewsHtml(value: string): string {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

export function replaceNewsTranslation(html: string, sid: string, text: string): string {
  const escapedSid = sid.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const span = new RegExp(`<span data-sid="${escapedSid}" class="i18n-pending">[\\s\\S]*?<\\/span>`);
  return html.replace(span, escapeNewsHtml(text));
}

export function splitCustomTranslationLine(
  line: string,
  maxLength: number,
): { chunks: string[]; separators: string[] } {
  if (line.length <= maxLength) {
    return { chunks: [line], separators: [] };
  }
  const chunks: string[] = [];
  const separators: string[] = [];
  let rest = line;
  while (rest.length > maxLength) {
    const cut = rest.lastIndexOf(" ", maxLength);
    if (cut > maxLength * 0.6) {
      chunks.push(rest.slice(0, cut));
      separators.push(" ");
      rest = rest.slice(cut + 1);
    } else {
      chunks.push(rest.slice(0, maxLength));
      separators.push("");
      rest = rest.slice(maxLength);
    }
  }
  chunks.push(rest);
  return { chunks, separators };
}

export function buildCustomTranslationJobs(source: string): {
  lines: CustomTranslationLine[];
  jobs: Array<{ sid: string; text: string }>;
} {
  const lines: CustomTranslationLine[] = [];
  const jobs: Array<{ sid: string; text: string }> = [];
  let jobIndex = 0;
  for (const line of source.split(/\r\n|\r|\n/)) {
    if (line.trim() === "") {
      lines.push({ kind: "blank" });
      continue;
    }
    if (!/\p{L}/u.test(line)) {
      lines.push({ kind: "raw", text: line });
      continue;
    }
    const { chunks, separators } = splitCustomTranslationLine(
      line,
      CUSTOM_TRANSLATION_CHUNK_CHARS,
    );
    const sids = chunks.map(() => {
      const sid = `custom_${jobIndex++}`;
      return sid;
    });
    chunks.forEach((chunk, index) => {
      jobs.push({ sid: sids[index], text: chunk });
    });
    lines.push({ kind: "translatable", chunks, sids, separators });
  }
  return { lines, jobs };
}

/** 分批调用后端翻译并回填。返回 false = 连续两批请求失败后中止：
 * 后端多半正在重启或翻译服务整体不可用，调用方应提示用户重试，
 * 而不是像旧版那样静默吞掉所有错误，让页面永远停在原文。
 * 返回 true = 全部翻完或调用方已取消。 */
export async function translateInBatches(
  jobs: Array<{ sid: string; text: string }>,
  language: string,
  onResults: (results: DeveloperNewsTranslationResult[]) => void,
  onProgress: (completed: number, total: number) => void,
  isCancelled: () => boolean,
): Promise<boolean> {
  let index = 0;
  let consecutiveFailures = 0;
  while (index < jobs.length) {
    if (isCancelled()) {
      return true;
    }
    const batch = jobs.slice(index, index + TRANSLATION_BATCH_SIZE);
    let results: DeveloperNewsTranslationResult[];
    try {
      results = await invoke<DeveloperNewsTranslationResult[]>("translate_developer_news_batch", {
        jobs: batch,
        language,
      });
      consecutiveFailures = 0;
    } catch {
      if (isCancelled()) {
        return true;
      }
      consecutiveFailures += 1;
      if (consecutiveFailures >= 2) {
        return false;
      }
      // 失败后等 1.5 秒重试同一批（dev 模式后端重启的间隙就能扛过去）。
      await new Promise((resolve) => setTimeout(resolve, 1500));
      continue;
    }
    if (isCancelled()) {
      return true;
    }
    onResults(results);
    // 只有真正成功的批次才推进进度——失败批不虚报 completed。
    index += TRANSLATION_BATCH_SIZE;
    onProgress(Math.min(index, jobs.length), jobs.length);
  }
  return true;
}

export function formatNewsEngineShare(tally: Record<string, number> | undefined): string | null {  if (!tally) return null;
  const entries = Object.entries(tally).filter(([, count]) => count > 0);
  const total = entries.reduce((sum, [, count]) => sum + count, 0);
  if (total === 0) return null;
  return entries
    .sort((left, right) => right[1] - left[1])
    .map(([engine, count]) => `${newsEngineLabels[engine] ?? engine} ${Math.round((count / total) * 100)}%`)
    .join(" · ");
}
