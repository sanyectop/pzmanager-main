/** 内置壁纸背景音乐播放器的纯逻辑（不依赖 DOM / React），供 tests/launchMusicUi.test.ts 覆盖。 */

/** 内置壁纸目录下的音源（Vite 把 public/ 挂到站点根）。 */
export const LAUNCH_MUSIC_URL = "/backgrounds/main.mp3";

/** 曲名（壁纸目录里 main.mp3 的原始曲目）。 */
export const LAUNCH_MUSIC_TRACK_TITLE = "万里升平 - 铁痕电台-MSR、MeLo_绿萝组";

/** 音量范围与默认值（0–100）。 */
export const MUSIC_VOLUME_MIN = 0;
export const MUSIC_VOLUME_MAX = 100;
export const MUSIC_VOLUME_DEFAULT = 30;

/** 键盘 / 按钮调节的步进。 */
export const MUSIC_SEEK_STEP_PERCENT = 5;
export const MUSIC_VOLUME_STEP = 5;

/** 持久化的播放状态（写进 settings.json 的 `launchMusic`）。 */
export type LaunchMusicState = {
  /** 存档格式版本。老存档没有这个字段，按新默认值迁移一次。 */
  v: number;
  /** 音量 0–100。 */
  volume: number;
  /** 关掉启动器时是否正在播放；下次启动据此续播。 */
  playing: boolean;
  /** 上次的播放位置（秒），续播从这里开始。 */
  positionSec: number;
  /** 播放器面板是否展开。 */
  open: boolean;
  /** 写入时间戳（毫秒）。settings.json 与本地缓存各存一份，启动时按它取新的那份。 */
  savedAt?: number;
};

/** 当前存档格式：从 2 起带 `v`，默认音量 30、默认自动播放。 */
export const LAUNCH_MUSIC_STATE_VERSION = 2;

/** 默认自动播放：没写过状态的新用户一进启动页就起播（用户手动暂停后会记成 false）。 */
export const DEFAULT_LAUNCH_MUSIC_STATE: LaunchMusicState = {
  v: LAUNCH_MUSIC_STATE_VERSION,
  volume: MUSIC_VOLUME_DEFAULT,
  playing: true,
  positionSec: 0,
  open: false,
};

/** 自动播放被 WebView 拒绝时，等第一次用户交互再补放。 */
export const MUSIC_AUTOPLAY_RETRY_EVENTS = ["pointerdown", "keydown"] as const;

/** 把任意来源（settings.json / 旧版本 / 手改）的值规整成合法状态，坏值一律回落默认。
 *
 *  老存档（没有 `v`，或版本号低于当前）是"暂停 + 音量 60"那套默认值写的，直接沿用会让
 *  "默认播放 / 默认音量 30"看起来没生效，所以整体按新默认迁移——只保留播放位置和面板展开。 */
export function normalizeLaunchMusicState(raw: unknown): LaunchMusicState {
  if (!raw || typeof raw !== "object") {
    return { ...DEFAULT_LAUNCH_MUSIC_STATE };
  }
  const record = raw as Record<string, unknown>;
  const version = typeof record.v === "number" ? record.v : 0;
  const legacy = version < LAUNCH_MUSIC_STATE_VERSION;
  return {
    v: LAUNCH_MUSIC_STATE_VERSION,
    volume: legacy ? MUSIC_VOLUME_DEFAULT : normalizeMusicVolume(record.volume),
    // 缺字段 / 非布尔：按默认值（自动播放）处理，这样旧配置升级后也能直接起播。
    playing: legacy
      ? DEFAULT_LAUNCH_MUSIC_STATE.playing
      : typeof record.playing === "boolean"
        ? record.playing
        : DEFAULT_LAUNCH_MUSIC_STATE.playing,
    positionSec: normalizeMusicPosition(record.positionSec),
    open: typeof record.open === "boolean" ? record.open : false,
    // 必须原样带回来：两份存档（settings.json / localStorage）靠它比新旧。
    savedAt:
      typeof record.savedAt === "number" && Number.isFinite(record.savedAt)
        ? record.savedAt
        : undefined,
  };
}

/** 音量夹到 0–100 并取整；非数字回落默认音量。 */
export function normalizeMusicVolume(value: unknown): number {
  const numeric = typeof value === "number" ? value : Number.NaN;
  if (!Number.isFinite(numeric)) {
    return MUSIC_VOLUME_DEFAULT;
  }
  return Math.round(Math.min(MUSIC_VOLUME_MAX, Math.max(MUSIC_VOLUME_MIN, numeric)));
}

/** 播放位置：负数 / 非数字 / Infinity / 缺省一律按 0 处理。 */
export function normalizeMusicPosition(value: unknown): number {
  const numeric = typeof value === "number" ? value : Number.NaN;
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return 0;
  }
  return numeric;
}

/** 音量百分比 → HTMLMediaElement.volume（0–1）。 */
export function musicAudioVolume(volumePercent: number): number {
  return normalizeMusicVolume(volumePercent) / 100;
}

/** 秒数格式化成 `m:ss`（时长未知时显示 `--:--`）。 */
export function formatMusicTime(seconds: number | null): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) {
    return "--:--";
  }
  const total = Math.floor(seconds);
  const minutes = Math.floor(total / 60);
  const rest = total % 60;
  return `${minutes}:${rest.toString().padStart(2, "0")}`;
}

/** 进度百分比（0–100）；时长未知或为 0 时给 0，避免 NaN 宽度。 */
export function musicProgressPercent(current: number, duration: number | null): number {
  if (duration == null || !Number.isFinite(duration) || duration <= 0) {
    return 0;
  }
  if (!Number.isFinite(current) || current <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, (current / duration) * 100));
}

/** 展开按钮的无障碍说明（`aria-label` / `title`）。 */
export function musicToggleLabel(open: boolean): string {
  return open ? "收起背景音乐播放器" : "展开背景音乐播放器";
}

/** 播放 / 暂停按钮的说明文案。 */
export function musicPlayLabel(playing: boolean): string {
  return playing ? "暂停背景音乐" : "播放背景音乐";
}

/** 落盘时该写哪个播放位置。
 *
 *  启动恢复的顺序是「读存档 → 起播 → 元数据就绪后定位」，中间 `onPlay` 会触发一次立刻落盘。
 *  此时 `audio.currentTime` 还是 0（还没定位），若直接拿它覆盖，存档里的进度就被抹掉了，
 *  下次启动只能从头播——恢复目标必须一直保留到 `loadedmetadata` 真正应用为止。 */
export function persistedPosition({
  restorePending,
  audioTime,
  storedPosition,
}: {
  restorePending: boolean;
  audioTime: number | null;
  storedPosition: number;
}): number {
  if (restorePending) {
    return storedPosition;
  }
  if (audioTime == null || !Number.isFinite(audioTime) || audioTime < 0) {
    return storedPosition;
  }
  return audioTime;
}

/** 本地（localStorage）缓存的键：`invoke` 在页面刷新/关闭瞬间可能被丢弃，
 *  同步写入的 localStorage 才是"刷新不丢"的兜底。 */
export const LAUNCH_MUSIC_STORAGE_KEY = "pzmanager.launchMusic";

/** 两份存档取新的：settings.json（可能落后）与 localStorage（刷新瞬间也能落盘）。
 *  `savedAt` 缺失的一律视为最旧，方便老数据自然被取代。 */
export function pickNewerMusicState(
  fromFile: LaunchMusicState,
  fromLocal: LaunchMusicState | null,
): LaunchMusicState {
  if (!fromLocal) return fromFile;
  const fileAt = fromFile.savedAt ?? 0;
  const localAt = fromLocal.savedAt ?? 0;
  return localAt > fileAt ? fromLocal : fromFile;
}
