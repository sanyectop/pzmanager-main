import {
  Avatar,
  Badge,
  Box,
  Button,
  HStack,
  Input,
  Menu,
  Portal,
  Slider,
  Stack,
  Switch,
  Text,
  Textarea,
} from "@chakra-ui/react";
import { faCircleInfo, faGear, faPlus, faTrash } from "@fortawesome/free-solid-svg-icons";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Image as TauriImage } from "@tauri-apps/api/image";
import { open } from "@tauri-apps/plugin-dialog";
import { writeImage, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  startTransition,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import "./App.css";
import {
  conciseNotification,
  shouldShowNotification,
  type NotificationKind,
} from "./developerMode";
import { FallbackImage } from "./FallbackImage";
import { steamWorkshopUrl } from "./externalLinks";
import { shaderpackSearchQueries } from "./shaderpackSearch";
import appManifest from "../package.json";
import { defaultLaunchArguments, joinLaunchArguments } from "./launchArgs";
import { MIN_MEMORY_MB, defaultMemoryLimitMb } from "./memorySettings";
import {
  DEFAULT_STEAM_RELAY_BASE,
  STEAM_RELAY_MODE_OPTIONS,
  previewImageFallbackUrl,
} from "./steamRelay";
import { DEV_OVERLAY_DLL_PATH } from "./overlayInjection";
import {
  DEFAULT_SEASON_FEATURES,
  DEFAULT_WALLPAPER_EFFECT,
  WALLPAPER_DAY_MAX,
  formatClock,
  formatWallpaperDate,
  normalizeWallpaperEffectSettings,
  wallpaperEffectAt,
  type SeasonFeatures,
  type WallpaperEffect,
  type WallpaperEffectSettings,
} from "./builtinWallpaper";
import { SeasonalWallpaper, loadSeasonFeatures } from "./SeasonalWallpaper";
import { AcrylicGlass } from "./AcrylicGlass";
import {
  filterJarMods,
  filterLuaMods,
  invertSelection,
  mergeItemsPreservingOrder,
  setSelectionForKeys,
  sortLuaModsForDisplay,
  toggleSelection,
  type LuaModFilter,
  type JarModFilter,
} from "./modManagement";
import {
  analyzeJarModConflicts,
  jarScanStatusLabel,
  toggleJarModExpansion,
  type JarModConflictRisk,
} from "./jarModUi";

type SteamProfile = {
  steamId: string;
  accountName: string;
  personaName: string;
};

type ProjectZomboidPaths = {
  profile: SteamProfile | null;
  installPath: string | null;
  workshopPath: string | null;
  zombieBuddy: {
    available: boolean;
    installed: boolean;
    sourcePath: string | null;
    missingComponents: string[];
  };
  pzOptimization: {
    available: boolean;
    installed: boolean;
    sourcePath: string | null;
    installerPath: string | null;
    installedRevision: string | null;
    availableRevision: string | null;
    missingComponents: string[];
  };
  viewpoint3d: {
    ready: boolean;
    mods: Array<{
      name: string;
      workshopId: string;
      subscribed: boolean;
      sourcePath: string | null;
    }>;
    missingComponents: string[];
  };
};

type DetectionState =
  | { status: "checking" }
  | { status: "found"; paths: ProjectZomboidPaths }
  | { status: "notFound" }
  | { status: "error"; message: string };

type InstallResult = {
  changed: boolean;
  alreadyInstalled: boolean;
  backupPath: string | null;
};

type PzOptimizationInstallResult = {
  changed: boolean;
  alreadyInstalled: boolean;
  sourcePath: string;
  installedRevision: string | null;
};

type JarModBatchDecisionResult = {
  updated: number;
  luaModsEnabled: string[];
};

type ModPresetJarMod = {
  modDir: string;
  modId: string;
  workshopId: number | null;
  packageName: string;
  enabled: boolean;
};

type ModPreset = {
  id: string;
  name: string;
  luaModIds: string[];
  jarMods: ModPresetJarMod[];
};

type ModPresetApplyResult = {
  luaEnabled: number;
  luaSkipped: string[];
  jarChanged: number;
};

type LuaModBatchToggleResult = {
  enabled: boolean;
  changed: string[];
  added: string[];
  missingRequires: string[];
  dependents: string[];
  skipped: string[];
};

type ManagedFile = {
  name: string;
  sizeBytes: number;
  modifiedAt: number | null;
};

type ManagedDirectory = {
  directory: string;
  files: ManagedFile[];
};

type LuaModEntry = {
  id: string;
  name: string;
  path: string;
  source: string;
  workshopId: string | null;
  enabled: boolean;
  loadOrder: number | null;
  requires: string[];
  loadAfter: string[];
  loadBefore: string[];
  loadFirst: boolean;
  loadLast: boolean;
  hasCommon: boolean;
  hasB42: boolean;
  isLoader: boolean;
  posterPath: string | null;
  author: string | null;
  version: string | null;
  description: string | null;
};

type ScannedJarMod = {
  id: string;
  infoId: string;
  modInfoPath: string;
  disabled: boolean;
  workshopId: number | null;
  jarPath: string;
  posterPath: string | null;
  packageName: string;
  jarHash: string;
  description: string | null;
  patchClasses: string[];
  patchMethods: Array<{
    annotation: string;
    methodName: string;
  }>;
  luaClasses: string[];
  decision: boolean | null;
};

type JarModScanUpdate = {
  scanId: number;
  status: "started" | "item" | "completed" | "error";
  scanned: number;
  total: number;
  item: ScannedJarMod | null;
  error: string | null;
};

type WorkshopBackgroundScanUpdate = {
  scanId: number;
  status: "started" | "item" | "completed" | "error";
  scanned: number;
  total: number;
  item: BackgroundEntry | null;
  error: string | null;
};

type OverlaySessionUpdate = {
  phase: string;
  message: string;
  pid: number | null;
  processName: string | null;
  zbLines: number;
  elapsedMs: number;
};

type OverlayZbProgress = {
  zbLines: number;
  phase: string;
  lastLine: string;
  elapsedMs: number;
};

/** 「检测到窗口 → 移除覆盖层」的启动时长（lastMs = 本次，previousMs = 上一次）。 */
type StartupTiming = {
  lastMs: number | null;
  previousMs: number | null;
};

/** 后端 check_launcher_update 的返回（src-tauri/src/update.rs）。 */
type LauncherUpdateStatus = {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  pubDate: string | null;
  notes: string | null;
  /** 「下载更新」按钮地址:后端已做 123 云盘 → GitHub 安装包 → 便携版回落。 */
  downloadUrl: string | null;
  releasePageUrl: string | null;
};

type ActivePage = "launch" | "overview" | "shots" | "shaderpacks" | "workshop" | "mods" | "settings";
type ModManagementPage = "lua" | "jar";
type ResourceCenterPage = "workshop" | "shaderDownload" | "developerNews";
type SettingsPage = "about" | "general" | "translator" | "console" | "downloader" | "modLoader" | "startup";

type PreviewState = {
  name: string;
  url: string;
  bytes: ArrayBuffer;
};

type WorkshopFilters = {
  browsesort: string;
  days: number;
  searchText: string;
  page: number;
  numPerPage: number;
  requiredTags: string[];
  excludedTags: string[];
};

type WorkshopTagMode = "requiredTags" | "excludedTags";

type WorkshopItem = {
  id: string;
  title: string;
  shortDescription: string;
  previewUrl: string;
  subscriptions: number;
  favorited: number;
  views: number;
  starRating: number;
  totalVotes: number;
  fileSize: string;
  timeCreated: number;
  timeUpdated: number;
  numComments: number;
  tags: string[];
  creatorId: string;
  creatorName: string;
  fileUrl: string;
};

type WorkshopResponse = {
  items: WorkshopItem[];
  meta: {
    currentPage: number;
    totalPages: number;
    totalCount: number;
  };
  steamUrl: string;
};

/** 创意工坊项目详情（右键菜单「窗口内打开」用）。
    正文由后端切成 data-sid 占位，再走与开发者新闻完全相同的翻译批次换回译文。 */
type WorkshopItemDetail = {
  id: string;
  title: string;
  titleSid: string;
  descriptionHtml: string;
  previewUrl: string;
  fileUrl: string;
  subscriptions: number;
  timeCreated: number;
  timeUpdated: number;
  fileSize: string;
  tags: string[];
  creatorId: string;
  translationJobs: Array<{ sid: string; text: string }>;
  language: string;
};

// 翻译语言是全局设置（设置 › 通用），开发者新闻与创意工坊详情都读它，页面内不再各放一个下拉。
const TRANSLATION_LANGUAGE_OPTIONS = [
  { label: "简体中文", value: "zh-CN" },
  { label: "繁體中文", value: "zh-TW" },
  { label: "English", value: "en" },
  { label: "日本語", value: "ja" },
  { label: "한국어", value: "ko" },
  { label: "Русский", value: "ru" },
];
const DEFAULT_TRANSLATION_LANGUAGE = "zh-CN";

/** 「模组加载器方案」（设置 › 模组加载器）：决定启动时给游戏挂哪个加载器。
    加新加载器时在这里加一项，并在 Rust 侧 MOD_LOADER_SCHEMES / mod_loader_policy_agent 同步。 */
const MOD_LOADER_SCHEME_OPTIONS: WorkshopMenuOption[] = [
  { value: "zombieBuddy", label: "ZombieBuddy" },
  { value: "none", label: "不使用（原版加载）" },
];
const DEFAULT_MOD_LOADER_SCHEME = "zombieBuddy";

type WorkshopOptions = {
  sortOptions: { value: string; label: string }[];
  tags: string[];
};

type ResourceFilters = {
  searchQuery: string;
  gameVersion: string;
  selectedTag: string;
  sortBy: string;
  downloadSource: string;
  page: number;
  pageSize: number;
};

type ResourceItem = {
  id: string;
  name: string;
  description: string;
  iconUrl: string;
  iconFallbackUrl: string | null;
  tags: string[];
  lastUpdated: string;
  downloads: number;
  source: string;
  websiteUrl: string;
  author: string;
  resourceType: string;
};

/** 已安装光影包在资源中心搜到的图标：主地址（镜像）+ 官方源兜底。 */
type ShaderpackIcon = {
  url: string;
  fallbackUrl: string | null;
};

type ResourceSearchResponse = {
  items: ResourceItem[];
  total: number;
  page: number;
  pageSize: number;
};

type DeveloperNewsItem = {
  gid: string;
  title: string;
  url: string;
  date: number;
  author: string;
  feedLabel: string;
  contentsHtml: string;
  titleSid: string;
  engineTally?: Record<string, number>;
};
type DeveloperNewsResponse = {
  appId: number;
  items: DeveloperNewsItem[];
  language: string;
  translationJobs: Array<{ sid: string; text: string }>;
};

type ResourceFile = {
  resourceId: string;
  name: string;
  releaseType: string;
  downloads: number;
  fileDate: string;
  downloadUrl: string;
  fallbackDownloadUrls: string[];
  fileName: string;
  gameVersions: string[];
  loaders: string[];
};

type ResourceVersionPack = {
  name: string;
  items: ResourceFile[];
};

type ResourceOptions = {
  sources: WorkshopMenuOption[];
  tags: Record<string, string[]>;
  sortOptions: Record<string, WorkshopMenuOption[]>;
};

type MinecraftVersionOption = {
  value: string;
  label: string;
};

type AppSettings = {
  developerMode: boolean;
  steamAccountHidden: boolean;
  backgroundPath: string | null;
  wallpaperEffect: unknown;
  shaderDownloadUrl: string | null;
  shaderDownloadDirectory: string | null;
  shaderDownloadThreads: number | null;
  memoryLimitMb: number | null;
  translationLanguage: string | null;
  fontContrast: number | null;
  /** 0–200：毛玻璃 / 亚克力的模糊强度，100 = 设计基准。 */
  blurStrength: number | null;
  /** 界面字体：完整的 CSS font-family 列表；空 = 默认字体栈。 */
  fontFamily: string | null;
  /** 界面字号缩放百分比（80–130），100 = 设计基准。 */
  fontScale: number | null;
  /** 记住的窗口尺寸（逻辑像素）；null = 使用默认 840×560。 */
  windowWidth: number | null;
  windowHeight: number | null;
  modLoaderScheme: string | null;
  fastLaunch: boolean;
  silentMemoryCleanup: boolean;
  silentMemoryCleanupThreshold: number;
  cleanMemoryOnLaunch: boolean;
  /** 「Steam 网络加速」模式：auto / relay / direct；null = 没写过，按 auto。 */
  steamRelayMode: string | null;
  /** 「Steam 网络加速」的中转地址；null/空 = 内置默认。 */
  steamRelayBase: string | null;
  /** 「Steam 网络加速 › 本地代理」；null/空 = 不用显式代理。 */
  steamProxy: string | null;
};

/** 后端 save_fast_launch 的返回值：是否创建/恢复了备份。 */
type FastLaunchSaveResult = {
  backupPath: string | null;
  restored: boolean;
  warning: string | null;
};

/** 后端 memclean::MemoryStatus（camelCase 序列化）。 */
type MemoryStatus = {
  totalMb: number;
  availableMb: number;
  /** 已用物理内存百分比（0–100）。 */
  usedPercent: number;
};

/** 后端 memclean::CleanupReport。 */
type MemoryCleanupReport = {
  trimmed: number;
  skipped: number;
  failed: number;
  /** 清理前后可用物理内存差（MB），口径与 PCL2 的「内存优化」一致。 */
  freedMb: number;
  usedPercentBefore: number;
  usedPercentAfter: number;
  elapsedMs: number;
};

type BackgroundEntry = {
  id: string;
  name: string;
  path: string;
  source: string;
  workshopId: string | null;
  modName: string | null;
};

/** 内置背景：图片随前端打包在 public/backgrounds/ 下，
    settings.json 的 backgroundPath 里存成 `builtin:<id>`（磁盘上没有对应文件）。 */
type BuiltinBackground = { id: string; name: string; url: string };

/** 内置「四季轮转」：由 SeasonalWallpaper 每秒绘制，url 只用于菜单缩略图。 */
const BUILTIN_SEASONAL_ID = "seasonal";
const BUILTIN_BACKGROUNDS: BuiltinBackground[] = [
  { id: BUILTIN_SEASONAL_ID, name: "四季轮转", url: "/backgrounds/4.jpg" },
];
const BUILTIN_BACKGROUND_PREFIX = "builtin:";
/** 旧版本的内置背景 id 是 default，读到它时当成四季轮转。 */
const LEGACY_DEFAULT_BUILTIN_ID = "default";
/** 手动时刻滑块的范围：一天的分钟数 0–1439。 */
const MINUTES_PER_DAY = 1440;

function builtinBackgroundFromPath(path: string | null | undefined): BuiltinBackground | null {
  if (!path || !path.startsWith(BUILTIN_BACKGROUND_PREFIX)) {
    return null;
  }
  const rawId = path.slice(BUILTIN_BACKGROUND_PREFIX.length);
  const id = rawId === LEGACY_DEFAULT_BUILTIN_ID ? BUILTIN_SEASONAL_ID : rawId;
  return BUILTIN_BACKGROUNDS.find((background) => background.id === id) ?? null;
}

type WorkshopMenuOption = {
  value: string;
  label: string;
};

const initialWorkshopFilters: WorkshopFilters = {
  browsesort: "trend",
  days: 7,
  searchText: "",
  page: 1,
  numPerPage: 30,
  requiredTags: [],
  excludedTags: [],
};

const initialShaderDownloadFilters: ResourceFilters = {
  searchQuery: "",
  gameVersion: "All",
  selectedTag: "All",
  sortBy: "relevance",
  downloadSource: "Modrinth",
  page: 1,
  pageSize: 20,
};

const MEMORY_STEP_MB = 512;
const MEMORY_WARNING_MB = 16384;
// 静默内存清理的阈值范围：与后端 memclean::normalize_threshold 保持一致。
const MIN_SILENT_MEMORY_PERCENT = 30;
const MAX_SILENT_MEMORY_PERCENT = 95;
const SILENT_MEMORY_PERCENT_STEP = 5;
const DEFAULT_SILENT_MEMORY_PERCENT = 80;
// 模糊度（设置 › 通用）：0 = 关掉模糊，100 = 设计基准，上限与后端 normalize_blur_strength 一致。
const MIN_BLUR_STRENGTH = 0;
const MAX_BLUR_STRENGTH = 200;
const BLUR_STRENGTH_STEP = 10;
const DEFAULT_BLUR_STRENGTH = 100;
// 界面字号（设置 › 通用）：根 font-size = 16px × 百分比/100，范围与后端 normalize_font_scale 一致。
const MIN_FONT_SCALE = 80;
const MAX_FONT_SCALE = 130;
const FONT_SCALE_STEP = 5;
const DEFAULT_FONT_SCALE = 100;
// 启动器窗口尺寸下限与两份 tauri conf 的 minWidth/minHeight 一致。
const DEFAULT_WINDOW_WIDTH = 840;
const DEFAULT_WINDOW_HEIGHT = 560;
// 界面字体预设（设置 › 通用）：存进 settings.json 的是完整 CSS font-family 列表。
// "custom" 不是真实取值——选中后由旁边的输入框决定实际字体（可填任意本机字体名或 CSS 列表）。
const FONT_FAMILY_PRESETS: WorkshopMenuOption[] = [
  { value: "default", label: "默认（Inter / 系统字体）" },
  { value: "yahei", label: "微软雅黑" },
  { value: "segoe", label: "Segoe UI" },
  { value: "simsun", label: "宋体（SimSun）" },
  { value: "consolas", label: "等宽（Consolas）" },
  { value: "custom", label: "自定义…" },
];
const FONT_FAMILY_PRESET_CSS: Record<string, string> = {
  yahei: '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  segoe: '"Segoe UI Variable Text", "Segoe UI", sans-serif',
  simsun: 'SimSun, "Microsoft YaHei", serif',
  consolas: 'Consolas, "Courier New", monospace',
};

/** 由存储的字体值推导下拉选中项：空 = 默认；等于某预设 = 该预设；其余（自定义串）= custom。 */
function fontFamilyChoiceFromValue(value: string): string {
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
const PAGE_TRANSITION_MS = 200;
// 与 App.css 里光影包信息弹框退场动画（.pz-resource-file-overlay.is-closing）时长保持一致。
const RESOURCE_DIALOG_EXIT_MS = 170;
const ZOMBIE_BUDDY_WORKSHOP_ID = "3619862853";
const PZ_OPTIMIZATION_WORKSHOP_ID = "3805285544";

const navigationItems = [
  { value: "launch", label: "启动", icon: "play", description: "选择账户、调整内存并启动 Project Zomboid" },
  { value: "overview", label: "支持库", icon: "library", description: "Project Zomboid 本机环境与安装状态" },
  { value: "shots", label: "截图管理", icon: "images", description: "浏览和复制 Viewpoint 截图" },
  { value: "shaderpacks", label: "光影管理", icon: "sun-medium", description: "导入和删除 Viewpoint 光影包" },
  { value: "workshop", label: "资源中心", icon: "package-open", description: "浏览创意工坊与光影包下载资源" },
  { value: "mods", label: "模组管理", icon: "file-cog", description: "管理 Lua 与 JAR 模组" },
  { value: "settings", label: "设置", icon: "sliders-horizontal", description: "加载控制台、下载器与其它设置" },
] as const satisfies Array<{ value: ActivePage; label: string; icon: string; description: string }>;

function detectionStateFromPaths(paths: ProjectZomboidPaths): DetectionState {
  return paths.profile || paths.installPath || paths.workshopPath || paths.zombieBuddy.available
    ? { status: "found", paths }
    : { status: "notFound" };
}

function mimeTypeFor(name: string): string {
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

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatMemoryMb(memoryMb: number): string {
  return memoryMb % 1024 === 0
    ? `${memoryMb / 1024} GB`
    : `${(memoryMb / 1024).toFixed(1)} GB`;
}

function overlayPhaseLabel(phase: string): string {
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
    case "error":
      return "失败";
    default:
      return phase;
  }
}

function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return minutes > 0 ? `${minutes} 分 ${seconds} 秒` : `${seconds} 秒`;
}

/** 右下角启动时长文案：有最终结果就显示结果（含与上次对比），否则显示实时计时。 */
function startupTimingText(
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

function formatWorkshopSize(value: string): string {
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

function formatWorkshopDate(timestamp: number): string {
  if (!timestamp) {
    return "-";
  }
  return new Date(timestamp * 1000).toLocaleDateString("zh-CN");
}

type ToastKind = NotificationKind;

type ToastItem = {
  id: number;
  message: string;
  kind: ToastKind;
  closing?: boolean;
};

/** 通知历史：与 Toast 分开保存，用于设置 › 控制台里的通知记录。 */
type NotificationEntry = {
  id: number;
  message: string;
  kind: ToastKind;
  createdAt: number;
  visible: boolean;
};

type TaskStatus = "running" | "success" | "error";

type ShaderDownloadRequest = {
  url: string;
  fallbackUrls: string[];
  fileName: string;
  directory: string;
  threads: number;
};

type TaskItem = {
  id: string;
  kind: "shader-download";
  title: string;
  detail: string;
  status: TaskStatus;
  downloaded: number;
  total: number | null;
  createdAt: number;
  finishedAt: number | null;
  error: string | null;
  savedPath: string | null;
  request: ShaderDownloadRequest;
};

function ToastViewport({ toasts, onClose }: { toasts: ToastItem[]; onClose: (id: number) => void }) {
  return (
    <Box aria-live="polite" className="pz-toast-viewport">
      {toasts.map((toast) => (
        <Box
          className={`pz-toast pz-toast-${toast.kind}${toast.closing ? " pz-toast-closing" : ""}`}
          key={toast.id}
          role="status"
        >
          <Box className="pz-toast-content">
            <Box className="pz-toast-indicator" />
            <Text className="pz-toast-message">{toast.message}</Text>
            <button
              aria-label="关闭提示"
              className="pz-toast-close"
              onClick={() => onClose(toast.id)}
              type="button"
            >
              ×
            </button>
          </Box>
          <Box className="pz-toast-progress" />
        </Box>
      ))}
    </Box>
  );
}

function notificationKindLabel(kind: ToastKind): string {
  return kind === "success" ? "成功" : kind === "error" ? "错误" : "信息";
}

function notificationKindPalette(kind: ToastKind): "green" | "red" | "blue" {
  return kind === "success" ? "green" : kind === "error" ? "red" : "blue";
}

function taskStatusLabel(status: TaskStatus): string {
  return status === "success" ? "已完成" : status === "error" ? "失败" : "进行中";
}

function taskStatusPalette(status: TaskStatus): "green" | "red" | "blue" {
  return status === "success" ? "green" : status === "error" ? "red" : "blue";
}

function NotificationConsole({
  entries,
  onClear,
}: {
  entries: NotificationEntry[];
  onClear: () => void;
}) {
  return (
    <Box className="pz-panel pz-notification-console" bg="white" borderColor="gray.200" borderRadius="md" borderWidth="1px" p="4">
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="220px">
          <Text color="gray.900" fontSize="sm" fontWeight="700">通知记录</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            本次运行产生的所有通知，包括简单模式下被隐藏的信息/成功提示。
          </Text>
        </Box>
        <HStack gap="2">
          <Text color="gray.500" fontSize="xs">{entries.length} 条</Text>
          <Button disabled={entries.length === 0} onClick={onClear} size="xs" variant="ghost">清空</Button>
        </HStack>
      </HStack>
      <Box aria-live="polite" className="pz-notification-console-list" mt="3" role="log">
        {entries.length === 0 ? (
          <Text color="gray.500" fontSize="sm" py="4" textAlign="center">本次运行还没有通知。</Text>
        ) : (
          [...entries].reverse().map((entry) => (
            <HStack align="start" className="pz-notification-row" gap="2" key={entry.id}>
              <Text className="pz-notification-time">
                {new Date(entry.createdAt).toLocaleTimeString("zh-CN", { hour12: false })}
              </Text>
              <Badge colorPalette={notificationKindPalette(entry.kind)} flexShrink="0" size="sm">
                {notificationKindLabel(entry.kind)}
              </Badge>
              {!entry.visible && (
                <Badge colorPalette="gray" flexShrink="0" size="sm" variant="subtle">简单模式隐藏</Badge>
              )}
              <Text className="pz-notification-message" color="gray.700" flex="1" fontSize="xs">
                {entry.message}
              </Text>
            </HStack>
          ))
        )}
      </Box>
    </Box>
  );
}

function TaskCenterPanel({
  tasks,
  onClose,
  onClearFinished,
  onRemove,
  onRetry,
}: {
  tasks: TaskItem[];
  onClose: () => void;
  onClearFinished: () => void;
  onRemove: (id: string) => void;
  onRetry: (task: TaskItem) => void;
}) {
  const running = tasks.filter((task) => task.status === "running").length;
  const finished = tasks.length - running;
  const ordered = [...tasks].sort((left, right) => {
    if (left.status === "running" && right.status !== "running") return -1;
    if (right.status === "running" && left.status !== "running") return 1;
    return right.createdAt - left.createdAt;
  });

  return (
    <Box aria-label="任务中心" className="pz-task-panel" role="dialog">
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="180px">
          <Text color="gray.900" fontSize="md" fontWeight="700">任务中心</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            {running > 0 ? running + " 个进行中" : "没有进行中的任务"}
            {finished > 0 ? " · " + finished + " 个已结束" : ""}
          </Text>
        </Box>
        <HStack gap="1">
          <Button disabled={finished === 0} onClick={onClearFinished} size="xs" variant="ghost">清除已完成</Button>
          <Button aria-label="关闭任务中心" onClick={onClose} size="xs" variant="ghost">×</Button>
        </HStack>
      </HStack>

      <Box className="pz-task-list" mt="3">
        {ordered.length === 0 ? (
          <Text color="gray.500" fontSize="sm" py="6" textAlign="center">
            暂无任务。下载光影等后台操作会出现在这里。
          </Text>
        ) : (
          ordered.map((task) => {
            const percent =
              task.total && task.total > 0
                ? Math.min(100, Math.round((task.downloaded / task.total) * 100))
                : null;
            return (
              <Box className={["pz-task-row", "pz-task-" + task.status].join(" ")} key={task.id}>
                <HStack align="start" gap="2" justify="space-between" wrap="wrap">
                  <Box flex="1" minW="0">
                    <Text color="gray.900" fontSize="sm" fontWeight="700" overflowWrap="anywhere">
                      {task.title}
                    </Text>
                    <Text color="gray.500" fontSize="xs" mt="1" overflowWrap="anywhere">
                      {task.detail}
                    </Text>
                  </Box>
                  <Badge colorPalette={taskStatusPalette(task.status)} flexShrink="0" size="sm">
                    {taskStatusLabel(task.status)}
                  </Badge>
                </HStack>

                {task.status === "running" && (
                  <Box mt="2">
                    <Box
                      aria-label={"任务进度：" + task.title}
                      aria-valuemax={100}
                      aria-valuemin={0}
                      aria-valuenow={percent ?? undefined}
                      className="pz-download-progress pz-task-progress"
                      role="progressbar"
                    >
                      <Box as="span" style={{ width: percent !== null ? percent + "%" : "35%" }} />
                    </Box>
                    <Text color="gray.500" fontSize="xs" mt="1">
                      {formatDownloadSize(task.downloaded)}
                      {task.total ? " / " + formatDownloadSize(task.total) : ""}
                      {percent !== null ? " · " + percent + "%" : ""}
                    </Text>
                  </Box>
                )}

                {task.status === "success" && task.savedPath && (
                  <Text color="green.700" fontSize="xs" mt="2" overflowWrap="anywhere" title={task.savedPath}>
                    已保存：{task.savedPath}
                  </Text>
                )}

                {task.status === "error" && task.error && (
                  <Text color="red.600" fontSize="xs" mt="2" overflowWrap="anywhere">
                    {task.error}
                  </Text>
                )}

                <HStack gap="2" justify="space-between" mt="2" wrap="wrap">
                  <Text color="gray.500" fontSize="xs">
                    {new Date(task.createdAt).toLocaleTimeString("zh-CN", { hour12: false })}
                  </Text>
                  <HStack gap="1">
                    {task.status === "error" && (
                      <Button onClick={() => onRetry(task)} size="xs" variant="outline">重试</Button>
                    )}
                    {task.status !== "running" && (
                      <Button onClick={() => onRemove(task.id)} size="xs" variant="ghost">移除</Button>
                    )}
                  </HStack>
                </HStack>
              </Box>
            );
          })
        )}
      </Box>
    </Box>
  );
}

function DeveloperNewsCard({ revealIndex, children }: { revealIndex: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const header = Array.from(element.children).filter(
      (child): child is HTMLElement => child instanceof HTMLElement && !child.classList.contains("pz-developer-news-content"),
    );
    const body = element.querySelectorAll<HTMLElement>(
      ".pz-developer-news-content p, .pz-developer-news-content h1, .pz-developer-news-content h2, .pz-developer-news-content h3, .pz-developer-news-content h4, .pz-developer-news-content li, .pz-developer-news-content img, .pz-developer-news-content blockquote",
    );
    const targets = [...header, ...Array.from(body)];
    targets.forEach((target, index) => {
      target.classList.add("pz-news-reveal");
      target.style.setProperty("--pz-reveal-index", String(revealIndex * 4 + Math.min(index, 11)));
    });
  }, [revealIndex]);
  return (
    <Box className="pz-developer-news-item" ref={ref}>
      {children}
    </Box>
  );
}

function EmptyState({ text }: { text: string }) {
  return (
    <Box className="pz-empty-state" borderColor="gray.200" borderRadius="sm" borderWidth="1px" p="6" textAlign="center">
      <Text color="gray.500">{text}</Text>
    </Box>
  );
}

function PathValue({
  label,
  value,
  fallback,
  error,
}: {
  label: string;
  value: string | null;
  fallback: string | null;
  error: boolean;
}) {
  return (
    <Box>
      <Text color="gray.500" fontSize="sm" fontWeight="600">
        {label}
      </Text>
      {value ? (
        <Box className="pz-value-box" bg="gray.50" borderColor="gray.200" borderRadius="sm" borderWidth="1px" mt="2" p="3">
          <Text color="gray.800" fontFamily="mono" fontSize="sm" overflowWrap="anywhere">
            {value}
          </Text>
        </Box>
      ) : (
        <Text color={error ? "red.600" : "gray.600"} mt="2">
          {fallback}
        </Text>
      )}
    </Box>
  );
}

function MediaPanel({
  title,
  description,
  directory,
  developerMode = false,
  children,
}: {
  title: string;
  description: string;
  directory?: string;
  developerMode?: boolean;
  children: ReactNode;
}) {
  return (
    <Box className="pz-panel pz-media-panel">
      <Box className="pz-media-panel-header">
        <Text color="gray.900" fontSize="lg" fontWeight="700">
          {title}
        </Text>
        {developerMode && (
          <Text color="gray.500" fontSize="sm" mt="1">
            {description}
          </Text>
        )}
        {developerMode && directory && (
          <Text color="gray.400" fontFamily="mono" fontSize="xs" mt="2" overflowWrap="anywhere">
            {directory}
          </Text>
        )}
      </Box>
      <Box className="pz-media-panel-body">{children}</Box>
    </Box>
  );
}

function ShotThumbnail({ name }: { name: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    invoke<ArrayBuffer>("read_viewpoint_shot", { name })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(name) }));
        setUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setFailed(true);
        }
      });

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [name]);

  return (
    <Box
      alignItems="center"
      aspectRatio="16 / 10"
      bg="gray.200"
      className="pz-thumbnail"
      display="flex"
      justifyContent="center"
      overflow="hidden"
    >
      {url ? (
        <FallbackImage
          alt={name}
          src={url}
          style={{ height: "100%", objectFit: "cover", width: "100%" }}
        />
      ) : (
        <Text color="gray.500" fontSize="xs">
          {failed ? "无法加载缩略图" : "正在加载缩略图..."}
        </Text>
      )}
    </Box>
  );
}

function WorkshopTagPicker({
  tags,
  requiredTags,
  excludedTags,
  onToggle,
  onRemove,
  compact = false,
}: {
  tags: string[];
  requiredTags: string[];
  excludedTags: string[];
  onToggle: (tag: string, mode: WorkshopTagMode) => void;
  onRemove: (tag: string, mode: WorkshopTagMode) => void;
  compact?: boolean;
}) {
  const renderTagMenu = (mode: WorkshopTagMode) => {
    const selectedTags = mode === "requiredTags" ? requiredTags : excludedTags;
    const colorPalette = mode === "requiredTags" ? "blue" : "red";
    const label = mode === "requiredTags" ? "包含" : "排除";

    return (
      <Menu.Root>
        <Menu.Trigger asChild>
          <Button
            colorPalette={colorPalette}
            size="xs"
            variant="outline"
          >
            <HStack gap="1">
              <Text as="span">{label}</Text>
              <Text aria-hidden="true" as="span" className="pz-workshop-tag-menu-chevron">
                ▾
              </Text>
            </HStack>
          </Button>
        </Menu.Trigger>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content" minW="220px">
            {tags.length > 0 ? (
              tags.map((tag) => {
                const selected = selectedTags.includes(tag);
                return (
                  <Menu.Item
                    key={tag}
                    value={tag}
                    onSelect={() => onToggle(tag, mode)}
                  >
                    <Text flex="1">{tag}</Text>
                    {selected && (
                      <Text color={`${colorPalette}.600`} fontSize="xs">
                        已选择
                      </Text>
                    )}
                  </Menu.Item>
                );
              })
            ) : (
              <Text color="gray.500" fontSize="sm" px="3" py="2">
                暂无可用标签
              </Text>
            )}
          </Menu.Content>
        </Menu.Positioner>
      </Menu.Root>
    );
  };

  return (
    <Box className={`pz-workshop-tag-picker${compact ? " pz-workshop-tag-picker-compact" : ""}`}>
      <HStack align="center" gap="3" wrap="wrap">
        <Text className="pz-filter-label" fontSize="xs" fontWeight="700">
          标签
        </Text>
        <HStack gap="1.5">
          {renderTagMenu("requiredTags")}
          {renderTagMenu("excludedTags")}
        </HStack>
        {!compact && (
          <Text className="pz-workshop-tag-hint" color="gray.500" fontSize="xs">
            从菜单中选择标签
          </Text>
        )}
      </HStack>

      {!compact && (
        <WorkshopTagSelectionSummary
          excludedTags={excludedTags}
          onRemove={onRemove}
          requiredTags={requiredTags}
        />
      )}
    </Box>
  );
}

function WorkshopTagSelectionSummary({
  requiredTags,
  excludedTags,
  onRemove,
}: {
  requiredTags: string[];
  excludedTags: string[];
  onRemove: (tag: string, mode: WorkshopTagMode) => void;
}) {
  if (requiredTags.length === 0 && excludedTags.length === 0) {
    return null;
  }

  return (
    <HStack className="pz-workshop-current-filters" gap="1.5" mt="3" wrap="wrap">
      <Text color="gray.500" fontSize="xs" whiteSpace="nowrap">
        当前筛选
      </Text>
      {requiredTags.map((tag) => (
        <Button
          key={`required-${tag}`}
          colorPalette="blue"
          onClick={() => onRemove(tag, "requiredTags")}
          size="xs"
          variant="outline"
        >
          包含 · {tag} ×
        </Button>
      ))}
      {excludedTags.map((tag) => (
        <Button
          key={`excluded-${tag}`}
          colorPalette="red"
          onClick={() => onRemove(tag, "excludedTags")}
          size="xs"
          variant="outline"
        >
          排除 · {tag} ×
        </Button>
      ))}
    </HStack>
  );
}

function WorkshopDropdown({
  ariaLabel,
  options,
  value,
  width,
  disabled,
  onChange,
}: {
  ariaLabel: string;
  options: WorkshopMenuOption[];
  value: string;
  width: string | Record<string, string>;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const selectedOption = options.find((option) => option.value === value);

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button
          aria-label={ariaLabel}
          className="pz-workshop-menu-trigger"
          disabled={disabled}
          size="sm"
          variant="outline"
          width={width}
        >
          <HStack gap="2" justify="space-between" width="100%">
            <Text flex="1" minW="0" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
              {selectedOption?.label ?? value}
            </Text>
            <Text aria-hidden="true" className="pz-workshop-tag-menu-chevron">
              ▾
            </Text>
          </HStack>
        </Button>
      </Menu.Trigger>
      <Menu.Positioner className="pz-menu-positioner">
        <Menu.Content className="pz-workshop-tag-menu-content pz-dropdown-menu-content">
          {options.map((option) => (
            <Menu.Item
              key={option.value}
              value={option.value}
              onSelect={() => onChange(option.value)}
            >
              <Text flex="1">{option.label}</Text>
              {option.value === value && (
                <Text color="blue.600" fontSize="xs">
                  已选择
                </Text>
              )}
            </Menu.Item>
          ))}
        </Menu.Content>
      </Menu.Positioner>
    </Menu.Root>
  );
}

function formatDownloadSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function ResourceDownloadBrowser({
  options,
  versions,
  onToast,
  onDownload,
}: {
  options: ResourceOptions | null;
  versions: MinecraftVersionOption[];
  onToast: (message: string, kind?: ToastKind) => void;
  onDownload: (file: ResourceFile) => void;
}) {
  const [filters, setFilters] = useState<ResourceFilters>(initialShaderDownloadFilters);
  const [response, setResponse] = useState<ResourceSearchResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedResource, setSelectedResource] = useState<ResourceItem | null>(null);
  const [versionPacks, setVersionPacks] = useState<ResourceVersionPack[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  // 关闭时先播退场动画再卸载，所以需要一个"正在关闭"的中间状态。
  const [closingResourceDialog, setClosingResourceDialog] = useState(false);
  const requestId = useRef(0);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resourceDialogRef = useRef<HTMLDivElement | null>(null);
  const resourceDialogCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 上一次稳定下来的弹框高度（px）：把"加载中 → 版本列表就绪"的高度跳变做成过渡动画。
  const resourceDialogHeightRef = useRef<number | null>(null);

  const sourceOptions = options?.sources ?? [];
  const tagOptions = (options?.tags[filters.downloadSource] ?? ["All"]).map((tag) => ({
    value: tag,
    label: tag === "All" ? "全部" : tag,
  }));
  const sortOptions = options?.sortOptions[filters.downloadSource] ?? [];
  const selectedSource = sourceOptions.find((option) => option.value === filters.downloadSource);

  async function loadResources(nextFilters: ResourceFilters = filters) {
    const currentRequest = ++requestId.current;
    setLoading(true);
    try {
      const nextResponse = await invoke<ResourceSearchResponse>("fetch_shader_resources", {
        query: {
          resourceType: "shader",
          searchQuery: nextFilters.searchQuery,
          gameVersion: nextFilters.gameVersion,
          selectedTag: nextFilters.selectedTag,
          sortBy: nextFilters.sortBy,
          downloadSource: nextFilters.downloadSource,
          page: nextFilters.page,
          pageSize: nextFilters.pageSize,
        },
      });
      if (currentRequest === requestId.current) {
        setResponse(nextResponse);
      }
    } catch (error: unknown) {
      if (currentRequest === requestId.current) {
        setResponse(null);
        onToast(error instanceof Error ? error.message : String(error), "error");
      }
    } finally {
      if (currentRequest === requestId.current) {
        setLoading(false);
      }
    }
  }

  function updateFilters(changes: Partial<ResourceFilters>, delay = 250, resetPage = true) {
    const nextFilters = {
      ...filters,
      ...changes,
      page: resetPage ? 1 : changes.page ?? filters.page,
    };
    setFilters(nextFilters);
    if (searchTimer.current) {
      clearTimeout(searchTimer.current);
    }
    searchTimer.current = setTimeout(() => void loadResources(nextFilters), delay);
  }

  function resetFilters() {
    const nextFilters = {
      ...initialShaderDownloadFilters,
      downloadSource: filters.downloadSource,
      sortBy: filters.downloadSource === "CurseForge" ? "Popularity" : "relevance",
    };
    setFilters(nextFilters);
    void loadResources(nextFilters);
  }

  async function openResource(resource: ResourceItem) {
    // 退场动画还没播完就又点了另一个光影包：取消关闭，直接换内容。
    if (resourceDialogCloseTimer.current != null) {
      clearTimeout(resourceDialogCloseTimer.current);
      resourceDialogCloseTimer.current = null;
    }
    setClosingResourceDialog(false);
    setSelectedResource(resource);
    setVersionPacks([]);
    setLoadingFiles(true);
    try {
      const files = await invoke<ResourceVersionPack[]>("fetch_shader_resource_files", {
        resourceId: resource.id,
        downloadSource: resource.source,
      });
      setVersionPacks(files);
    } catch (error: unknown) {
      onToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLoadingFiles(false);
    }
  }

  function closeResourceDialog() {
    if (resourceDialogCloseTimer.current != null) {
      return;
    }
    setClosingResourceDialog(true);
    resourceDialogCloseTimer.current = setTimeout(() => {
      resourceDialogCloseTimer.current = null;
      setClosingResourceDialog(false);
      setSelectedResource(null);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  useEffect(
    () => () => {
      if (resourceDialogCloseTimer.current != null) {
        clearTimeout(resourceDialogCloseTimer.current);
        resourceDialogCloseTimer.current = null;
      }
    },
    [],
  );

  // 弹框高度随内容变化（读取版本文件 → 列表就绪），量高后显式过渡，见 useDialogAutoHeight。
  useDialogAutoHeight(resourceDialogRef, resourceDialogHeightRef, [
    selectedResource,
    loadingFiles,
    versionPacks,
  ]);
  useEffect(() => {
    if (options) {
      void loadResources();
    }
  }, [options, versions.length]);

  return (
    <Box className="pz-resource-browser">
      <Box className="pz-panel-header" p={{ base: "3", md: "4" }}>
        <HStack align="start" justify="space-between" gap="3" wrap="wrap">
          <Box>
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              光影包下载
            </Text>
          </Box>
          <HStack gap="2">
            <Button loading={loading} onClick={() => void loadResources()} size="sm" variant="outline">
              刷新
            </Button>
            <Button onClick={resetFilters} size="sm" variant="ghost">
              重置筛选
            </Button>
          </HStack>
        </HStack>
      </Box>

      <Box className="pz-resource-download-body">
        <HStack className="pz-resource-filter-row" gap="2" wrap="wrap">
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">标签</Text>
            <WorkshopDropdown
              ariaLabel="选择光影标签"
              onChange={(value) => updateFilters({ selectedTag: value })}
              options={tagOptions}
              value={filters.selectedTag}
              width={{ base: "96px", md: "112px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">游戏版本</Text>
            <WorkshopDropdown
              ariaLabel="选择 Minecraft 游戏版本"
              onChange={(value) => updateFilters({ gameVersion: value })}
              options={versions.map((version) => ({ value: version.value, label: version.label }))}
              value={filters.gameVersion}
              width={{ base: "104px", md: "128px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">下载源</Text>
            <WorkshopDropdown
              ariaLabel="选择下载源"
              onChange={(value) =>
                updateFilters({
                  downloadSource: value,
                  selectedTag: "All",
                  sortBy: value === "CurseForge" ? "Popularity" : "relevance",
                })
              }
              options={sourceOptions}
              value={filters.downloadSource}
              width={{ base: "120px", md: "145px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">排序</Text>
            <WorkshopDropdown
              ariaLabel="选择资源排序方式"
              onChange={(value) => updateFilters({ sortBy: value })}
              options={sortOptions}
              value={filters.sortBy}
              width={{ base: "100px", md: "124px" }}
            />
          </HStack>
        </HStack>

        <HStack className="pz-resource-search-row" gap="2" mt="2" wrap="wrap">
          <Text className="pz-filter-label" whiteSpace="nowrap">名称</Text>
          <Input
            flex="1"
            maxW={{ base: "100%", md: "340px" }}
            onChange={(event) => updateFilters({ searchQuery: event.currentTarget.value }, 400)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                void loadResources();
              }
            }}
            placeholder="搜索光影包名称"
            size="sm"
            value={filters.searchQuery}
          />
          <Button colorPalette="blue" loading={loading} onClick={() => void loadResources()} size="sm">
            搜索
          </Button>
        </HStack>

        <HStack className="pz-resource-meta-row" color="gray.500" fontSize="xs" justify="space-between" mt="3" wrap="wrap">
          <Text>{selectedSource?.label ?? filters.downloadSource} · Minecraft Shader Packs</Text>
          <Text>{response ? `共 ${response.total.toLocaleString()} 个资源` : "等待加载"}</Text>
        </HStack>

        <Box className="pz-resource-download-list">
          {loading && !response ? (
            <Text color="gray.500" py="6" textAlign="center">正在读取 Minecraft 光影源...</Text>
          ) : response && response.items.length > 0 ? (
            response.items.map((resource) => (
              <Box
                className="pz-resource-download-item"
                key={`${resource.source}-${resource.id}`}
                onClick={() => void openResource(resource)}
                role="button"
                tabIndex={0}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    void openResource(resource);
                  }
                }}
              >
                <Box className="pz-resource-download-icon">
                  {resource.iconUrl ? <FallbackImage alt="" fallbackSrc={resource.iconFallbackUrl} loading="lazy" src={resource.iconUrl} /> : <Text>MC</Text>}
                </Box>
                <Box minW="0" flex="1">
                  <HStack align="start" className="pz-resource-download-title-row" gap="2" wrap="wrap">
                    <Text color="gray.900" flex="1" fontSize="sm" fontWeight="700" lineClamp="1" minW="0">
                      {resource.name}
                    </Text>
                    <HStack className="pz-resource-download-tags" gap="1" flexShrink="0" justify="flex-end" wrap="wrap">
                      <Badge colorPalette={resource.source === "Modrinth" ? "green" : "orange"} size="sm">
                        {resource.source}
                      </Badge>
                      {resource.tags.slice(0, 3).map((tag) => (
                        <Badge key={tag} colorPalette="blue" size="sm" variant="subtle">{tag}</Badge>
                      ))}
                    </HStack>
                  </HStack>
                  <Text color="gray.500" fontSize="xs" lineClamp="2" mt="1">
                    {resource.description || "暂无描述"}
                  </Text>
                </Box>
              </Box>
            ))
          ) : (
            <EmptyState text="没有匹配的光影包。" />
          )}
        </Box>

        {response && Math.ceil(response.total / response.pageSize) > 1 && (
          <HStack justify="center" mt="3" gap="1" wrap="wrap">
            <Button
              disabled={filters.page <= 1}
              onClick={() => updateFilters({ page: filters.page - 1 }, 0, false)}
              size="sm"
              variant="outline"
            >
              上一页
            </Button>
            <Text color="gray.500" fontSize="xs" px="2">
              第 {response.page} / {Math.ceil(response.total / response.pageSize)} 页
            </Text>
            <Button
              disabled={filters.page >= Math.ceil(response.total / response.pageSize)}
              onClick={() => updateFilters({ page: filters.page + 1 }, 0, false)}
              size="sm"
              variant="outline"
            >
              下一页
            </Button>
          </HStack>
        )}
      </Box>

      {/* 正文框 .pz-main 带 backdrop-filter + overflow:hidden，会让 position:fixed 的弹层
          以它为包含块并被裁掉，所以这里必须 Portal 到 body 才能覆盖整个窗口。 */}
      {selectedResource && (
        <Portal>
          <Box
            className={`pz-resource-file-overlay ${closingResourceDialog ? "is-closing" : ""}`}
            onClick={closeResourceDialog}
          >
            <Box className="pz-resource-file-dialog" onClick={(event) => event.stopPropagation()} ref={resourceDialogRef}>
              <HStack justify="space-between" gap="3">
                <Box minW="0">
                  <Text color="gray.900" fontWeight="700" lineClamp="1">{selectedResource.name}</Text>
                  <Text color="gray.500" fontSize="xs" mt="1">{selectedResource.source} · 选择文件下载</Text>
                  <HStack color="gray.500" fontSize="xs" gap="3" mt="1" wrap="wrap">
                    <Text>更新 {new Date(selectedResource.lastUpdated).toLocaleDateString("zh-CN")}</Text>
                    <Text>下载 {selectedResource.downloads.toLocaleString()}</Text>
                    <Text>作者 {selectedResource.author || "未知作者"}</Text>
                  </HStack>
                </Box>
                <Button aria-label="关闭文件列表" onClick={closeResourceDialog} size="sm" variant="ghost">×</Button>
              </HStack>
              <Box className="pz-resource-file-list">
                {loadingFiles ? (
                  <Text color="gray.500" py="5" textAlign="center">正在读取版本文件...</Text>
                ) : versionPacks.length > 0 ? (
                  versionPacks.flatMap((pack) => pack.items).map((file) => (
                    <HStack className="pz-resource-file-row" key={`${file.fileName}-${file.fileDate}`} justify="space-between" gap="3">
                      <Box minW="0">
                        <Text color="gray.800" fontSize="sm" fontWeight="600" overflowWrap="anywhere">{file.fileName}</Text>
                        <Text color="gray.500" fontSize="xs" mt="1">
                          {file.releaseType} · {file.gameVersions.slice(0, 3).join("、") || "版本未知"} · 下载 {file.downloads.toLocaleString()}
                        </Text>
                      </Box>
                      <Button
                        colorPalette="blue"
                        flexShrink="0"
                        onClick={() => {
                          onDownload(file);
                          closeResourceDialog();
                        }}
                        size="sm"
                      >
                        下载
                      </Button>
                    </HStack>
                  ))
                ) : (
                  <Text color="gray.500" py="5" textAlign="center">没有可用下载文件。</Text>
                )}
              </Box>
            </Box>
          </Box>
        </Portal>
      )}
    </Box>
  );
}

type FastLaunchCacheStatus = {
  exists: boolean;
  generatedAt: number;
  modCount: number;
  modSetHash: string;
  matches: boolean;
};

/** 极速启动的缓存状态行：注入器就是按这个指纹判断能不能用缓存。
    数据源 `fast_launch_cache_status`（读缓存文件 + 和当前启用列表比对）。 */
function FastLaunchCacheStatusLine({ enabled }: { enabled: boolean }) {
  const [status, setStatus] = useState<FastLaunchCacheStatus | null>(null);

  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      return;
    }
    let active = true;
    invoke<FastLaunchCacheStatus>("fast_launch_cache_status")
      .then((value) => {
        if (active) {
          setStatus(value);
        }
      })
      .catch(() => {
        if (active) {
          setStatus(null);
        }
      });
    return () => {
      active = false;
    };
  }, [enabled]);

  if (!enabled) {
    return null;
  }
  if (!status) {
    return (
      <Text color="gray.500" fontSize="xs" mt="1">
        正在检查加速记录…
      </Text>
    );
  }
  if (!status.exists) {
    return (
      <Text color="gray.500" fontSize="xs" mt="1">
        还没有加速记录：开启本开关后，到「模组管理 › JAR」点一次「刷新 JAR 列表」即可生成。
      </Text>
    );
  }
  return (
    <Text color={status.matches ? "gray.500" : "orange.700"} fontSize="xs" mt="1">
      上次加速生成于 {new Date(status.generatedAt * 1000).toLocaleString()} · 记录了 {status.modCount} 个模组
      {status.matches
        ? " · 与当前启用的模组一致，启动时会直接使用"
        : " · 模组列表已变化，建议到「模组管理 › JAR」点一次「刷新 JAR 列表」更新"}
    </Text>
  );
}

function BackgroundThumbnail({ path }: { path: string }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;
    invoke<ArrayBuffer>("read_background", { path })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes]));
        setUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setUrl(null);
        }
      });

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [path]);

  return url ? (
    <FallbackImage alt="" className="pz-background-menu-thumbnail" loading="lazy" src={url} />
  ) : (
    <Box className="pz-background-menu-thumbnail-placeholder" />
  );
}

function JarModPoster({ name, path }: { name: string; path: string | null }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    if (!path) {
      setUrl(null);
      return () => {
        active = false;
      };
    }

    invoke<ArrayBuffer>("read_jar_mod_poster", { path })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(path) }));
        setUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setUrl(null);
        }
      });

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [path]);

  const initial = name.trim().charAt(0).toUpperCase() || "?";
  return (
    <Box className="pz-jar-mod-poster" aria-hidden="true">
      {url ? <FallbackImage alt="" loading="lazy" src={url} /> : <Box className="pz-jar-mod-poster-placeholder"><Text>{initial}</Text></Box>}
    </Box>
  );
}

function isModRowInteractiveTarget(target: EventTarget) {
  return (target as HTMLElement).closest(
    "button, input, textarea, select, a, [data-pz-mod-switch], .pz-jar-mod-details-shell",
  ) !== null;
}

function LuaModPoster({ name, path }: { name: string; path: string | null }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;
    if (!path) { setUrl(null); return () => { active = false; }; }
    invoke<ArrayBuffer>("read_lua_mod_poster", { path })
      .then((bytes) => { if (active) { objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(path) })); setUrl(objectUrl); } })
      .catch(() => { if (active) setUrl(null); });
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [path]);
  const initial = name.trim().charAt(0).toUpperCase() || "?";
  return <Box className="pz-jar-mod-poster" aria-hidden="true">{url ? <FallbackImage alt="" loading="lazy" src={url} /> : <Box className="pz-jar-mod-poster-placeholder"><Text>{initial}</Text></Box>}</Box>;
}

function SleepingModRow({
  render,
}: {
  render: () => ReactNode;
}) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [awake, setAwake] = useState(false);
  const [sleepHeight, setSleepHeight] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (awake && shellRef.current) {
      setSleepHeight(shellRef.current.offsetHeight);
    }
  }, [awake]);

  useEffect(() => {
    const element = shellRef.current;
    if (!element) {
      return;
    }
    if (typeof IntersectionObserver === "undefined") {
      setAwake(true);
      return;
    }

    const root = element.closest(".pz-lua-mod-list, .pz-jar-mods-body");
    const observer = new IntersectionObserver(
      ([entry]) => {
        const nextAwake = entry?.isIntersecting ?? true;
        if (!nextAwake && shellRef.current) {
          setSleepHeight(shellRef.current.offsetHeight);
        }
        setAwake(nextAwake);
      },
      {
        root,
        rootMargin: "480px 0px",
      },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <Box
      className={`pz-mod-sleep-row${awake ? " is-awake" : " is-sleeping"}`}
      ref={shellRef}
      style={!awake && sleepHeight !== null ? { height: `${sleepHeight}px` } : undefined}
    >
      {awake ? render() : <Box className="pz-mod-sleep-placeholder" />}
    </Box>
  );
}

function ModBatchBar({
  busyAction,
  negativeLabel,
  onClear,
  onInvert,
  onNegative,
  onPositive,
  onSelectAll,
  positiveLabel,
  selectedCount,
  totalCount,
}: {
  busyAction: "positive" | "negative" | null;
  negativeLabel: string;
  onClear: () => void;
  onInvert: () => void;
  onNegative: () => void;
  onPositive: () => void;
  onSelectAll: () => void;
  positiveLabel: string;
  selectedCount: number;
  totalCount: number;
}) {
  const busy = busyAction !== null;
  return (
    <HStack className="pz-mod-batch-bar" gap="1" wrap="wrap">
      <Text className="pz-mod-batch-count">已选 {selectedCount} / {totalCount}</Text>
      <Menu.Root>
        <Menu.Trigger asChild>
          <Button disabled={busy || totalCount === 0} size="xs" variant="ghost">
            选择操作
          </Button>
        </Menu.Trigger>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content" minW="150px">
            <Menu.Item onSelect={onSelectAll} value="select-all">全选当前列表</Menu.Item>
            <Menu.Item onSelect={onInvert} value="invert">反选当前列表</Menu.Item>
            <Menu.Item disabled={selectedCount === 0} onSelect={onClear} value="clear">取消选择</Menu.Item>
          </Menu.Content>
        </Menu.Positioner>
      </Menu.Root>
      <Box className="pz-mod-batch-spacer" />
      <Button
        colorPalette="green"
        disabled={busy || selectedCount === 0}
        loading={busyAction === "positive"}
        onClick={onPositive}
        size="xs"
        variant="subtle"
      >
        {positiveLabel}
      </Button>
      <Button
        colorPalette="red"
        disabled={busy || selectedCount === 0}
        loading={busyAction === "negative"}
        onClick={onNegative}
        size="xs"
        variant="subtle"
      >
        {negativeLabel}
      </Button>
    </HStack>
  );
}

function ZombieBuddyLoaderPanel({
  busy,
  detection,
  developerMode,
  expanded,
  luaMod,
  schemeEnabled,
  onOpenModLoaderSettings,
  onToggle,
  onToggleLuaMod,
}: {
  busy: boolean;
  detection: DetectionState;
  developerMode: boolean;
  expanded: boolean;
  luaMod: LuaModEntry | null;
  /** 「模组加载器方案」是否选中了 ZombieBuddy。 */
  schemeEnabled: boolean;
  onOpenModLoaderSettings: () => void;
  onToggle: () => void;
  onToggleLuaMod: (mod: LuaModEntry) => void;
}) {
  const loader = detection.status === "found" ? detection.paths.zombieBuddy : null;
  const statusLabel =
    detection.status === "checking"
      ? "检测中"
      : loader?.installed
        ? "已安装"
        : loader?.available
          ? "待安装"
          : "未检测到";
  const statusPalette =
    loader?.installed ? "green" : loader?.available ? "orange" : "gray";

  return (
    <Box className={`pz-jar-conflict-panel pz-zombiebuddy-loader-panel${expanded ? " is-expanded" : ""}`} role="status">
      <Button
        aria-expanded={expanded}
        className="pz-jar-conflict-summary pz-zombiebuddy-loader-summary"
        onClick={onToggle}
        size="sm"
        variant="ghost"
      >
        <HStack gap="2" minW="0" wrap="wrap">
          <Box className="pz-jar-conflict-indicator pz-zombiebuddy-loader-indicator" />
          <Text color="gray.800" fontSize="sm" fontWeight="700">ZombieBuddy 模组加载器</Text>
          <Badge colorPalette={statusPalette} size="sm">{statusLabel}</Badge>
          {!schemeEnabled && <Badge colorPalette="gray" size="sm">未启用</Badge>}
          {developerMode && (
            <Badge colorPalette={luaMod?.enabled ? "green" : "gray"} size="sm">
              {luaMod?.enabled ? "Lua 模组已启用" : "Lua 模组未启用"}
            </Badge>
          )}
        </HStack>
        <Text aria-hidden="true" className="pz-jar-conflict-chevron">{expanded ? "▴" : "▾"}</Text>
      </Button>
      <Box className="pz-jar-conflict-details-shell" aria-hidden={!expanded}>
        <Box className="pz-jar-conflict-details">
          {developerMode && (
            <>
              <Text color="gray.500" fontSize="xs">
                这是 JAR 模组加载器，不属于普通 Lua 可排序模组。安装并启用后，ZombieBuddy 会负责审批和加载带 @Patch / @Exposer 注解的 JAR。
              </Text>
              <HStack gap="3" mt="2" wrap="wrap">
                <Text color="gray.600" fontSize="xs">Workshop ID：{ZOMBIE_BUDDY_WORKSHOP_ID}</Text>
                <Text color="gray.600" fontSize="xs">加载器 ID：ZombieBuddy</Text>
                <Text color="gray.600" fontSize="xs">
                  加载器方案：{schemeEnabled ? "ZombieBuddy" : "不使用（原版加载）"}
                </Text>
                {luaMod && (
                  <Text color="gray.600" fontSize="xs">
                    Lua 模组：{luaMod.enabled ? "已启用" : "未启用"}
                  </Text>
                )}
              </HStack>
              {loader?.sourcePath && (
                <Text color="gray.500" fontSize="xs" mt="2" overflowWrap="anywhere">
                  来源：{loader.sourcePath}
                </Text>
              )}
            </>
          )}
          {!loader?.installed && loader?.missingComponents.length ? (
            <Text color="gray.500" fontSize="xs" mt="2">
              缺少：{loader.missingComponents.join("、")}
            </Text>
          ) : null}
          <HStack gap="2" mt="3" wrap="wrap">
            {luaMod && (
              <Button
                colorPalette={luaMod.enabled ? "red" : "green"}
                disabled={busy}
                loading={busy}
                onClick={() => onToggleLuaMod(luaMod)}
                size="xs"
                variant="outline"
              >
                {luaMod.enabled ? "停用 Lua 模组" : "启用 Lua 模组"}
              </Button>
            )}
            {!loader?.installed && (
              <Button colorPalette="blue" onClick={onOpenModLoaderSettings} size="xs" variant="outline">
                前往 设置 › 模组加载器
              </Button>
            )}
          </HStack>
        </Box>
      </Box>
    </Box>
  );
}

/** 支持库页的安装项：外观与「模组管理」里的模组行一致（同样的卡片、摘要排版、
    徽标区和 .pz-jar-mod-details-shell 展开过渡），三行统一放在一个面板里。
    只复用 pz-mod-row 卡片外壳，不带 pz-jar-mod-row：后者会套用模组的
    启用/拒绝状态底色（灰底 + 左侧状态条），而这里表达的是安装状态，
    由右侧状态标签说明，行本身保持中性。 */
function SupportOptionRow({
  badge,
  badgePalette,
  description,
  expanded,
  name,
  onToggle,
  children,
}: {
  badge: string;
  badgePalette: "green" | "orange" | "gray" | "blue";
  description: string;
  expanded: boolean;
  name: string;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <Box
      className={`pz-mod-row pz-support-option${expanded ? " pz-jar-mod-row-expanded" : ""}`}
      onClick={(event) => {
        const target = event.target as HTMLElement;
        if (target.closest("button, .pz-jar-mod-details-shell")) {
          return;
        }
        onToggle();
      }}
    >
      <Box className="pz-mod-summary pz-compact-mod-summary">
        <Box className="pz-mod-select-area">
          <Box className="pz-jar-mod-summary-content" minW="0">
            <Text className="pz-jar-mod-name" color="gray.900" fontWeight="700" overflowWrap="anywhere">
              {name}
            </Text>
            <Text className="pz-jar-mod-description" color="gray.500">
              {description}
            </Text>
          </Box>
        </Box>
        <HStack className="pz-jar-mod-badges" gap="1" wrap="nowrap">
          <Badge colorPalette={badgePalette}>{badge}</Badge>
        </HStack>
        <HStack align="center" className="pz-mod-actions" gap="1">
          <Button
            aria-expanded={expanded}
            aria-label={`${expanded ? "收起" : "展开"} ${name}详情`}
            className="pz-jar-mod-detail-button"
            onClick={(event) => {
              event.stopPropagation();
              onToggle();
            }}
            size="xs"
            title={expanded ? "收起" : "详情"}
            variant="ghost"
          >
            {expanded ? "×" : "⋯"}
          </Button>
        </HStack>
      </Box>
      <Box aria-hidden={!expanded} className="pz-jar-mod-details-shell">
        <Box className="pz-jar-mod-details">
          <Box className="pz-jar-mod-details-body">{children}</Box>
        </Box>
      </Box>
    </Box>
  );
}

function BackgroundMenu({
  customBackgrounds,
  workshopBackgrounds,
  workshopLoading,
  workshopScanProgress,
  onChooseCustom,
  onOpenRecommendedWorkshop,
  onScanWorkshop,
  onSelect,
  onSelectBuiltin,
  wallpaperEffect,
  wallpaperSettings,
  onSetWallpaperDay,
  onSetWallpaperTime,
  onToggleWallpaperPin,
  onToggleWallpaperAutoContrast,
}: {
  customBackgrounds: BackgroundEntry[];
  workshopBackgrounds: BackgroundEntry[];
  workshopLoading: boolean;
  workshopScanProgress: { scanned: number; total: number };
  onChooseCustom: () => void;
  onOpenRecommendedWorkshop: (workshopId: string) => void;
  onScanWorkshop: () => void;
  onSelect: (background: BackgroundEntry) => void;
  onSelectBuiltin: (background: BuiltinBackground) => void;
  wallpaperEffect: WallpaperEffect;
  wallpaperSettings: WallpaperEffectSettings;
  onSetWallpaperDay: (dayOfYear: number | null) => void;
  onSetWallpaperTime: (timeMinutes: number | null) => void;
  onToggleWallpaperPin: () => void;
  onToggleWallpaperAutoContrast: () => void;
}) {
  const workshopGroups = Array.from(
    workshopBackgrounds.reduce((groups, background) => {
      const key = `${background.workshopId ?? "unknown"}:${background.modName ?? background.name}`;
      const current = groups.get(key);
      if (current) {
        current.push(background);
      } else {
        groups.set(key, [background]);
      }
      return groups;
    }, new Map<string, BackgroundEntry[]>()),
  );

  const renderBackgroundItem = (background: BackgroundEntry, detailOnly = false) => (
    <Menu.Item
      key={background.id}
      value={background.id}
      onSelect={() => onSelect(background)}
    >
      <BackgroundThumbnail path={background.path} />
      <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
        {detailOnly ? (background.name.split("·", 2)[1]?.trim() ?? background.name) : background.name}
      </Text>
    </Menu.Item>
  );

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button className="pz-background-menu-trigger" size="sm" variant="outline">
          <HStack gap="1.5">
            <Text>自定义背景</Text>
            <Text aria-hidden="true" className="pz-workshop-tag-menu-chevron">
              ▾
            </Text>
          </HStack>
        </Button>
      </Menu.Trigger>
      <Portal>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 4,
              strategy: "fixed",
              flip: true,
              shift: 8,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">内置背景</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                {BUILTIN_BACKGROUNDS.map((background) => (
                  <Menu.Item
                    key={background.id}
                    value={`builtin-${background.id}`}
                    onSelect={() => onSelectBuiltin(background)}
                  >
                    <FallbackImage alt="" className="pz-background-menu-thumbnail" loading="lazy" src={background.url} />
                    <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                      {background.name}
                    </Text>
                  </Menu.Item>
                ))}
                <Menu.Separator />
                <Text color="gray.500" fontSize="xs" px="3" py="1">
                  四季轮转 · {wallpaperEffect.blendLabel} · {wallpaperEffect.clockLabel}
                  {wallpaperEffect.pinned ? " · 已固定" : wallpaperSettings.mode === "manual" ? " · 手动" : ""}
                </Text>
                <Menu.Item value="wallpaper-pin" onSelect={onToggleWallpaperPin}>
                  {wallpaperEffect.pinned ? "取消固定" : "固定当前效果"}
                </Menu.Item>
                <Menu.Item value="wallpaper-auto-contrast" onSelect={onToggleWallpaperAutoContrast}>
                  {wallpaperEffect.autoContrast ? "✓ " : ""}自动对比度（{wallpaperEffect.contrast.toFixed(2)}x）
                </Menu.Item>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 4,
                    strategy: "fixed",
                    flip: true,
                    shift: 8,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">手动季节</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item value="season-auto" onSelect={() => onSetWallpaperDay(null)}>
                          {wallpaperSettings.dayOfYear === null ? "✓ " : ""}自动（跟随日期）
                        </Menu.Item>
                        <Box px="3" py="2" minW="240px" onKeyDown={(event) => event.stopPropagation()}>
                          <Text color="gray.500" fontSize="xs" mb="2">
                            {wallpaperSettings.dayOfYear === null
                              ? "拖动选择日期（季节随之变化）"
                              : `${formatWallpaperDate(wallpaperSettings.dayOfYear)} · ${wallpaperEffect.blendLabel}`}
                          </Text>
                          <Slider.Root
                            aria-label={["壁纸日期"]}
                            className="pz-memory-slider"
                            max={WALLPAPER_DAY_MAX}
                            min={0}
                            step={1}
                            value={[wallpaperSettings.dayOfYear ?? wallpaperEffect.dayOfYear]}
                            onValueChange={({ value }) => {
                              const nextValue = value[0];
                              if (nextValue !== undefined) {
                                onSetWallpaperDay(nextValue);
                              }
                            }}
                          >
                            <Slider.Control>
                              <Slider.Track>
                                <Slider.Range />
                              </Slider.Track>
                              <Slider.Thumb index={0}>
                                <Slider.HiddenInput />
                              </Slider.Thumb>
                            </Slider.Control>
                          </Slider.Root>
                        </Box>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 4,
                    strategy: "fixed",
                    flip: true,
                    shift: 8,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">手动时刻</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item value="time-auto" onSelect={() => onSetWallpaperTime(null)}>
                          {wallpaperSettings.timeMinutes === null ? "✓ " : ""}自动（跟随真实时间）
                        </Menu.Item>
                        <Box px="3" py="2" minW="240px" onKeyDown={(event) => event.stopPropagation()}>
                          <Text color="gray.500" fontSize="xs" mb="2">
                            {wallpaperSettings.timeMinutes === null
                              ? "拖动选择时刻（时间随之变化）"
                              : `${formatClock(wallpaperSettings.timeMinutes)} · ${wallpaperEffect.phaseLabel}`}
                          </Text>
                          <Slider.Root
                            aria-label={["壁纸时刻"]}
                            className="pz-memory-slider"
                            max={MINUTES_PER_DAY - 1}
                            min={0}
                            step={1}
                            value={[wallpaperSettings.timeMinutes ?? wallpaperEffect.minutes]}
                            onValueChange={({ value }) => {
                              const nextValue = value[0];
                              if (nextValue !== undefined) {
                                onSetWallpaperTime(nextValue);
                              }
                            }}
                          >
                            <Slider.Control>
                              <Slider.Track>
                                <Slider.Range />
                              </Slider.Track>
                              <Slider.Thumb index={0}>
                                <Slider.HiddenInput />
                              </Slider.Thumb>
                            </Slider.Control>
                          </Slider.Root>
                        </Box>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>
          <Menu.Separator />
          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 4,
              strategy: "fixed",
              flip: true,
              shift: 8,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">自定义图片</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                <Menu.Item value="choose-custom" onSelect={onChooseCustom}>
                  选择图片
                </Menu.Item>
                {customBackgrounds.length > 0 && <Menu.Separator />}
                {customBackgrounds.map((background) => renderBackgroundItem(background))}
                {customBackgrounds.length === 0 && (
                  <Text color="gray.500" fontSize="sm" px="3" py="2">
                    暂无自定义图片
                  </Text>
                )}
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>

          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 4,
              strategy: "fixed",
              flip: true,
              shift: 8,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">创意工坊订阅</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                <Menu.Item value="scan-workshop" onSelect={onScanWorkshop}>
                  {workshopLoading
                    ? workshopScanProgress.scanned > 0
                      ? `正在查找... 已发现 ${workshopScanProgress.scanned} 个`
                      : "正在查找..."
                    : "查找创意工坊背景目录"}
                </Menu.Item>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 4,
                    strategy: "fixed",
                    flip: true,
                    shift: 8,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">推荐的创意工坊项目</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item
                          value="project-anime-backgrounds"
                          onSelect={() => onOpenRecommendedWorkshop("3498201101")}
                        >
                          <Text flex="1">Project Anime Backgrounds</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="l4d2-zombie-siege"
                          onSelect={() => onOpenRecommendedWorkshop("3388808256")}
                        >
                          <Text flex="1">L4D2丧尸围攻 主菜单壁纸</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="weeb-background"
                          onSelect={() => onOpenRecommendedWorkshop("3791676161")}
                        >
                          <Text flex="1">Weeb Background</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="blue-archive-visual-innovation"
                          onSelect={() => onOpenRecommendedWorkshop("3410321790")}
                        >
                          <Text flex="1">Blue Archive Visual Innovation</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="dying-light-main-menu-background"
                          onSelect={() => onOpenRecommendedWorkshop("3141620300")}
                        >
                          <Text flex="1">Dying Light Main Menu Background</Text>
                        </Menu.Item>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                {workshopGroups.length > 0 && <Menu.Separator />}
                {workshopGroups.map(([key, backgrounds]) => (
                  <Menu.Root
                    key={key}
                    positioning={{
                      placement: "right-start",
                      gutter: 4,
                      strategy: "fixed",
                      flip: true,
                      shift: 8,
                      overflowPadding: 8,
                    }}
                  >
                    <Menu.TriggerItem>
                      <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                        {backgrounds[0].modName ?? "未命名模组"}
                      </Text>
                      <Text aria-hidden="true">›</Text>
                    </Menu.TriggerItem>
                    <Portal>
                      <Menu.Positioner className="pz-menu-positioner">
                        <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        {backgrounds.map((background) => renderBackgroundItem(background, true))}
                        </Menu.Content>
                      </Menu.Positioner>
                    </Portal>
                  </Menu.Root>
                ))}
                {workshopBackgrounds.length === 0 && !workshopLoading && (
                  <Text color="gray.500" fontSize="sm" px="3" py="2">
                    请先查找创意工坊背景
                  </Text>
                )}
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>

          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  );
}

function isTauriRuntime() {
  return Boolean(
    typeof window !== "undefined" &&
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__,
  );
}

function WindowTitlebar({
  children,
  taskCount,
  taskPanelOpen,
  onToggleTasks,
}: {
  children: ReactNode;
  taskCount: number;
  taskPanelOpen: boolean;
  onToggleTasks: () => void;
}) {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const currentWindow = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const syncMaximized = () => { void currentWindow.isMaximized().then(setIsMaximized).catch(() => undefined); };
    syncMaximized();
    void currentWindow.onResized(syncMaximized).then((cleanup) => {
      if (disposed) cleanup(); else unlisten = cleanup;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // 浏览器预览下没有原生窗口：只保留导航，不渲染拖动区与窗口按钮。
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const runWindowAction = (action: () => Promise<void>) => { void action().catch(() => undefined); };
  return (
    <Box className="pz-window-titlebar">
      {children}
      <Box aria-label="拖动窗口" className="pz-window-drag-region" data-tauri-drag-region onMouseDown={(event) => { if (currentWindow && event.button === 0) runWindowAction(() => currentWindow.startDragging()); }} onDoubleClick={(event) => { if (currentWindow && !(event.target as HTMLElement).closest("[data-titlebar-control]")) runWindowAction(() => currentWindow.toggleMaximize()); }} />
      <button
        aria-expanded={taskPanelOpen}
        aria-label={taskCount > 0 ? "任务中心，" + taskCount + " 个进行中" : "任务中心"}
        className={["pz-task-trigger", taskCount > 0 ? "is-busy" : "", taskPanelOpen ? "is-open" : ""].join(" ")}
        data-titlebar-control
        onClick={onToggleTasks}
        title="任务中心"
        type="button"
      >
        <span className="pz-task-trigger-label">任务</span>
        {taskCount > 0 && <span className="pz-task-trigger-badge">{taskCount}</span>}
      </button>
      {currentWindow && (
        <Box className="pz-window-controls">
          <button aria-label="最小化" className="pz-window-control" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.minimize())} title="最小化" type="button"><span className="pz-window-icon pz-window-icon-minimize" /></button>
          <button aria-label={isMaximized ? "还原窗口" : "最大化"} className="pz-window-control" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.toggleMaximize())} title={isMaximized ? "还原窗口" : "最大化"} type="button"><span className={`pz-window-icon ${isMaximized ? "pz-window-icon-restore" : "pz-window-icon-maximize"}`} /></button>
          <button aria-label="关闭" className="pz-window-control pz-window-control-close" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.close())} title="关闭" type="button"><span className="pz-window-icon pz-window-icon-close" /></button>
        </Box>
      )}
    </Box>
  );
}

/** 弹窗高度随内容变化（加载中 → 内容就绪）时显式做过渡：
    量出目标高度 → 从旧高度动画过去 → transitionend 交还 height: auto，不把高度写死。
    时长与缓动来自 App.css 里 .pz-resource-file-dialog 的 height 过渡（260ms / ease-out）；
    首次挂载只记录高度、不做动画。deps 传所有会改变弹窗高度的状态。 */
function useDialogAutoHeight(
  dialogRef: { current: HTMLDivElement | null },
  lastHeightRef: { current: number | null },
  deps: readonly unknown[],
) {
  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) {
      lastHeightRef.current = null;
      return;
    }
    // 空闲时高度是 auto，上一次的稳定高度存在 ref 里；若正在过渡则用当前实际高度接上。
    const previousHeight =
      dialog.style.height === "" ? lastHeightRef.current : dialog.getBoundingClientRect().height;
    dialog.style.transition = "none";
    dialog.style.height = "auto";
    const targetHeight = dialog.getBoundingClientRect().height;
    if (previousHeight == null || Math.abs(targetHeight - previousHeight) < 1) {
      dialog.style.transition = "";
      dialog.style.height = "";
      lastHeightRef.current = targetHeight;
      return;
    }
    dialog.style.height = `${previousHeight}px`;
    // 强制一次布局，让下面的过渡从旧高度开始而不是直接跳到新高度。
    void dialog.offsetHeight;
    dialog.style.transition = "";
    dialog.style.height = `${targetHeight}px`;
    const handleTransitionEnd = (event: TransitionEvent) => {
      // 只认自己这一层的 height 过渡（子元素的过渡事件会冒泡上来）。
      if (event.target !== dialog || event.propertyName !== "height") {
        return;
      }
      dialog.style.height = "";
      lastHeightRef.current = targetHeight;
    };
    dialog.addEventListener("transitionend", handleTransitionEnd);
    return () => dialog.removeEventListener("transitionend", handleTransitionEnd);
  }, deps);
}

function App() {
  const [detection, setDetection] = useState<DetectionState>({ status: "checking" });
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const toastIdRef = useRef(0);
  const [notificationHistory, setNotificationHistory] = useState<NotificationEntry[]>([]);
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  const [taskPanelOpen, setTaskPanelOpen] = useState(false);
  const taskIdRef = useRef(0);
  const [installing, setInstalling] = useState(false);
  const [installingPzOptimization, setInstallingPzOptimization] = useState(false);
  const [uninstallingPzOptimization, setUninstallingPzOptimization] = useState(false);
  const [configuringViewpoint, setConfiguringViewpoint] = useState(false);
  const [launchingGame, setLaunchingGame] = useState(false);
  /** 点击启动后按钮禁用 3s */
  const [launchCooldown, setLaunchCooldown] = useState(false);
  /** 是否正在轮询游戏进程/窗口 */
  const [watchingGame, setWatchingGame] = useState(false);
  /** 游戏窗口是否已就绪（决定按钮左滑 + 停止按钮划入） */
  const [gameWindowReady, setGameWindowReady] = useState(false);
  const [gamePid, setGamePid] = useState<number | null>(null);
  const [stoppingGame, setStoppingGame] = useState(false);
  const [injectingOverlay, setInjectingOverlay] = useState(false);
  const [overlayDllPath, setOverlayDllPath] = useState(DEV_OVERLAY_DLL_PATH);
  const [overlaySession, setOverlaySession] = useState<OverlaySessionUpdate | null>(null);
  const [overlayZbProgress, setOverlayZbProgress] = useState<OverlayZbProgress | null>(null);
  /** 本次启动的最终测量结果（收到后端 startup-timing 事件后才有） */
  const [startupResult, setStartupResult] = useState<StartupTiming | null>(null);
  /** 前端检测到窗口就绪的时刻：测量出结果前用它显示实时计时 */
  const [startupStartedAt, setStartupStartedAt] = useState<number | null>(null);
  /** 实时计时的当前时刻（只在不显示最终结果时每 0.5s 更新） */
  const [startupClock, setStartupClock] = useState(0);
  const [systemMemoryMb, setSystemMemoryMb] = useState<number | null>(null);
  const [memoryLimitMb, setMemoryLimitMb] = useState<number | null>(null);
  const [memoryError, setMemoryError] = useState<string | null>(null);
  const [developerMode, setDeveloperMode] = useState(false);
  const developerModeRef = useRef(false);
  const [developerModeSaving, setDeveloperModeSaving] = useState(false);
  // 全局翻译语言（设置 › 通用）：开发者新闻和创意工坊详情都读这一份。
  const [translationLanguage, setTranslationLanguage] = useState(DEFAULT_TRANSLATION_LANGUAGE);
  const [translationLanguageSaving, setTranslationLanguageSaving] = useState(false);
  // 设置 › 翻译器：自定义文本翻译，默认跟随通用里的翻译语言。
  const [translatorLanguage, setTranslatorLanguage] = useState(DEFAULT_TRANSLATION_LANGUAGE);
  const [translatorSource, setTranslatorSource] = useState("");
  const [translatorResult, setTranslatorResult] = useState("");
  const [translatorBusy, setTranslatorBusy] = useState(false);
  const [translatorProgress, setTranslatorProgress] = useState({ completed: 0, total: 0 });
  const [translatorEngine, setTranslatorEngine] = useState<string | null>(null);
  const [translatorError, setTranslatorError] = useState<string | null>(null);
  const translatorRunIdRef = useRef(0);
  // 字体对比度（0–100，设置 › 通用）：把浅色面上的文字往近黑推，0 = 保持现有配色。
  const [fontContrast, setFontContrast] = useState(0);
  const fontContrastSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 模糊度（0–200，设置 › 通用）：全站毛玻璃 / 亚克力的 blur 半径都乘这个百分比。
  const [blurStrength, setBlurStrength] = useState(DEFAULT_BLUR_STRENGTH);
  const blurStrengthSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 界面字号（80–130，设置 › 通用）：根 font-size = 16px × 百分比/100，全站 rem 文字随之缩放。
  const [fontScale, setFontScale] = useState(DEFAULT_FONT_SCALE);
  const fontScaleSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 界面字体（设置 › 通用）：存完整 CSS font-family 列表，空串 = 默认字体栈。
  const [fontFamily, setFontFamily] = useState("");
  const fontFamilySaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 下拉框当前选中的预设项（与 fontFamily 分开记：选「自定义」时保留原值给输入框编辑）。
  const [fontFamilyChoice, setFontFamilyChoice] = useState("default");
  // 记住的窗口尺寸（仅用于设置页展示；启动时的实际恢复由后端 setup 完成）。
  const [savedWindowSize, setSavedWindowSize] = useState<[number, number] | null>(null);
  const windowSizeSaveTimerRef = useRef<number | null>(null);
  // 模组加载器方案（设置 › 模组加载器）：启动时按它决定要不要挂加载器 agent。
  const [modLoaderScheme, setModLoaderScheme] = useState(DEFAULT_MOD_LOADER_SCHEME);
  const [modLoaderSchemeSaving, setModLoaderSchemeSaving] = useState(false);
  // 极速启动（实验性，设置 › 启动）：只在方案 = ZombieBuddy 时有意义。
  const [fastLaunch, setFastLaunch] = useState(false);
  const [fastLaunchSaving, setFastLaunchSaving] = useState(false);
  // 静默内存清理 / 启动时清理内存（设置 › 启动）。
  const [silentMemoryCleanup, setSilentMemoryCleanup] = useState(false);
  const [silentMemoryCleanupThreshold, setSilentMemoryCleanupThreshold] = useState(
    DEFAULT_SILENT_MEMORY_PERCENT,
  );
  const [silentMemoryCleanupSaving, setSilentMemoryCleanupSaving] = useState(false);
  const [cleanMemoryOnLaunch, setCleanMemoryOnLaunch] = useState(false);
  const [cleanMemoryOnLaunchSaving, setCleanMemoryOnLaunchSaving] = useState(false);
  const [memoryStatus, setMemoryStatus] = useState<MemoryStatus | null>(null);
  const [lastCleanupReport, setLastCleanupReport] = useState<MemoryCleanupReport | null>(null);
  const [memoryCleaning, setMemoryCleaning] = useState(false);
  const silentMemoryCleanupRef = useRef(false);
  const silentMemoryThresholdSaveTimerRef = useRef<number | null>(null);
  const [backgroundPath, setBackgroundPath] = useState<string | null>(null);
  const [shaderDownloadUrl, setShaderDownloadUrl] = useState("");
  const [shaderDownloadFileName, setShaderDownloadFileName] = useState("");
  const [shaderDownloadSaving, setShaderDownloadSaving] = useState(false);
  const [shaderDownloadDirectory, setShaderDownloadDirectory] = useState("");
  const [shaderDownloadThreads, setShaderDownloadThreads] = useState(4);
  const [steamRelayMode, setSteamRelayMode] = useState("auto");
  const [steamRelayBase, setSteamRelayBase] = useState("");
  const [steamRelaySaving, setSteamRelaySaving] = useState(false);
  const [steamRelayTesting, setSteamRelayTesting] = useState(false);
  /** 「测试连接」的结果文案；null = 还没测过。 */
  const [steamRelayPing, setSteamRelayPing] = useState<string | null>(null);
  const [steamProxy, setSteamProxy] = useState("");
  const [steamProxySaving, setSteamProxySaving] = useState(false);
  const [steamProxyDetecting, setSteamProxyDetecting] = useState(false);
  /** 「自动检测」的结果文案；null = 还没测过。 */
  const [steamProxyHint, setSteamProxyHint] = useState<string | null>(null);
  /** 实际生效的中转地址：输入框留空 = 内置默认。 */
  const effectiveSteamRelayBase = steamRelayBase.trim() || DEFAULT_STEAM_RELAY_BASE;
  const [backgroundUrl, setBackgroundUrl] = useState<string | null>(null);
  const [wallpaperEffectSettings, setWallpaperEffectSettings] = useState<WallpaperEffectSettings>(
    DEFAULT_WALLPAPER_EFFECT,
  );
  const [wallpaperNow, setWallpaperNow] = useState(() => new Date());
  const [seasonFeatures, setSeasonFeatures] = useState<SeasonFeatures>(DEFAULT_SEASON_FEATURES);
  /** 当前是否显示内置四季轮转（没设置过背景时也是它）。 */
  const wallpaperSeasonal =
    !backgroundPath || builtinBackgroundFromPath(backgroundPath)?.id === BUILTIN_SEASONAL_ID;
  const wallpaperEffect = useMemo(
    () => wallpaperEffectAt(wallpaperNow, wallpaperEffectSettings, seasonFeatures),
    [wallpaperNow, wallpaperEffectSettings, seasonFeatures],
  );
  const [customBackgrounds, setCustomBackgrounds] = useState<BackgroundEntry[]>([]);
  const [workshopBackgrounds, setWorkshopBackgrounds] = useState<BackgroundEntry[]>([]);
  const [workshopBackgroundLoading, setWorkshopBackgroundLoading] = useState(false);
  const [workshopBackgroundScanProgress, setWorkshopBackgroundScanProgress] = useState({
    scanned: 0,
    total: 0,
  });
  const [activePage, setActivePage] = useState<ActivePage>("launch");
  const [navActivePage, setNavActivePage] = useState<ActivePage>("launch");
  const [pageTransitionPhase, setPageTransitionPhase] = useState<
    "idle" | "exiting" | "entering"
  >("idle");
  /** 是否让 .pz-main 外壳（背景 / 边框 / 阴影）参与整页过渡。
   *  只有切到启动页时旧页面需要整体滑出；其它页面切换只动正文内容，
   *  背景和正文框保持不动。 */
  const [pageTransitionMovesShell, setPageTransitionMovesShell] = useState(false);
  const [pageTransitionDirection, setPageTransitionDirection] = useState<"forward" | "backward">(
    "forward",
  );
  const pageTransitionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const memorySaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [launchArguments, setLaunchArguments] = useState<string[]>(defaultLaunchArguments);
  const updateLaunchArguments = (next: string[]) => setLaunchArguments(next); void updateLaunchArguments;
  const [launchSettingsOpen, setLaunchSettingsOpen] = useState(false);
  const [launchDetailsOpen, setLaunchDetailsOpen] = useState(false);
  const launchCommandRef = useRef<HTMLDivElement | null>(null);
  const [resourceCenterPage, setResourceCenterPage] = useState<ResourceCenterPage>("workshop");
  const [settingsPage, setSettingsPage] = useState<SettingsPage>("general");
  const [launcherUpdate, setLauncherUpdate] = useState<LauncherUpdateStatus | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateCheckError, setUpdateCheckError] = useState<string | null>(null);
  // 「设置 › 关于 › 反馈信息」：弹窗状态与上传结果。
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const [feedbackClosing, setFeedbackClosing] = useState(false);
  const [feedbackDescription, setFeedbackDescription] = useState("");
  const [feedbackSubmitting, setFeedbackSubmitting] = useState(false);
  const [feedbackError, setFeedbackError] = useState<string | null>(null);
  const [feedbackResult, setFeedbackResult] = useState<{
    feedbackId: string;
    uploaded: string[];
    skipped: string[];
  } | null>(null);

  const [modManagementPage, setModManagementPage] = useState<ModManagementPage>("lua");
  const [luaModFilters, setLuaModFilters] = useState<Set<LuaModFilter>>(new Set());
  const [luaModSearch, setLuaModSearch] = useState("");
  const [luaModSortMode, setLuaModSortMode] = useState<"load" | "name">("load");
  const [jarModSearch, setJarModSearch] = useState("");
  const [jarModFilter, setJarModFilter] = useState<JarModFilter>("all");
  const [shots, setShots] = useState<ManagedDirectory | null>(null);
  const [shaderpacks, setShaderpacks] = useState<ManagedDirectory | null>(null);
  const [shaderpackIcons, setShaderpackIcons] = useState<Record<string, ShaderpackIcon | null>>({});
  const [shaderpackSearchTerms, setShaderpackSearchTerms] = useState<Record<string, string | null>>({});
  /** 图标图片两个地址都加载不出来、已经换 CurseForge 源重搜过的包，防止反复换源。 */
  const shaderpackIconResourced = useRef<Set<string>>(new Set());
  const [steamAccountHidden, setSteamAccountHidden] = useState(false);
  const [workshopBackgroundInfoOpen, setWorkshopBackgroundInfoOpen] = useState(false);
  const [jarMods, setJarMods] = useState<ScannedJarMod[] | null>(null);
  const [luaMods, setLuaMods] = useState<LuaModEntry[] | null>(null);
  const [luaModsLoading, setLuaModsLoading] = useState(false);
  const [luaModBatchAction, setLuaModBatchAction] = useState<"positive" | "negative" | null>(null);
  const [selectedLuaModKeys, setSelectedLuaModKeys] = useState<Set<string>>(new Set());
  const [expandedLuaModKeys, setExpandedLuaModKeys] = useState<Set<string>>(new Set());
  const [luaModActionId, setLuaModActionId] = useState<string | null>(null);
  const [zombieBuddyPanelExpanded, setZombieBuddyPanelExpanded] = useState(false);
  // 支持库页的安装选项：默认只显示名称与状态标签，详情折叠。
  const [supportZombieBuddyExpanded, setSupportZombieBuddyExpanded] = useState(false);
  const [supportPzOptimizationExpanded, setSupportPzOptimizationExpanded] = useState(false);
  const [supportViewpointExpanded, setSupportViewpointExpanded] = useState(false);
  const [jarModBatchAction, setJarModBatchAction] = useState<"positive" | "negative" | null>(null);
  const [modPresets, setModPresets] = useState<ModPreset[]>([]);
  const [modPresetName, setModPresetName] = useState("");
  const [modPresetBusy, setModPresetBusy] = useState(false);
  const rowDragRef = useRef<{
    kind: "lua" | "jar";
    startKey: string;
    select: boolean;
    moved: boolean;
    /** 已被本次拖动改写过的行 → 该行拖动前的选中状态；划出选区时用于恢复。 */
    originals: Map<string, boolean>;
  } | null>(null);
  const suppressRowClickRef = useRef(false);
  const [selectedJarModKeys, setSelectedJarModKeys] = useState<Set<string>>(new Set());
  const [expandedJarModKeys, setExpandedJarModKeys] = useState<Set<string>>(new Set());
  const [jarConflictExpanded, setJarConflictExpanded] = useState(false);
  const [jarModsLoading, setJarModsLoading] = useState(false);
  const [jarScanFrame, setJarScanFrame] = useState(0);
  const [jarScanProgress, setJarScanProgress] = useState({ scanned: 0, total: 0 });
  const jarScanIdRef = useRef<number | null>(null);
  const workshopBackgroundScanIdRef = useRef<number | null>(null);
  const [loadingMedia, setLoadingMedia] = useState(false);
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const [copyingShot, setCopyingShot] = useState(false);
  const [deletingShaderpack, setDeletingShaderpack] = useState<string | null>(null);
  const dropZoneRef = useRef<HTMLDivElement>(null);
  const [workshopOptions, setWorkshopOptions] = useState<WorkshopOptions | null>(null);
  const [workshopFilters, setWorkshopFilters] = useState<WorkshopFilters>(initialWorkshopFilters);
  const [workshopResponse, setWorkshopResponse] = useState<WorkshopResponse | null>(null);
  const [workshopLoading, setWorkshopLoading] = useState(false);
  const workshopRequestId = useRef(0);
  const workshopSearchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [developerNews, setDeveloperNews] = useState<DeveloperNewsResponse | null>(null);
  const [developerNewsLoading, setDeveloperNewsLoading] = useState(false);
  const [developerNewsTranslationJobs, setDeveloperNewsTranslationJobs] = useState<Array<{ sid: string; text: string }>>([]);
  const [developerNewsTranslationProgress, setDeveloperNewsTranslationProgress] = useState({ completed: 0, total: 0 });
  const developerNewsRequestId = useRef(0);

  // 创意工坊右键「窗口内打开」的模态框：目标项目、抓到的详情、翻译进度。
  const [workshopDetailTarget, setWorkshopDetailTarget] = useState<WorkshopItem | null>(null);
  const [workshopDetail, setWorkshopDetail] = useState<WorkshopItemDetail | null>(null);
  const [workshopDetailLoading, setWorkshopDetailLoading] = useState(false);
  const [workshopDetailClosing, setWorkshopDetailClosing] = useState(false);
  const [workshopDetailProgress, setWorkshopDetailProgress] = useState({ completed: 0, total: 0 });
  const workshopDetailCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 「加载中 → 正文就绪」会把弹窗撑高，量出高度后做过渡（见 useDialogAutoHeight）。
  const workshopDetailDialogRef = useRef<HTMLDivElement | null>(null);
  const workshopDetailDialogHeightRef = useRef<number | null>(null);
  const [resourceOptions, setResourceOptions] = useState<ResourceOptions | null>(null);
  const [minecraftVersions, setMinecraftVersions] = useState<MinecraftVersionOption[]>([]);
  const jarModsForConflict = jarMods?.map((mod) => ({
    ...mod,
    decision: mod.disabled ? false : mod.decision,
  }));
  const jarModConflicts = jarModsForConflict ? analyzeJarModConflicts(jarModsForConflict) : [];
  const approvedJarModCount =
    jarMods?.filter((mod) => mod.decision === true && !mod.disabled).length ?? 0;
  const zombieBuddyLuaMod = luaMods?.find((mod) => mod.isLoader) ?? null;
  const totalLuaModCount = luaMods?.filter((mod) => !mod.isLoader).length ?? 0;
  const luaModSearchText = luaModSearch.trim().toLowerCase();
  const filteredLuaMods = luaMods
    ? (() => {
        const filtered = filterLuaMods(
          luaMods.filter((mod) => !mod.isLoader),
          luaModFilters,
        ).filter((mod) =>
          !luaModSearchText ||
          [mod.name, mod.id, mod.author ?? "", mod.description ?? ""]
            .some((value) => value.toLowerCase().includes(luaModSearchText)),
        );
        return luaModSortMode === "name"
          ? [...filtered].sort((left, right) =>
              left.name.localeCompare(right.name, "zh-CN", { sensitivity: "base" }),
            )
          : sortLuaModsForDisplay(filtered);
      })()
    : undefined;
  const jarModSearchText = jarModSearch.trim().toLowerCase();
  const filteredJarMods = jarMods
    ? filterJarMods(
        jarMods.filter((mod) =>
          !jarModSearchText ||
          [mod.id, mod.infoId, mod.packageName, mod.description ?? ""]
            .some((value) => value.toLowerCase().includes(jarModSearchText)),
        ),
        jarModFilter,
      )
    : undefined;
  const selectedLuaModCount = selectedLuaModKeys.size;
  const selectedJarModCount = selectedJarModKeys.size;
  function dismissToast(id: number) {
    setToasts((current) => current.map((toast) => (toast.id === id ? { ...toast, closing: true } : toast)));
    window.setTimeout(() => setToasts((current) => current.filter((toast) => toast.id !== id)), 180);
  }
  function showToast(message: string, kind: ToastKind = "info") {
    const id = ++toastIdRef.current;
    const visible = shouldShowNotification(kind, developerModeRef.current);
    setNotificationHistory((current) => {
      const next = [...current, { id, message, kind, createdAt: Date.now(), visible }];
      return next.length > 500 ? next.slice(next.length - 500) : next;
    });
    if (!visible) {
      return;
    }
    const visibleMessage = conciseNotification(message, developerModeRef.current);
    setToasts((current) => [...current.slice(-3), { id, message: visibleMessage, kind }]);
    window.setTimeout(() => dismissToast(id), 3000);
  }

  function clearNotificationHistory() {
    setNotificationHistory([]);
  }

  // 检查启动器更新。manual=false 用于启动时的静默检查:失败完全无感,
  // 有新版本只弹一次 toast,详情和下载入口都在 设置 › 关于。
  async function runUpdateCheck(manual: boolean): Promise<void> {
    if (!isTauriRuntime()) {
      if (manual) {
        showToast("当前环境不支持检查更新（仅桌面端可用）。", "info");
      }
      return;
    }
    setUpdateChecking(true);
    setUpdateCheckError(null);
    try {
      const status = await invoke<LauncherUpdateStatus>("check_launcher_update");
      setLauncherUpdate(status);
      if (manual) {
        showToast(
          status.updateAvailable
            ? `发现新版本 v${status.latestVersion}，可在下方查看并下载。`
            : `已是最新版本（v${status.currentVersion}）。`,
          status.updateAvailable ? "success" : "info",
        );
      }
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      setUpdateCheckError(message);
      if (manual) {
        showToast(message, "error");
      }
    } finally {
      setUpdateChecking(false);
    }
  }

  /** 「设置 › 关于 › 反馈信息」：上传最近一次启动的日志 + 错误描述。 */
  function openFeedbackDialog() {
    setFeedbackResult(null);
    setFeedbackError(null);
    setFeedbackClosing(false);
    setFeedbackOpen(true);
  }

  function closeFeedbackDialog() {
    setFeedbackClosing(true);
    window.setTimeout(() => {
      setFeedbackOpen(false);
      setFeedbackClosing(false);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  async function submitFeedback() {
    if (feedbackSubmitting || !feedbackDescription.trim()) {
      return;
    }
    setFeedbackSubmitting(true);
    setFeedbackError(null);
    try {
      const result = await invoke<{
        feedbackId: string;
        uploaded: string[];
        skipped: string[];
      }>("submit_feedback", { description: feedbackDescription });
      setFeedbackResult(result);
      setFeedbackDescription("");
      showToast(
        result.feedbackId ? `反馈已提交（编号 ${result.feedbackId}）` : "反馈已提交",
        "success",
      );
    } catch (error: unknown) {
      setFeedbackError(error instanceof Error ? error.message : String(error));
    } finally {
      setFeedbackSubmitting(false);
    }
  }

  async function updateDeveloperMode(enabled: boolean) {
    const previous = developerModeRef.current;
    developerModeRef.current = enabled;
    setDeveloperMode(enabled);
    setDeveloperModeSaving(true);
    try {
      await invoke("save_developer_mode", { enabled });
    } catch (error: unknown) {
      developerModeRef.current = previous;
      setDeveloperMode(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setDeveloperModeSaving(false);
    }
  }

  async function updateSteamAccountHidden(hidden: boolean) {
    const previous = steamAccountHidden;
    setSteamAccountHidden(hidden);
    try {
      await invoke("save_steam_account_hidden", { hidden });
    } catch (error: unknown) {
      setSteamAccountHidden(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 设置页「通用 › 翻译语言」：写入设置后作废已抓好的新闻，
      下次进「开发者新闻」会按新语言重新取一遍并重新翻译。 */
  async function updateTranslationLanguage(language: string) {
    const previous = translationLanguage;
    setTranslationLanguage(language);
    setTranslationLanguageSaving(true);
    try {
      await invoke("save_translation_language", { language });
      setDeveloperNews(null);
      setDeveloperNewsTranslationJobs([]);
      setDeveloperNewsTranslationProgress({ completed: 0, total: 0 });
    } catch (error: unknown) {
      setTranslationLanguage(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setTranslationLanguageSaving(false);
    }
  }

  /** 设置页「通用 › Steam 网络加速」：模式与中转地址的保存、测试连接。 */
  async function saveSteamRelaySettings(mode: string, base: string) {
    setSteamRelaySaving(true);
    try {
      await invoke("save_steam_relay_settings", { mode, base });
      setSteamRelayPing(null);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setSteamRelaySaving(false);
    }
  }

  async function testSteamRelay() {
    if (steamRelayTesting) {
      return;
    }
    setSteamRelayTesting(true);
    setSteamRelayPing(null);
    try {
      const result = await invoke<{ ok: boolean; latencyMs: number; error: string | null }>(
        "ping_steam_relay",
        { base: effectiveSteamRelayBase },
      );
      setSteamRelayPing(
        result.ok
          ? `连接正常，往返 ${result.latencyMs} ms`
          : `连接失败：${result.error ?? "未知错误"}`,
      );
    } catch (error: unknown) {
      setSteamRelayPing(`连接失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setSteamRelayTesting(false);
    }
  }

  /** 本地代理：空串 = 清空（退回系统代理）。后端会校验格式并明确报错。 */
  async function saveSteamProxy(proxy: string) {
    setSteamProxySaving(true);
    try {
      await invoke("save_steam_proxy", { proxy });
      setSteamProxyHint(null);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setSteamProxySaving(false);
    }
  }

  /** 自动检测本地代理：扫常见 HTTP/SOCKS 端口，只保留真能访问 Steam 的。 */
  async function detectSteamProxy() {
    if (steamProxyDetecting) {
      return;
    }
    setSteamProxyDetecting(true);
    setSteamProxyHint(null);
    try {
      const found = await invoke<Array<{ proxy: string; latencyMs: number }>>("detect_steam_proxy");
      if (found.length === 0) {
        setSteamProxyHint("没有检测到可用的本地代理。装了 Clash / V2Ray 等工具的话，请在其界面确认「混合端口 / HTTP 端口」后手动填入。");
        return;
      }
      const best = found[0];
      setSteamProxy(best.proxy);
      await invoke("save_steam_proxy", { proxy: best.proxy });
      setSteamProxyHint(
        `已选用 ${best.proxy}（往返 ${best.latencyMs} ms）${found.length > 1 ? `，共发现 ${found.length} 个可用代理` : ""}`,
      );
    } catch (error: unknown) {
      setSteamProxyHint(`检测失败：${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setSteamProxyDetecting(false);
    }
  }

  /** 设置 › 翻译器：把自定义文本按行切好，复用资源中心的分批翻译管线。 */
  async function translateCustomText() {
    const source = translatorSource;
    if (!source.trim()) {
      setTranslatorError("请输入要翻译的文本。");
      return;
    }
    if (source.length > CUSTOM_TRANSLATION_MAX_CHARS) {
      setTranslatorError(`文本过长，单次最多 ${CUSTOM_TRANSLATION_MAX_CHARS} 个字符。`);
      return;
    }

    const runId = ++translatorRunIdRef.current;
    const { lines, jobs } = buildCustomTranslationJobs(source);
    setTranslatorBusy(true);
    setTranslatorError(null);
    setTranslatorEngine(null);
    setTranslatorResult("");
    setTranslatorProgress({ completed: 0, total: jobs.length });

    if (jobs.length === 0) {
      setTranslatorResult(source);
      setTranslatorBusy(false);
      setTranslatorError("没有可翻译的文字，已保留原文。");
      return;
    }

    const translations = new Map<string, { text: string; engine: string }>();
    try {
      await translateInBatches(
        jobs,
        translatorLanguage,
        (results) => {
          if (runId !== translatorRunIdRef.current) return;
          for (const result of results) {
            translations.set(result.sid, { text: result.text, engine: result.engine });
          }
        },
        (completed, total) => {
          if (runId === translatorRunIdRef.current) {
            setTranslatorProgress({ completed, total });
          }
        },
        () => runId !== translatorRunIdRef.current,
      );
      if (runId !== translatorRunIdRef.current) return;

      const translatedLines = lines.map((line) => {
        if (line.kind === "blank") return "";
        if (line.kind === "raw") return line.text;
        let output = "";
        line.sids.forEach((sid, index) => {
          if (index > 0) output += line.separators[index - 1] ?? "";
          const translated = translations.get(sid)?.text;
          output += translated && translated.trim() !== "" ? translated : line.chunks[index];
        });
        return output;
      });
      setTranslatorResult(translatedLines.join("\n"));

      const successful = jobs.filter((job) => {
        const result = translations.get(job.sid);
        return result !== undefined && result.engine !== "none" && result.text.trim() !== "";
      });
      const engines = [
        ...new Set(
          successful
            .map((job) => translations.get(job.sid)?.engine)
            .filter((value): value is string => Boolean(value)),
        ),
      ];
      setTranslatorEngine(engines.length > 0 ? engines.join(" / ") : null);
      if (successful.length === 0) {
        setTranslatorError("翻译服务没有返回结果，已保留原文。");
      } else if (successful.length < jobs.length) {
        setTranslatorError(`有 ${jobs.length - successful.length} 段未翻译成功，已保留原文。`);
      }
    } catch (error: unknown) {
      if (runId === translatorRunIdRef.current) {
        setTranslatorError(error instanceof Error ? error.message : String(error));
      }
    } finally {
      if (runId === translatorRunIdRef.current) {
        setTranslatorBusy(false);
      }
    }
  }

  function clearTranslator() {
    translatorRunIdRef.current += 1;
    setTranslatorSource("");
    setTranslatorResult("");
    setTranslatorEngine(null);
    setTranslatorError(null);
    setTranslatorProgress({ completed: 0, total: 0 });
    setTranslatorBusy(false);
  }

  async function copyTranslatorResult() {
    if (!translatorResult) return;
    try {
      await writeText(translatorResult);
      showToast("译文已复制到剪贴板。", "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  function updateMemoryLimit(nextValue: number) {
    setMemoryLimitMb(nextValue);
    if (memorySaveTimerRef.current !== null) {
      window.clearTimeout(memorySaveTimerRef.current);
    }
    memorySaveTimerRef.current = window.setTimeout(() => {
      memorySaveTimerRef.current = null;
      void invoke("save_memory_limit", { memoryMb: nextValue }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 250);
  }

  function jarConflictRiskLabel(risk: JarModConflictRisk): string {
    switch (risk) {
      case "high":
        return "高风险";
      case "mediumHigh":
        return "中高风险";
      case "mediumLow":
        return "中低风险";
    }
  }

  function jarConflictRiskPalette(risk: JarModConflictRisk): "red" | "orange" | "blue" {
    switch (risk) {
      case "high":
        return "red";
      case "mediumHigh":
        return "orange";
      case "mediumLow":
        return "blue";
    }
  }

  useEffect(() => () => {
    if (memorySaveTimerRef.current !== null) {
      window.clearTimeout(memorySaveTimerRef.current);
    }
  }, []);

  useEffect(() => {
    developerModeRef.current = developerMode;
    if (!developerMode) {
      setToasts((current) => current.filter((toast) => toast.kind === "error"));
    }
  }, [developerMode]);

  /** 跳到「设置 › 模组加载器」：支持库、模组管理里的指路按钮都走这里。 */
  function openModLoaderSettings() {
    setSettingsPage("modLoader");
    navigateToPage("settings");
  }

  function navigateToPage(nextPage: ActivePage) {
    if (nextPage === activePage && pageTransitionPhase === "idle") {
      return;
    }

    if (pageTransitionTimerRef.current !== null) {
      clearTimeout(pageTransitionTimerRef.current);
      pageTransitionTimerRef.current = null;
    }

    setNavActivePage(nextPage);

    // 启动页与普通页面的切换现在与普通页面之间完全同构：滑出 → 换页 → 滑入。
    // 早期切到启动页只滑出、离开启动页直接硬切，是因为亚克力面板靠 backdrop-filter，
    // 整页做 opacity/transform 动画期间会丢模糊、先空一帧；面板改成克隆壁纸 +
    // clip-path 的无模糊实现后，这个限制已不存在，启动页也能完整参与过渡。
    // 启动页是整页固定层（.pz-main-launch），所以这两类切换让 .pz-main 外壳一起动
    // （movesShell）：正文 Stack 与外壳同曲线同位移，观感是整页滑动。
    const isLaunchTransition = nextPage === "launch" || activePage === "launch";
    setPageTransitionMovesShell(isLaunchTransition);
    setPageTransitionDirection(
      navigationItems.findIndex((item) => item.value === nextPage) >=
        navigationItems.findIndex((item) => item.value === activePage)
        ? "forward"
        : "backward",
    );
    setPageTransitionPhase("exiting");
    pageTransitionTimerRef.current = setTimeout(() => {
      setActivePage(nextPage);
      setPageTransitionPhase("entering");
      pageTransitionTimerRef.current = setTimeout(() => {
        setPageTransitionPhase("idle");
        pageTransitionTimerRef.current = null;
      }, PAGE_TRANSITION_MS);
    }, PAGE_TRANSITION_MS);
  }

  useEffect(() => {
    return () => {
      if (pageTransitionTimerRef.current !== null) {
        clearTimeout(pageTransitionTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!launchSettingsOpen && !launchDetailsOpen) {
      return;
    }
    const closeLaunchMenus = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (!target) {
        return;
      }
      if (launchCommandRef.current?.contains(target)) {
        return;
      }
      // Chakra menus (background picker, tag picker) render into a portal, so a
      // click inside them is outside the popover shell.
      if (target.closest(".pz-menu-positioner, .pz-workshop-tag-menu-content")) {
        return;
      }
      setLaunchSettingsOpen(false);
      setLaunchDetailsOpen(false);
    };
    document.addEventListener("mousedown", closeLaunchMenus);
    return () => document.removeEventListener("mousedown", closeLaunchMenus);
  }, [launchSettingsOpen, launchDetailsOpen]);

  useEffect(() => {
    if (!taskPanelOpen) {
      return;
    }
    const closeOnOutside = (event: MouseEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.closest(".pz-task-panel, .pz-task-trigger")) {
        return;
      }
      setTaskPanelOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setTaskPanelOpen(false);
      }
    };
    document.addEventListener("mousedown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [taskPanelOpen]);

  useEffect(() => {
    if (!jarModsLoading) {
      setJarScanFrame(0);
      return;
    }
    const timer = window.setInterval(() => {
      setJarScanFrame((current) => (current + 1) % 4);
    }, 250);
    return () => window.clearInterval(timer);
  }, [jarModsLoading]);

  async function refreshDetection() {
    const paths = await invoke<ProjectZomboidPaths>("detect_project_zomboid");
    setDetection(detectionStateFromPaths(paths));
  }

  async function loadLuaMods() {
    setLuaModsLoading(true);
    try {
      setLuaMods(sortLuaModsForDisplay(await invoke<LuaModEntry[]>("list_lua_mods")));
      setSelectedLuaModKeys(new Set());
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLuaModsLoading(false);
    }
  }

  /**
   * 扫描 JAR 模组。
   *
   * `refreshCache` 决定这次扫描要不要**重建极速启动的预扫描缓存**：
   * 只有用户显式点「刷新 JAR 列表」才传 `true`；进页面时的自动扫描传 `false`，
   * 后端会保持已有缓存不动（缓存持久化，见 docs/fast-launch-status.md §3.1）。
   */
  async function loadJarMods(refreshCache = false) {
    setJarModsLoading(true);
    setJarMods([]);
    setSelectedJarModKeys(new Set());
    setJarScanProgress({ scanned: 0, total: 0 });
    jarScanIdRef.current = null;
    try {
      const paths = await invoke<ProjectZomboidPaths>("detect_project_zomboid");
      const nextDetection = detectionStateFromPaths(paths);
      setDetection(nextDetection);
      if (nextDetection.status !== "found" || !paths.zombieBuddy.installed) {
        setJarModsLoading(false);
        return;
      }
      const scanId = await invoke<number>("start_zombie_buddy_jar_scan", { refreshCache });
      jarScanIdRef.current = scanId;
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      setJarModsLoading(false);
    }
  }

  function jarModKey(mod: ScannedJarMod) {
    return `${mod.jarPath}-${mod.packageName}`;
  }

  function luaModKey(mod: LuaModEntry) {
    return `${mod.source}-${mod.path}`;
  }

  function toggleLuaModFilter(filter: LuaModFilter) {
    setLuaModFilters((current) => {
      const next = new Set(current);
      if (next.has(filter)) {
        next.delete(filter);
      } else {
        next.add(filter);
      }
      return next;
    });
  }

  function toggleLuaModExpansion(mod: LuaModEntry) {
    setExpandedLuaModKeys((current) => toggleSelection(current, luaModKey(mod)));
  }

  function beginRowDrag(kind: "lua" | "jar", key: string, selected: boolean) {
    suppressRowClickRef.current = false;
    rowDragRef.current = {
      kind,
      startKey: key,
      select: !selected,
      moved: false,
      originals: new Map([[key, selected]]),
    };
  }

  /** 拖动勾选：模拟 Windows 桌面框选。选区是"起始行 ↔ 当前行"的索引区间，
      区间内所有行套用拖动方向（选中/取消），区间外的行一律恢复拖动前的状态——
      所以回移时被划出选区的行即使光标没再碰到也会取消，快速甩动跳过的行也会被覆盖。 */
  function extendRowDrag(kind: "lua" | "jar", key: string) {
    const drag = rowDragRef.current;
    if (!drag || drag.kind !== kind || (!drag.moved && key === drag.startKey)) return;
    const order =
      kind === "lua"
        ? (filteredLuaMods ?? []).map(luaModKey)
        : (filteredJarMods ?? []).map(jarModKey);
    const startIndex = order.indexOf(drag.startKey);
    const currentIndex = order.indexOf(key);
    if (startIndex < 0 || currentIndex < 0) return;
    drag.moved = true;
    const inRange = new Set(
      order.slice(Math.min(startIndex, currentIndex), Math.max(startIndex, currentIndex) + 1),
    );
    for (const rowKey of inRange) {
      if (!drag.originals.has(rowKey)) {
        drag.originals.set(
          rowKey,
          kind === "lua" ? selectedLuaModKeys.has(rowKey) : selectedJarModKeys.has(rowKey),
        );
      }
    }
    startTransition(() => {
      if (kind === "lua") {
        setSelectedLuaModKeys((current) => {
          const next = new Set(current);
          drag.originals.forEach((original, rowKey) => {
            const target = inRange.has(rowKey) ? drag.select : original;
            if (target) {
              next.add(rowKey);
            } else {
              next.delete(rowKey);
            }
          });
          return next;
        });
      } else {
        setSelectedJarModKeys((current) => {
          const next = new Set(current);
          drag.originals.forEach((original, rowKey) => {
            const target = inRange.has(rowKey) ? drag.select : original;
            if (target) {
              next.add(rowKey);
            } else {
              next.delete(rowKey);
            }
          });
          return next;
        });
      }
    });
  }

  function toggleLuaModSelection(mod: LuaModEntry, element?: HTMLElement) {
    if (suppressRowClickRef.current) {
      suppressRowClickRef.current = false;
      return;
    }
    const key = luaModKey(mod);
    const nextSelected = element
      ? !element.classList.contains("is-selected")
      : !selectedLuaModKeys.has(key);
    if (element) {
      element.classList.toggle("is-selected", nextSelected);
      element.setAttribute("aria-checked", String(nextSelected));
    }
    startTransition(() => {
      setSelectedLuaModKeys((current) => toggleSelection(current, luaModKey(mod)));
    });
  }

  function toggleJarModSelection(mod: ScannedJarMod, element?: HTMLElement) {
    if (suppressRowClickRef.current) {
      suppressRowClickRef.current = false;
      return;
    }
    const key = jarModKey(mod);
    const nextSelected = element
      ? !element.classList.contains("is-selected")
      : !selectedJarModKeys.has(key);
    if (element) {
      element.classList.toggle("is-selected", nextSelected);
      element.setAttribute("aria-checked", String(nextSelected));
    }
    startTransition(() => {
      setSelectedJarModKeys((current) => toggleSelection(current, jarModKey(mod)));
    });
  }

  function selectAllVisibleLuaMods() {
    const keys = (filteredLuaMods ?? []).map(luaModKey);
    startTransition(() => {
      setSelectedLuaModKeys((current) => setSelectionForKeys(current, keys, true));
    });
  }

  function invertVisibleLuaMods() {
    const keys = (filteredLuaMods ?? []).map(luaModKey);
    startTransition(() => {
      setSelectedLuaModKeys((current) => invertSelection(current, keys));
    });
  }

  function selectAllJarMods() {
    const keys = (filteredJarMods ?? []).map(jarModKey);
    startTransition(() => {
      setSelectedJarModKeys((current) => setSelectionForKeys(current, keys, true));
    });
  }

  function invertVisibleJarMods() {
    const keys = (filteredJarMods ?? []).map(jarModKey);
    startTransition(() => {
      setSelectedJarModKeys((current) => invertSelection(current, keys));
    });
  }

  async function refreshLuaModStatesPreservingOrder() {
    const latest = await invoke<LuaModEntry[]>("list_lua_mods");
    setLuaMods((current) =>
      current
        ? mergeItemsPreservingOrder(current, latest, luaModKey)
        : sortLuaModsForDisplay(latest),
    );
    return latest;
  }

  async function setJarModDecisions(mods: ScannedJarMod[], decision: boolean) {
    const targets = mods.filter((mod) => (mod.decision === true && !mod.disabled) !== decision);
    if (targets.length === 0) {
      showToast("所选 JAR 模组已经处于该状态。", "info");
      return;
    }
    setJarModBatchAction(decision ? "positive" : "negative");
    try {
      const result = await invoke<JarModBatchDecisionResult>(
        "set_zombie_buddy_mods_decisions",
        {
          updates: targets.map((mod) => ({
            modId: mod.id,
            modInfoPath: mod.modInfoPath,
            packageName: mod.packageName,
            workshopId: mod.workshopId,
            jarHash: mod.jarHash,
            decision,
            luaModId: mod.infoId,
          })),
        },
      );
      const updatedKeys = new Set(targets.map(jarModKey));
      setJarMods((current) =>
        current?.map((item) =>
          updatedKeys.has(jarModKey(item))
            ? {
                ...item,
                decision,
                disabled: !decision,
                modInfoPath: decision
                  ? item.modInfoPath.replace(/\.disabled$/i, "")
                  : item.modInfoPath.endsWith(".disabled")
                    ? item.modInfoPath
                    : `${item.modInfoPath}.disabled`,
              }
            : item,
        ) ?? current,
      );
      setSelectedJarModKeys(new Set());
      const action = decision ? "已启用" : "已禁用";
      const luaSummary =
        result.luaModsEnabled.length > 0
          ? `，并启用了 ${result.luaModsEnabled.length} 个 Lua 模组`
          : "";
      showToast(`${action} ${result.updated} 个 JAR 模组${luaSummary}。`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setJarModBatchAction(null);
    }
  }

  async function applyJarModFileState() {
    try {
      await invoke<number>("apply_jar_mod_file_state");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function setLuaModsEnabled(mods: LuaModEntry[], enabled: boolean) {
    const actionable = mods.filter((mod) => mod.enabled !== enabled);
    if (actionable.length === 0) {
      showToast("所选 Lua 模组已经处于该状态。", "info");
      return;
    }
    setLuaModBatchAction(enabled ? "positive" : "negative");
    try {
      const result = await invoke<LuaModBatchToggleResult>("set_lua_mods_enabled", {
        modIds: actionable.map((mod) => mod.id),
        enabled,
      });
      await refreshLuaModStatesPreservingOrder();
      setSelectedLuaModKeys(new Set());
      const changedTargets = result.changed.filter((id) =>
        actionable.some((mod) => mod.id.toLowerCase() === id.toLowerCase()),
      ).length;
      const addedPrerequisites = result.added.filter(
        (id) => !actionable.some((mod) => mod.id.toLowerCase() === id.toLowerCase()),
      ).length;
      if (result.missingRequires.length > 0) {
        showToast(
          `已${enabled ? "启用" : "停用"} ${changedTargets} 个模组，但缺少前置模组：${result.missingRequires.join("、")}`,
          "error",
        );
      } else if (result.dependents.length > 0) {
        showToast(
          `已停用 ${changedTargets} 个模组；以下已启用模组仍依赖它们：${result.dependents.join("、")}`,
          "info",
        );
      } else if (changedTargets === 0 && result.skipped.length > 0) {
        showToast("所选模组均不是 B42 模组，无法启用。", "info");
      } else if (changedTargets === 0) {
        showToast("所选 Lua 模组没有发生状态变化。", "info");
      } else {
        const skippedSummary =
          enabled && result.skipped.length > 0
            ? `，跳过 ${result.skipped.length} 个非 B42 模组`
            : "";
        showToast(
          enabled
            ? `已启用 ${changedTargets} 个 Lua 模组${addedPrerequisites > 0 ? `，并补齐 ${addedPrerequisites} 个前置模组` : ""}${skippedSummary}。`
            : `已停用 ${changedTargets} 个 Lua 模组。`,
          "success",
        );
      }
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLuaModBatchAction(null);
    }
  }

  async function loadModPresets() {
    try {
      setModPresets(await invoke<ModPreset[]>("list_mod_presets"));
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function saveModPreset() {
    const name = modPresetName.trim();
    if (!name) {
      showToast("请先输入预设名称。", "info");
      return;
    }
    setModPresetBusy(true);
    try {
      const saved = await invoke<ModPreset>("save_mod_preset", { name });
      setModPresets((current) => [...current.filter((preset) => preset.name !== saved.name), saved]);
      setModPresetName("");
      const jarEnabled = saved.jarMods.filter((mod) => mod.enabled).length;
      showToast(`已保存预设「${saved.name}」：Lua ${saved.luaModIds.length} 个，JAR 启用 ${jarEnabled} 个。`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setModPresetBusy(false);
    }
  }

  async function applyModPreset(preset: ModPreset) {
    if (modPresetBusy) return;
    setModPresetBusy(true);
    try {
      const result = await invoke<ModPresetApplyResult>("apply_mod_preset", { id: preset.id });
      await loadLuaMods();
      if (modManagementPage === "jar") {
        await loadJarMods();
      }
      const skipped = result.luaSkipped.length > 0 ? `，跳过 ${result.luaSkipped.length} 个不存在或非 B42 的 Lua 模组` : "";
      showToast(`已切换到预设「${preset.name}」：启用 ${result.luaEnabled} 个 Lua 模组，JAR 状态变更 ${result.jarChanged} 个${skipped}。`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setModPresetBusy(false);
    }
  }

  async function deleteModPreset(preset: ModPreset) {
    try {
      await invoke("delete_mod_preset", { id: preset.id });
      setModPresets((current) => current.filter((item) => item.id !== preset.id));
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function moveLuaMod(mod: LuaModEntry, direction: -1 | 1) {
    if (!luaMods || luaModActionId || luaModBatchAction) return;
    const enabled = luaMods
      .filter((entry) => entry.enabled && entry.loadOrder !== null)
      .sort((left, right) => (left.loadOrder ?? 0) - (right.loadOrder ?? 0));
    const index = enabled.findIndex((entry) => entry.id.toLowerCase() === mod.id.toLowerCase());
    const target = index + direction;
    if (index < 0 || target < 0 || target >= enabled.length) return;
    const ids = enabled.map((entry) => entry.id);
    [ids[index], ids[target]] = [ids[target], ids[index]];
    setLuaModActionId(luaModKey(mod));
    try {
      await invoke<string[]>("save_lua_mod_order", { modIds: ids });
      await loadLuaMods();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLuaModActionId(null);
    }
  }

  async function refreshMedia() {
    const lookupShaderpackIcons = async (files: ManagedFile[]) => {
      const entries = await Promise.all(files.map(async (file) => {
        const queries = shaderpackSearchQueries(file.name);
        if (queries.length === 0) return [file.name, null, null] as const;
        try {
          const result = await searchShaderpackIcon(queries, "Modrinth");
          return [file.name, result?.icon ?? null, result?.usedQuery ?? null] as const;
        } catch {
          return [file.name, null, null] as const;
        }
      }));
      setShaderpackIcons(Object.fromEntries(entries.map(([name, icon]) => [name, icon])));
      setShaderpackSearchTerms(Object.fromEntries(entries.map(([name, , query]) => [name, query ?? null])));
    };

    setLoadingMedia(true);
    try {
      const [shotDirectory, shaderpackDirectory] = await Promise.all([
        invoke<ManagedDirectory>("list_viewpoint_shots"),
        invoke<ManagedDirectory>("list_viewpoint_shaderpacks"),
      ]);
      setShots(shotDirectory);
      setShaderpacks(shaderpackDirectory);
      setShaderpackIcons({});
      setShaderpackSearchTerms({});
      shaderpackIconResourced.current.clear();
      void lookupShaderpackIcons(shaderpackDirectory.files);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLoadingMedia(false);
    }
  }

  /** 在指定下载源按候选搜索词找一个光影包图标（镜像主地址 + 官方源兜底）。 */
  async function searchShaderpackIcon(
    queries: string[],
    downloadSource: "Modrinth" | "CurseForge",
  ): Promise<{ icon: ShaderpackIcon; usedQuery: string } | null> {
    const search = (searchQuery: string) =>
      invoke<ResourceSearchResponse>("fetch_shader_resources", {
        query: {
          resourceType: "shader",
          searchQuery,
          gameVersion: "All",
          selectedTag: "All",
          sortBy: "relevance",
          downloadSource,
          page: 1,
          pageSize: 1,
        },
      });
    let usedQuery = queries[0];
    let response = await search(usedQuery);
    for (const fallbackQuery of queries.slice(1)) {
      if (response.items.length > 0) break;
      usedQuery = fallbackQuery;
      response = await search(fallbackQuery);
    }
    const item = response.items[0];
    if (!item?.iconUrl) return null;
    return { icon: { url: item.iconUrl, fallbackUrl: item.iconFallbackUrl }, usedQuery };
  }

  /** 图标在镜像和官方源都加载不出来时，换 CurseForge 源重搜一次；搜不到就保持现状。 */
  async function reSourceShaderpackIcon(name: string) {
    if (shaderpackIconResourced.current.has(name)) return;
    shaderpackIconResourced.current.add(name);
    const queries = shaderpackSearchQueries(name);
    if (queries.length === 0) return;
    try {
      const result = await searchShaderpackIcon(queries, "CurseForge");
      if (result) {
        setShaderpackIcons((prev) => ({ ...prev, [name]: result.icon }));
      }
    } catch {
      // 换源搜索失败时保留现状（破图占位），不打断页面
    }
  }

  async function loadDeveloperNews(language = translationLanguage) {
    const requestId = ++developerNewsRequestId.current;
    setDeveloperNewsLoading(true);
    try {
      const response = await invoke<DeveloperNewsResponse>("fetch_developer_news", {
        count: 20,
        language,
        onlyGid: null,
      });
      if (requestId === developerNewsRequestId.current) {
        setDeveloperNews(response);
        setDeveloperNewsTranslationJobs(response.translationJobs ?? []);
        setDeveloperNewsTranslationProgress({ completed: 0, total: response.translationJobs?.length ?? 0 });
      }
    } catch (error: unknown) {
      if (requestId === developerNewsRequestId.current) {
        showToast(error instanceof Error ? error.message : String(error), "error");
      }
    } finally {
      if (requestId === developerNewsRequestId.current) {
        setDeveloperNewsLoading(false);
      }
    }
  }
  async function loadBackgroundOptions() {
    try {
      const backgrounds = await invoke<BackgroundEntry[]>("list_custom_backgrounds");
      setCustomBackgrounds(backgrounds);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function chooseCustomBackground() {
    const selected = await open({
      title: "选择自定义背景图片",
      multiple: false,
      filters: [{ name: "背景图片", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }],
    });
    if (!selected || Array.isArray(selected)) {
      return;
    }
    try {
      const background = await invoke<BackgroundEntry>("import_custom_background", {
        sourcePath: selected,
      });
      setCustomBackgrounds((current) => [...current, background]);
      await applyBackground(background);
      showToast(`已设置背景：${background.name}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function scanWorkshopBackgrounds() {
    setWorkshopBackgrounds([]);
    setWorkshopBackgroundLoading(true);
    setWorkshopBackgroundScanProgress({ scanned: 0, total: 0 });
    workshopBackgroundScanIdRef.current = null;
    try {
      const scanId = await invoke<number>("start_workshop_background_scan");
      workshopBackgroundScanIdRef.current = scanId;
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      setWorkshopBackgroundLoading(false);
    }
  }

  async function applyBackground(background: BackgroundEntry) {
    try {
      await invoke("set_background_path", { path: background.path });
      setBackgroundPath(background.path);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 四季轮转的手动 / 固定设置落盘；写失败时回退到改之前的状态。 */
  async function updateWallpaperEffect(next: WallpaperEffectSettings) {
    const previous = wallpaperEffectSettings;
    setWallpaperEffectSettings(next);
    try {
      await invoke("set_wallpaper_effect", { effect: next });
    } catch (error: unknown) {
      setWallpaperEffectSettings(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 季节或时刻只要有一项被指定就是手动模式；两项都回到「自动」时整体恢复跟随真实时间。 */
  function wallpaperModeFor(settings: WallpaperEffectSettings): WallpaperEffectSettings["mode"] {
    return settings.dayOfYear !== null || settings.timeMinutes !== null ? "manual" : "auto";
  }

  function setWallpaperDay(dayOfYear: number | null) {
    const next = { ...wallpaperEffectSettings, dayOfYear };
    void updateWallpaperEffect({ ...next, mode: wallpaperModeFor(next) });
  }

  function setWallpaperTime(timeMinutes: number | null) {
    const next = { ...wallpaperEffectSettings, timeMinutes };
    void updateWallpaperEffect({ ...next, mode: wallpaperModeFor(next) });
  }

  /** 自动对比度开关：开启后按季节图的亮度分布自动调整对比度。 */
  function toggleWallpaperAutoContrast() {
    void updateWallpaperEffect({
      ...wallpaperEffectSettings,
      autoContrast: !wallpaperEffectSettings.autoContrast,
    });
  }

  /** 固定：把当前时刻冻结下来；再点一次取消固定，恢复跟随真实时间。 */
  function toggleWallpaperPin() {
    const pinned = !wallpaperEffectSettings.pinned;
    void updateWallpaperEffect({
      ...wallpaperEffectSettings,
      pinned,
      pinnedAt: pinned ? Date.now() : null,
    });
  }

  async function selectBuiltinBackground(background: BuiltinBackground) {
    try {
      const path = `${BUILTIN_BACKGROUND_PREFIX}${background.id}`;
      await invoke("set_background_path", { path });
      setBackgroundPath(path);
      showToast(`已设置背景：${background.name}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 极速启动（实验性）：开关落盘并维护启动文件备份；真正的缓存生成与注入在启动/扫描流程里按这个开关走。 */
  async function updateFastLaunch(enabled: boolean) {
    const previous = fastLaunch;
    setFastLaunch(enabled);
    setFastLaunchSaving(true);
    try {
      const result = await invoke<FastLaunchSaveResult>("save_fast_launch", { enabled });
      if (enabled) {
        showToast(
          result.backupPath
            ? "已开启极速启动，原有的启动配置已自动备份。"
            : "已开启极速启动。",
          "success",
        );
      } else if (result.restored) {
        showToast("已关闭极速启动，启动配置已恢复到开启前的状态。", "success");
      } else {
        showToast(result.warning ?? "已关闭极速启动。", "info");
      }
    } catch (error: unknown) {
      setFastLaunch(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setFastLaunchSaving(false);
    }
  }

  /** 静默内存清理：开关与阈值一起落盘；后端收到后立刻更新轮询线程，不用重启启动器。 */
  async function updateSilentMemoryCleanup(enabled: boolean) {
    const previous = silentMemoryCleanup;
    silentMemoryCleanupRef.current = enabled;
    setSilentMemoryCleanup(enabled);
    setSilentMemoryCleanupSaving(true);
    try {
      await invoke("save_silent_memory_cleanup", {
        enabled,
        thresholdPercent: silentMemoryCleanupThreshold,
      });
    } catch (error: unknown) {
      silentMemoryCleanupRef.current = previous;
      setSilentMemoryCleanup(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setSilentMemoryCleanupSaving(false);
    }
  }

  /** 清理阈值：拖动时只改状态，落盘做 300ms 防抖（和字体对比度一个做法）。 */
  function updateSilentMemoryCleanupThreshold(nextValue: number) {
    const clamped = Math.min(
      MAX_SILENT_MEMORY_PERCENT,
      Math.max(MIN_SILENT_MEMORY_PERCENT, Math.round(nextValue)),
    );
    setSilentMemoryCleanupThreshold(clamped);
    if (silentMemoryThresholdSaveTimerRef.current !== null) {
      window.clearTimeout(silentMemoryThresholdSaveTimerRef.current);
    }
    silentMemoryThresholdSaveTimerRef.current = window.setTimeout(() => {
      silentMemoryThresholdSaveTimerRef.current = null;
      void invoke("save_silent_memory_cleanup", {
        enabled: silentMemoryCleanupRef.current,
        thresholdPercent: clamped,
      }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 300);
  }

  /** 启动时清理内存：只落盘；真正的清理发生在后端的 launch_project_zomboid 里。 */
  async function updateCleanMemoryOnLaunch(enabled: boolean) {
    const previous = cleanMemoryOnLaunch;
    setCleanMemoryOnLaunch(enabled);
    setCleanMemoryOnLaunchSaving(true);
    try {
      await invoke("save_clean_memory_on_launch", { enabled });
    } catch (error: unknown) {
      setCleanMemoryOnLaunch(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setCleanMemoryOnLaunchSaving(false);
    }
  }

  /** 「立即清理一次」：等后端跑完再提示，典型 100–300ms。 */
  async function cleanMemoryNow() {
    setMemoryCleaning(true);
    try {
      const report = await invoke<MemoryCleanupReport>("clean_memory_now");
      setLastCleanupReport(report);
      const status = await invoke<MemoryStatus>("get_memory_status").catch(() => null);
      if (status !== null) {
        setMemoryStatus(status);
      }
      showToast(
        report.freedMb > 0
          ? `清理完成，释放了 ${report.freedMb} MB 内存`
          : "清理完成，当前可用内存没有明显变化",
        "success",
      );
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setMemoryCleaning(false);
    }
  }

  /** 切换模组加载器方案：启动时才按方案改写启动配置，所以这里只落盘。 */
  async function updateModLoaderScheme(scheme: string) {
    const previous = modLoaderScheme;
    setModLoaderScheme(scheme);
    setModLoaderSchemeSaving(true);
    try {
      await invoke("save_mod_loader_scheme", { scheme });
    } catch (error: unknown) {
      setModLoaderScheme(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setModLoaderSchemeSaving(false);
    }
  }

  /** 字体对比度：拖动时只改状态（CSS 变量立刻生效），落盘做 250ms 防抖。 */
  function updateFontContrast(nextValue: number) {
    setFontContrast(nextValue);
    if (fontContrastSaveTimerRef.current !== null) {
      window.clearTimeout(fontContrastSaveTimerRef.current);
    }
    fontContrastSaveTimerRef.current = window.setTimeout(() => {
      fontContrastSaveTimerRef.current = null;
      void invoke("save_font_contrast", { value: nextValue }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 250);
  }

  /** 模糊度：拖动时只改状态（CSS 变量立刻生效），落盘做 250ms 防抖。 */
  function updateBlurStrength(nextValue: number) {
    const clamped = Math.min(MAX_BLUR_STRENGTH, Math.max(MIN_BLUR_STRENGTH, Math.round(nextValue)));
    setBlurStrength(clamped);
    if (blurStrengthSaveTimerRef.current !== null) {
      window.clearTimeout(blurStrengthSaveTimerRef.current);
    }
    blurStrengthSaveTimerRef.current = window.setTimeout(() => {
      blurStrengthSaveTimerRef.current = null;
      void invoke("save_blur_strength", { value: clamped }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 250);
  }

  /** 界面字号：拖动时只改状态（CSS 变量立刻生效），落盘做 250ms 防抖。 */
  function updateFontScale(nextValue: number) {
    const clamped = Math.min(MAX_FONT_SCALE, Math.max(MIN_FONT_SCALE, Math.round(nextValue)));
    setFontScale(clamped);
    if (fontScaleSaveTimerRef.current !== null) {
      window.clearTimeout(fontScaleSaveTimerRef.current);
    }
    fontScaleSaveTimerRef.current = window.setTimeout(() => {
      fontScaleSaveTimerRef.current = null;
      void invoke("save_font_scale", { value: clamped }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 250);
  }

  /** 界面字体：改完立即生效（CSS 变量），落盘做 250ms 防抖。空串 = 默认字体栈。 */
  function updateFontFamily(nextValue: string) {
    setFontFamily(nextValue);
    if (fontFamilySaveTimerRef.current !== null) {
      window.clearTimeout(fontFamilySaveTimerRef.current);
    }
    fontFamilySaveTimerRef.current = window.setTimeout(() => {
      fontFamilySaveTimerRef.current = null;
      void invoke("save_font_family", { value: nextValue }).catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
    }, 250);
  }

  /** 静默内存清理：轮询系统占用（设置页要显示），并接收后端"已清理"事件。 */
  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let active = true;
    const refresh = () => {
      invoke<MemoryStatus>("get_memory_status")
        .then((status) => {
          if (active) {
            setMemoryStatus(status);
          }
        })
        .catch(() => {
          // 读不到内存状态不影响任何功能，静默忽略。
        });
    };
    refresh();
    const timer = window.setInterval(refresh, 5000);
    const unlisten = listen<MemoryCleanupReport>("memory-cleaned", (event) => {
      setLastCleanupReport(event.payload);
      refresh();
    });
    return () => {
      active = false;
      window.clearInterval(timer);
      void unlisten.then((fn) => fn());
    };
  }, []);

  // 启动几秒后静默检查一次启动器更新：网络失败静默忽略；
  // 有新版本弹 toast 引导去 设置 › 关于，手动检查/下载也在那里。
  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    const timer = window.setTimeout(() => {
      invoke<LauncherUpdateStatus>("check_launcher_update")
        .then((status) => {
          setLauncherUpdate(status);
          if (status.updateAvailable) {
            showToast(`发现新版本 v${status.latestVersion}，到 设置 › 关于 查看并下载。`, "success");
          }
        })
        .catch(() => undefined);
    }, 2500);
    return () => window.clearTimeout(timer);
  }, []);

  // 对比度只影响颜色：写在 documentElement 上（不是 .pz-app-shell），
  // 这样 Portal 到 body 的菜单、弹窗、toast 也能继承到。
  useEffect(() => {
    document.documentElement.style.setProperty("--pz-text-contrast", String(fontContrast / 100));
    return () => {
      document.documentElement.style.removeProperty("--pz-text-contrast");
    };
  }, [fontContrast]);

  // 模糊度同理：App.css 里每一处 blur 都写成 calc(<基准> * var(--pz-blur-scale, 1))，
  // 所以在 documentElement 上改一个变量就能同时作用于面板、菜单、标题栏、弹窗和 Toast。
  useEffect(() => {
    document.documentElement.style.setProperty("--pz-blur-scale", String(blurStrength / 100));
    return () => {
      document.documentElement.style.removeProperty("--pz-blur-scale");
    };
  }, [blurStrength]);

  // 界面字号同理：App.css 根 font-size 写成 calc(16px * var(--pz-font-scale, 1))，
  // 全站 rem 字号（含 Chakra 的 rem 令牌）随一个变量整体缩放；控件高度等 px 值保持不变。
  useEffect(() => {
    document.documentElement.style.setProperty("--pz-font-scale", String(fontScale / 100));
    return () => {
      document.documentElement.style.removeProperty("--pz-font-scale");
    };
  }, [fontScale]);

  // 界面字体：空串 = 默认字体栈（不写变量，回落 App.css font-family 里的完整 fallback 列表）。
  useEffect(() => {
    const trimmed = fontFamily.trim();
    if (trimmed) {
      document.documentElement.style.setProperty("--pz-font-family", trimmed);
    } else {
      document.documentElement.style.removeProperty("--pz-font-family");
    }
  }, [fontFamily]);

  // 记住窗口尺寸：拖动结束后防抖落盘，下次启动由后端 setup 恢复。
  // 尺寸由后端自行读取（省掉 inner-size / scale-factor 两个窗口权限），最大化时不记忆；
  // 恢复默认是失败可见的操作，这里的静默失败只影响"下次是否记住"，不值得打断用户。
  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    const currentWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    void currentWindow.onResized(() => {
      if (windowSizeSaveTimerRef.current !== null) {
        window.clearTimeout(windowSizeSaveTimerRef.current);
      }
      windowSizeSaveTimerRef.current = window.setTimeout(() => {
        windowSizeSaveTimerRef.current = null;
        void currentWindow.isMaximized().then((maximized) => {
          if (maximized) {
            return;
          }
          return invoke<[number, number]>("save_window_size").then((size) => {
            setSavedWindowSize(size);
          });
        }).catch(() => undefined);
      }, 500);
    })
      .then((cleanup) => {
        unlisten = cleanup;
      })
      .catch(() => undefined);
    return () => {
      unlisten?.();
      if (windowSizeSaveTimerRef.current !== null) {
        window.clearTimeout(windowSizeSaveTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }
    let active = true;

    invoke<ProjectZomboidPaths>("detect_project_zomboid")
      .then((paths) => {
        if (!active) {
          return;
        }
        setDetection(detectionStateFromPaths(paths));
      })
      .catch((error: unknown) => {
        if (!active) {
          return;
        }
        const message = error instanceof Error ? error.message : String(error);
        setDetection({ status: "error", message });
      });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;

    Promise.allSettled([
      invoke<number>("get_system_memory_mb"),
      invoke<AppSettings>("get_app_settings"),
    ])
      .then(([memoryResult, settingsResult]) => {
        if (!active) {
          return;
        }
        if (settingsResult.status === "fulfilled") {
          setDeveloperMode(Boolean(settingsResult.value.developerMode));
          developerModeRef.current = Boolean(settingsResult.value.developerMode);
          setSteamAccountHidden(Boolean(settingsResult.value.steamAccountHidden));
          const loadedTranslationLanguage = settingsResult.value.translationLanguage ?? DEFAULT_TRANSLATION_LANGUAGE;
          setTranslationLanguage(loadedTranslationLanguage);
          setTranslatorLanguage(loadedTranslationLanguage);
          setFontContrast(settingsResult.value.fontContrast ?? 0);
          setBlurStrength(settingsResult.value.blurStrength ?? DEFAULT_BLUR_STRENGTH);
          setFontScale(settingsResult.value.fontScale ?? DEFAULT_FONT_SCALE);
          const loadedFontFamily = settingsResult.value.fontFamily ?? "";
          setFontFamily(loadedFontFamily);
          setFontFamilyChoice(fontFamilyChoiceFromValue(loadedFontFamily));
          if (
            settingsResult.value.windowWidth != null &&
            settingsResult.value.windowHeight != null
          ) {
            setSavedWindowSize([
              settingsResult.value.windowWidth,
              settingsResult.value.windowHeight,
            ]);
          }
          setModLoaderScheme(settingsResult.value.modLoaderScheme ?? DEFAULT_MOD_LOADER_SCHEME);
          setFastLaunch(Boolean(settingsResult.value.fastLaunch));
          setSilentMemoryCleanup(Boolean(settingsResult.value.silentMemoryCleanup));
          silentMemoryCleanupRef.current = Boolean(settingsResult.value.silentMemoryCleanup);
          setSilentMemoryCleanupThreshold(
            settingsResult.value.silentMemoryCleanupThreshold > 0
              ? settingsResult.value.silentMemoryCleanupThreshold
              : DEFAULT_SILENT_MEMORY_PERCENT,
          );
          setCleanMemoryOnLaunch(Boolean(settingsResult.value.cleanMemoryOnLaunch));
          setBackgroundPath(settingsResult.value.backgroundPath);
          setWallpaperEffectSettings(normalizeWallpaperEffectSettings(settingsResult.value.wallpaperEffect));
          setShaderDownloadUrl(settingsResult.value.shaderDownloadUrl ?? "");
          setShaderDownloadDirectory(settingsResult.value.shaderDownloadDirectory ?? "");
          setShaderDownloadThreads(settingsResult.value.shaderDownloadThreads ?? 4);
          setSteamRelayMode(settingsResult.value.steamRelayMode ?? "auto");
          // 等于默认值的地址按「没自定义」显示，输入框留空、placeholder 展示默认。
          setSteamRelayBase(
            settingsResult.value.steamRelayBase === DEFAULT_STEAM_RELAY_BASE
              ? ""
              : (settingsResult.value.steamRelayBase ?? ""),
          );
          setSteamProxy(settingsResult.value.steamProxy ?? "");
        }
        if (memoryResult.status === "rejected") {
          setMemoryError(
            memoryResult.reason instanceof Error ? memoryResult.reason.message : String(memoryResult.reason),
          );
          return;
        }
        const totalMemoryMb = memoryResult.value;
        const settings = settingsResult.status === "fulfilled" ? settingsResult.value : null;
        const defaultMemoryMb = defaultMemoryLimitMb(totalMemoryMb);
        setSystemMemoryMb(totalMemoryMb);
        setMemoryLimitMb(
          settings?.memoryLimitMb !== null &&
            settings?.memoryLimitMb !== undefined &&
            settings.memoryLimitMb >= MIN_MEMORY_MB &&
            settings.memoryLimitMb <= totalMemoryMb
            ? settings.memoryLimitMb
            : defaultMemoryMb,
        );
        setMemoryError(null);
      })

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    void loadBackgroundOptions();
    void scanWorkshopBackgrounds();
  }, []);

  useEffect(() => {
    if (!wallpaperSeasonal || wallpaperEffectSettings.pinned) {
      return;
    }
    // 四季轮转每秒重算一次，全年全天连续变化；固定时效果不再随时间变化，就不需要计时器。
    setWallpaperNow(new Date());
    const timer = window.setInterval(() => setWallpaperNow(new Date()), 1000);
    return () => window.clearInterval(timer);
  }, [wallpaperSeasonal, wallpaperEffectSettings.pinned]);

  useEffect(() => {
    if (!wallpaperSeasonal) {
      return;
    }
    // 只分析一次季节图的亮度分布，结果给自动对比度用。
    let active = true;
    void loadSeasonFeatures().then((value) => {
      if (active) {
        setSeasonFeatures(value);
      }
    });
    return () => {
      active = false;
    };
  }, [wallpaperSeasonal]);

  useEffect(() => {
    const builtin = builtinBackgroundFromPath(backgroundPath);
    if (builtin?.id === BUILTIN_SEASONAL_ID || !backgroundPath) {
      // 四季轮转由 SeasonalWallpaper 逐层绘制，这里不设整张背景图。
      setBackgroundUrl(null);
      return;
    }
    if (builtin) {
      setBackgroundUrl(builtin.url);
      return;
    }
    // 自定义图片 / 创意工坊背景走 Rust 读文件，转成 object URL。
    let active = true;
    let objectUrl: string | null = null;
    invoke<ArrayBuffer>("read_background", { path: backgroundPath })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes]));
        setBackgroundUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setBackgroundUrl(null);
        }
      });
    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [backgroundPath]);

  useEffect(() => {
    void refreshMedia();
  }, []);

  useEffect(() => {
    invoke<WorkshopOptions>("workshop_options")
      .then(setWorkshopOptions)
      .catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
  }, []);

  useEffect(() => {
    if (activePage !== "workshop") {
      return;
    }
    if (resourceCenterPage === "workshop") {
      void loadWorkshop(workshopFilters);
    }
  }, [activePage, resourceCenterPage]);

  useEffect(() => {
    Promise.allSettled([
      invoke<ResourceOptions>("resource_options"),
      invoke<MinecraftVersionOption[]>("fetch_minecraft_versions"),
    ])
      .then(([optionsResult, versionsResult]) => {
        if (optionsResult.status === "fulfilled") {
          setResourceOptions(optionsResult.value);
        }
        if (versionsResult.status === "fulfilled") {
          setMinecraftVersions(versionsResult.value);
        }
        if (optionsResult.status === "rejected" || versionsResult.status === "rejected") {
          const error =
            optionsResult.status === "rejected"
              ? optionsResult.reason
              : versionsResult.status === "rejected"
                ? versionsResult.reason
                : "读取 Minecraft 光影资源配置失败";
          showToast(error instanceof Error ? error.message : String(error), "error");
        }
      })
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    void unlistenClose;


    getCurrentWebview()
      .listen<JarModScanUpdate>("jar-mod-scan-update", (event) => {
        if (!active) {
          return;
        }
        const update = event.payload;
        if (jarScanIdRef.current !== null && update.scanId !== jarScanIdRef.current) {
          return;
        }
        if (jarScanIdRef.current === null) {
          jarScanIdRef.current = update.scanId;
        }
        setJarScanProgress({ scanned: update.scanned, total: update.total });
        if (update.status === "started") {
          setJarModsLoading(true);
          return;
        }
        if (update.status === "item") {
          const item = update.item;
          if (item) {
            setJarMods((current) => [
              ...(current ?? []).filter(
                (mod) =>
                  mod.jarPath !== item.jarPath ||
                  mod.packageName !== item.packageName,
              ),
              item,
            ]);
          }
          return;
        }
        if (update.status === "completed") {
          setJarModsLoading(false);
          return;
        }
        setJarModsLoading(false);
        showToast(update.error ?? "JAR 模组扫描失败", "error");
      })
      .then((cleanup) => {
        if (active) {
          unlisten = cleanup;
        } else {
          cleanup();
        }
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisten?.();

    };
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    void unlistenClose;

    getCurrentWebview()
      .listen<WorkshopBackgroundScanUpdate>("workshop-background-scan-update", (event) => {
        if (!active) {
          return;
        }
        const update = event.payload;
        if (
          workshopBackgroundScanIdRef.current !== null &&
          update.scanId !== workshopBackgroundScanIdRef.current
        ) {
          return;
        }
        if (workshopBackgroundScanIdRef.current === null) {
          workshopBackgroundScanIdRef.current = update.scanId;
        }
        setWorkshopBackgroundScanProgress({
          scanned: update.scanned,
          total: update.total,
        });
        if (update.status === "started") {
          setWorkshopBackgroundLoading(true);
          return;
        }
        if (update.status === "item") {
          const item = update.item;
          if (item) {
            setWorkshopBackgrounds((current) => [
              ...current.filter((background) => background.id !== item.id),
              item,
            ]);
          }
          return;
        }
        if (update.status === "completed") {
          setWorkshopBackgroundLoading(false);
          showToast(
            update.scanned > 0
              ? `已找到 ${update.scanned} 个创意工坊背景。`
              : "没有找到创意工坊背景图片。",
            update.scanned > 0 ? "success" : "info",
          );
          return;
        }
        setWorkshopBackgroundLoading(false);
        showToast(update.error ?? "创意工坊背景扫描失败", "error");
      })
      .then((cleanup) => {
        if (active) {
          unlisten = cleanup;
        } else {
          cleanup();
        }
      })
      .catch(() => undefined);

    return () => {
      active = false;
      unlisten?.();

    };
  }, []);

  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return;
    let unlisten: (() => void) | undefined;
    void listen<{ downloadId: string; fileName: string; downloadedBytes: number; totalBytes: number | null }>("shader-download-progress", (event) => {
      const update = event.payload;
      setTasks((current) => current.map((task) =>
        task.id === update.downloadId
          ? { ...task, downloaded: update.downloadedBytes, total: update.totalBytes }
          : task,
      ));
    }).then((cleanup) => { unlisten = cleanup; }).catch(() => undefined);
    return () => unlisten?.();
  }, []);

  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return;
    let unlistenSession: (() => void) | undefined;
    let unlistenProgress: (() => void) | undefined;
    void listen<OverlaySessionUpdate>("overlay-session", (event) => {
      setOverlaySession(event.payload);
      if (event.payload.phase === "error") {
        showToast(event.payload.message, "error");
      }
    }).then((cleanup) => { unlistenSession = cleanup; }).catch(() => undefined);
    void listen<OverlayZbProgress>("overlay-zb-progress", (event) => {
      setOverlayZbProgress(event.payload);
    }).then((cleanup) => { unlistenProgress = cleanup; }).catch(() => undefined);
    return () => {
      unlistenSession?.();
      unlistenProgress?.();
    };
  }, []);
  // 启动时长：挂载时读上次记录；之后每次"检测到主菜单标记"后端会推一条新的。
  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return;
    let unlisten: (() => void) | undefined;
    void invoke<StartupTiming>("startup_timing").then(setStartupResult).catch(() => undefined);
    void listen<StartupTiming>("startup-timing", (event) => {
      setStartupResult(event.payload);
      setStartupStartedAt(null);
    }).then((cleanup) => { unlisten = cleanup; }).catch(() => undefined);
    return () => unlisten?.();
  }, []);
  // 窗口已出现但还没收到最终测量结果时，每 0.5s 刷新一次实时计时。
  useEffect(() => {
    if (!gameWindowReady || startupStartedAt === null || startupResult !== null) {
      return;
    }
    setStartupClock(Date.now());
    const timer = window.setInterval(() => setStartupClock(Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [gameWindowReady, startupStartedAt, startupResult]);
  useEffect(() => {
    const endRowDrag = () => {
      if (rowDragRef.current?.moved) {
        suppressRowClickRef.current = true;
        window.setTimeout(() => {
          suppressRowClickRef.current = false;
        }, 0);
      }
      rowDragRef.current = null;
    };
    window.addEventListener("pointerup", endRowDrag);
    window.addEventListener("pointercancel", endRowDrag);
    return () => {
      window.removeEventListener("pointerup", endRowDrag);
      window.removeEventListener("pointercancel", endRowDrag);
    };
  }, []);

  useEffect(() => {
    if (activePage !== "mods") {
      return;
    }
    // 动态加载：先让浏览器画完进入模组页的这一帧，再把扫描请求发给后端
    // （真正的 Lua/JAR 扫描在 Rust 侧的后台线程/阻塞线程池里跑，不再占用 UI 主线程）。
    const handle = window.setTimeout(() => {
      void loadModPresets();
      if (modManagementPage === "jar") {
        void loadJarMods();
      } else {
        void applyJarModFileState();
        void loadLuaMods();
      }
    }, 120);
    return () => window.clearTimeout(handle);
  }, [activePage, modManagementPage]);

  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) {
      return;
    }

    let unlisten: (() => void) | undefined;
    let unlistenClose: (() => void) | undefined;
    void unlistenClose;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (activePage !== "shaderpacks" || event.payload.type !== "drop") {
          return;
        }
        const rect = dropZoneRef.current?.getBoundingClientRect();
        const scale = window.devicePixelRatio || 1;
        const position = event.payload.position;
        const isInsideDropZone =
          rect &&
          position.x / scale >= rect.left &&
          position.x / scale <= rect.right &&
          position.y / scale >= rect.top &&
          position.y / scale <= rect.bottom;
        if (!isInsideDropZone) {
          showToast("请将 ZIP 光影包拖到导入区域内。", "error");
          return;
        }
        const zipPaths = event.payload.paths.filter((path) => path.toLowerCase().endsWith(".zip"));
        if (zipPaths.length > 0) {
          void importShaderpacks(zipPaths);
        } else if (event.payload.paths.length > 0) {
          showToast("只支持导入 ZIP 光影包文件。", "error");
        }
      })
      .then((cleanup) => {
        unlisten = cleanup;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();

    };
  }, [navActivePage]);

  async function installZombieBuddy() {
    setInstalling(true);
    try {
      const result = await invoke<InstallResult>("install_zombie_buddy");
      showToast(
        result.alreadyInstalled
          ? "ZombieBuddy 已经安装过了。"
          : `ZombieBuddy 安装完成。${result.backupPath ? `原文件已备份到：${result.backupPath}` : ""}`,
        "success",
      );
      await refreshDetection();
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      showToast(message, "error");
    } finally {
      setInstalling(false);
    }
  }

  async function installPzOptimization() {
    setInstallingPzOptimization(true);
    try {
      const result = await invoke<PzOptimizationInstallResult>("install_pz_optimization");
      showToast(
        result.alreadyInstalled
          ? "PZ Optimization 已经安装过了。"
          : `PZ Optimization 安装完成${result.installedRevision ? `（版本 ${result.installedRevision}）` : ""}。`,
        "success",
      );
      await refreshDetection();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setInstallingPzOptimization(false);
    }
  }

  async function uninstallPzOptimization() {
    const confirmed = window.confirm(
      "确定卸载 PZ Optimization 吗？卸载会自动还原被替换的游戏文件，游戏本体不受影响。",
    );
    if (!confirmed) {
      return;
    }
    setUninstallingPzOptimization(true);
    try {
      const result = await invoke<{ changed: boolean; message: string }>("uninstall_pz_optimization");
      showToast(result.message, "success");
      await refreshDetection();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setUninstallingPzOptimization(false);
    }
  }

  async function configureViewpointThreeD() {
    setConfiguringViewpoint(true);
    try {
      const result = await invoke<InstallResult>("configure_viewpoint_three_d");
      showToast(
        result.alreadyInstalled
          ? "3D 模组已经配置过了。"
          : `3D 模组配置完成。${result.backupPath ? `原文件已备份到：${result.backupPath}` : ""}`,
        "success",
      );
      await refreshDetection();
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      showToast(message, "error");
    } finally {
      setConfiguringViewpoint(false);
    }
  }

  // 每 0.3s 轮询：进程是否创建、窗口是否就绪；窗口就绪后按钮左滑、停止按钮划入。
  useEffect(() => {
    if (!watchingGame && !gameWindowReady) {
      return;
    }
    let active = true;
    const poll = async () => {
      try {
        const status = await invoke<{
          running: boolean;
          pid: number | null;
          processName: string | null;
          windowReady: boolean;
        }>("check_game_process");
        if (!active) {
          return;
        }
        if (status.running) {
          setGamePid(status.pid ?? null);
        }
        if (status.windowReady && !gameWindowReady) {
          setGameWindowReady(true);
          setWatchingGame(false);
          // 启动时长从"检测到窗口"起算：先清掉上一次的结果，显示实时计时。
          setStartupResult(null);
          setStartupStartedAt(Date.now());
          setStartupClock(Date.now());
          return;
        }
        // 游戏自己退出了：把按钮滑回原位
        if (!status.running && gameWindowReady) {
          setGameWindowReady(false);
          setGamePid(null);
          setWatchingGame(false);
        }
      } catch {
        // 轮询失败忽略，下一拍继续
      }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 300);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [watchingGame, gameWindowReady]);

  async function stopGameProcess() {
    setStoppingGame(true);
    try {
      const message = await invoke<string>("stop_game_process");
      showToast(gamePid === null ? message : `${message}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setStoppingGame(false);
      setGameWindowReady(false);
      setGamePid(null);
      setWatchingGame(false);
      setOverlaySession(null);
      setOverlayZbProgress(null);
      setStartupStartedAt(null);
    }
  }

  async function launchProjectZomboid() {
    setLaunchingGame(true);
    try {
      if (memoryLimitMb === null) {
        throw new Error(memoryError ?? "正在读取系统内存，请稍后再试。");
      }
      const normalizedLaunchArguments = launchArguments.map((argument) => argument.trim()).filter(Boolean);
      const joinedLaunchArguments = joinLaunchArguments(normalizedLaunchArguments);
      await invoke("launch_project_zomboid", {
        launchArgs: normalizedLaunchArguments,
        memoryMb: memoryLimitMb,
        // 极速启动（实验性）走 JVMTI agent 早期加载，需要把覆盖层 DLL 的路径交给后端，
        // 由它在 ProjectZomboid64.json / .bat 里写 `-agentpath:`（开关关闭时后端会把它摘掉）。
        fastLaunchDllPath: overlayDllPath.trim() || null,
      });
      setOverlaySession(null);
      setOverlayZbProgress(null);
      setStartupResult(null);
      setStartupStartedAt(null);
      // 点击后禁用 3 秒，并开始每 0.3s 轮询游戏进程/窗口（复用注入 DLL 的检测逻辑）。
      setLaunchCooldown(true);
      window.setTimeout(() => setLaunchCooldown(false), 3000);
      setGameWindowReady(false);
      setGamePid(null);
      setWatchingGame(true);
      let overlayNote = "未开启游戏加载进度监控。";
      if (overlayDllPath.trim()) {
        try {
          overlayNote = await invoke<string>("start_overlay_session", {
            dllPath: overlayDllPath.trim(),
          });
        } catch (error: unknown) {
          overlayNote = `无法监控游戏加载进度：${error instanceof Error ? error.message : String(error)}`;
        }
      }
      showToast(
        `游戏已启动。内存限制 ${formatMemoryMb(memoryLimitMb)}${joinedLaunchArguments ? `，启动参数 ${joinedLaunchArguments}` : ""}。${overlayNote}`,
        "success",
      );
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      showToast(message, "error");
    } finally {
      setLaunchingGame(false);
    }
  }

  async function chooseOverlayDll() {
    try {
      const selected = await open({
        title: "选择覆盖层 DLL",
        multiple: false,
        filters: [{ name: "覆盖层 DLL", extensions: ["dll"] }],
      });
      if (typeof selected === "string" && selected.trim()) {
        setOverlayDllPath(selected);
      }
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function injectOverlayDll() {
    setInjectingOverlay(true);
    try {
      const message = await invoke<string>("start_overlay_session", {
        dllPath: overlayDllPath.trim(),
      });
      showToast(message, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setInjectingOverlay(false);
    }
  }

  async function openShot(name: string) {
    try {
      const bytes = await invoke<ArrayBuffer>("read_viewpoint_shot", { name });
      const mime = mimeTypeFor(name);
      const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
      setPreview({ name, url, bytes });
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function copyPreview() {
    if (!preview) {
      return;
    }
    setCopyingShot(true);
    try {
      const bitmap = await createImageBitmap(new Blob([preview.bytes], { type: mimeTypeFor(preview.name) }));
      const canvas = document.createElement("canvas");
      canvas.width = bitmap.width;
      canvas.height = bitmap.height;
      const context = canvas.getContext("2d");
      if (!context) {
        throw new Error("无法创建图片画布。");
      }
      context.drawImage(bitmap, 0, 0);
      bitmap.close();
      const rgba = context.getImageData(0, 0, canvas.width, canvas.height);
      const image = await TauriImage.new(new Uint8Array(rgba.data), canvas.width, canvas.height);
      await writeImage(image);
      await image.close();
      showToast("截图已复制到剪贴板。", "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setCopyingShot(false);
    }
  }

  async function importShaderpacks(paths: string[]) {
    setLoadingMedia(true);
    try {
      const imported: string[] = [];
      for (const path of paths) {
        imported.push(await invoke<string>("import_viewpoint_shaderpack", { sourcePath: path }));
      }
      await refreshMedia();
      showToast(`已导入 ${imported.length} 个光影包：${imported.join("、")}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      await refreshMedia();
    } finally {
      setLoadingMedia(false);
    }
  }

  async function chooseShaderpacks() {
    const selected = await open({
      title: "选择 Viewpoint 光影包",
      multiple: true,
      filters: [{ name: "Minecraft 光影包", extensions: ["zip"] }],
    });
    const paths = selected ? (Array.isArray(selected) ? selected : [selected]) : [];
    if (paths.length > 0) {
      await importShaderpacks(paths);
    }
  }

  async function deleteShaderpack(name: string) {
    setDeletingShaderpack(name);
    try {
      await invoke("delete_viewpoint_shaderpack", { name });
      await refreshMedia();
      showToast(`已删除光影包：${name}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setDeletingShaderpack(null);
    }
  }

  function updateTask(id: string, patch: Partial<TaskItem>) {
    setTasks((current) => current.map((task) => (task.id === id ? { ...task, ...patch } : task)));
  }

  function startShaderDownloadTask(
    request: ShaderDownloadRequest,
    title: string,
    detail = "",
  ): TaskItem {
    const id = "shader-task-" + (++taskIdRef.current);
    const task: TaskItem = {
      id,
      kind: "shader-download",
      title: title.trim() || request.fileName.trim() || "光影包下载",
      detail: detail.trim() || request.fileName.trim() || request.url,
      status: "running",
      downloaded: 0,
      total: null,
      createdAt: Date.now(),
      finishedAt: null,
      error: null,
      savedPath: null,
      request,
    };
    setTasks((current) => [task, ...current]);
    showToast("已加入任务中心：" + task.title, "info");
    void executeShaderDownloadTask(task);
    return task;
  }

  async function executeShaderDownloadTask(task: TaskItem) {
    const attempt = (target: string) =>
      invoke<string>("download_shader_file", {
        downloadId: task.id,
        url: target,
        fileName: task.request.fileName,
        directory: task.request.directory,
        threads: task.request.threads,
      });

    try {
      let savedPath: string;
      try {
        savedPath = await attempt(task.request.url);
      } catch (error: unknown) {
        // PCL2 风格：镜像源失败后依次回退到备用源（官方源、CDN 域名变体）
        const fallbacks = task.request.fallbackUrls.filter((url) => url !== task.request.url);
        if (fallbacks.length === 0) {
          throw error;
        }
        let lastError: unknown = error;
        savedPath = "";
        for (const [index, fallbackUrl] of fallbacks.entries()) {
          updateTask(task.id, { detail: `镜像源不可用，正在改用备用源重试（${index + 1}/${fallbacks.length}）…` });
          if (index === 0) {
            showToast("镜像下载源不可用，正在改用备用源重试…", "info");
          }
          try {
            savedPath = await attempt(fallbackUrl);
            lastError = null;
            break;
          } catch (fallbackError: unknown) {
            lastError = fallbackError;
          }
        }
        if (lastError) {
          throw lastError;
        }
      }
      updateTask(task.id, {
        status: "success",
        savedPath,
        finishedAt: Date.now(),
      });
      await refreshMedia();
      showToast("已下载并保存至：" + savedPath, "success");
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      updateTask(task.id, { status: "error", error: message, finishedAt: Date.now() });
      showToast(message, "error");
    }
  }

  function startShaderDownload(file: ResourceFile) {
    startShaderDownloadTask(
      {
        url: file.downloadUrl,
        fallbackUrls: file.fallbackDownloadUrls,
        fileName: file.fileName,
        directory: shaderDownloadDirectory,
        threads: shaderDownloadThreads,
      },
      file.fileName,
      [file.releaseType, file.gameVersions.slice(0, 3).join("、") || "版本未知"].filter(Boolean).join(" · "),
    );
  }

  function startCustomShaderDownload() {
    const url = shaderDownloadUrl.trim();
    if (!url) {
      showToast("请输入下载链接。", "info");
      return;
    }
    const title = shaderDownloadFileName.trim() || url.split("/").pop() || "光影包下载";
    startShaderDownloadTask(
      {
        url,
        fallbackUrls: [],
        fileName: shaderDownloadFileName,
        directory: shaderDownloadDirectory,
        threads: shaderDownloadThreads,
      },
      title,
      "自定义下载",
    );
  }

  function retryShaderDownload(task: TaskItem) {
    const nextTask: TaskItem = {
      ...task,
      status: "running",
      downloaded: 0,
      total: null,
      error: null,
      savedPath: null,
      finishedAt: null,
      detail: task.request.fileName.trim() || task.detail,
    };
    setTasks((current) => current.map((item) => (item.id === task.id ? nextTask : item)));
    void executeShaderDownloadTask(nextTask);
  }

  function removeTask(id: string) {
    setTasks((current) => current.filter((task) => task.id !== id));
  }

  function clearFinishedTasks() {
    setTasks((current) => current.filter((task) => task.status === "running"));
  }

  async function saveShaderDownloadSettings() {
    setShaderDownloadSaving(true);
    try {
      await invoke("save_shader_download_settings", { url: shaderDownloadUrl, directory: shaderDownloadDirectory, threads: shaderDownloadThreads });
      showToast("下载器设置已保存", "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setShaderDownloadSaving(false);
    }
  }

  async function chooseShaderDownloadDirectory() {
    const selected = await open({ title: "选择光影包保存目录", directory: true, multiple: false });
    if (typeof selected === "string") setShaderDownloadDirectory(selected);
  }

  async function loadWorkshop(filters: WorkshopFilters) {
    const requestId = ++workshopRequestId.current;
    setWorkshopLoading(true);
    try {
      const response = await invoke<WorkshopResponse>("browse_workshop", {
        filters: {
          browsesort: filters.browsesort,
          days: filters.days,
          searchText: filters.searchText,
          page: filters.page,
          numPerPage: filters.numPerPage,
          requiredTags: filters.requiredTags,
          excludedTags: filters.excludedTags,
        },
      });
      if (requestId === workshopRequestId.current) {
        setWorkshopResponse(response);
      }
    } catch (error: unknown) {
      if (requestId === workshopRequestId.current) {
        showToast(error instanceof Error ? error.message : String(error), "error");
        setWorkshopResponse(null);
      }
    } finally {
      if (requestId === workshopRequestId.current) {
        setWorkshopLoading(false);
      }
    }
  }

  function scheduleWorkshopLoad(nextFilters: WorkshopFilters, delay = 250) {
    setWorkshopFilters(nextFilters);
    if (workshopSearchTimer.current) {
      clearTimeout(workshopSearchTimer.current);
    }
    workshopSearchTimer.current = setTimeout(() => {
      void loadWorkshop(nextFilters);
    }, delay);
  }

  function updateWorkshopFilters(
    changes: Partial<WorkshopFilters>,
    delay = 250,
    resetPage = true,
  ) {
    scheduleWorkshopLoad(
      {
        ...workshopFilters,
        ...changes,
        page: resetPage ? 1 : changes.page ?? workshopFilters.page,
      },
      delay,
    );
  }

  function toggleWorkshopTag(tag: string, kind: WorkshopTagMode) {
    const otherKind: WorkshopTagMode = kind === "requiredTags" ? "excludedTags" : "requiredTags";
    const isSelected = workshopFilters[kind].includes(tag);
    updateWorkshopFilters({
      [kind]: isSelected
        ? workshopFilters[kind].filter((value) => value !== tag)
        : [...workshopFilters[kind], tag],
      [otherKind]: isSelected
        ? workshopFilters[otherKind]
        : workshopFilters[otherKind].filter((value) => value !== tag),
    });
  }

  function removeWorkshopTag(tag: string, kind: WorkshopTagMode) {
    updateWorkshopFilters({
      [kind]: workshopFilters[kind].filter((value) => value !== tag),
    });
  }

  function resetWorkshopFilters() {
    const reset = {
      ...initialWorkshopFilters,
      requiredTags: [],
      excludedTags: [],
    };
    scheduleWorkshopLoad(reset, 0);
  }

  async function openWorkshopItem(workshopId: string, fallbackUrl: string) {
    try {
      showToast("正在打开 Steam 创意工坊资源。", "info");
      await openUrl(steamWorkshopUrl(workshopId));
      return;
    } catch {
      try {
        await openUrl(fallbackUrl);
      } catch (error: unknown) {
        showToast(error instanceof Error ? error.message : String(error), "error");
      }
    }
  }

  function openRecommendedWorkshop(workshopId: string) {
    void openUrl(steamWorkshopUrl(workshopId)).catch((error: unknown) => {
      showToast(error instanceof Error ? error.message : String(error), "error");
    });
  }

  /** 右键「窗口内打开」：抓详情 → 在模态框里按开发者新闻那条翻译管线翻译。
      语言取设置里的全局翻译语言（设置 › 通用）。 */
  async function openWorkshopItemInWindow(item: WorkshopItem) {
    setWorkshopDetailTarget(item);
    setWorkshopDetail(null);
    setWorkshopDetailClosing(false);
    setWorkshopDetailProgress({ completed: 0, total: 0 });
    setWorkshopDetailLoading(true);
    try {
      const detail = await invoke<WorkshopItemDetail>("fetch_workshop_item_detail", {
        id: item.id,
        language: translationLanguage,
      });
      setWorkshopDetail(detail);
    } catch (error: unknown) {
      // 抓不到就别留一个空弹窗：收起并报错。
      setWorkshopDetailTarget(null);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setWorkshopDetailLoading(false);
    }
  }

  function closeWorkshopDetail() {
    if (workshopDetailCloseTimer.current != null) {
      return;
    }
    setWorkshopDetailClosing(true);
    workshopDetailCloseTimer.current = setTimeout(() => {
      workshopDetailCloseTimer.current = null;
      setWorkshopDetailClosing(false);
      setWorkshopDetailTarget(null);
      setWorkshopDetail(null);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  useEffect(
    () => () => {
      if (workshopDetailCloseTimer.current != null) {
        clearTimeout(workshopDetailCloseTimer.current);
        workshopDetailCloseTimer.current = null;
      }
    },
    [],
  );

  // 详情弹窗打开期间支持 Escape 关闭（ui-style-guide §7）。
  useEffect(() => {
    if (!workshopDetailTarget) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeWorkshopDetail();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [workshopDetailTarget]);

  useEffect(() => {
    if (!feedbackOpen) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeFeedbackDialog();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [feedbackOpen]);

  const statusMessage =
    detection.status === "checking"
      ? "正在检查 Steam 游戏库..."
      : detection.status === "notFound"
        ? "未检测到 Project Zomboid 安装。"
          : detection.status === "error"
            ? detection.message
            : null;

  const selectedWorkshopBackground = backgroundPath ? workshopBackgrounds.find((background) => background.path === backgroundPath && background.source === "workshop") ?? null : null;
  const selectedWorkshopName = selectedWorkshopBackground?.modName ?? selectedWorkshopBackground?.name.split(" · ", 1)[0] ?? "";

  const startupTimingLabel = startupTimingText(startupResult, startupStartedAt, startupClock);

  const launchContent = (
    <>
      <AcrylicGlass backgroundUrl={backgroundUrl} seasonal={wallpaperSeasonal} effect={wallpaperEffect} />
      {selectedWorkshopBackground && (
        <Box
          className={`pz-launch-workshop-info${workshopBackgroundInfoOpen ? " is-open" : ""}`}
          data-pz-acrylic="workshop"
        >
          <Box
            aria-hidden={!workshopBackgroundInfoOpen}
            className="pz-launch-workshop-info-details"
            id="pz-workshop-background-info"
          >
            <Text className="pz-launch-workshop-info-title">创意工坊背景</Text>
            <Text className="pz-launch-workshop-info-name" title={selectedWorkshopName}>{selectedWorkshopName}</Text>
            <Text className="pz-launch-workshop-info-id" title={`Workshop ID: ${selectedWorkshopBackground.workshopId}`}>Workshop ID: {selectedWorkshopBackground.workshopId}</Text>
          </Box>
          <Button
            aria-controls="pz-workshop-background-info"
            aria-expanded={workshopBackgroundInfoOpen}
            aria-label={workshopBackgroundInfoOpen ? "收起创意工坊背景信息" : "展开创意工坊背景信息"}
            className="pz-launch-workshop-info-toggle"
            onClick={() => setWorkshopBackgroundInfoOpen((open) => !open)}
            title={workshopBackgroundInfoOpen ? "收起创意工坊背景信息" : "展开创意工坊背景信息"}
            variant="ghost"
          >
            <span aria-hidden="true" className="pz-launch-workshop-info-chevron">›</span>
          </Button>
        </Box>
      )}
      <HStack className="pz-launch-bottom-actions" align="stretch" gap="3">
        <Box className={`pz-launch-account${gameWindowReady ? " pz-launch-slide-down" : ""}`} data-pz-acrylic="account">
          {detection.status === "found" && detection.paths.profile ? (
            <HStack align="center" gap="3" minW="0">
              <Avatar.Root colorPalette="blue" size="md">
                <Avatar.Fallback name={detection.paths.profile.personaName} />
              </Avatar.Root>
              <Box minW="0">
                <Text color="gray.500" fontSize="xs" fontWeight="700" letterSpacing="0.08em">
                  STEAM ACCOUNT
                </Text>
                <Text color="gray.800" fontSize="md" fontWeight="700" mt="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                  {detection.paths.profile.personaName}
                </Text>
                <Box aria-label={steamAccountHidden ? "显示 Steam 账号" : "隐藏 Steam 账号"} as="button" className={`pz-steam-id-toggle${steamAccountHidden ? " is-hidden" : ""}`} onClick={() => void updateSteamAccountHidden(!steamAccountHidden)}>
                  <Text className="pz-steam-id-value" color="gray.500" fontSize="xs" overflowWrap="anywhere">{detection.paths.profile.accountName}</Text>
                  <Box aria-hidden="true" className="pz-steam-id-line" />
                </Box>
              </Box>
            </HStack>
          ) : (
            <Text color="gray.600" fontSize="sm">
              {statusMessage ?? "正在读取本机账户信息..."}
            </Text>
          )}
        </Box>

        <Box className="pz-launch-command" data-pz-acrylic="command" ref={launchCommandRef}>
        <Box
          aria-hidden={!launchDetailsOpen}
          className={`pz-launch-popover-shell pz-launch-details-popover-shell${launchDetailsOpen ? " is-open" : ""}`}
        >
          <Box className="pz-launch-details-popover">
            <HStack justify="space-between" gap="2">
              <Text color="gray.500" fontSize="xs" fontWeight="700">
                启动参数
              </Text>
              <Button
                aria-label="新增启动参数"
                className="pz-launch-argument-action"
                onClick={() => setLaunchArguments((current) => [...current, ""])}
                size="xs"
                title="新增启动参数"
                variant="ghost"
              >
                <FontAwesomeIcon aria-hidden="true" icon={faPlus} />
              </Button>
            </HStack>
            <Stack gap="1.5" mt="2">
              {launchArguments.length > 0 ? (
                launchArguments.map((argument, index) => (
                  <HStack key={`launch-argument-${index}`} gap="1">
                    <Input
                      aria-label={`启动参数 ${index + 1}`}
                      className="pz-launch-argument-input"
                      fontFamily="mono"
                      onChange={(event) => {
                        const nextValue = event.currentTarget.value;
                        setLaunchArguments((current) =>
                          current.map((item, itemIndex) => (itemIndex === index ? nextValue : item)),
                        );
                      }}
                      placeholder={`参数 ${index + 1}`}
                      size="sm"
                      value={argument}
                    />
                    <Button
                      aria-label={`删除启动参数 ${index + 1}`}
                      className="pz-launch-argument-action"
                      onClick={() =>
                        setLaunchArguments(launchArguments.filter((_, itemIndex) => itemIndex !== index))
                      }
                      size="xs"
                      title="删除启动参数"
                      variant="ghost"
                    >
                      <FontAwesomeIcon aria-hidden="true" icon={faTrash} />
                    </Button>
                  </HStack>
                ))
              ) : (
                <Text color="gray.500" fontSize="xs">
                  暂无启动参数
                </Text>
              )}
            </Stack>
            <Text color="gray.500" fontSize="xs" mt="2">
              多个参数会在启动时按从上到下的顺序自动组合，用空格分隔。
            </Text>
          </Box>
        </Box>

        <Box
          aria-hidden={!launchSettingsOpen}
          className={`pz-launch-popover-shell pz-launch-settings-popover-shell${launchSettingsOpen ? " is-open" : ""}`}
        >
          <Box className="pz-launch-settings-popover">
            <HStack justify="space-between" gap="3" mb="2" wrap="wrap">
              <Box>
                <Text color="gray.800" fontSize="sm" fontWeight="700">
                  启动设置
                </Text>
                <Text color="gray.500" fontSize="xs">
                  调整内存并选择背景
                </Text>
              </Box>
              <BackgroundMenu
                customBackgrounds={customBackgrounds}
                onChooseCustom={() => void chooseCustomBackground()}
                onOpenRecommendedWorkshop={openRecommendedWorkshop}
                onScanWorkshop={() => void scanWorkshopBackgrounds()}
                onSelect={(background) => {
                  void applyBackground(background);
                  showToast(`已设置背景：${background.name}`, "success");
                }}
                onSelectBuiltin={(background) => void selectBuiltinBackground(background)}
                wallpaperEffect={wallpaperEffect}
                wallpaperSettings={wallpaperEffectSettings}
                onSetWallpaperDay={setWallpaperDay}
                onSetWallpaperTime={setWallpaperTime}
                onToggleWallpaperPin={toggleWallpaperPin}
                onToggleWallpaperAutoContrast={toggleWallpaperAutoContrast}
                workshopBackgrounds={workshopBackgrounds}
                workshopLoading={workshopBackgroundLoading}
                workshopScanProgress={workshopBackgroundScanProgress}
              />
            </HStack>

            <HStack justify="space-between" gap="3" mb="2" wrap="wrap">
              <Text color="gray.700" fontSize="sm" fontWeight="700">
                游戏内存
              </Text>
              <Text color="blue.700" fontFamily="mono" fontSize="sm" fontWeight="700">
                {memoryLimitMb === null ? "读取中..." : formatMemoryMb(memoryLimitMb)}
              </Text>
            </HStack>
            {systemMemoryMb !== null && memoryLimitMb !== null ? (
              <>
                <Box className="pz-memory-slider-wrap">
                  <Slider.Root
                    aria-label={["游戏内存限制"]}
                    className="pz-memory-slider"
                    max={systemMemoryMb}
                    min={MIN_MEMORY_MB}
                    step={MEMORY_STEP_MB}
                    value={[memoryLimitMb]}
                    onValueChange={({ value }) => {
                      const nextValue = value[0];
                      if (nextValue !== undefined) {
                        updateMemoryLimit(nextValue);
                      }
                    }}
                  >
                    <Slider.Control>
                      <Slider.Track>
                        <Slider.Range />
                      </Slider.Track>
                      <Slider.Thumb index={0}>
                        <Slider.HiddenInput />
                      </Slider.Thumb>
                    </Slider.Control>
                  </Slider.Root>
                </Box>
                <HStack justify="space-between" color="gray.500" fontSize="xs" mt="2">
                  <Text>{formatMemoryMb(MIN_MEMORY_MB)}</Text>
                  <Text>系统上限 {formatMemoryMb(systemMemoryMb)}</Text>
                </HStack>
                <HStack className="pz-memory-recommendations" gap="3" mt="2" wrap="wrap">
                  <Text color="green.700" fontSize="xs">
                    默认：物理内存的一半
                  </Text>
                  {MEMORY_WARNING_MB <= systemMemoryMb && (
                    <Text color="orange.700" fontSize="xs">
                      参考线：16 GB
                    </Text>
                  )}
                </HStack>
                {memoryLimitMb > MEMORY_WARNING_MB && (
                  <Text color="orange.700" fontSize="xs" mt="2">
                    超出 16 GB 可能会导致游戏运行不稳定。
                  </Text>
                )}
              </>
            ) : (
              <Text color={memoryError ? "red.600" : "gray.500"} fontSize="xs">
                {memoryError ?? "正在读取系统物理内存..."}
              </Text>
            )}
          </Box>
        </Box>

        <Box className="pz-launch-card-shell">
          <Button
            className={`pz-launch-card-button${gameWindowReady ? " pz-launch-slide-down" : ""}`}
            disabled={detection.status !== "found" || !detection.paths.installPath || launchCooldown}
            loading={launchingGame}
            onClick={() => void launchProjectZomboid()}
            size="lg"
            variant="plain"
            width="100%"
          >
            <Stack align="center" gap="0">
              <Text className="pz-launch-card-title">启动游戏</Text>
              <Text className="pz-launch-card-subtitle">Project Zomboid</Text>
            </Stack>
          </Button>
          <Button
            className={`pz-launch-card-button pz-launch-stop-button${gameWindowReady ? " is-in" : ""}`}
            disabled={!gameWindowReady}
            loading={stoppingGame}
            onClick={() => void stopGameProcess()}
            size="lg"
            variant="plain"
            width="100%"
          >
            <Stack align="center" gap="0">
              <Text className="pz-launch-card-title">停止游戏进程</Text>
              <Text className="pz-launch-card-subtitle">强制结束 Project Zomboid</Text>
            </Stack>
          </Button>
          <HStack className="pz-launch-command-toolbar" justify="end" gap="1">
            <Button
              aria-label="启动设置"
              className={`pz-launch-icon-button${gameWindowReady ? " pz-launch-slide-left" : ""}`}
              onClick={(event) => {
                event.stopPropagation();
                setLaunchSettingsOpen((open) => !open);
                setLaunchDetailsOpen(false);
              }}
              title="启动设置"
              variant="plain"
            >
              <FontAwesomeIcon aria-hidden="true" className="pz-launch-fa-icon" icon={faGear} />
            </Button>
            <Button
              aria-label="启动详情"
              className={`pz-launch-icon-button pz-launch-info-button${gameWindowReady ? " pz-launch-slide-left" : ""}`}
              onClick={(event) => {
                event.stopPropagation();
                setLaunchDetailsOpen((open) => !open);
                setLaunchSettingsOpen(false);
              }}
              title="启动详情"
              variant="plain"
            >
              <FontAwesomeIcon aria-hidden="true" className="pz-launch-fa-icon" icon={faCircleInfo} />
            </Button>
          </HStack>
          {gameWindowReady && startupTimingLabel !== "" && (
            <Text className="pz-launch-startup-timing">{startupTimingLabel}</Text>
          )}
        </Box>
        </Box>
      </HStack>
    </>
  );

  // 加载控制台面板：原在启动页，现移入「设置 → 控制台」。
  const overlayConsolePanel = (
      <Box
        className="pz-panel"
        bg="white"
        borderColor="gray.200"
        borderRadius="md"
        borderWidth="1px"
        p="4"
      >
        <HStack align="start" gap="2" justify="space-between" wrap="wrap">
          <Text color="gray.900" fontSize="sm" fontWeight="700">
            {developerMode
              ? modLoaderScheme === "zombieBuddy"
                ? "覆盖层与 ZombieBuddy 加载进度"
                : "覆盖层与游戏加载进度"
              : "游戏加载状态"}
          </Text>
          {overlaySession && (
            <Badge
              colorPalette={
                overlaySession.phase === "error"
                  ? "red"
                  : overlaySession.phase === "finished"
                    ? "green"
                    : "blue"
              }
            >
              {overlayPhaseLabel(overlaySession.phase)}
            </Badge>
          )}
        </HStack>
        {developerMode && (
          <Text color="gray.500" fontSize="xs" mt="1">
            启动游戏后自动注入覆盖层，并保持红色直到
            {modLoaderScheme === "zombieBuddy" ? " ZombieBuddy " : "游戏"}加载完成。
          </Text>
        )}
        {overlaySession && (
          <Text color="gray.700" fontSize="xs" mt="2">
            {overlaySession.message}
          </Text>
        )}
        {developerMode && overlayZbProgress && (
          <Box mt="2">
            <HStack color="gray.700" fontSize="xs" gap="2" wrap="wrap">
              <Text fontWeight="700">[ZB] {overlayZbProgress.phase}</Text>
              <Text>已输出 {overlayZbProgress.zbLines} 行</Text>
              <Text>已用 {formatElapsed(overlayZbProgress.elapsedMs)}</Text>
            </HStack>
            <Text
              color="gray.500"
              fontFamily="mono"
              fontSize="xs"
              mt="1"
              wordBreak="break-all"
            >
              {overlayZbProgress.lastLine}
            </Text>
          </Box>
        )}
        <HStack gap="2" mt="3" wrap="wrap">
          <Button
            colorPalette="red"
            disabled={
              !overlayDllPath.trim() ||
              (overlaySession !== null &&
                overlaySession.phase !== "finished" &&
                overlaySession.phase !== "error")
            }
            loading={injectingOverlay}
            onClick={() => void injectOverlayDll()}
            size="sm"
          >
            开始监听游戏加载
          </Button>
          {developerMode && (
            <Button onClick={() => void chooseOverlayDll()} size="sm" variant="outline">
              选择 DLL…
            </Button>
          )}
        </HStack>
        {developerMode && (
          <Text color="gray.500" fontFamily="mono" fontSize="xs" mt="2" wordBreak="break-all">
            {overlayDllPath}
          </Text>
        )}
      </Box>
  );

  const consoleContent = (
    <Stack gap="4">
      {overlayConsolePanel}
      <NotificationConsole entries={notificationHistory} onClear={clearNotificationHistory} />
    </Stack>
  );

  const pzOptimizationStatus =
    detection.status === "found" ? detection.paths.pzOptimization : null;
  const pzOptimizationNeedsUpdate =
    Boolean(
      pzOptimizationStatus?.installed &&
      pzOptimizationStatus.availableRevision &&
      pzOptimizationStatus.installedRevision !== pzOptimizationStatus.availableRevision,
    );
  const pzOptimizationInstalledCurrent = Boolean(
    pzOptimizationStatus?.installed &&
    pzOptimizationStatus.availableRevision &&
    pzOptimizationStatus.installedRevision === pzOptimizationStatus.availableRevision,
  );

  // 支持库页的三项安装内容同属一个面板；三项都不可见时整个面板不渲染。
  // 覆盖每个行自己的显示条件，保证"有行要显示"才出现面板。
  const supportPanelVisible =
    detection.status === "found" &&
    Boolean(
      detection.paths.installPath ||
        detection.paths.zombieBuddy.available ||
        detection.paths.zombieBuddy.installed ||
        detection.paths.viewpoint3d.mods.some((item) => item.subscribed),
    );

  const overviewContent = (
    <>
      {detection.status === "found" && supportPanelVisible && (
        <Box className="pz-panel pz-support-panel">
          <Stack gap="3">
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              支持组件
            </Text>
            <Stack className="pz-support-option-list" gap="2">
              {detection.status === "found" &&
                (detection.paths.installPath ||
                  detection.paths.zombieBuddy.available ||
                  detection.paths.zombieBuddy.installed) && (
              <SupportOptionRow
                badge={
                  detection.paths.zombieBuddy.installed
                    ? "已安装"
                    : detection.paths.zombieBuddy.available
                      ? "待安装"
                      : "模组未下载"
                }
                badgePalette={
                  detection.paths.zombieBuddy.installed
                    ? "green"
                    : detection.paths.zombieBuddy.available
                      ? "orange"
                      : "gray"
                }
                description="负责加载 JAR 类型的模组；普通 Lua 模组不受影响，无需额外设置。"
                expanded={supportZombieBuddyExpanded}
                name="ZombieBuddy 模组加载器"
                onToggle={() => setSupportZombieBuddyExpanded((value) => !value)}
              >
                {developerMode && (
                  <Text color="gray.500" fontSize="sm">
                    配置：Both（JSON + Steam 启动参数 + BAT）
                  </Text>
                )}
                <Text color="gray.600" fontSize="sm" mt="2">
                  {detection.paths.zombieBuddy.installed
                    ? "已安装。相关设置都在「设置 › 模组加载器」里。"
                    : `未安装${
                        detection.paths.zombieBuddy.missingComponents.length
                          ? `（缺少：${detection.paths.zombieBuddy.missingComponents.join("、")}）`
                          : ""
                      }。到「设置 › 模组加载器」即可完成安装。`}
                </Text>
                {!detection.paths.zombieBuddy.available &&
                  !detection.paths.zombieBuddy.installed && (
                    <Text color="gray.600" fontSize="sm" mt="2">
                      {detection.paths.zombieBuddy.sourcePath
                        ? "Workshop 文件不完整，请在 Steam 中重新下载 ZombieBuddy。"
                        : "请先在 Steam 创意工坊订阅并下载 ZombieBuddy。"}
                    </Text>
                  )}
                <HStack mt="3" wrap="wrap">
                  <Button onClick={openModLoaderSettings} size="sm" variant="outline">
                    前往 设置 › 模组加载器
                  </Button>
                </HStack>
              </SupportOptionRow>
              )}

              {detection.status === "found" && detection.paths.installPath && pzOptimizationStatus && (
              <SupportOptionRow
                badge={
                  pzOptimizationInstalledCurrent
                    ? "已安装"
                    : pzOptimizationNeedsUpdate
                      ? "可更新"
                      : pzOptimizationStatus.available
                        ? "可安装"
                        : "未下载"
                }
                badgePalette={
                  pzOptimizationInstalledCurrent
                    ? "green"
                    : pzOptimizationStatus.available
                      ? pzOptimizationNeedsUpdate
                        ? "orange"
                        : "blue"
                      : "gray"
                }
                description="优化游戏性能的模组，以替换部分游戏文件的方式安装，不会改动游戏主程序；卸载时会自动还原。"
                expanded={supportPzOptimizationExpanded}
                name="PZ Optimization"
                onToggle={() => setSupportPzOptimizationExpanded((value) => !value)}
              >
                <Text color="gray.600" fontSize="sm">
                  {pzOptimizationInstalledCurrent
                    ? "已安装 PZ Optimization。"
                    : pzOptimizationStatus.available
                      ? "创意工坊文件已下载，点击下方按钮即可自动安装。"
                      : "请先在 Steam 创意工坊订阅并等待 PZ Optimization 下载完成。"}
                </Text>
                {developerMode && (
                  <Stack gap="0.5" mt="2">
                    <Text color="gray.500" fontFamily="mono" fontSize="xs" overflowWrap="anywhere">
                      来源：{pzOptimizationStatus.sourcePath ?? "未找到"}
                    </Text>
                    <Text color="gray.500" fontFamily="mono" fontSize="xs">
                      已安装版本：{pzOptimizationStatus.installedRevision ?? "未检测到"}
                    </Text>
                    <Text color="gray.500" fontFamily="mono" fontSize="xs">
                      Workshop 版本：{pzOptimizationStatus.availableRevision ?? "未检测到"}
                    </Text>
                  </Stack>
                )}
                <HStack gap="2" mt="3" wrap="wrap">
                  {!pzOptimizationStatus.available && (
                    <Button
                      onClick={() => openRecommendedWorkshop(PZ_OPTIMIZATION_WORKSHOP_ID)}
                      size="sm"
                      variant="outline"
                    >
                      打开创意工坊
                    </Button>
                  )}
                  <Button
                    colorPalette="blue"
                    disabled={
                      !pzOptimizationStatus.available ||
                      pzOptimizationInstalledCurrent
                    }
                    loading={installingPzOptimization}
                    onClick={() => void installPzOptimization()}
                    size="sm"
                  >
                    {pzOptimizationInstalledCurrent
                      ? "已安装"
                      : pzOptimizationNeedsUpdate
                        ? "一键更新"
                        : "一键安装"}
                  </Button>
                  {pzOptimizationStatus.installed && (
                    <Button
                      colorPalette="red"
                      loading={uninstallingPzOptimization}
                      onClick={() => void uninstallPzOptimization()}
                      size="sm"
                      variant="outline"
                    >
                      卸载
                    </Button>
                  )}
                </HStack>
                <Text color="orange.700" fontSize="xs" mt="3">
                  安装会替换部分游戏文件，卸载时会自动还原。安装前请先完全退出游戏；暂未测试与其他模组、多人游戏的兼容性。
                </Text>
              </SupportOptionRow>
              )}

              {detection.status === "found" &&
                (detection.paths.installPath || detection.paths.viewpoint3d.mods.some((item) => item.subscribed)) && (
              <SupportOptionRow
                badge={detection.paths.viewpoint3d.ready ? "订阅完整" : "需要订阅"}
                badgePalette={detection.paths.viewpoint3d.ready ? "green" : "orange"}
                description="需要订阅 Project Viewpoint 和 ZombieBuddy 两个创意工坊模组。"
                expanded={supportViewpointExpanded}
                name="Project Viewpoint 3D 模组配置"
                onToggle={() => setSupportViewpointExpanded((value) => !value)}
              >
                <HStack className="pz-viewpoint-mod-list" gap="1" mt="2" wrap="wrap">
                  {detection.paths.viewpoint3d.mods.map((mod) => (
                    <Badge
                      key={mod.name}
                      colorPalette={mod.subscribed ? "green" : "gray"}
                      variant={mod.subscribed ? "subtle" : "outline"}
                    >
                      {mod.name} · {mod.subscribed ? "已订阅" : "未订阅"}
                    </Badge>
                  ))}
                </HStack>
                {!detection.paths.viewpoint3d.ready && (
                  <Text color="orange.700" fontSize="sm" mt="2">
                    缺少：{detection.paths.viewpoint3d.missingComponents.join("、")}
                  </Text>
                )}
                <HStack gap="1" mt="3" wrap="wrap">
                  <Button
                    colorPalette="blue"
                    disabled={!detection.paths.viewpoint3d.ready || !detection.paths.installPath}
                    loading={configuringViewpoint}
                    onClick={() => void configureViewpointThreeD()}
                    size="sm"
                  >
                    一键配置 3D 模组
                  </Button>
                </HStack>
              </SupportOptionRow>
              )}
            </Stack>
          </Stack>
        </Box>
      )}

      {developerMode && (
        <Box className="pz-panel" bg="white" borderColor="gray.200" borderRadius="md" borderWidth="1px" p={{ base: "4", md: "6" }}>
          <Stack gap="4">
            <PathValue
              label="Steam 安装路径"
              value={detection.status === "found" ? detection.paths.installPath : null}
              fallback={detection.status === "found" ? "未检测到 Project Zomboid 安装目录。" : statusMessage}
              error={detection.status === "error"}
            />
            <PathValue
              label="创意工坊路径（108600）"
              value={detection.status === "found" ? detection.paths.workshopPath : null}
              fallback={detection.status === "error" ? detection.message : "未检测到创意工坊目录。"}
              error={detection.status === "error"}
            />
          </Stack>
        </Box>
      )}
    </>
  );

  const shotsContent = (
    <MediaPanel
      title="Viewpoint 截图"
      description="预览截图并复制到系统剪贴板。"
      directory={shots?.directory}
      developerMode={developerMode}
    >
      {loadingMedia && !shots ? (
        <Text color="gray.500">正在读取截图目录...</Text>
      ) : shots && shots.files.length > 0 ? (
        <Box display="grid" gap="3" gridTemplateColumns="repeat(auto-fill, minmax(160px, 1fr))">
          {shots.files.map((shot) => (
            <Box
              className="pz-media-card"
              key={shot.name}
              bg="gray.50"
              borderColor="gray.200"
              borderRadius="sm"
              borderWidth="1px"
              cursor="pointer"
              onClick={() => void openShot(shot.name)}
              overflow="hidden"
              _hover={{ borderColor: "blue.400", boxShadow: "sm" }}
            >
              <ShotThumbnail name={shot.name} />
              <Box p="2">
                <Text color="gray.800" fontSize="sm" fontWeight="600" overflowWrap="anywhere">
                  {shot.name}
                </Text>
                <Text color="gray.500" fontSize="xs" mt="1">
                  {formatBytes(shot.sizeBytes)}
                </Text>
              </Box>
            </Box>
          ))}
        </Box>
      ) : (
        <EmptyState text="截图目录中暂无图片。" />
      )}
    </MediaPanel>
  );

  const shaderpacksContent = (
    <MediaPanel
      title="Viewpoint 光影包"
      description="导入 Minecraft ZIP 光影包，文件会复制到 Viewpoint 光影目录。"
      directory={shaderpacks?.directory}
      developerMode={developerMode}
    >
      <Box
        className="pz-dropzone"
        ref={dropZoneRef}
        borderColor="blue.300"
        borderRadius="md"
        borderStyle="dashed"
        borderWidth="2px"
        p={{ base: "5", md: "6" }}
        textAlign="center"
      >
        <Text color="gray.700" fontWeight="700">
          将 ZIP 光影包拖到这里
        </Text>
        <Text color="gray.500" fontSize="sm" mt="2">
          或从文件选择器导入，已存在同名文件不会被覆盖
        </Text>
        <Button colorPalette="blue" loading={loadingMedia} mt="4" onClick={() => void chooseShaderpacks()}>
          选择 ZIP 文件
        </Button>
      </Box>

      <Stack gap="2" mt="4">
        {shaderpacks && shaderpacks.files.length > 0 ? (
          shaderpacks.files.map((pack) => (
            <HStack
              title={shaderpackSearchTerms[pack.name] ? `搜索词：${shaderpackSearchTerms[pack.name]}` : "搜索词：无"}
              className="pz-list-row"
              key={pack.name}
              align="center"
              borderBottomColor="gray.200"
              borderBottomWidth="1px"
              justify="space-between"
              minH="12"
              gap="3"
            >
              <Box className="pz-resource-download-icon">
                {shaderpackIcons[pack.name] ? (
                  <FallbackImage
                    alt=""
                    fallbackSrc={shaderpackIcons[pack.name]?.fallbackUrl}
                    loading="lazy"
                    onFail={() => void reSourceShaderpackIcon(pack.name)}
                    src={shaderpackIcons[pack.name]?.url ?? ""}
                  />
                ) : <Text>MC</Text>}
              </Box>
              <Box minW="0" flex="1">
                <Text color="gray.800" fontSize="sm" fontWeight="600" overflowWrap="anywhere">
                  {pack.name}
                </Text>
                <Text color="gray.500" fontSize="xs" mt="1">
                  {formatBytes(pack.sizeBytes)}
                </Text>
              </Box>
              <Button
                colorPalette="red"
                loading={deletingShaderpack === pack.name}
                onClick={() => void deleteShaderpack(pack.name)}
                size="sm"
                variant="outline"
              >
                删除
              </Button>
            </HStack>
          ))
        ) : (
          <EmptyState text="光影包目录中暂无 ZIP 文件。" />
        )}
      </Stack>
    </MediaPanel>
  );

  const workshopContent = (
    <Box
      className="pz-panel pz-workshop-page"
      bg="white"
      borderColor="gray.200"
      borderRadius="md"
      borderWidth="1px"
      overflow="hidden"
    >
      <Box className="pz-panel-header" p={{ base: "4", md: "5" }}>
        <HStack align="start" justify="space-between" gap="3" wrap="wrap">
          <Box>
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              Project Zomboid 创意工坊
            </Text>
          </Box>
          <HStack gap="2">
            <Button
              loading={workshopLoading}
              onClick={() => void loadWorkshop(workshopFilters)}
              size="sm"
              variant="outline"
            >
              刷新
            </Button>
            <Button onClick={resetWorkshopFilters} size="sm" variant="ghost">
              重置筛选
            </Button>
          </HStack>
        </HStack>
      </Box>

      <Box className="pz-workshop-browser">
        <Stack gap="2">
          <HStack className="pz-workshop-filter-row" gap="2" wrap="wrap">
            <HStack gap="2">
              <Text className="pz-filter-label" whiteSpace="nowrap">
                排序
              </Text>
              <WorkshopDropdown
                ariaLabel="选择排序方式"
                onChange={(value) => updateWorkshopFilters({ browsesort: value })}
                options={workshopOptions?.sortOptions ?? []}
                value={workshopFilters.browsesort}
                width={{ base: "128px", md: "156px" }}
              />
            </HStack>

            {workshopFilters.browsesort === "trend" && (
              <HStack gap="2">
                <Text className="pz-filter-label" whiteSpace="nowrap">
                  时间
                </Text>
                <WorkshopDropdown
                  ariaLabel="选择趋势时间范围"
                  onChange={(value) => updateWorkshopFilters({ days: Number(value) })}
                  options={[
                    { value: "1", label: "1 天" },
                    { value: "7", label: "7 天" },
                    { value: "30", label: "30 天" },
                    { value: "90", label: "90 天" },
                    { value: "365", label: "1 年" },
                  ]}
                  value={String(workshopFilters.days)}
                  width="104px"
                />
              </HStack>
            )}

            <HStack gap="2">
              <Text className="pz-filter-label" whiteSpace="nowrap">
                每页
              </Text>
              <WorkshopDropdown
                ariaLabel="选择每页数量"
                onChange={(value) => updateWorkshopFilters({ numPerPage: Number(value) }, 0)}
                options={[
                  { value: "15", label: "15" },
                  { value: "30", label: "30" },
                ]}
                value={String(workshopFilters.numPerPage)}
                width="80px"
              />
            </HStack>

            <WorkshopTagPicker
              compact
              excludedTags={workshopFilters.excludedTags}
              onRemove={removeWorkshopTag}
              onToggle={toggleWorkshopTag}
              requiredTags={workshopFilters.requiredTags}
              tags={workshopOptions?.tags ?? []}
            />
          </HStack>

          <HStack className="pz-workshop-search-row" gap="2" wrap="wrap">
            <Text className="pz-filter-label" whiteSpace="nowrap">
              名称
            </Text>
            <Input
              flex="1"
              maxW={{ base: "100%", md: "340px" }}
              onChange={(event) => updateWorkshopFilters({ searchText: event.currentTarget.value.trim() }, 400)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void loadWorkshop(workshopFilters);
                }
              }}
              placeholder="搜索模组标题或描述"
              size="sm"
              value={workshopFilters.searchText}
            />
            <Button
              colorPalette="blue"
              loading={workshopLoading}
              onClick={() => void loadWorkshop(workshopFilters)}
              size="sm"
            >
              搜索
            </Button>
          </HStack>

          <WorkshopTagSelectionSummary
            excludedTags={workshopFilters.excludedTags}
            onRemove={removeWorkshopTag}
            requiredTags={workshopFilters.requiredTags}
          />
        </Stack>

        <Box className="pz-workshop-results" mt="4">
          {workshopLoading && !workshopResponse ? (
            <Text color="gray.500" py="6" textAlign="center">
              正在读取 Steam 创意工坊...
            </Text>
          ) : workshopResponse ? (
            <>
              {workshopResponse.items.length > 0 ? (
                <Box className="pz-workshop-list">
                  {workshopResponse.items.map((item) => (
                    <Menu.Root
                      key={item.id}
                      positioning={{ strategy: "fixed", flip: true, shift: 8, overflowPadding: 8 }}
                    >
                      {/* 整行都是右键触发区；左键保持原来的「直接打开」逻辑。 */}
                      <Menu.ContextTrigger asChild>
                        <Box
                          className="pz-workshop-list-item"
                          onClick={() => void openWorkshopItem(item.id, item.fileUrl)}
                          role="button"
                          tabIndex={0}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              void openWorkshopItem(item.id, item.fileUrl);
                            }
                          }}
                        >
                          <Box className="pz-workshop-list-preview">
                            {item.previewUrl ? (
                              <FallbackImage
                                alt=""
                                fallbackSrc={previewImageFallbackUrl(
                                  item.previewUrl,
                                  effectiveSteamRelayBase,
                                  steamRelayMode,
                                )}
                                loading="lazy"
                                src={item.previewUrl}
                              />
                            ) : (
                              <Text color="gray.500" fontSize="xs">
                                无预览图
                              </Text>
                            )}
                          </Box>
                          <Box className="pz-workshop-list-body">
                            <HStack align="start" justify="space-between" gap="2" wrap="wrap">
                              <Text
                                color="gray.900"
                                fontSize="sm"
                                fontWeight="700"
                                lineClamp="1"
                                minW="0"
                              >
                                {item.title || "未命名模组"}
                              </Text>
                              <HStack className="pz-workshop-tags-inline" gap="1" flexShrink="0">
                                {item.tags.slice(0, 3).map((tag) => (
                                  <Badge key={tag} colorPalette="blue" size="sm" variant="subtle">
                                    {tag}
                                  </Badge>
                                ))}
                              </HStack>
                            </HStack>
                            <Text className="pz-workshop-description" color="gray.500" fontSize="xs" lineClamp="1" mt="1">
                              {item.shortDescription || "暂无描述"}
                            </Text>
                            <HStack
                              className="pz-workshop-list-meta"
                              color="gray.500"
                              fontSize="xs"
                              gap="3"
                              mt="1"
                              wrap="wrap"
                            >
                              <Text>更新时间 {formatWorkshopDate(item.timeUpdated)}</Text>
                              <Text>订阅 {item.subscriptions.toLocaleString()}</Text>
                              <Text>浏览 {item.views.toLocaleString()}</Text>
                              <Text>作者 {item.creatorName || "未知作者"}</Text>
                              <Text>大小 {formatWorkshopSize(item.fileSize)}</Text>
                            </HStack>
                          </Box>
                        </Box>
                      </Menu.ContextTrigger>
                      {/* 挂到 Portal：菜单若留在结果列表里，会落进列表的 mask-image 与
                          层叠上下文（渐隐遮罩会把它淡掉），也会被列表子元素的入场动画选择器命中。 */}
                      <Portal>
                        <Menu.Positioner className="pz-menu-positioner">
                          <Menu.Content className="pz-workshop-context-menu">
                            <Menu.Item
                              onSelect={() => void openWorkshopItem(item.id, item.fileUrl)}
                              value="steam"
                            >
                              <Text>在 Steam 中打开</Text>
                            </Menu.Item>
                            <Menu.Item
                              onSelect={() => void openWorkshopItemInWindow(item)}
                              value="window"
                            >
                              <Text>在窗口内打开</Text>
                            </Menu.Item>
                          </Menu.Content>
                        </Menu.Positioner>
                      </Portal>
                    </Menu.Root>
                  ))}
                </Box>
              ) : (
                <EmptyState text="没有匹配的结果，试试放宽筛选条件。" />
              )}

              <HStack
                className="pz-workshop-result-summary"
                color="gray.500"
                fontSize="sm"
                justify="space-between"
                mt="2"
                gap="3"
                wrap="wrap"
              >
                <Text>共 {workshopResponse.meta.totalCount.toLocaleString()} 个结果</Text>
                <Text>
                  第 {workshopResponse.meta.currentPage} / {workshopResponse.meta.totalPages} 页 · 本页{" "}
                  {workshopResponse.items.length} 条
                </Text>
              </HStack>

              {workshopResponse.meta.totalPages > 1 && (
                <HStack className="pz-workshop-pagination" justify="center" gap="1" wrap="wrap">
                  <Button
                    disabled={workshopFilters.page <= 1}
                    onClick={() => updateWorkshopFilters({ page: workshopFilters.page - 1 }, 0, false)}
                    size="sm"
                    variant="outline"
                  >
                    上一页
                  </Button>
                  {Array.from(
                    new Set(
                      [
                        1,
                        workshopResponse.meta.totalPages,
                        workshopFilters.page,
                        workshopFilters.page - 1,
                        workshopFilters.page + 1,
                      ].filter((page) => page >= 1 && page <= workshopResponse.meta.totalPages),
                    ),
                  )
                    .sort((left, right) => left - right)
                    .map((page) => (
                      <Button
                        key={page}
                        colorPalette={page === workshopFilters.page ? "blue" : "gray"}
                        onClick={() => updateWorkshopFilters({ page }, 0, false)}
                        size="sm"
                        variant={page === workshopFilters.page ? "solid" : "outline"}
                      >
                        {page}
                      </Button>
                    ))}
                  <Button
                    disabled={workshopFilters.page >= workshopResponse.meta.totalPages}
                    onClick={() => updateWorkshopFilters({ page: workshopFilters.page + 1 }, 0, false)}
                    size="sm"
                    variant="outline"
                  >
                    下一页
                  </Button>
                </HStack>
              )}
            </>
          ) : (
            <EmptyState text="准备加载创意工坊..." />
          )}
        </Box>
      </Box>

      {/* 和光影下载弹窗同样的理由：正文框 .pz-main 带 backdrop-filter + overflow:hidden，
          会把 position:fixed 的弹层裁掉，所以这里必须 Portal 到 body。 */}
      {workshopDetailTarget && (
        <Portal>
          <Box
            className={`pz-resource-file-overlay ${workshopDetailClosing ? "is-closing" : ""}`}
            onClick={closeWorkshopDetail}
          >
            <Box
              aria-label={`${workshopDetailTarget.title || "创意工坊项目"} 详情`}
              aria-modal="true"
              className="pz-resource-file-dialog pz-workshop-detail-dialog"
              onClick={(event) => event.stopPropagation()}
              ref={workshopDetailDialogRef}
              role="dialog"
            >
              <HStack align="start" gap="3" justify="space-between">
                <Box minW="0">
                  <Text color="gray.900" fontWeight="700">
                    {workshopDetail?.title || workshopDetailTarget.title || "未命名模组"}
                  </Text>
                  <HStack color="gray.500" fontSize="xs" gap="3" mt="1" wrap="wrap">
                    <Text>更新时间 {formatWorkshopDate(workshopDetailTarget.timeUpdated)}</Text>
                    <Text>订阅 {workshopDetailTarget.subscriptions.toLocaleString()}</Text>
                    <Text>大小 {formatWorkshopSize(workshopDetailTarget.fileSize)}</Text>
                    <Text>作者 {workshopDetailTarget.creatorName || "未知作者"}</Text>
                  </HStack>
                </Box>
                <HStack align="center" flexShrink="0" gap="2">
                  {/* 翻译语言统一在「设置 › 通用」里改，这里只显示进度。 */}
                  <Text aria-label="详情翻译进度" color="gray.500" fontSize="xs" minW="72px" textAlign="right">
                    {translationLanguage === "en" || workshopDetailProgress.total === 0
                      ? "原文"
                      : `${workshopDetailProgress.completed}/${workshopDetailProgress.total}`}
                  </Text>
                  <Button aria-label="关闭详情" onClick={closeWorkshopDetail} size="sm" variant="ghost">
                    ×
                  </Button>
                </HStack>
              </HStack>
              <Box className="pz-workshop-detail-body">
                {workshopDetailLoading ? (
                  <Text color="gray.500" py="6" textAlign="center">
                    正在读取创意工坊详情...
                  </Text>
                ) : workshopDetail ? (
                  workshopDetail.descriptionHtml ? (
                    <Box
                      className="pz-developer-news-content"
                      dangerouslySetInnerHTML={{ __html: workshopDetail.descriptionHtml }}
                    />
                  ) : (
                    <Text color="gray.500" py="6" textAlign="center">
                      该项目没有填写说明。
                    </Text>
                  )
                ) : null}
              </Box>
              <HStack className="pz-workshop-detail-footer" gap="2" justify="space-between" mt="3" wrap="wrap">
                <Text color="gray.500" fontSize="xs" minW="0" overflowWrap="anywhere">
                  {workshopDetail?.tags.length ? `标签 ${workshopDetail.tags.slice(0, 6).join("、")}` : ""}
                </Text>
                <Button
                  flexShrink="0"
                  onClick={() => void openWorkshopItem(workshopDetailTarget.id, workshopDetailTarget.fileUrl)}
                  size="sm"
                  variant="outline"
                >
                  在 Steam 中打开 ↗
                </Button>
              </HStack>
            </Box>
          </Box>
        </Portal>
      )}
    </Box>
  );

  useEffect(() => {
    let cancelled = false;
    // 分批翻译交给 translateInBatches（与创意工坊详情同一条管线），
    // 这里只负责把译文写回新闻列表和来源占比。
    void translateInBatches(
      developerNewsTranslationJobs,
      translationLanguage,
      (results) => {
        setDeveloperNews((current) => {
          if (!current) return current;
          const bySid = new Map(results.map((result) => [result.sid, result]));
          return { ...current, items: current.items.map((item) => {
            const engineTally = { ...(item.engineTally ?? {}) };
            const titleResult = item.titleSid ? bySid.get(item.titleSid) : undefined;
            if (titleResult) {
              engineTally[titleResult.engine] = (engineTally[titleResult.engine] ?? 0) + 1;
            }
            let contentsHtml = item.contentsHtml;
            for (const result of results) {
              if (!contentsHtml.includes(`data-sid="${result.sid}"`)) continue;
              engineTally[result.engine] = (engineTally[result.engine] ?? 0) + 1;
              contentsHtml = replaceNewsTranslation(contentsHtml, result.sid, result.text);
            }
            return { ...item, title: titleResult ? titleResult.text : item.title, contentsHtml, engineTally };
          }) };
        });
      },
      (completed, total) => setDeveloperNewsTranslationProgress({ completed, total }),
      () => cancelled,
    );
    return () => { cancelled = true; };
  }, [developerNewsTranslationJobs, translationLanguage]);

  // 创意工坊详情：同一批 jobs、同一套 data-sid 占位替换，只是写回详情而不是新闻。
  // 依赖取 jobs 数组本身的引用（替换正文时保持同一引用），因此不会自我触发。
  const workshopDetailJobs = workshopDetail?.translationJobs ?? null;
  const workshopDetailJobsLanguage = workshopDetail?.language ?? "";
  useEffect(() => {
    if (!workshopDetailJobs || workshopDetailJobs.length === 0 || !workshopDetailJobsLanguage) {
      return;
    }
    let cancelled = false;
    void translateInBatches(
      workshopDetailJobs,
      workshopDetailJobsLanguage,
      (results) => {
        setWorkshopDetail((current) => {
          if (!current) return current;
          const bySid = new Map(results.map((result) => [result.sid, result]));
          const titleResult = current.titleSid ? bySid.get(current.titleSid) : undefined;
          let descriptionHtml = current.descriptionHtml;
          for (const result of results) {
            if (!descriptionHtml.includes(`data-sid="${result.sid}"`)) continue;
            descriptionHtml = replaceNewsTranslation(descriptionHtml, result.sid, result.text);
          }
          return { ...current, title: titleResult ? titleResult.text : current.title, descriptionHtml };
        });
      },
      (completed, total) => setWorkshopDetailProgress({ completed, total }),
      () => cancelled,
    );
    return () => { cancelled = true; };
  }, [workshopDetailJobs, workshopDetailJobsLanguage]);

  // 「正在读取创意工坊详情…」换成正文时弹窗会被撑高，把这次高度跳变做成过渡；
  // 翻译分批回填也会改高度，所以 workshopDetail 也在依赖里（差值 <1px 会被忽略）。
  useDialogAutoHeight(workshopDetailDialogRef, workshopDetailDialogHeightRef, [
    workshopDetailTarget,
    workshopDetailLoading,
    workshopDetail,
  ]);
  const developerNewsContent = (
    <Box className="pz-panel pz-developer-news-page" bg="white" borderColor="gray.200" borderRadius="md" borderWidth="1px" overflow="hidden">
      <Box className="pz-panel-header" borderBottomColor="gray.200" borderBottomWidth="1px" p={{ base: "4", md: "5" }}>
        <HStack align="start" justify="space-between" gap="3" wrap="wrap">
          <Box><Text color="gray.900" fontSize="lg" fontWeight="700">开发者新闻</Text><Text color="gray.500" fontSize="xs" mt="1">Project Zomboid · Steam News</Text></Box>
          <HStack gap="2">
            {/* 翻译语言统一在「设置 › 通用」里改，这里只显示进度。 */}
            <HStack align="center" gap="2"><Text aria-label="新闻翻译进度" color="gray.500" fontSize="xs" minW="72px" textAlign="right">{translationLanguage === "en" || developerNewsTranslationProgress.total === 0 ? "原文" : `${developerNewsTranslationProgress.completed}/${developerNewsTranslationProgress.total}`}</Text></HStack>            <Button loading={developerNewsLoading} onClick={() => void loadDeveloperNews()} size="sm" variant="outline">刷新</Button>
          </HStack>
        </HStack>
      </Box>
      <Box className="pz-developer-news-list">
        {developerNewsLoading && !developerNews ? <Text color="gray.500" py="8" textAlign="center">正在读取开发者新闻...</Text> : developerNews?.items.length ? developerNews.items.map((item, index) => {
          const engineShare = formatNewsEngineShare(item.engineTally);
          return (
            <DeveloperNewsCard key={item.gid} revealIndex={index}>
              <Text color="gray.900" fontSize="md" fontWeight="700">{item.title}</Text>
              <Text color="gray.500" fontSize="xs" mt="1">{new Date(item.date * 1000).toLocaleString()} {item.author ? `· ${item.author}` : ""} {item.feedLabel ? `· ${item.feedLabel}` : ""}</Text>
              <Box className="pz-developer-news-content" dangerouslySetInnerHTML={{ __html: item.contentsHtml }} mt="3" />
              <HStack align="center" gap="2" justify="space-between" mt="1" wrap="wrap">
                <Box>{item.url && <Button className="pz-news-source-link" onClick={() => void openUrl(item.url)} size="xs" variant="ghost">查看原文 ↗</Button>}</Box>
                {engineShare && (
                  <Text className="pz-news-engine-share" title="本条新闻的翻译提供商占比">
                    翻译来源 {engineShare}
                  </Text>
                )}
              </HStack>
            </DeveloperNewsCard>
          );
        }) : <EmptyState text="暂无开发者新闻。" />}
      </Box>
    </Box>
  );

  const downloaderSettingsContent = (
    <Box className="pz-panel pz-download-settings-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">下载器</Text>
      {developerMode && (
        <Text color="gray.500" fontSize="sm" mt="1">自定义下载链接将保存到指定目录，支持 HTTP Range 多线程下载。</Text>
      )}
      <Box className="pz-download-settings-form" mt="5">
        <Text color="gray.700" fontSize="sm">下载链接</Text>
        <Input value={shaderDownloadUrl} onChange={(event) => setShaderDownloadUrl(event.currentTarget.value)} placeholder="https://example.com/shader.zip" />
        <Text color="gray.700" fontSize="sm">文件名（可选，留空时从链接提取）</Text>
        <Input value={shaderDownloadFileName} onChange={(event) => setShaderDownloadFileName(event.currentTarget.value)} placeholder="例如 Complementary.zip" />
        <Text color="gray.700" fontSize="sm">光影包保存目录</Text>
        <HStack gap="2">
          <Input flex="1" minW="0" value={shaderDownloadDirectory} onChange={(event) => setShaderDownloadDirectory(event.currentTarget.value)} placeholder="留空使用默认光影包文件夹" />
          <Button aria-label="浏览目录" onClick={() => void chooseShaderDownloadDirectory()} size="sm" variant="outline">浏览</Button>
        </HStack>
        <Text color="gray.700" fontSize="sm">并行线程数（1–16）</Text>
        <Input type="number" min={1} max={16} value={shaderDownloadThreads} onChange={(event) => setShaderDownloadThreads(Math.min(16, Math.max(1, Number(event.currentTarget.value) || 1)))} />
        <HStack gap="2" mt="1">
          <Button colorPalette="blue" disabled={!shaderDownloadUrl.trim()} onClick={startCustomShaderDownload} size="sm">开始下载</Button>
          <Button disabled={shaderDownloadSaving} loading={shaderDownloadSaving} onClick={() => void saveShaderDownloadSettings()} size="sm" variant="outline">保存设置</Button>
        </HStack>
        <Text color="gray.500" fontSize="xs" mt="2">
          下载会加入右上角任务中心，可以继续操作其它页面。
        </Text>
      </Box>
    </Box>
  );

  const generalSettingsContent = (
    <Box className="pz-panel pz-general-settings-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">通用</Text>
      <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">开发者模式</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            开启后显示更多技术细节，包括加载方式、模组标签、冲突分析和详细通知。
          </Text>
        </Box>
        <Switch.Root
          aria-label="开发者模式"
          checked={developerMode}
          disabled={developerModeSaving}
          onCheckedChange={(details) => void updateDeveloperMode(details.checked)}
        >
          <Switch.HiddenInput />
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
        </Switch.Root>
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">翻译语言</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            开发者新闻和创意工坊详情的正文、标题都按这个语言翻译；选 English 时不翻译，直接显示原文。
          </Text>
        </Box>
        {/* 与资源中心筛选行同一个下拉组件，样式完全一致（含 26px 控件高度与 ▾ 箭头）。 */}
        <WorkshopDropdown
          ariaLabel="翻译语言"
          disabled={translationLanguageSaving}
          onChange={(value) => void updateTranslationLanguage(value)}
          options={TRANSLATION_LANGUAGE_OPTIONS}
          value={translationLanguage}
          width="9rem"
        />
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">字体对比度</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            觉得文字和背景颜色太接近、看不清楚时往右拉：文字会变得更深，最左端保持默认配色。
          </Text>
        </Box>
        <Box flex="1" maxW="320px" minW="200px">
          <HStack justify="space-between" mb="1">
            <Text color="gray.500" fontSize="xs">
              {fontContrast === 0 ? "默认配色" : "文字压深"}
            </Text>
            <Text color="blue.700" fontFamily="mono" fontSize="xs" fontWeight="700">
              {fontContrast}%
            </Text>
          </HStack>
          {/* 与启动页内存滑块同一个样式类，观感一致（类名叫 memory 是历史原因）。 */}
          <Slider.Root
            aria-label={["字体对比度"]}
            className="pz-memory-slider"
            max={100}
            min={0}
            step={5}
            value={[fontContrast]}
            onValueChange={({ value }) => {
              const nextValue = value[0];
              if (nextValue !== undefined) {
                updateFontContrast(nextValue);
              }
            }}
          >
            <Slider.Control>
              <Slider.Track>
                <Slider.Range />
              </Slider.Track>
              <Slider.Thumb index={0}>
                <Slider.HiddenInput />
              </Slider.Thumb>
            </Slider.Control>
          </Slider.Root>
        </Box>
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">模糊度</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            毛玻璃效果的强度：往左更通透（0 等于关闭模糊），往右更厚重。
            面板、菜单、弹层和提示气泡会一起变化。
          </Text>
        </Box>
        <Box flex="1" maxW="320px" minW="200px">
          <HStack justify="space-between" mb="1">
            <Text color="gray.500" fontSize="xs">
              {blurStrength === 0
                ? "关闭模糊"
                : blurStrength === DEFAULT_BLUR_STRENGTH
                  ? "默认"
                  : blurStrength < DEFAULT_BLUR_STRENGTH
                    ? "更通透"
                    : "更厚重"}
            </Text>
            <Text color="blue.700" fontFamily="mono" fontSize="xs" fontWeight="700">
              {blurStrength}%
            </Text>
          </HStack>
          {/* 与上面两项同一个滑块样式（类名叫 memory 是历史原因）。 */}
          <Slider.Root
            aria-label={["模糊度"]}
            className="pz-memory-slider"
            max={MAX_BLUR_STRENGTH}
            min={MIN_BLUR_STRENGTH}
            step={BLUR_STRENGTH_STEP}
            value={[blurStrength]}
            onValueChange={({ value }) => {
              const nextValue = value[0];
              if (nextValue !== undefined) {
                updateBlurStrength(nextValue);
              }
            }}
          >
            <Slider.Control>
              <Slider.Track>
                <Slider.Range />
              </Slider.Track>
              <Slider.Thumb index={0}>
                <Slider.HiddenInput />
              </Slider.Thumb>
            </Slider.Control>
          </Slider.Root>
        </Box>
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">界面字号</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            整体放大或缩小界面文字：往左更紧凑，往右更大更易读。窗口和控件大小保持不变。
          </Text>
        </Box>
        <Box flex="1" maxW="320px" minW="200px">
          <HStack justify="space-between" mb="1">
            <Text color="gray.500" fontSize="xs">
              {fontScale === DEFAULT_FONT_SCALE
                ? "默认"
                : fontScale < DEFAULT_FONT_SCALE
                  ? "更紧凑"
                  : "更大"}
            </Text>
            <Text color="blue.700" fontFamily="mono" fontSize="xs" fontWeight="700">
              {fontScale}%
            </Text>
          </HStack>
          {/* 与上面两项同一个滑块样式（类名叫 memory 是历史原因）。 */}
          <Slider.Root
            aria-label={["界面字号"]}
            className="pz-memory-slider"
            max={MAX_FONT_SCALE}
            min={MIN_FONT_SCALE}
            step={FONT_SCALE_STEP}
            value={[fontScale]}
            onValueChange={({ value }) => {
              const nextValue = value[0];
              if (nextValue !== undefined) {
                updateFontScale(nextValue);
              }
            }}
          >
            <Slider.Control>
              <Slider.Track>
                <Slider.Range />
              </Slider.Track>
              <Slider.Thumb index={0}>
                <Slider.HiddenInput />
              </Slider.Thumb>
            </Slider.Control>
          </Slider.Root>
        </Box>
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">界面字体</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            选择系统常见字体；选「自定义…」后可填入本机安装的任意字体名，也可以直接粘贴 CSS font-family 列表。
          </Text>
        </Box>
        <HStack gap="2" align="center">
          <WorkshopDropdown
            ariaLabel="界面字体"
            onChange={(value) => {
              setFontFamilyChoice(value);
              if (value === "custom") {
                // 保留当前值（可能是预设的 CSS 列表），由旁边的输入框直接编辑。
                return;
              }
              updateFontFamily(FONT_FAMILY_PRESET_CSS[value] ?? "");
            }}
            options={FONT_FAMILY_PRESETS}
            value={fontFamilyChoice}
            width="11rem"
          />
          {fontFamilyChoice === "custom" && (
            <Input
              aria-label="自定义字体"
              placeholder="字体名，如 思源黑体 / Sarasa Gothic SC"
              value={fontFamily}
              width="13rem"
              onChange={(event) => updateFontFamily(event.target.value)}
            />
          )}
        </HStack>
      </HStack>
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">窗口大小</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            直接拖动窗口边缘即可调整大小（最小 {DEFAULT_WINDOW_WIDTH} × {DEFAULT_WINDOW_HEIGHT}），
            启动器会自动记住，下次启动按这个尺寸打开。
          </Text>
        </Box>
        <HStack gap="2" align="center">
          <Text color="gray.500" fontSize="xs">
            {savedWindowSize
              ? `已记住 ${savedWindowSize[0]} × ${savedWindowSize[1]}`
              : `默认 ${DEFAULT_WINDOW_WIDTH} × ${DEFAULT_WINDOW_HEIGHT}`}
          </Text>
          <Button
            aria-label="恢复默认窗口大小"
            size="sm"
            variant="outline"
            onClick={() => {
              void invoke("restore_default_window_size")
                .then(() => setSavedWindowSize(null))
                .catch((error: unknown) => {
                  showToast(error instanceof Error ? error.message : String(error), "error");
                });
            }}
          >
            恢复默认
          </Button>
        </HStack>
      </HStack>
      <Box mt="5">
        <Text color="gray.800" fontSize="sm" fontWeight="700">Steam 网络加速</Text>
        <Text color="gray.500" fontSize="xs" mt="1">
          大陆网络直连 Steam 创意工坊会失败：默认经中转服务器访问工坊列表、项目详情和开发者新闻，预览图加载失败时也走中转兜底。
          「自动」= 先试直连，失败自动切中转；「中转优先」= 一直走中转；「仅直连」= 不用中转（直连被墙时会打不开）。
          UU 这类只加速 Steam 客户端的工具不会接管启动器，开着也仍需中转；如果你装了 Clash / V2Ray
          等带本地端口的代理，填在下面通常比中转更快。
        </Text>
        <HStack align="center" gap="2" mt="3" wrap="wrap">
          <WorkshopDropdown
            ariaLabel="Steam 网络加速模式"
            disabled={steamRelaySaving}
            onChange={(value) => {
              setSteamRelayMode(value);
              void saveSteamRelaySettings(value, steamRelayBase);
            }}
            options={STEAM_RELAY_MODE_OPTIONS}
            value={steamRelayMode}
            width="11rem"
          />
          <Input
            aria-label="Steam 中转服务器地址"
            placeholder={`默认：${DEFAULT_STEAM_RELAY_BASE}`}
            value={steamRelayBase}
            width="18rem"
            onBlur={() => void saveSteamRelaySettings(steamRelayMode, steamRelayBase)}
            onChange={(event) => setSteamRelayBase(event.currentTarget.value)}
          />
          <Button
            className="pz-steam-relay-action"
            colorPalette="blue"
            disabled={steamRelayTesting}
            size="sm"
            variant="outline"
            onClick={() => void testSteamRelay()}
          >
            {steamRelayTesting ? "测试中…" : "测试连接"}
          </Button>
        </HStack>
        {steamRelayPing && (
          <Text
            color={steamRelayPing.startsWith("连接失败") ? "red.500" : "green.700"}
            fontSize="xs"
            mt="2"
          >
            {steamRelayPing}
          </Text>
        )}
        <HStack align="center" gap="2" mt="3" wrap="wrap">
          <Input
            aria-label="本地代理"
            disabled={steamProxySaving}
            placeholder="本地代理（可选）：http://127.0.0.1:7897"
            value={steamProxy}
            width="18rem"
            onBlur={() => void saveSteamProxy(steamProxy)}
            onChange={(event) => setSteamProxy(event.currentTarget.value)}
          />
          <Button
            className="pz-steam-relay-action"
            disabled={steamProxyDetecting}
            size="sm"
            variant="outline"
            onClick={() => void detectSteamProxy()}
          >
            {steamProxyDetecting ? "检测中…" : "自动检测"}
          </Button>
          {steamProxy && (
            <Button
              className="pz-steam-relay-action"
              disabled={steamProxySaving}
              size="sm"
              variant="ghost"
              onClick={() => {
                setSteamProxy("");
                void saveSteamProxy("");
              }}
            >
              清除
            </Button>
          )}
        </HStack>
        {steamProxyHint && (
          <Text
            color={steamProxyHint.startsWith("已选用") ? "green.700" : "gray.600"}
            fontSize="xs"
            mt="2"
          >
            {steamProxyHint}
          </Text>
        )}
      </Box>
    </Box>
  );

  /** 设置 › 翻译器：自定义文本翻译，复用资源中心同一套翻译管线。 */
  const translatorSettingsContent = (
    <Box className="pz-panel pz-translator-settings-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">翻译器</Text>
      <Text color="gray.500" fontSize="sm" mt="1">
        翻译自定义文本，与开发者新闻、创意工坊详情使用同一条翻译线路（Google 优先，失败自动换用 MyMemory）。
      </Text>

      <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">目标语言</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            默认取「通用 › 翻译语言」，在这里改只影响本页，不会改全局设置。
          </Text>
        </Box>
        <WorkshopDropdown
          ariaLabel="翻译器目标语言"
          disabled={translatorBusy}
          onChange={(value) => {
            setTranslatorLanguage(value);
            setTranslatorError(null);
          }}
          options={TRANSLATION_LANGUAGE_OPTIONS}
          value={translatorLanguage}
          width="9rem"
        />
      </HStack>

      <Box mt="4">
        <HStack align="center" gap="2" justify="space-between" wrap="wrap">
          <Text color="gray.800" fontSize="sm" fontWeight="700">原文</Text>
          <Text
            color={translatorSource.length > CUSTOM_TRANSLATION_MAX_CHARS ? "red.600" : "gray.500"}
            fontSize="xs"
          >
            {translatorSource.length} / {CUSTOM_TRANSLATION_MAX_CHARS}
          </Text>
        </HStack>
        <Textarea
          aria-label="翻译器原文"
          className="pz-translator-textarea"
          disabled={translatorBusy}
          mt="1.5"
          onChange={(event) => {
            setTranslatorSource(event.currentTarget.value);
            setTranslatorError(null);
          }}
          onKeyDown={(event) => {
            if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
              event.preventDefault();
              void translateCustomText();
            }
          }}
          placeholder="输入要翻译的文本；按行翻译，空行会保留。Ctrl + Enter 开始翻译。"
          rows={8}
          value={translatorSource}
        />
      </Box>

      <HStack gap="2" justify="flex-end" mt="2">
        <Button
          disabled={translatorSource.length === 0 && translatorResult.length === 0}
          onClick={clearTranslator}
          size="sm"
          variant="ghost"
        >
          清空
        </Button>
        <Button
          colorPalette="blue"
          disabled={
            translatorBusy ||
            !translatorSource.trim() ||
            translatorSource.length > CUSTOM_TRANSLATION_MAX_CHARS
          }
          loading={translatorBusy}
          onClick={() => void translateCustomText()}
          size="sm"
        >
          翻译
        </Button>
      </HStack>

      {translatorBusy && (
        <Text color="gray.500" fontSize="xs" mt="2">
          正在翻译 {translatorProgress.completed} / {translatorProgress.total} 段…
        </Text>
      )}
      {translatorError && (
        <Text color="red.600" fontSize="xs" mt="2">
          {translatorError}
        </Text>
      )}

      {translatorResult && (
        <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
          <HStack align="center" gap="2" justify="space-between" wrap="wrap">
            <HStack gap="2">
              <Text color="gray.800" fontSize="sm" fontWeight="700">译文</Text>
              {translatorEngine && (
                <Badge colorPalette="blue" size="sm">
                  翻译来源{" "}
                  {translatorEngine
                    .split(" / ")
                    .map((engine) => newsEngineLabels[engine] ?? engine)
                    .join(" / ")}
                </Badge>
              )}
            </HStack>
            <Button onClick={() => void copyTranslatorResult()} size="xs" variant="outline">
              复制译文
            </Button>
          </HStack>
          <Textarea
            aria-label="翻译器译文"
            className="pz-translator-textarea"
            mt="1.5"
            readOnly
            rows={8}
            value={translatorResult}
          />
        </Box>
      )}
    </Box>
  );

  // 设置页：结构与资源中心一致（左侧分类 + 右侧内容）。
  /** 设置 › 模组加载器：方案选择 + 当前方案的加载器状态与安装。
      加载器相关的东西只在这里，其它页面只做状态提示和指路，避免各页各写一套。 */
  const modLoaderSettingsContent = (
    <Box className="pz-panel pz-mod-loader-settings-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">模组加载器</Text>
      <Text color="gray.500" fontSize="sm" mt="1">
        模组加载器负责让 JAR 模组在游戏里运行；下面的选择决定启动游戏时是否启用它。
      </Text>
      <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">模组加载器方案</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            更改会在下一次点「启动游戏」时生效；选「不使用」则保持游戏原有启动方式，不做任何修改。
          </Text>
        </Box>
        <WorkshopDropdown
          ariaLabel="模组加载器方案"
          disabled={modLoaderSchemeSaving}
          onChange={(value) => void updateModLoaderScheme(value)}
          options={MOD_LOADER_SCHEME_OPTIONS}
          value={modLoaderScheme}
          width="11rem"
        />
      </HStack>

      {detection.status === "found" && (
        <Stack gap="2" mt="5">
          <HStack gap="2" wrap="wrap">
            <Text color="gray.800" fontSize="sm" fontWeight="700">ZombieBuddy 加载器</Text>
            <Badge
              colorPalette={
                detection.paths.zombieBuddy.installed
                  ? "green"
                  : detection.paths.zombieBuddy.available
                    ? "orange"
                    : "gray"
              }
              size="sm"
            >
              {detection.paths.zombieBuddy.installed
                ? "已安装"
                : detection.paths.zombieBuddy.available
                  ? "待安装"
                  : "模组未下载"}
            </Badge>
            {modLoaderScheme !== "zombieBuddy" && (
              <Badge colorPalette="gray" size="sm">未启用</Badge>
            )}
          </HStack>
          <Text color="gray.600" fontSize="sm">
            {detection.paths.zombieBuddy.installed
              ? "已安装。JAR 模组管理页可以直接读取已加载的模组。"
              : `缺少：${
                  detection.paths.zombieBuddy.missingComponents.join("、") || "未检测到加载器文件"
                }`}
          </Text>
          {!detection.paths.zombieBuddy.available && !detection.paths.zombieBuddy.installed && (
            <Text color="gray.600" fontSize="sm">
              {detection.paths.zombieBuddy.sourcePath
                ? "创意工坊文件不完整，请在 Steam 中重新下载 ZombieBuddy。"
                : "请先在 Steam 创意工坊订阅并下载 ZombieBuddy。"}
            </Text>
          )}
          {!detection.paths.zombieBuddy.installed &&
            detection.paths.zombieBuddy.available &&
            detection.paths.installPath && (
            <HStack wrap="wrap">
              <Button colorPalette="blue" loading={installing} onClick={installZombieBuddy} size="sm">
                安装 ZombieBuddy
              </Button>
            </HStack>
          )}
          <Text color="orange.700" fontSize="xs">
            安装会自动备份并修改游戏的启动配置。JAR 模组可以运行本机代码，请确认你信任 ZombieBuddy 的来源。
          </Text>
          {developerMode && (
            <Text color="gray.500" fontSize="xs">
              配置：Both（JSON + Steam 启动参数 + BAT）· Workshop ID：{ZOMBIE_BUDDY_WORKSHOP_ID}
            </Text>
          )}
        </Stack>
      )}
    </Box>
  );

  /** 设置 › 启动：启动阶段的实验性优化开关。
      「极速启动」只在模组加载器方案 = ZombieBuddy 时可用（其它方案没有可短路的加载器）。 */
  const startupSettingsContent = (
    <Box className="pz-panel pz-startup-settings-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">启动</Text>
      <Text color="gray.500" fontSize="sm" mt="1">
        启动阶段的实验性优化，默认关闭；出问题关掉就能回到原版流程。
      </Text>
      <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
        <Box flex="1" minW="240px">
          <HStack gap="2" wrap="wrap">
            <Text color="gray.800" fontSize="sm" fontWeight="700">极速启动</Text>
            <Badge colorPalette="orange" size="sm">实验性</Badge>
          </HStack>
          <Text color="gray.500" fontSize="xs" mt="1">
            让游戏跳过启动时的模组全量扫描，直接使用启动器预先整理好的模组清单，明显加快启动速度。
            任何一步出问题都会自动退回游戏原本的加载方式，不影响正常游玩。
          </Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            需要通过本启动器的「启动游戏」按钮启动游戏。开启时会先备份游戏的启动配置
            （原文件保存在 .pzmanager\fast-launch-backup），关闭时自动恢复。
          </Text>
          {modLoaderScheme === "zombieBuddy" ? (
            <Text color="gray.500" fontSize="xs" mt="1">
              清单只包含「已启用」的 JAR 模组，会一直保留：改动模组后想立即生效，
              到「模组管理 › JAR」点一次「刷新 JAR 列表」即可。
            </Text>
          ) : (
            <Text color="orange.700" fontSize="xs" mt="1">
              仅支持 ZombieBuddy 模组加载器：当前选择的是「不使用（原版加载）」，请先到
              设置 › 模组加载器 更改。
            </Text>
          )}
          <Text color="gray.500" fontSize="xs" mt="1">
            此功能会修改游戏的启动方式，建议只在单机或自建服务器环境使用。
          </Text>
          <FastLaunchCacheStatusLine enabled={fastLaunch && modLoaderScheme === "zombieBuddy"} />
        </Box>
        <Switch.Root
          aria-label="极速启动"
          checked={fastLaunch}
          disabled={fastLaunchSaving || modLoaderScheme !== "zombieBuddy"}
          onCheckedChange={(details) => void updateFastLaunch(details.checked)}
        >
          <Switch.HiddenInput />
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
        </Switch.Root>
      </HStack>

      <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
        <HStack align="center" gap="4" justify="space-between" wrap="wrap">
          <Box flex="1" minW="240px">
            <HStack gap="2" wrap="wrap">
              <Text color="gray.800" fontSize="sm" fontWeight="700">
                静默内存清理
              </Text>
              {memoryStatus !== null && (
                <Badge
                  colorPalette={
                    memoryStatus.usedPercent >= silentMemoryCleanupThreshold ? "orange" : "gray"
                  }
                  size="sm"
                >
                  当前占用 {memoryStatus.usedPercent}%
                </Badge>
              )}
            </HStack>
            <Text color="gray.500" fontSize="xs" mt="1">
              后台每 10 秒检查一次系统内存占用，达到下面的阈值就自动清理一次（类似 PCL2 的「内存优化」）。
              两次清理至少间隔 60 秒；正在运行的游戏不受影响。
            </Text>
            <Box mt="3" maxW="420px">
              <HStack justify="space-between" mb="1">
                <Text color="gray.600" fontSize="xs">
                  触发阈值
                </Text>
                <Text color="blue.700" fontFamily="mono" fontSize="sm" fontWeight="700">
                  {silentMemoryCleanupThreshold}%
                </Text>
              </HStack>
              <Slider.Root
                aria-label={["静默内存清理触发阈值"]}
                className="pz-memory-slider"
                disabled={!silentMemoryCleanup || silentMemoryCleanupSaving}
                max={MAX_SILENT_MEMORY_PERCENT}
                min={MIN_SILENT_MEMORY_PERCENT}
                step={SILENT_MEMORY_PERCENT_STEP}
                value={[silentMemoryCleanupThreshold]}
                onValueChange={({ value }) => {
                  const nextValue = value[0];
                  if (nextValue !== undefined) {
                    updateSilentMemoryCleanupThreshold(nextValue);
                  }
                }}
              >
                <Slider.Control>
                  <Slider.Track>
                    <Slider.Range />
                  </Slider.Track>
                  <Slider.Thumb index={0}>
                    <Slider.HiddenInput />
                  </Slider.Thumb>
                </Slider.Control>
              </Slider.Root>
              <HStack color="gray.500" fontSize="xs" justify="space-between" mt="1">
                <Text>{MIN_SILENT_MEMORY_PERCENT}%（更频繁）</Text>
                <Text>{MAX_SILENT_MEMORY_PERCENT}%（更保守）</Text>
              </HStack>
            </Box>
            <HStack align="center" gap="3" mt="3" wrap="wrap">
              <Button
                loading={memoryCleaning}
                onClick={() => void cleanMemoryNow()}
                size="xs"
                variant="outline"
              >
                立即清理一次
              </Button>
              {lastCleanupReport !== null && (
                <Text color="gray.500" fontSize="xs">
                  上次清理释放 {lastCleanupReport.freedMb} MB，用时 {lastCleanupReport.elapsedMs} ms
                  {lastCleanupReport.trimmed > 0 ? `（${lastCleanupReport.trimmed} 个进程）` : ""}
                </Text>
              )}
            </HStack>
          </Box>
          <Switch.Root
            aria-label="静默内存清理"
            checked={silentMemoryCleanup}
            disabled={silentMemoryCleanupSaving}
            onCheckedChange={(details) => void updateSilentMemoryCleanup(details.checked)}
          >
            <Switch.HiddenInput />
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
          </Switch.Root>
        </HStack>

        <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
          <Box flex="1" minW="240px">
            <Text color="gray.800" fontSize="sm" fontWeight="700">
              启动时清理内存
            </Text>
            <Text color="gray.500" fontSize="xs" mt="1">
              点「启动游戏」时先清理一次，再把游戏拉起来。此时游戏还没开始占内存，清理效果最好；
              清理失败或读不到内存状态都不会影响启动。
            </Text>
          </Box>
          <Switch.Root
            aria-label="启动时清理内存"
            checked={cleanMemoryOnLaunch}
            disabled={cleanMemoryOnLaunchSaving}
            onCheckedChange={(details) => void updateCleanMemoryOnLaunch(details.checked)}
          >
            <Switch.HiddenInput />
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
          </Switch.Root>
        </HStack>
      </Box>
    </Box>
  );

  /** 反馈文案里的时间：精确到分钟（秒级变化会让整句文案不停跳动）。 */
  const feedbackCutoffLabel = new Date().toLocaleString("zh-CN", {
    hour12: false,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });

  const aboutSettingsContent = (
    <Box className="pz-panel pz-about-panel">
      <Text color="gray.900" fontSize="lg" fontWeight="700">关于</Text>
      <Text color="gray.500" fontSize="sm" mt="1">
        PZMANAGER 的版本与作者信息。
      </Text>
      <Box mt="4">
        <HStack gap="2" wrap="wrap">
          <Text color="gray.800" fontSize="md" fontWeight="800">PZMANAGER</Text>
          <Badge colorPalette="blue" size="sm">v{appManifest.version}</Badge>
        </HStack>
        <Text color="gray.500" fontSize="sm" mt="2">
          Project Zomboid（僵尸毁灭工程）的 Windows 桌面启动器 + 模组管理器：
          整合创意工坊订阅、JAR 模组管理、内置翻译器与下载器，不修改游戏本体。
        </Text>
      </Box>
      <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
        <HStack justify="space-between" align="center" gap="3" wrap="wrap">
          <Box flex="1" minW="240px">
            <Text color="gray.800" fontSize="sm" fontWeight="700">反馈问题</Text>
            <Text color="gray.500" fontSize="xs" mt="0.5">
              点击该按钮仅会上传 {feedbackCutoffLabel} 前最后一次启动的日志，请确保您的问题是该次启动时发生的。
            </Text>
          </Box>
          <Button
            colorPalette="blue"
            onClick={openFeedbackDialog}
            size="sm"
            variant="outline"
          >
            反馈信息
          </Button>
        </HStack>
      </Box>
      <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
        <HStack justify="space-between" align="center" gap="3" wrap="wrap">
          <Box>
            <Text color="gray.800" fontSize="sm" fontWeight="700">检查更新</Text>
            <Text color="gray.500" fontSize="xs" mt="0.5">启动时会自动检查一次，这里可以手动检查。</Text>
          </Box>
          <Button
            colorPalette="blue"
            loading={updateChecking}
            onClick={() => void runUpdateCheck(true)}
            size="sm"
            variant="outline"
          >
            检查更新
          </Button>
        </HStack>
        {updateCheckError ? (
          <Text color="red.500" fontSize="xs" mt="2" whiteSpace="pre-wrap">检查失败：{updateCheckError}</Text>
        ) : launcherUpdate ? (
          <Box mt="2">
            {launcherUpdate.updateAvailable ? (
              <>
                <HStack gap="2" wrap="wrap" align="center">
                  <Text color="gray.500" fontSize="sm">最新版本</Text>
                  <Badge colorPalette="green" size="sm">v{launcherUpdate.latestVersion}</Badge>
                  {launcherUpdate.pubDate ? (
                    <Text color="gray.400" fontSize="xs">发布于 {launcherUpdate.pubDate}</Text>
                  ) : null}
                </HStack>
                {launcherUpdate.notes ? (
                  <Box mt="2">
                    <Text color="gray.800" fontSize="sm" fontWeight="700">更新日志</Text>
                    <Text color="gray.500" fontSize="xs" mt="1" whiteSpace="pre-wrap">{launcherUpdate.notes}</Text>
                  </Box>
                ) : null}
                <HStack gap="2" mt="3" wrap="wrap">
                  {launcherUpdate.downloadUrl ? (
                    <Button
                      colorPalette="blue"
                      onClick={() => void openUrl(launcherUpdate.downloadUrl ?? "")}
                      size="sm"
                    >
                      下载更新
                    </Button>
                  ) : null}
                  {launcherUpdate.releasePageUrl ? (
                    <Button
                      onClick={() => void openUrl(launcherUpdate.releasePageUrl ?? "")}
                      size="sm"
                      variant="ghost"
                    >
                      打开发布页 ↗
                    </Button>
                  ) : null}
                </HStack>
              </>
            ) : (
              <>
                <Text color="gray.500" fontSize="sm">已是最新版本（v{launcherUpdate.currentVersion}）。</Text>
                {/* 最新版同样展示更新日志：就是当前版本自身的发布说明。 */}
                {launcherUpdate.notes ? (
                  <Box mt="2">
                    <Text color="gray.800" fontSize="sm" fontWeight="700">更新日志</Text>
                    {launcherUpdate.pubDate ? (
                      <Text color="gray.400" fontSize="xs" mt="0.5">发布于 {launcherUpdate.pubDate}</Text>
                    ) : null}
                    <Text color="gray.500" fontSize="xs" mt="1" whiteSpace="pre-wrap">{launcherUpdate.notes}</Text>
                  </Box>
                ) : null}
              </>
            )}
          </Box>
        ) : (
          <Text color="gray.400" fontSize="xs" mt="2">还没有检查结果。</Text>
        )}
      </Box>
      <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
        <Text color="gray.800" fontSize="sm" fontWeight="700">作者</Text>
        <HStack gap="3" mt="2" align="center">
          <Avatar.Root colorPalette="blue" size="md">
            <Avatar.Image src="/avatars/developer.png" alt="叁葉" />
            <Avatar.Fallback name="叁葉" />
          </Avatar.Root>
          <Box>
            <HStack gap="2" align="center">
              <Text color="gray.600" fontSize="sm">叁葉</Text>
              <Text color="gray.500" fontSize="xs">邮箱 / QQ：757739990@qq.com</Text>
            </HStack>
            <Text color="gray.500" fontSize="xs" mt="0.5">Project Zomboid Manager的开发者!</Text>
          </Box>
        </HStack>
      </Box>
      <Box mt="4">
        <Text color="gray.800" fontSize="sm" fontWeight="700">官方网站</Text>
        <HStack gap="2" mt="1" align="center">
          <Text color="gray.500" fontSize="sm" overflowWrap="anywhere">https://pzm.sanyec.top</Text>
          <Button
            onClick={() => void openUrl("https://pzm.sanyec.top")}
            size="sm"
            variant="ghost"
          >
            打开 ↗
          </Button>
        </HStack>
      </Box>
      <Box mt="4">
        <Text color="gray.800" fontSize="sm" fontWeight="700">项目仓库</Text>
        <HStack gap="2" mt="1" align="center">
          <Text color="gray.500" fontSize="sm" overflowWrap="anywhere">
            https://github.com/sanyectop/pzmanager-main
          </Text>
          <Button
            onClick={() => void openUrl("https://github.com/sanyectop/pzmanager-main")}
            size="sm"
            variant="ghost"
          >
            打开 ↗
          </Button>
        </HStack>
      </Box>
    </Box>
  );

  const settingsContent = (
    <Box className="pz-resource-center pz-settings-center">
      <Box as="nav" aria-label="设置分类" className="pz-resource-center-sidebar">
        <Text className="pz-resource-center-sidebar-title">设置分类</Text>
        <Stack gap="1">
          <Button
            aria-current={settingsPage === "about" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "about" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("about")}
            size="sm"
            variant="ghost"
          >
            关于
          </Button>
          <Button
            aria-current={settingsPage === "general" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "general" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("general")}
            size="sm"
            variant="ghost"
          >
            通用
          </Button>
          <Button
            aria-current={settingsPage === "translator" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "translator" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("translator")}
            size="sm"
            variant="ghost"
          >
            翻译器
          </Button>
          <Button
            aria-current={settingsPage === "console" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "console" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("console")}
            size="sm"
            variant="ghost"
          >
            控制台
          </Button>
          <Button
            aria-current={settingsPage === "downloader" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "downloader" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("downloader")}
            size="sm"
            variant="ghost"
          >
            下载器
          </Button>
          <Button
            aria-current={settingsPage === "modLoader" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "modLoader" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("modLoader")}
            size="sm"
            variant="ghost"
          >
            模组加载器
          </Button>
          <Button
            aria-current={settingsPage === "startup" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "startup" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setSettingsPage("startup")}
            size="sm"
            variant="ghost"
          >
            启动
          </Button>
        </Stack>
      </Box>
      <Box className="pz-resource-center-main">
        {settingsPage === "about"
          ? aboutSettingsContent
          : settingsPage === "general"
            ? generalSettingsContent
          : settingsPage === "translator"
            ? translatorSettingsContent
            : settingsPage === "console"
              ? consoleContent
              : settingsPage === "downloader"
                ? downloaderSettingsContent
                : settingsPage === "modLoader"
                  ? modLoaderSettingsContent
                  : startupSettingsContent}
      </Box>
    </Box>
  );

  const resourceCenterContent = (
    <Box className="pz-resource-center">
      <Box as="nav" aria-label="资源中心分类" className="pz-resource-center-sidebar">
        <Text className="pz-resource-center-sidebar-title">资源类型</Text>
        <Stack gap="1">
          <Button
            aria-current={resourceCenterPage === "workshop" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              resourceCenterPage === "workshop" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setResourceCenterPage("workshop")}
            size="sm"
            variant="ghost"
          >
            创意工坊
          </Button>
          <Button
            aria-current={resourceCenterPage === "shaderDownload" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              resourceCenterPage === "shaderDownload" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => setResourceCenterPage("shaderDownload")}
            size="sm"
            variant="ghost"
          >
            光影包下载
          </Button>
          <Button
            aria-current={resourceCenterPage === "developerNews" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${resourceCenterPage === "developerNews" ? "pz-resource-center-sidebar-item-active" : ""}`}
            onClick={() => { setResourceCenterPage("developerNews"); if (!developerNews) void loadDeveloperNews(); }}
            size="sm"
            variant="ghost"
          >
            开发者新闻
          </Button>
        </Stack>
      </Box>
      <Box className="pz-resource-center-main">
                {resourceCenterPage === "workshop" ? (
          workshopContent
        ) : resourceCenterPage === "developerNews" ? (
          developerNewsContent
        ) : (
          <ResourceDownloadBrowser
            onToast={showToast}
            onDownload={startShaderDownload}
            options={resourceOptions}
            versions={minecraftVersions}
          />
        )}
      </Box>
    </Box>
  );

  const selectedLuaMods = (luaMods ?? []).filter(
    (mod) => !mod.isLoader && selectedLuaModKeys.has(luaModKey(mod)),
  );
  const selectedJarMods = (jarMods ?? []).filter((mod) => selectedJarModKeys.has(jarModKey(mod)));

  const luaModsContent = (
    <Box className="pz-panel pz-lua-mods-page">
      <HStack className="pz-mod-page-header" justify="space-between" gap="3" wrap="wrap">
        <Box>
          <Text color="gray.900" fontSize="lg" fontWeight="700">Lua 模组管理</Text>
          <Text color="gray.500" fontSize="xs" mt="0.5">
            显示 {filteredLuaMods?.length ?? 0} / {totalLuaModCount} 个模组
          </Text>
        </Box>
        <HStack className="pz-mod-header-tools" gap="2" wrap="wrap">
          <Input
            aria-label="搜索 Lua 模组"
            onChange={(event) => setLuaModSearch(event.currentTarget.value)}
            placeholder="搜索名称、ID、作者或简介"
            size="sm"
            value={luaModSearch}
          />
          <Button loading={luaModsLoading} onClick={() => void loadLuaMods()} size="sm" variant="outline">
            刷新
          </Button>
        </HStack>
      </HStack>
      <ModBatchBar
        busyAction={luaModBatchAction}
        negativeLabel="停用"
        onClear={() => startTransition(() => setSelectedLuaModKeys(new Set()))}
        onInvert={invertVisibleLuaMods}
        onNegative={() => void setLuaModsEnabled(selectedLuaMods, false)}
        onPositive={() => void setLuaModsEnabled(selectedLuaMods, true)}
        onSelectAll={selectAllVisibleLuaMods}
        positiveLabel="启用"
        selectedCount={selectedLuaModCount}
        totalCount={filteredLuaMods?.length ?? 0}
      />
      <Box className="pz-lua-mod-list">
        {luaModsLoading && !luaMods ? (
          <Text color="gray.500" py="6" textAlign="center">正在读取 Lua 模组列表...</Text>
        ) : filteredLuaMods && filteredLuaMods.length > 0 ? (
          filteredLuaMods.map((mod) => {
            const key = luaModKey(mod);
            return (
              <SleepingModRow
                key={key}
                render={() => {
                  const selected = selectedLuaModKeys.has(key);
                  const expanded = expandedLuaModKeys.has(key);
                  const busy = luaModActionId === key || luaModBatchAction !== null;
                  const enabledOrder = (luaMods ?? [])
                    .filter((entry) => entry.enabled && entry.loadOrder !== null)
                    .sort((left, right) => (left.loadOrder ?? 0) - (right.loadOrder ?? 0));
                  const orderIndex = enabledOrder.findIndex(
                    (entry) => entry.id.toLowerCase() === mod.id.toLowerCase(),
                  );
                  return (
              <Box
                aria-checked={selected}
                aria-label={`选择 ${mod.name}`}
                className={`pz-mod-row pz-lua-mod-row${selected ? " is-selected" : ""}${mod.enabled ? " is-enabled" : ""}${expanded ? " pz-jar-mod-row-expanded" : ""}`}
                key={key}
                onClick={(event) => {
                  const target = event.target as HTMLElement;
                  if (target.closest("button, .pz-jar-mod-details-shell")) {
                    return;
                  }
                  toggleLuaModSelection(mod, event.currentTarget);
                }}
                onKeyDown={(event) => {
                  if ((event.target as HTMLElement).closest("button")) {
                    return;
                  }
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    toggleLuaModSelection(mod, event.currentTarget);
                  }
                }}
                onMouseDown={(event) => {
                  if (event.button === 0 && !isModRowInteractiveTarget(event.target)) event.preventDefault();
                }}
                onPointerDown={(event) => {
                  if (event.button !== 0 || isModRowInteractiveTarget(event.target)) return;
                  beginRowDrag("lua", key, selected);
                }}
                  onPointerEnter={() => extendRowDrag("lua", key)}
                role="checkbox"
                tabIndex={0}
              >
                <Box className="pz-mod-summary pz-compact-mod-summary">
                  <Box className="pz-mod-select-area">
                    <LuaModPoster name={mod.name} path={mod.posterPath} />
                    <Box className="pz-jar-mod-summary-content" minW="0">
                      <HStack align="center" className="pz-lua-mod-title-row" gap="2" minW="0">
                        <Text className="pz-lua-mod-name" color="gray.800" fontWeight="700">{mod.name}</Text>
                      </HStack>
                      {(mod.description || mod.author || mod.version) && (
                        <Text className="pz-lua-mod-description" color="gray.500" lineClamp="1">
                          {[
                            mod.description,
                            mod.author ? `作者 ${mod.author}` : null,
                            mod.version ? `v${mod.version}` : null,
                          ].filter(Boolean).join(" · ")}
                        </Text>
                      )}
                      <Text className="pz-lua-mod-location" color="gray.400" fontSize="xs">ID · {mod.id}</Text>
                    </Box>
                  </Box>
                  <HStack className="pz-jar-mod-badges pz-lua-mod-badges" gap="1" wrap="nowrap">
                    {mod.hasB42 && <Badge colorPalette="green">B42</Badge>}
                    {mod.requires.length > 0 && (
                      <Badge colorPalette="orange">前置 {mod.requires.length}</Badge>
                    )}
                    {developerMode && mod.source === "workshop" && (
                      <Badge colorPalette="blue">Workshop {mod.workshopId ?? ""}</Badge>
                    )}
                  </HStack>
                  <HStack align="center" className="pz-mod-actions pz-lua-mod-actions" gap="1">
                    <Box
                      className="pz-mod-switch-shell"
                      data-pz-mod-switch
                      onClick={(event) => event.stopPropagation()}
                      onKeyDown={(event) => event.stopPropagation()}
                    >
                      <Switch.Root
                        aria-label={`${mod.enabled ? "停用" : "启用"} ${mod.name}`}
                        checked={mod.enabled}
                        disabled={busy || (!mod.enabled && !mod.hasB42)}
                        onCheckedChange={(details) => void setLuaModsEnabled([mod], details.checked)}
                        size="sm"
                        title={!mod.hasB42 && !mod.enabled ? "非 B42 模组不能通过启动器启用" : mod.enabled ? "停用" : "启用"}
                      >
                        <Switch.HiddenInput />
                        <Switch.Control>
                          <Switch.Thumb />
                        </Switch.Control>
                      </Switch.Root>
                    </Box>
                    <HStack className="pz-mod-hover-actions" gap="1">
                      <Button
                        aria-label={`上移 ${mod.name}`}
                        className="pz-lua-mod-order-button"
                        disabled={busy || orderIndex <= 0}
                        onClick={(event) => {
                          event.stopPropagation();
                          void moveLuaMod(mod, -1);
                        }}
                        size="xs"
                        title="上移"
                        variant="ghost"
                      >
                        ↑
                      </Button>
                      <Button
                        aria-label={`下移 ${mod.name}`}
                        className="pz-lua-mod-order-button"
                        disabled={busy || orderIndex < 0 || orderIndex >= enabledOrder.length - 1}
                        onClick={(event) => {
                          event.stopPropagation();
                          void moveLuaMod(mod, 1);
                        }}
                        size="xs"
                        title="下移"
                        variant="ghost"
                      >
                        ↓
                      </Button>
                      <Button
                        aria-expanded={expanded}
                        aria-label={`${expanded ? "收起" : "展开"} ${mod.name}`}
                        className="pz-lua-mod-detail-button pz-mod-more-button"
                        onClick={(event) => {
                          event.stopPropagation();
                          toggleLuaModExpansion(mod);
                        }}
                        size="xs"
                        title={expanded ? "收起" : "详情"}
                        variant="ghost"
                      >
                        {expanded ? "×" : "⋯"}
                      </Button>
                    </HStack>
                  </HStack>
                </Box>
                <Box aria-hidden={!expanded} className="pz-jar-mod-details-shell">
                  <Box className="pz-jar-mod-details">
                    <Box className="pz-jar-mod-details-body">
                      <Text color="gray.600" fontSize="xs" overflowWrap="anywhere">ID：{mod.id}</Text>
                      {developerMode && (
                        <Text color="gray.500" fontSize="xs" mt="1" overflowWrap="anywhere">路径：{mod.path}</Text>
                      )}
                      <Text color="gray.500" fontSize="xs" mt="1">
                        来源：{mod.source === "workshop" ? `创意工坊 ${mod.workshopId ?? ""}` : "本地"}
                        {mod.hasCommon ? " · common" : ""}
                        {mod.hasB42 ? " · B42" : " · 非 B42"}
                      </Text>
                      <Text color={mod.requires.length > 0 ? "gray.600" : "gray.500"} fontSize="xs" mt="1" overflowWrap="anywhere">
                        前置模组：{mod.requires.length > 0 ? mod.requires.join("、") : "无"}
                      </Text>
                      <Text color="gray.500" fontSize="xs" mt="1">
                        {mod.enabled
                          ? `加载顺序：${mod.loadOrder ?? "-"}`
                          : "勾选此模组后，可使用“启用选中”批量启用（仅 B42 模组）"}
                      </Text>
                    </Box>
                  </Box>
                </Box>
              </Box>
                  );
                }}
              />
            );
          })
        ) : (
          <EmptyState text={luaMods && luaMods.length > 0 ? "没有符合当前筛选条件的模组。" : "未发现本地或创意工坊 Lua 模组。"} />
        )}
      </Box>
    </Box>
  );
  const jarModsContent = (
    <Box className="pz-panel pz-jar-mods-page">
      <HStack className="pz-mod-page-header" justify="space-between" gap="3" wrap="wrap">
        <Box>
          <Text color="gray.900" fontSize="lg" fontWeight="700">JAR 模组管理</Text>
          <Text color="gray.500" fontSize="xs" mt="0.5">
            显示 {filteredJarMods?.length ?? 0} / {jarMods?.length ?? 0} 个模组
          </Text>
        </Box>
        <HStack className="pz-mod-header-tools" gap="2" wrap="wrap">
          <Input
            aria-label="搜索 JAR 模组"
            onChange={(event) => setJarModSearch(event.currentTarget.value)}
            placeholder="搜索名称、包名或描述"
            size="sm"
            value={jarModSearch}
          />
          <Button
            loading={jarModsLoading}
            onClick={() => void loadJarMods(true)}
            size="sm"
            title="重新扫描创意工坊和本地的 JAR 模组，并更新极速启动用的模组清单"
            variant="outline"
          >
            刷新 JAR 列表
          </Button>
        </HStack>
      </HStack>
      {filteredJarMods && filteredJarMods.length > 0 && (
        <ModBatchBar
          busyAction={jarModBatchAction}
          negativeLabel="禁用"
          onClear={() => startTransition(() => setSelectedJarModKeys(new Set()))}
          onInvert={invertVisibleJarMods}
          onNegative={() => void setJarModDecisions(selectedJarMods, false)}
          onPositive={() => void setJarModDecisions(selectedJarMods, true)}
          onSelectAll={selectAllJarMods}
          positiveLabel="启用"
          selectedCount={selectedJarModCount}
          totalCount={filteredJarMods.length}
        />
      )}
      <Box className="pz-jar-mods-body">
        <ZombieBuddyLoaderPanel
          busy={luaModBatchAction !== null}
          detection={detection}
          developerMode={developerMode}
          expanded={zombieBuddyPanelExpanded}
          luaMod={zombieBuddyLuaMod}
          onOpenModLoaderSettings={openModLoaderSettings}
          onToggle={() => setZombieBuddyPanelExpanded((expanded) => !expanded)}
          onToggleLuaMod={(mod) => void setLuaModsEnabled([mod], !mod.enabled)}
          schemeEnabled={modLoaderScheme === "zombieBuddy"}
        />
        {detection.status === "checking" ? (
          <Text color="gray.500" py="6" textAlign="center">
            正在检查模组加载器状态...
          </Text>
        ) : modLoaderScheme === "none" ? (
          <Box className="pz-jar-mods-locked" textAlign="center">
            <Badge colorPalette="gray">未启用模组加载器</Badge>
            <Text color="gray.800" fontWeight="700" mt="3">
              当前没有启用模组加载器
            </Text>
            <Text color="gray.500" fontSize="sm" mt="1">
              JAR 模组需要加载器才能运行。要管理它们，请先到 设置 › 模组加载器 选择 ZombieBuddy。
            </Text>
            <Button colorPalette="blue" mt="4" onClick={openModLoaderSettings} size="sm">
              前往 设置 › 模组加载器
            </Button>
          </Box>
        ) : detection.status !== "found" || !detection.paths.zombieBuddy.installed ? (
          <Box className="pz-jar-mods-locked" textAlign="center">
            <Badge colorPalette="orange">需要 ZombieBuddy</Badge>
            <Text color="gray.800" fontWeight="700" mt="3">
              请前往 设置 › 模组加载器 安装加载器
            </Text>
            <Text color="gray.500" fontSize="sm" mt="1">
              安装完成后返回此页面读取已加载的 JAR 模组。
            </Text>
            <Button colorPalette="blue" mt="4" onClick={openModLoaderSettings} size="sm">
              前往 设置 › 模组加载器
            </Button>
          </Box>
        ) : jarMods ? (
          <Stack className="pz-jar-mod-list" gap="2">
            <HStack justify="space-between" color="gray.500" fontSize="sm" mb="1" wrap="wrap">
              <Text>
                {jarModsLoading
                  ? developerMode
                    ? jarScanStatusLabel(jarScanFrame, jarScanProgress.scanned, jarScanProgress.total)
                    : "正在扫描模组…"
                  : developerMode
                    ? "扫描 mod.info 与 mod.info.disabled 中的 ZombieBuddy 注解"
                    : "已读取 JAR 模组"}
              </Text>
              <Text>{jarModsLoading ? `已发现 ${filteredJarMods?.length ?? 0} 个 JAR 模组` : `发现 ${filteredJarMods?.length ?? 0} 个 JAR 模组`}</Text>
            </HStack>
            {jarModsLoading && jarScanProgress.total > 0 && (
              <Box className="pz-jar-scan-progress" aria-label="JAR 模组扫描进度">
                <Box
                  className="pz-jar-scan-progress-value"
                  style={{
                    width: `${Math.min(100, (jarScanProgress.scanned / jarScanProgress.total) * 100)}%`,
                  }}
                />
              </Box>
            )}
            {developerMode && !jarModsLoading && (filteredJarMods?.length ?? 0) > 0 && (
              <Box className={`pz-jar-conflict-panel${jarConflictExpanded ? " is-expanded" : ""}`} role="status">
                <Button className="pz-jar-conflict-summary" onClick={() => setJarConflictExpanded((expanded) => !expanded)} size="sm" variant="ghost" aria-expanded={jarConflictExpanded}>
                  <HStack gap="2" minW="0"><Box className="pz-jar-conflict-indicator" /><Text color="gray.800" fontSize="sm" fontWeight="700">已批准模组冲突分析</Text><Badge colorPalette="gray" size="sm">{approvedJarModCount} 个已批准模组</Badge></HStack>
                  <Text aria-hidden="true" className="pz-jar-conflict-chevron">{jarConflictExpanded ? "▴" : "▾"}</Text>
                </Button>
                <Box className="pz-jar-conflict-details-shell" aria-hidden={!jarConflictExpanded}>
                  <Box className="pz-jar-conflict-details">
                    <Text color="gray.500" fontSize="xs">仅基于已批准模组中解析到的注入器和方法名推断，实际冲突还取决于目标类、注入点和加载顺序。</Text>
                    {approvedJarModCount < 2 ? <Text color="gray.600" fontSize="xs" mt="2">已批准模组不足两个，暂时无法进行模组间冲突判断。</Text> : jarModConflicts.length === 0 ? <Text color="green.700" fontSize="xs" mt="2">当前没有检测到共享注入器和方法名的潜在冲突。</Text> : <Stack className="pz-jar-conflict-list" gap="1" mt="2">{jarModConflicts.map((conflict) => <HStack align="start" className="pz-jar-conflict-row" gap="2" key={`${conflict.risk}-${conflict.annotation}-${conflict.methodName}-${conflict.modIds.join("-")}`}><Badge colorPalette={jarConflictRiskPalette(conflict.risk)} size="sm">{jarConflictRiskLabel(conflict.risk)}</Badge><Text color="gray.600" fontFamily="mono" fontSize="xs" overflowWrap="anywhere">{conflict.modIds[0]} ↔ {conflict.modIds[1]}：@{conflict.annotation} {conflict.methodName}</Text></HStack>)}</Stack>}
                  </Box>
                </Box>
              </Box>
            )}            {filteredJarMods && filteredJarMods.length > 0 ? (
              filteredJarMods.map((mod) => {
                const modKey = jarModKey(mod);
                return (
                  <SleepingModRow
                    key={modKey}
                    render={() => {
                      const selected = selectedJarModKeys.has(modKey);
                      const expanded = expandedJarModKeys.has(modKey);
                      const toggleExpanded = () => {
                        setExpandedJarModKeys((current) => toggleJarModExpansion(current, modKey));
                      };

                      return (
                  <Box
                    aria-checked={selected}
                    aria-label={`选择 ${mod.id}`}
                    className={`pz-mod-row pz-jar-mod-row${selected ? " is-selected" : ""}${mod.decision === true && !mod.disabled ? " is-enabled" : ""}${mod.disabled || mod.decision === false ? " is-rejected" : ""}${expanded ? " pz-jar-mod-row-expanded" : ""}`}
                    key={modKey}
                    onClick={(event) => {
                      const target = event.target as HTMLElement;
                      if (target.closest("button, .pz-jar-mod-details-shell")) {
                        return;
                      }
                      toggleJarModSelection(mod, event.currentTarget);
                    }}
                    onKeyDown={(event) => {
                      if ((event.target as HTMLElement).closest("button")) {
                        return;
                      }
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        toggleJarModSelection(mod, event.currentTarget);
                      }
                    }}
                    onMouseDown={(event) => {
                      if (event.button === 0 && !isModRowInteractiveTarget(event.target)) event.preventDefault();
                    }}
                    onPointerDown={(event) => {
                      if (event.button !== 0 || isModRowInteractiveTarget(event.target)) return;
                      beginRowDrag("jar", modKey, selected);
                    }}
                    onPointerEnter={() => extendRowDrag("jar", modKey)}
                    role="checkbox"
                    tabIndex={0}
                  >
                    <Box className="pz-mod-summary pz-compact-mod-summary">
                      <Box className="pz-mod-select-area">
                        <JarModPoster name={mod.id} path={mod.posterPath} />
                        <Box className="pz-jar-mod-summary-content" minW="0">
                          <Text className="pz-jar-mod-name" color="gray.900" fontWeight="700" overflowWrap="anywhere">
                            {mod.id}
                          </Text>
                          {mod.description && (
                            <Text className="pz-jar-mod-description" color="gray.500" lineClamp="1">
                              {mod.description}
                            </Text>
                          )}
                          {developerMode && (
                            <Text className="pz-jar-mod-annotation-count" color="gray.500" fontSize="xs" mt="1">
                              {mod.packageName} · @Patch {mod.patchClasses.length} · @Exposer.LuaClass {mod.luaClasses.length}
                            </Text>
                          )}
                        </Box>
                      </Box>
                      <HStack className="pz-jar-mod-badges" gap="1" wrap="nowrap">
                        {developerMode ? (
                          <>
                            {mod.workshopId !== null && (
                              <Badge colorPalette="blue">Workshop {mod.workshopId}</Badge>
                            )}
                          </>
                        ) : null}
                      </HStack>
                      <HStack align="center" className="pz-mod-actions pz-lua-mod-actions" gap="1">
                        <Box
                          className="pz-mod-switch-shell"
                          data-pz-mod-switch
                          onClick={(event) => event.stopPropagation()}
                          onKeyDown={(event) => event.stopPropagation()}
                        >
                          <Switch.Root
                            aria-label={`${mod.decision === true && !mod.disabled ? "禁用" : "启用"} ${mod.id}`}
                            checked={mod.decision === true && !mod.disabled}
                            disabled={luaModBatchAction !== null || jarModBatchAction !== null}
                            onCheckedChange={(details) => void setJarModDecisions([mod], details.checked)}
                            size="sm"
                          >
                            <Switch.HiddenInput />
                            <Switch.Control>
                              <Switch.Thumb />
                            </Switch.Control>
                          </Switch.Root>
                        </Box>
                        <HStack className="pz-mod-hover-actions" gap="1">
                          <Button
                            aria-expanded={expanded}
                            aria-label={`${expanded ? "收起" : "展开"} ${mod.id}`}
                            className="pz-jar-mod-detail-button pz-mod-more-button"
                            onClick={(event) => {
                              event.stopPropagation();
                              toggleExpanded();
                            }}
                            size="xs"
                            title={expanded ? "收起" : "详情"}
                            variant="ghost"
                          >
                            {expanded ? "×" : "⋯"}
                          </Button>
                        </HStack>
                      </HStack>
                    </Box>
                    <Box aria-hidden={!expanded} className="pz-jar-mod-details-shell">
                      <Box className="pz-jar-mod-details">
                        <Box className="pz-jar-mod-details-body">
                          {developerMode && (
                            <>
                              <Text color="gray.600" fontSize="xs" overflowWrap="anywhere">
                                包：{mod.packageName}
                              </Text>
                              <Text color="gray.500" fontSize="xs" mt="1" overflowWrap="anywhere">
                                JAR：{mod.jarPath}
                              </Text>
                              <Text color="gray.500" fontSize="xs" mt="1" overflowWrap="anywhere">
                                配置：{mod.modInfoPath}
                              </Text>
                            </>
                          )}
                          <Text color={mod.disabled ? "red.600" : "green.700"} fontSize="xs" mt="1">
                            {developerMode
                              ? mod.disabled
                                ? "已通过 mod.info.disabled 禁用，其他启动器不会读取此模组。"
                                : "已启用 mod.info。"
                              : mod.disabled || mod.decision === false
                                ? "已禁用。"
                                : mod.decision === true
                                  ? "已启用。"
                                  : "尚未启用。"}
                          </Text>
                          {developerMode && mod.patchClasses.length > 0 && (
                            <Text color="gray.500" fontFamily="mono" fontSize="xs" mt="2" overflowWrap="anywhere">
                              @Patch：{mod.patchClasses.join("、")}
                            </Text>
                          )}
                          {developerMode && mod.patchMethods.length > 0 && (
                            <Stack gap="1" mt="2">
                              <Text color="gray.500" fontSize="xs">
                                具体方法：
                              </Text>
                              {mod.patchMethods.map((patchMethod) => (
                                <Text
                                  color="gray.500"
                                  fontFamily="mono"
                                  fontSize="xs"
                                  key={`${patchMethod.annotation}-${patchMethod.methodName}`}
                                  overflowWrap="anywhere"
                                >
                                  @{patchMethod.annotation}：{patchMethod.methodName}
                                </Text>
                              ))}
                            </Stack>
                          )}
                          {developerMode && mod.luaClasses.length > 0 && (
                            <Text color="gray.500" fontFamily="mono" fontSize="xs" mt="2" overflowWrap="anywhere">
                              @Exposer.LuaClass：{mod.luaClasses.join("、")}
                            </Text>
                          )}
                        </Box>
                      </Box>
                    </Box>
                  </Box>
                      );
                    }}
                  />
                );
              })
            ) : (
              <EmptyState
                text={
                  jarMods && jarMods.length > 0
                    ? "没有符合当前搜索条件的 JAR 模组。"
                    : developerMode
                      ? "没有扫描到包含 ZombieBuddy @Patch 或 @Exposer.LuaClass 注解的 JAR 模组。"
                      : "没有扫描到可由 ZombieBuddy 管理的 JAR 模组。"
                }
              />
            )}
          </Stack>
        ) : (
          <Text color="gray.500" py="6" textAlign="center">
            尚未扫描 JAR 模组。
          </Text>
        )}
      </Box>
    </Box>
  );
  const modsContent = (
    <Box className="pz-resource-center pz-mod-management">
      <Box as="nav" aria-label="模组管理分类" className="pz-resource-center-sidebar pz-mod-sidebar">
        <Text className="pz-resource-center-sidebar-title">模组类型</Text>
        <Stack gap="1">
          <Button className={`pz-resource-center-sidebar-item ${modManagementPage === "lua" ? "pz-resource-center-sidebar-item-active" : ""}`} onClick={() => setModManagementPage("lua")} size="sm" variant="ghost">Lua 模组</Button>
          <Button className={`pz-resource-center-sidebar-item ${modManagementPage === "jar" ? "pz-resource-center-sidebar-item-active" : ""}`} onClick={() => setModManagementPage("jar")} size="sm" variant="ghost">JAR 模组</Button>
        </Stack>
        {modManagementPage === "lua" && (
          <>
            <Text className="pz-mod-sidebar-section-title">筛选</Text>
            <Stack gap="1">
              <Button
                aria-pressed={luaModFilters.size === 0}
                className={`pz-resource-center-sidebar-item ${luaModFilters.size === 0 ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setLuaModFilters(new Set())}
                size="sm"
                variant="ghost"
              >
                全部
              </Button>
              <Button
                aria-pressed={luaModFilters.has("enabled")}
                className={`pz-resource-center-sidebar-item ${luaModFilters.has("enabled") ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => toggleLuaModFilter("enabled")}
                size="sm"
                variant="ghost"
              >
                已启用
              </Button>
              <Button
                aria-pressed={luaModFilters.has("disabled")}
                className={`pz-resource-center-sidebar-item ${luaModFilters.has("disabled") ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => toggleLuaModFilter("disabled")}
                size="sm"
                variant="ghost"
              >
                未启用
              </Button>
              <Button
                aria-pressed={luaModFilters.has("b42")}
                className={`pz-resource-center-sidebar-item ${luaModFilters.has("b42") ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => toggleLuaModFilter("b42")}
                size="sm"
                variant="ghost"
              >
                B42
              </Button>
            </Stack>
            <Text className="pz-mod-sidebar-section-title">排序</Text>
            <Stack gap="1">
              <Button
                aria-pressed={luaModSortMode === "load"}
                className={`pz-resource-center-sidebar-item ${luaModSortMode === "load" ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setLuaModSortMode("load")}
                size="sm"
                variant="ghost"
              >
                加载顺序
              </Button>
              <Button
                aria-pressed={luaModSortMode === "name"}
                className={`pz-resource-center-sidebar-item ${luaModSortMode === "name" ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setLuaModSortMode("name")}
                size="sm"
                variant="ghost"
              >
                名称
              </Button>
            </Stack>
          </>
        )}
        {modManagementPage === "jar" && (
          <>
            <Text className="pz-mod-sidebar-section-title">筛选</Text>
            <Stack gap="1">
              <Button
                aria-pressed={jarModFilter === "all"}
                className={`pz-resource-center-sidebar-item ${jarModFilter === "all" ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setJarModFilter("all")}
                size="sm"
                variant="ghost"
              >
                全部
              </Button>
              <Button
                aria-pressed={jarModFilter === "enabled"}
                className={`pz-resource-center-sidebar-item ${jarModFilter === "enabled" ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setJarModFilter("enabled")}
                size="sm"
                variant="ghost"
              >
                已启用
              </Button>
              <Button
                aria-pressed={jarModFilter === "disabled"}
                className={`pz-resource-center-sidebar-item ${jarModFilter === "disabled" ? "pz-resource-center-sidebar-item-active" : ""}`}
                onClick={() => setJarModFilter("disabled")}
                size="sm"
                variant="ghost"
              >
                未启用
              </Button>
            </Stack>
          </>
        )}
        <Text className="pz-mod-sidebar-section-title">预设</Text>
        <Stack className="pz-mod-preset-list" gap="1">
          {modPresets.map((preset) => (
            <HStack className="pz-mod-preset-item" gap="1" key={preset.id}>
              <Button
                className="pz-resource-center-sidebar-item pz-mod-preset-apply"
                disabled={modPresetBusy}
                onClick={() => void applyModPreset(preset)}
                size="sm"
                title={`切换到预设「${preset.name}」`}
                variant="ghost"
              >
                {preset.name}
              </Button>
              <Button
                aria-label={`删除预设 ${preset.name}`}
                disabled={modPresetBusy}
                onClick={() => void deleteModPreset(preset)}
                size="xs"
                variant="ghost"
              >
                <FontAwesomeIcon icon={faTrash} />
              </Button>
            </HStack>
          ))}
          <Input
            aria-label="预设名称"
            onChange={(event) => setModPresetName(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void saveModPreset();
            }}
            placeholder="预设名称"
            size="sm"
            value={modPresetName}
          />
          <Button disabled={modPresetBusy} loading={modPresetBusy} onClick={() => void saveModPreset()} size="sm" variant="outline">
            保存当前为预设
          </Button>
        </Stack>
      </Box>
      <Box className="pz-resource-center-main">{modManagementPage === "lua" ? luaModsContent : jarModsContent}</Box>
    </Box>
  );
  return (
    <Box
      as="main"
      className={`pz-app-shell ${developerMode ? "pz-developer-mode" : "pz-simple-mode"}${wallpaperSeasonal ? " pz-app-shell-seasonal" : ""}`}
      minH="100vh"
      style={
        backgroundUrl
          ? {
              backgroundImage: `url("${backgroundUrl}")`,
              backgroundPosition: "center",
              backgroundRepeat: "no-repeat",
              backgroundSize: "cover",
            }
          : undefined
      }
    >
      {wallpaperSeasonal ? <SeasonalWallpaper effect={wallpaperEffect} /> : null}
      <WindowTitlebar
        onToggleTasks={() => setTaskPanelOpen((open) => !open)}
        taskCount={tasks.filter((task) => task.status === "running").length}
        taskPanelOpen={taskPanelOpen}
      >
        <Box className="pz-sidebar-brand">
          <Box className="pz-top-nav-logo">PZM</Box>
          <Text className="pz-top-nav-title">PZMANAGER</Text>
        </Box>
        <Box as="aside" className="pz-sidebar">
          <Stack as="nav" aria-label="应用主导航" className="pz-side-nav" gap="1">
            {navigationItems.map((item) => (
              <Button
                className={`pz-side-nav-item ${navActivePage === item.value ? "pz-side-nav-item-active" : ""}`}
                key={item.value}
                aria-label={item.label}
                onClick={() => navigateToPage(item.value)}
                size="md"
                variant="ghost"
              >
                {(
                <Box
                  as="span"
                  aria-hidden="true"
                  className={`pz-side-nav-icon pz-side-nav-icon-${item.icon}`}
                />)}
                <Box
                  as="span"
                  className={`pz-side-nav-label ${
                    navActivePage === item.value ? "pz-side-nav-label-active" : ""
                  }`}
                >
                  {item.label}
                </Box>
              </Button>
            ))}
          </Stack>
        </Box>
      </WindowTitlebar>
      {taskPanelOpen && (
        <TaskCenterPanel
          onClearFinished={clearFinishedTasks}
          onClose={() => setTaskPanelOpen(false)}
          onRemove={removeTask}
          onRetry={retryShaderDownload}
          tasks={tasks}
        />
      )}
      <Box className={`pz-app-layout ${activePage === "launch" ? "pz-app-layout-launch" : ""}`}>
        <Box
          className={`pz-main ${activePage === "launch" ? "pz-main-launch" : "pz-main-framed"} pz-main-${pageTransitionMovesShell ? pageTransitionPhase : "idle"}`}
          data-direction={pageTransitionDirection}
        >
          <Box
            className={`pz-app-content ${
              activePage === "workshop" || activePage === "mods" || activePage === "settings" ? "pz-app-content-resource-center" : ""
            } ${activePage === "launch" ? "pz-app-content-launch" : ""}`}
          >
            <Stack
              className={`pz-page-transition pz-page-transition-${pageTransitionPhase}`}
              data-direction={pageTransitionDirection}
              gap="0"
            >
              {activePage === "launch" && launchContent}
              {activePage === "overview" && overviewContent}
              {activePage === "shots" && shotsContent}
              {activePage === "shaderpacks" && shaderpacksContent}
              {activePage === "workshop" && resourceCenterContent}
              {activePage === "mods" && modsContent}
              {activePage === "settings" && settingsContent}
            </Stack>
          </Box>
        </Box>
      </Box>

      {preview && (
        <Box
          className="pz-modal-overlay"
          alignItems="center"
          bg="blackAlpha.700"
          display="flex"
          inset="0"
          justifyContent="center"
          onClick={() => {
            URL.revokeObjectURL(preview.url);
            setPreview(null);
          }}
          p="3"
          position="fixed"
          zIndex="modal"
        >
          <Box
            className="pz-modal"
            bg="white"
            borderRadius="md"
            maxH="90vh"
            maxW="min(960px, 100%)"
            onClick={(event) => event.stopPropagation()}
            overflow="auto"
            p={{ base: "3", md: "5" }}
            width="100%"
          >
            <HStack justify="space-between" gap="2">
              <Text color="gray.900" fontWeight="700" overflowWrap="anywhere">
                {preview.name}
              </Text>
              <Button
                onClick={() => {
                  URL.revokeObjectURL(preview.url);
                  setPreview(null);
                }}
                size="sm"
                variant="ghost"
              >
                关闭
              </Button>
            </HStack>
            <Box bg="gray.100" borderRadius="sm" mt="3" overflow="auto" textAlign="center">
              <FallbackImage alt={preview.name} className="is-large" src={preview.url} style={{ display: "block", margin: "0 auto", maxHeight: "65vh", maxWidth: "100%" }} />
            </Box>
            <HStack justify="end" mt="4">
              <Button colorPalette="blue" loading={copyingShot} onClick={() => void copyPreview()}>
                复制图片
              </Button>
            </HStack>
          </Box>
        </Box>
      )}
      {feedbackOpen && (
        <Portal>
          <Box
            className={`pz-resource-file-overlay ${feedbackClosing ? "is-closing" : ""}`}
            onClick={closeFeedbackDialog}
          >
            <Box
              aria-label="反馈信息"
              aria-modal="true"
              className="pz-resource-file-dialog pz-feedback-dialog"
              onClick={(event) => event.stopPropagation()}
              role="dialog"
            >
              <HStack justify="space-between" gap="3">
                <Box minW="0">
                  <Text color="gray.900" fontWeight="700">反馈问题</Text>
                  <Text color="gray.500" fontSize="xs" mt="1">
                    仅上传 {feedbackCutoffLabel} 前最后一次启动的日志
                  </Text>
                </Box>
                <Button aria-label="关闭反馈" onClick={closeFeedbackDialog} size="sm" variant="ghost">
                  ×
                </Button>
              </HStack>
              {feedbackResult ? (
                <Box mt="4">
                  <Text color="gray.800" fontSize="sm" fontWeight="700">反馈已提交，感谢！</Text>
                  {feedbackResult.feedbackId && (
                    <Text color="gray.600" fontSize="xs" mt="1">编号：{feedbackResult.feedbackId}</Text>
                  )}
                  <Text color="gray.500" fontSize="xs" mt="2">
                    已上传：{feedbackResult.uploaded.join("、") || "无"}
                  </Text>
                  {feedbackResult.skipped.length > 0 && (
                    <Text color="gray.500" fontSize="xs" mt="1">
                      未找到：{feedbackResult.skipped.join("、")}
                    </Text>
                  )}
                  <HStack justify="flex-end" mt="4">
                    <Button colorPalette="blue" onClick={closeFeedbackDialog} size="sm">关闭</Button>
                  </HStack>
                </Box>
              ) : (
                <>
                  <Text color="gray.500" fontSize="xs" mt="2">
                    将上传 %TEMP%\overlay.log（最近一次游戏启动的部分）与
                    %USERPROFILE%\Zomboid\console.txt。日志可能包含游戏路径、Steam ID
                    等信息，仅用于排查问题。
                  </Text>
                  <Textarea
                    aria-label="错误详情"
                    disabled={feedbackSubmitting}
                    mt="3"
                    onChange={(event) => setFeedbackDescription(event.currentTarget.value)}
                    placeholder="请描述遇到的问题：做了什么操作、期望结果、实际结果。复现步骤越具体越好。"
                    rows={6}
                    value={feedbackDescription}
                  />
                  {feedbackError && (
                    <Text color="red.500" fontSize="xs" mt="2">{feedbackError}</Text>
                  )}
                  <HStack justify="flex-end" gap="2" mt="3">
                    <Button disabled={feedbackSubmitting} onClick={closeFeedbackDialog} size="sm" variant="ghost">
                      取消
                    </Button>
                    <Button
                      colorPalette="blue"
                      disabled={feedbackSubmitting || !feedbackDescription.trim()}
                      loading={feedbackSubmitting}
                      onClick={() => void submitFeedback()}
                      size="sm"
                    >
                      提交反馈
                    </Button>
                  </HStack>
                </>
              )}
            </Box>
          </Box>
        </Portal>
      )}
      <ToastViewport onClose={dismissToast} toasts={toasts} />
    </Box>
  );
}

export default App;
type DeveloperNewsTranslationResult = { sid: string; text: string; engine: string };

function escapeNewsHtml(value: string): string {
  return String(value ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}
function replaceNewsTranslation(html: string, sid: string, text: string): string {
  const escapedSid = sid.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const span = new RegExp(`<span data-sid="${escapedSid}" class="i18n-pending">[\\s\\S]*?<\\/span>`);
  return html.replace(span, escapeNewsHtml(text));
}

const CUSTOM_TRANSLATION_MAX_CHARS = 20000;
const CUSTOM_TRANSLATION_CHUNK_CHARS = 1800;

type CustomTranslationLine =
  | { kind: "blank" }
  | { kind: "raw"; text: string }
  | { kind: "translatable"; chunks: string[]; sids: string[]; separators: string[] };

/** 单行超过翻译接口的舒适长度时切开；优先在空格处切，中文没有空格就硬切。 */
function splitCustomTranslationLine(
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

/** 按行拆自定义文本：空行原样保留，纯符号/数字行不送翻译，长行按需切块。 */
function buildCustomTranslationJobs(source: string): {
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

/** 每批送多少条占位去翻译：开发者新闻与创意工坊详情共用同一条管线。 */
const TRANSLATION_BATCH_SIZE = 15;

/** 把后端切好的 data-sid 占位分批送去翻译，每批回来就交给 onResults 写回。
    isCancelled() 返回 true 时立即停手并丢弃结果（组件卸载、切换语言或换条目）。 */
async function translateInBatches(
  jobs: Array<{ sid: string; text: string }>,
  language: string,
  onResults: (results: DeveloperNewsTranslationResult[]) => void,
  onProgress: (completed: number, total: number) => void,
  isCancelled: () => boolean,
): Promise<void> {
  for (let index = 0; index < jobs.length; index += TRANSLATION_BATCH_SIZE) {
    if (isCancelled()) {
      return;
    }
    const batch = jobs.slice(index, index + TRANSLATION_BATCH_SIZE);
    try {
      const results = await invoke<DeveloperNewsTranslationResult[]>("translate_developer_news_batch", {
        jobs: batch,
        language,
      });
      if (isCancelled()) {
        return;
      }
      onResults(results);
    } catch {
      if (isCancelled()) {
        return;
      }
    } finally {
      if (!isCancelled()) {
        onProgress(Math.min(index + batch.length, jobs.length), jobs.length);
      }
    }
  }
}

const newsEngineLabels: Record<string, string> = {
  google: "Google",
  mymemory: "MyMemory",
  none: "未翻译",
};

function formatNewsEngineShare(tally: Record<string, number> | undefined): string | null {
  if (!tally) return null;
  const entries = Object.entries(tally).filter(([, count]) => count > 0);
  const total = entries.reduce((sum, [, count]) => sum + count, 0);
  if (total === 0) return null;
  return entries
    .sort((left, right) => right[1] - left[1])
    .map(([engine, count]) => `${newsEngineLabels[engine] ?? engine} ${Math.round((count / total) * 100)}%`)
    .join(" · ");
}
