import { Avatar, Badge, Box, Button, Checkbox, HStack, Input, Menu, Portal, Slider, Stack, Switch, Text, Textarea } from "@chakra-ui/react";
import { faAnglesDown, faAnglesUp, faCircleInfo, faClockRotateLeft, faCubes, faGear, faHouse, faImages, faPlus, faScrewdriverWrench, faSun, faTrash } from "@fortawesome/free-solid-svg-icons";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { exit } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Image as TauriImage } from "@tauri-apps/api/image";
import { open } from "@tauri-apps/plugin-dialog";
import { writeImage, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { startTransition, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import "./App.css";
import { conciseNotification, shouldShowNotification } from "./developerMode";
import { FallbackImage } from "./FallbackImage";
import { steamWorkshopUrl } from "./externalLinks";
import { shaderpackSearchQueries } from "./shaderpackSearch";
import appManifest from "../package.json";
import { defaultLaunchArguments, joinLaunchArguments } from "./launchArgs";
import {
  MEMORY_CLEAN_POLL_MS,
  applyMemoryCleanSample,
  createMemoryCleanTracker,
  memoryCleanFinishedDetail,
  memoryCleanProgressDetail,
  memoryCleanupSystemDetail,
  shouldFinishMemoryClean,
} from "./memoryClean";
import { MIN_MEMORY_MB, defaultMemoryLimitMb } from "./memorySettings";
import {
  DEFAULT_STEAM_RELAY_BASE,
  STEAM_RELAY_MODE_OPTIONS,
  previewImageFallbackUrl,
} from "./steamRelay";
import { filterModVersionTargets, formatVersionBytes, formatVersionElapsed, formatVersionEta, formatVersionTime, modSourceLabel, shouldWarnVersionStoreOnSystemDrive, shortVersionHash, versionBaseTypeLabel, versionChangeSummary, versionDeltaChainDepth, versionMatchLabel, versionPhaseLabel, versionProgressPercent, versionStepLabel, versionStoreOnSystemDrive, versionLabelById, compareVersionStrings, parseDevBuildVersion, createVersionEtaTracker, updateVersionEta, type ModVersionFilter, type ModVersionOperationUpdate, type ModVersionStatus, type ModVersionTarget, type ModPackRecord, type VersionOperationUpdate, type VersionRecord, type VersionStatus, type VersionVerifyResult } from "./versioning";
import { DEV_OVERLAY_DLL_PATH } from "./overlayInjection";
import { DEFAULT_SEASON_FEATURES, DEFAULT_WALLPAPER_EFFECT, normalizeWallpaperEffectSettings, wallpaperEffectAt, type SeasonFeatures, type WallpaperEffectSettings } from "./builtinWallpaper";
import { SeasonalWallpaper, loadSeasonFeatures } from "./SeasonalWallpaper";
import { AcrylicGlass } from "./AcrylicGlass";
import {
  filterJarMods,
  filterLuaMods,
  invertSelection,
  mergeItemsPreservingOrder,
  moveModIdBlock,
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
import { type ModWorkshopLink } from "./modWorkshop";
import {
  ActivePage,
  AppSettings,
  BackgroundEntry,
  BuiltinBackground,
  DetectionState,
  DeveloperNewsResponse,
  FastLaunchSaveResult,
  FeedbackHistoryEntry,
  InstallResult,
  JarModBatchDecisionResult,
  JarModScanUpdate,
  LauncherLogTail,
  LauncherUpdateStatus,
  AutoSortLuaModsResult,
  LuaModBatchToggleResult,
  LuaModEntry,
  ManagedDirectory,
  ManagedFile,
  MemoryCleanupReport,
  MemoryStatus,
  MinecraftVersionOption,
  ModManagementPage,
  ModPreset,
  ModPresetApplyResult,
  NotificationEntry,
  OverlaySessionUpdate,
  OverlayZbProgress,
  PreviewState,
  ProjectZomboidPaths,
  PzOptimizationInstallResult,
  ResourceCenterPage,
  ResourceFile,
  ResourceOptions,
  ResourceSearchResponse,
  ScannedJarMod,
  SettingsPage,
  ShaderDownloadRequest,
  ShaderpackIcon,
  StartupTiming,
  TaskItem,
  ToastItem,
  ToastKind,
  VersionTab,
  WorkshopBackgroundScanUpdate,
  WorkshopFilters,
  WorkshopItem,
  WorkshopItemDetail,
  WorkshopOptions,
  WorkshopResponse,
  WorkshopTagMode,
} from "./app/types";
import {
  BLUR_STRENGTH_STEP,
  BUILTIN_BACKGROUND_PREFIX,
  BUILTIN_SEASONAL_ID,
  CUSTOM_TRANSLATION_MAX_CHARS,
  DEFAULT_BLUR_STRENGTH,
  DEFAULT_FONT_SCALE,
  DEFAULT_MOD_LOADER_SCHEME,
  DEFAULT_OVERLAY_BACKGROUND_COLOR,
  DEFAULT_SILENT_MEMORY_PERCENT,
  DEFAULT_TRANSLATION_LANGUAGE,
  DEFAULT_WINDOW_HEIGHT,
  DEFAULT_WINDOW_WIDTH,
  FONT_FAMILY_PRESETS,
  FONT_FAMILY_PRESET_CSS,
  FONT_SCALE_STEP,
  MAX_BLUR_STRENGTH,
  MAX_FONT_SCALE,
  MAX_SILENT_MEMORY_PERCENT,
  MEMORY_STEP_MB,
  MEMORY_WARNING_MB,
  MIN_BLUR_STRENGTH,
  MIN_FONT_SCALE,
  MIN_SILENT_MEMORY_PERCENT,
  MOD_LOADER_SCHEME_OPTIONS,
  OVERLAY_BACKGROUND_PRESETS,
  PAGE_TRANSITION_MS,
  PZ_OPTIMIZATION_WORKSHOP_ID,
  RESOURCE_DIALOG_EXIT_MS,
  SILENT_MEMORY_PERCENT_STEP,
  TASK_REMOVE_EXIT_MS,
  TRANSLATION_LANGUAGE_OPTIONS,
  VERSION_DIFF_EXIT_MS,
  ZOMBIE_BUDDY_WORKSHOP_ID,
  initialWorkshopFilters,
  navigationItems,
  newsEngineLabels,
} from "./app/constants";
import {
  DeveloperNewsCard,
  EmptyState,
  HintDot,
  HtmlContentWithImageReveal,
  MediaPanel,
  PathValue,
  ScrollingLabel,
  ShotThumbnail,
  type ShotsSource,
  SkeletonGate,
  SkeletonList,
  SkeletonNews,
} from "./components/common";
import {
  JarModPoster,
  LuaModPoster,
  ModBatchBar,
  ModRowContextMenu,
  SleepingModRow,
  SupportOptionRow,
  ZombieBuddyLoaderPanel,
} from "./components/mods";
import {
  buildCustomTranslationJobs,
  builtinBackgroundFromPath,
  detectionStateFromPaths,
  fontFamilyChoiceFromValue,
  formatBytes,
  formatElapsed,
  formatMemoryMb,
  formatNewsEngineShare,
  formatWorkshopDate,
  formatWorkshopSize,
  isModRowInteractiveTarget,
  isTauriRuntime,
  mimeTypeFor,
  overlayPhaseLabel,
  replaceNewsTranslation,
  startupTimingText,
  translateInBatches,
} from "./app/utils";
import { BackgroundMenu } from "./components/backgrounds";
import { FastLaunchCacheStatusLine, FastLaunchWarningDialog } from "./components/fastLaunch";
import { ModPackNameDialog, ModVersionDialog, ModVersionTimeline, PzOptRestoreWarningDialog, VersionStoreDriveDialog } from "./components/version";
import { NotificationConsole, TaskCenterPanel, ToastViewport } from "./components/tasks";
import { ResourceDownloadBrowser, WorkshopDropdown, WorkshopTagPicker, WorkshopTagSelectionSummary } from "./components/workshop";
import { WindowTitlebar, useDialogAutoHeight } from "./components/shell";

/** 当前构建的内测信息：版本号写成 `x.y.z-devbuild.<构建号>` 时才有（见 pytools/version_manager.py）。 */
const APP_DEV_BUILD = parseDevBuildVersion(appManifest.version);

/** 往上找第一个真正能滚的祖先：拖动时靠它做边缘自动滚动。 */
function scrollParentOf(element: HTMLElement) {
  let current = element.parentElement;
  while (current && current !== document.body) {
    const overflowY = getComputedStyle(current).overflowY;
    if (
      (overflowY === "auto" || overflowY === "scroll") &&
      current.scrollHeight > current.clientHeight + 1
    ) {
      return current;
    }
    current = current.parentElement;
  }
  return null;
}

function App() {
  const [detection, setDetection] = useState<DetectionState>({ status: "checking" });
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const toastIdRef = useRef(0);
  const [notificationHistory, setNotificationHistory] = useState<NotificationEntry[]>([]);
  const [tasks, setTasks] = useState<TaskItem[]>([]);
  /** 正在播移除动画的任务 id；动画走完才真正从 tasks 里删掉。 */
  const [taskClosingIds, setTaskClosingIds] = useState<string[]>([]);
  const [taskPanelOpen, setTaskPanelOpen] = useState(false);
  // 任务中心退出动画：先播 .pz-task-panel-out 再卸载，三条关闭路径（× / 点外部 / Esc）共用。
  const [taskPanelClosing, setTaskPanelClosing] = useState(false);
  const taskPanelCloseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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
  // 覆盖层背景色（设置 › 通用）：下发给注入到游戏里的 DLL，下次启动游戏时生效。
  const [overlayBackgroundColor, setOverlayBackgroundColor] = useState(
    DEFAULT_OVERLAY_BACKGROUND_COLOR,
  );
  const overlayBackgroundSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 记住的窗口尺寸（仅用于设置页展示；启动时的实际恢复由后端 setup 完成）。
  const [savedWindowSize, setSavedWindowSize] = useState<[number, number] | null>(null);
  const windowSizeSaveTimerRef = useRef<number | null>(null);
  // 模组加载器方案（设置 › 模组加载器）：启动时按它决定要不要挂加载器 agent。
  const [modLoaderScheme, setModLoaderScheme] = useState(DEFAULT_MOD_LOADER_SCHEME);
  const [modLoaderSchemeSaving, setModLoaderSchemeSaving] = useState(false);
  // 极速启动（实验性，设置 › 启动）：只在方案 = ZombieBuddy 时有意义。
  const [fastLaunch, setFastLaunch] = useState(false);
  const [fastLaunchSaving, setFastLaunchSaving] = useState(false);
  // 打开极速启动前先弹一次警告；关闭不需要确认。
  const [fastLaunchWarningOpen, setFastLaunchWarningOpen] = useState(false);
  const [fastLaunchWarningClosing, setFastLaunchWarningClosing] = useState(false);
  // 静默内存清理 / 启动时清理内存（设置 › 启动）。
  const [silentMemoryCleanup, setSilentMemoryCleanup] = useState(false);
  const [silentMemoryCleanupThreshold, setSilentMemoryCleanupThreshold] = useState(
    DEFAULT_SILENT_MEMORY_PERCENT,
  );
  const [silentMemoryCleanupSaving, setSilentMemoryCleanupSaving] = useState(false);
  const [cleanMemoryOnLaunch, setCleanMemoryOnLaunch] = useState(false);
  const [cleanMemoryOnLaunchSaving, setCleanMemoryOnLaunchSaving] = useState(false);
  /** 激进内存清理：常规清理之外再清系统工作集 / 文件缓存 / 已修改页 / 待机缓存（系统级步骤需要管理员）。 */
  const [aggressiveMemoryClean, setAggressiveMemoryClean] = useState(false);
  const [aggressiveMemoryCleanSaving, setAggressiveMemoryCleanSaving] = useState(false);
  /** 当前启动器是否以管理员身份运行；null = 还没问过后端。 */
  const [launcherElevated, setLauncherElevated] = useState<boolean | null>(null);
  const [restartingAsAdmin, setRestartingAsAdmin] = useState(false);
  const [memoryStatus, setMemoryStatus] = useState<MemoryStatus | null>(null);
  const [lastCleanupReport, setLastCleanupReport] = useState<MemoryCleanupReport | null>(null);
  const [memoryCleaning, setMemoryCleaning] = useState(false);
  /** 「立即清理一次」同一时间只跑一条任务（按钮在跑的时候也会 loading）。 */
  const memoryCleanTaskRunningRef = useRef(false);
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
  // 设置内分页切换的滑入方向：按侧栏顺序判断前进/后退，与主页面切换的方向感一致。
  const [settingsTransitionDirection, setSettingsTransitionDirection] = useState<"forward" | "backward">("forward");
  // 「设置 › 版本」：版本隔离状态、后台操作进度与存储目录草稿。
  const [versionTab, setVersionTab] = useState<VersionTab>("game");
  const [versionStatus, setVersionStatus] = useState<VersionStatus | null>(null);
  const [versionStatusLoading, setVersionStatusLoading] = useState(false);
  const [versionStatusError, setVersionStatusError] = useState<string | null>(null);
  const [versionTransitionDirection, setVersionTransitionDirection] = useState<"forward" | "backward">("forward");
  const [versionOperation, setVersionOperation] = useState<VersionOperationUpdate | null>(null);
  // 还原后 PZ Optimization 未能自动重装：还原继续完成，结束后弹模态框警告。
  const [pzOptRestoreWarning, setPzOptRestoreWarning] = useState<string | null>(null);
  // 时间估算：进度事件按百分比变化率平滑出当前步骤的剩余时间，事件驱动刷新。
  const versionEtaTrackerRef = useRef(createVersionEtaTracker());
  const [versionTiming, setVersionTiming] = useState<{ startedAt: number | null; etaSeconds: number | null }>({
    startedAt: null,
    etaSeconds: null,
  });
  const [versionOperationRunning, setVersionOperationRunning] = useState(false);
  const [versionVerified, setVersionVerified] = useState<VersionVerifyResult | null>(null);
  const [versionLabelDraft, setVersionLabelDraft] = useState("");
  const [versionStoreDraft, setVersionStoreDraft] = useState("");
  const [versionStoreSaving, setVersionStoreSaving] = useState(false);
  const [versionParallel, setVersionParallel] = useState(true);
  const [versionParallelSaving, setVersionParallelSaving] = useState(false);
  const [versionIncludeMods, setVersionIncludeMods] = useState(true);
  const [versionIncludeModsSaving, setVersionIncludeModsSaving] = useState(false);
  const [versionExpandedId, setVersionExpandedId] = useState<string | null>(null);
  // 收起「查看改动」时要等折叠动画播完再卸载内容，否则高度会瞬间跳没。
  const [versionClosingId, setVersionClosingId] = useState<string | null>(null);
  const versionStatusRequestRef = useRef(0);
  // 备份在 C 盘时，每次版本操作前先提醒换盘；「不再弹出」写进应用设置。
  const [versionStorePath, setVersionStorePath] = useState("");
  const [versionDriveNoticeOpen, setVersionDriveNoticeOpen] = useState(false);
  const [versionDriveNoticeClosing, setVersionDriveNoticeClosing] = useState(false);
  const [versionDriveNoticeDontAsk, setVersionDriveNoticeDontAsk] = useState(false);
  const [versionDriveNoticeDismissed, setVersionDriveNoticeDismissed] = useState(false);
  const versionDriveNoticeActionRef = useRef<(() => void) | null>(null);
  const versionStoreInputRef = useRef<HTMLInputElement | null>(null);
  // 「设置 › 版本 › 模组版本」：模组列表、按需加载的状态、行内展开与「版本记录」模态框。
  const [modVersionTargets, setModVersionTargets] = useState<ModVersionTarget[] | null>(null);
  const [modVersionTargetsLoading, setModVersionTargetsLoading] = useState(false);
  const [modVersionPacks, setModVersionPacks] = useState<ModPackRecord[] | null>(null);
  // 整合包命名 / 重命名弹窗：save = 一键保存时起名（可留空），rename = 改已有整合包的显示名。
  const [modPackNameDialog, setModPackNameDialog] = useState<{
    mode: "save" | "rename";
    pack: ModPackRecord | null;
  } | null>(null);
  const [modPackNameDialogClosing, setModPackNameDialogClosing] = useState(false);
  const [modPackRenaming, setModPackRenaming] = useState(false);
  const [modVersionSearch, setModVersionSearch] = useState("");
  const [modVersionFilter, setModVersionFilter] = useState<ModVersionFilter>("all");
  const [modVersionStatuses, setModVersionStatuses] = useState<Record<string, ModVersionStatus>>({});
  const [modVersionExpandedKey, setModVersionExpandedKey] = useState<string | null>(null);
  // 「详情」收起时先播折叠动画，结束后再卸载内容（和游戏版本「查看改动」一致）。
  const [modVersionClosingKey, setModVersionClosingKey] = useState<string | null>(null);
  const [modVersionDialog, setModVersionDialog] = useState<{ rootPath: string; name: string } | null>(null);
  const [modVersionDialogClosing, setModVersionDialogClosing] = useState(false);
  const [modVersionDialogLoading, setModVersionDialogLoading] = useState(false);
  const [modVersionOperation, setModVersionOperation] = useState<ModVersionOperationUpdate | null>(null);
  const [modVersionOperationRunning, setModVersionOperationRunning] = useState(false);
  // 事件监听是在挂载时注册的，用 ref 判断「当前打开的模态框属于哪个模组」，
  // 避免闭包里读到旧的对话框状态。
  const modVersionDialogRootRef = useRef<string | null>(null);
  // 版本操作在任务中心里的任务 id（事件回调是挂载时注册的，用 ref 关联）。
  const versionTaskIdRef = useRef<string | null>(null);
  const modVersionTaskIdRef = useRef<string | null>(null);
  // 「设置 › 日志」：启动器日志尾部。
  const [launcherLogTail, setLauncherLogTail] = useState<LauncherLogTail | null>(null);
  const [launcherLogLoading, setLauncherLogLoading] = useState(false);
  const [launcherLogError, setLauncherLogError] = useState<string | null>(null);
  const [launcherUpdate, setLauncherUpdate] = useState<LauncherUpdateStatus | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateCheckError, setUpdateCheckError] = useState<string | null>(null);
  // 「设置 › 关于 › 反馈信息」：弹窗状态与上传结果。
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const [feedbackClosing, setFeedbackClosing] = useState(false);
  const [feedbackDescription, setFeedbackDescription] = useState("");
  const [feedbackContact, setFeedbackContact] = useState("");
  const [feedbackSubmitting, setFeedbackSubmitting] = useState(false);
  const [feedbackError, setFeedbackError] = useState<string | null>(null);
  /** 本机记录过的反馈编号（设置 › 关于 ›「已提交的反馈」，新的在前）。 */
  const [feedbackHistory, setFeedbackHistory] = useState<FeedbackHistoryEntry[]>([]);
  const [feedbackHistoryOpen, setFeedbackHistoryOpen] = useState(false);
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
  const [luaModBatchAction, setLuaModBatchAction] = useState<"positive" | "negative" | "sort" | null>(null);
  const [selectedLuaModKeys, setSelectedLuaModKeys] = useState<Set<string>>(new Set());
  const [expandedLuaModKeys, setExpandedLuaModKeys] = useState<Set<string>>(new Set());
  const [luaModActionId, setLuaModActionId] = useState<string | null>(null);
  /** Lua 列表的入场滑入动画播完后置 true：之后换位（拖动、筛选）不再重播，详见 .pz-lua-mod-list.is-settled。 */
  const [luaModListSettled, setLuaModListSettled] = useState(false);
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
  /** Lua 模组拖动排序：拖动中的模组（小写 ID）、实时预览顺序与一起搬动的整组模组 ID。 */
  const [luaReorder, setLuaReorder] = useState<{ key: string; ids: string[]; group: string[] } | null>(null);
  const luaReorderRef = useRef<{
    key: string;
    ids: string[];
    group: string[];
    original: string[];
    /** 这次跟着指针一起走的行（多选时是整组）及其高度、相对指针的抓取偏移。 */
    rows: Array<{ element: HTMLElement; grabOffset: number; height: number }>;
    /** 块顶到指针的距离：块顶 = clientY - blockTopDelta，抓住的那一行因此始终贴着指针。 */
    blockTopDelta: number;
    /** 最近一次指针的 Y：换位导致布局变化后，用它把整组重新对齐到指针。 */
    lastClientY: number;
    /** 拖动所在的滚动框（靠近上下边缘时自动滚动）。 */
    scroller: HTMLElement | null;
    /** 上一次看到的滚动位置：滚动后要把整组重新对位到指针。 */
    lastScrollTop: number;
  } | null>(null);
  const [luaReorderSaving, setLuaReorderSaving] = useState(false);
  const luaReorderCommitRef = useRef<(ids: string[]) => void>(() => {});
  /** 列表顺序变化时的 FLIP 起点：渲染期间（DOM 更新前）抓的每行 offsetTop。 */
  const luaFlipRef = useRef<{ signature: string; tops: Map<string, number> } | null>(null);
  /** 松手后待放开的行：等最终顺序那次渲染提交之后，再统一滑回槽位。 */
  const luaDropRowsRef = useRef<Array<{ element: HTMLElement }> | null>(null);
  const [luaDropToken, setLuaDropToken] = useState(0);
  /** 拖动期间逐帧回调（自动滚动 + 对位）与它的 rAF 句柄。 */
  const luaDragTickRef = useRef<() => void>(() => {});
  const luaDragUpdateRef = useRef<() => void>(() => {});
  const luaDragFrameRef = useRef<number | null>(null);
  const [selectedJarModKeys, setSelectedJarModKeys] = useState<Set<string>>(new Set());
  const [expandedJarModKeys, setExpandedJarModKeys] = useState<Set<string>>(new Set());
  const [jarConflictExpanded, setJarConflictExpanded] = useState(false);
  const [jarModsLoading, setJarModsLoading] = useState(false);
  const [jarScanFrame, setJarScanFrame] = useState(0);
  const [jarScanProgress, setJarScanProgress] = useState({ scanned: 0, total: 0 });
  const jarScanIdRef = useRef<number | null>(null);
  // 启动后是否已经为 JAR 页面发起过首次扫描：再次进入页面不再自动扫描，改由「刷新 JAR 列表」手动触发。
  const jarModsScanStartedRef = useRef(false);
  const workshopBackgroundScanIdRef = useRef<number | null>(null);
  const [loadingMedia, setLoadingMedia] = useState(false);
  // 隐私协议同意流程：首次启动 / 升级后 / 更新源出现新版本时弹出；不同意则退出应用。
  const [privacyConsentedVersion, setPrivacyConsentedVersion] = useState<string | null>(null);
  const [privacyBusy, setPrivacyBusy] = useState(false);
  const [showPrivacyConsent, setShowPrivacyConsent] = useState(false);
  const [privacyAgreeCollect, setPrivacyAgreeCollect] = useState(false);
  const [privacyAgreeSend, setPrivacyAgreeSend] = useState(false);
  const [privacyTrapChecked, setPrivacyTrapChecked] = useState(false);
  // 隐私协议同意流程：首次启动 / 升级后 / 更新源出现新版本时弹出；不同意则退出应用。
const [preview, setPreview] = useState<PreviewState | null>(null);
  const [previewClosing, setPreviewClosing] = useState(false);
  const previewCloseTimerRef = useRef<number | null>(null);
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
  /** 拖动排序期间的预览顺序：按拖动中的 ids 重排（未参与排序的行保持原相对位置）。 */
  const luaReorderIndex = luaReorder
    ? new Map(luaReorder.ids.map((id, index) => [id.toLowerCase(), index]))
    : null;
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
        const sorted = luaModSortMode === "name"
          ? [...filtered].sort((left, right) =>
              left.name.localeCompare(right.name, "zh-CN", { sensitivity: "base" }),
            )
          : sortLuaModsForDisplay(filtered);
        if (!luaReorderIndex) {
          return sorted;
        }
        // Array.prototype.sort 稳定：未参与排序的行（未启用/被筛掉的加载器）保持原顺序排在后面。
        return [...sorted].sort(
          (left, right) =>
            (luaReorderIndex.get(left.id.toLowerCase()) ?? Number.MAX_SAFE_INTEGER) -
            (luaReorderIndex.get(right.id.toLowerCase()) ?? Number.MAX_SAFE_INTEGER),
        );
      })()
    : undefined;
  // 列表顺序变了（拖动换位、置顶/置底、启用/停用、排序）就走 FLIP：渲染期间、DOM 还没更新时
  // 先记下每行的 offsetTop 当起点，提交后再由下面的 useLayoutEffect 反向位移 + 过渡回位。
  // offsetTop 相对列表本身（.pz-lua-mod-list 是 position: relative），不受 transform 和滚动影响。
  const luaOrderSignature = (filteredLuaMods ?? []).map(luaModKey).join("\u0000");
  if (luaFlipRef.current?.signature !== luaOrderSignature) {
    const tops = new Map<string, number>();
    document
      .querySelectorAll<HTMLElement>("[data-lua-mod-key]")
      .forEach((row) => {
        const key = row.dataset.luaModKey;
        if (key) {
          tops.set(key, row.offsetTop);
        }
      });
    luaFlipRef.current = { signature: luaOrderSignature, tops };
  }
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
  /** 选中项里真正占加载顺序的模组数：多选拖动时整组搬，未启用/加载器不算数。 */
  const selectedEnabledLuaModCount = (luaMods ?? []).filter(
    (mod) => mod.enabled && !mod.isLoader && selectedLuaModKeys.has(luaModKey(mod)),
  ).length;
  const selectedJarModCount = selectedJarModKeys.size;
  function dismissToast(id: number) {
    setToasts((current) => current.map((toast) => (toast.id === id ? { ...toast, closing: true } : toast)));
    window.setTimeout(() => setToasts((current) => current.filter((toast) => toast.id !== id)), 180);
  }
  function showToast(message: string, kind: ToastKind = "info") {
    const id = ++toastIdRef.current;
    if (kind === "error" && isTauriRuntime()) {
      void invoke("log_frontend_message", { level: "error", message }).catch(() => undefined);
    }
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
            : status.devBuild
              ? `当前是内测版本 DevBuild ${status.devBuildId}。`
              : status.unknownVersion
                ? `当前是未知版本（v${status.currentVersion}），比更新源发布的还新。`
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
      }>("submit_feedback", {
        description: feedbackDescription,
        // 联系方式选填：空串按"没填"传给后端，不会拼进描述。
        contact: feedbackContact.trim() || null,
      });
      setFeedbackResult(result);
      setFeedbackDescription("");
      setFeedbackContact("");
      void loadFeedbackHistory();
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

  /** 已提交的反馈编号（本机记录，读失败就当空列表）。 */
  async function loadFeedbackHistory() {
    try {
      setFeedbackHistory(await invoke<FeedbackHistoryEntry[]>("list_submitted_feedback"));
    } catch {
      setFeedbackHistory([]);
    }
  }

  function copyFeedbackId(id: string) {
    void writeText(id)
      .then(() => showToast(`已复制反馈编号 ${id}`, "success"))
      .catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
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

  /** 关闭任务中心：播完退出动画再真正卸载，动画期间忽略重复关闭。 */
  function closeTaskPanel() {
    if (!taskPanelOpen || taskPanelClosing) {
      return;
    }
    setTaskPanelClosing(true);
    if (taskPanelCloseTimerRef.current !== null) {
      clearTimeout(taskPanelCloseTimerRef.current);
    }
    taskPanelCloseTimerRef.current = setTimeout(() => {
      setTaskPanelOpen(false);
      setTaskPanelClosing(false);
      taskPanelCloseTimerRef.current = null;
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  /** 跳到「设置 › 模组加载器」：支持库、模组管理里的指路按钮都走这里。 */
  function openModLoaderSettings() {
    switchSettingsPage("modLoader");
    navigateToPage("settings");
  }

  /** 设置内切换分页：记录滑动方向（按侧栏顺序），供换页动画判断前进/后退。 */
  function switchSettingsPage(nextPage: SettingsPage) {
    setSettingsPage((current) => {
      if (current !== nextPage) {
        const order: SettingsPage[] = ["about", "general", "toolbox", "logs", "modLoader", "startup"];
        setSettingsTransitionDirection(
          order.indexOf(nextPage) >= order.indexOf(current) ? "forward" : "backward",
        );
      }
      return nextPage;
    });
  }

  /** 「版本」页切换左侧分区：按列表顺序记录前进/后退，内容区据此播放方向性滑入。 */
  function switchVersionTab(nextTab: VersionTab) {
    setVersionTab((current) => {
      if (current !== nextTab) {
        const order: VersionTab[] = ["game", "mods", "history", "support", "shaderpacks", "shots"];
        setVersionTransitionDirection(
          order.indexOf(nextTab) >= order.indexOf(current) ? "forward" : "backward",
        );
      }
      return nextTab;
    });
  }

  function navigateToPage(nextPage: ActivePage) {
    // 支持库 / 截图 / 光影包已并入「版本」页的分区：旧的页面入口统一重定向过去，
    // 保证「去支持库安装」这类既有跳转仍能落到正确位置。
    if (nextPage === "overview" || nextPage === "shots" || nextPage === "shaderpacks") {
      switchVersionTab(nextPage === "overview" ? "support" : nextPage);
      nextPage = "versions";
    }
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

  // 隐私协议弹窗判定（派生状态，三个触发合一）：
  // ① 从未同意；② 同意版本落后于当前运行版本（升级后首次启动）；
  // ③ 更新源出现了新版本，且其对应的隐私政策还没同意过（更新前重新同意）。
  // 同意后三条件同时不成立，弹窗自动收敛消失。
  useEffect(() => {
    const current = appManifest.version;
    const needConsent =
      !privacyConsentedVersion ||
      compareVersionStrings(privacyConsentedVersion, current) < 0 ||
      Boolean(
        launcherUpdate?.updateAvailable &&
          launcherUpdate.latestVersion !== privacyConsentedVersion,
      );
    setShowPrivacyConsent(needConsent);
  }, [privacyConsentedVersion, launcherUpdate]);

  useEffect(() => {
    return () => {
      if (pageTransitionTimerRef.current !== null) {
        clearTimeout(pageTransitionTimerRef.current);
      }
      if (taskPanelCloseTimerRef.current !== null) {
        clearTimeout(taskPanelCloseTimerRef.current);
      }
      if (previewCloseTimerRef.current !== null) {
        clearTimeout(previewCloseTimerRef.current);
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
      closeTaskPanel();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeTaskPanel();
      }
    };
    document.addEventListener("mousedown", closeOnOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("mousedown", closeOnOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
    // closeTaskPanel 是稳定的普通函数声明，这里只关心开关状态。
  }, [taskPanelOpen, taskPanelClosing]);

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
      // 只有真正发起了扫描才记为「本次运行已扫描」：加载器没装时提前返回，
      // 装好后再进入页面仍会自动扫描一次。
      jarModsScanStartedRef.current = true;
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

  /** 「自动排序」：按依赖与分类重排全部已启用 Lua 模组的加载顺序，可选先备份预设。 */
  async function autoSortLuaMods() {
    if (luaModBatchAction) return;
    const proceed = window.confirm(
      "自动排序会按依赖与分类重排所有已启用 Lua 模组的加载顺序（参考 Mod Load Order Sorter 的规则：加载器最前 → loadFirst → 分类归堆 → loadLast → 依赖拓扑）。\n\n确定继续吗？",
    );
    if (!proceed) return;
    const backup = window.confirm(
      "是否先备份当前预设？\n\n备份会把当前的启用列表与顺序保存成一个预设，之后可随时一键切回。\n\n[确定] 先备份再排序　[取消] 不备份直接排序",
    );
    setLuaModBatchAction("sort");
    try {
      const backupName = backup
        ? `自动排序前备份 ${new Date().toLocaleString("zh-CN", { hour12: false })}`
        : null;
      const result = await invoke<AutoSortLuaModsResult>("auto_sort_lua_mods", { backupName });
      await loadLuaMods();
      const backupText = result.backupPreset ? `，已保存备份预设「${result.backupPreset}」` : "";
      const moveText = result.moved > 0 ? `${result.moved} / ${result.total} 个模组的顺序有调整` : "顺序已经是最佳，无需调整";
      showToast(`自动排序完成：${moveText}${backupText}`, "success");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setLuaModBatchAction(null);
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

  /** 参与手动排序的已启用模组 ID：加载器（ZombieBuddy 等）固定在 default.txt 最前，不参与排序。 */
  function movableLuaModIds() {
    return (luaMods ?? [])
      .filter((entry) => entry.enabled && entry.loadOrder !== null && !entry.isLoader)
      .sort((left, right) => (left.loadOrder ?? 0) - (right.loadOrder ?? 0))
      .map((entry) => entry.id);
  }

  /** 把可排序部分的顺序写回完整已启用清单：加载器保持原位，其余按 ids 依次填回去。 */
  function composeLuaModOrder(ids: string[]) {
    let cursor = 0;
    return (luaMods ?? [])
      .filter((entry) => entry.enabled && entry.loadOrder !== null)
      .sort((left, right) => (left.loadOrder ?? 0) - (right.loadOrder ?? 0))
      .map((entry) => (entry.isLoader ? entry.id : ids[cursor++] ?? entry.id));
  }

  /** 把新的加载顺序乐观写进本地数据（只重排 loadOrder，加载器位置不变）。
      拖动松手时要先做这一步再撤预览，否则列表会先回弹到旧顺序、等后端保存完再滑过来。 */
  function applyLuaModOrder(ids: string[]) {
    setLuaMods((current) => {
      if (!current) {
        return current;
      }
      const enabled = current
        .filter((entry) => entry.enabled && entry.loadOrder !== null)
        .sort((left, right) => (left.loadOrder ?? 0) - (right.loadOrder ?? 0));
      let cursor = 0;
      const full = enabled.map((entry) => (entry.isLoader ? entry.id : ids[cursor++] ?? entry.id));
      const order = new Map(full.map((id, index) => [id.toLowerCase(), index + 1]));
      return current.map((entry) => {
        const next = order.get(entry.id.toLowerCase());
        return next === undefined || !entry.enabled ? entry : { ...entry, loadOrder: next };
      });
    });
  }

  /** 后端会按 requires / loadAfter / loadBefore 重新约束顺序，被驳回时给用户一句话，而不是静默无反应。 */
  function reportLuaModOrderResult(requested: string[], applied: string[]) {
    if (requested.length === 0) return;
    const requestedKeys = new Set(requested.map((id) => id.toLowerCase()));
    const appliedOrder = applied.filter((id) => requestedKeys.has(id.toLowerCase()));
    const blocked = requested.find(
      (id, index) => appliedOrder[index]?.toLowerCase() !== id.toLowerCase(),
    );
    if (blocked === undefined) return;
    const name =
      luaMods?.find((entry) => entry.id.toLowerCase() === blocked.toLowerCase())?.name ?? blocked;
    showToast(`「${name}」没能移动到该位置：前置模组（requires / loadBefore）限定了加载顺序。`, "info");
  }

  /** 一键置顶 / 置底：把这一个模组挪到可排序列表的最前 / 最后，其余模组相对顺序不变。 */
  async function moveLuaModToEdge(mod: LuaModEntry, edge: "top" | "bottom") {
    if (!luaMods || luaModActionId || luaReorderSaving || luaModBatchAction) return;
    const ids = movableLuaModIds();
    const index = ids.findIndex((entry) => entry.toLowerCase() === mod.id.toLowerCase());
    if (index < 0) return;
    const target = edge === "top" ? 0 : ids.length - 1;
    if (index === target) return;
    const [moved] = ids.splice(index, 1);
    ids.splice(target, 0, moved);
    setLuaModActionId(luaModKey(mod));
    // 本地先排好，后端回来前列表就已经在新位置（不会先停一下再滑）。
    applyLuaModOrder(ids);
    try {
      const applied = await invoke<string[]>("save_lua_mod_order", {
        modIds: composeLuaModOrder(ids),
      });
      reportLuaModOrderResult(ids, applied);
      await loadLuaMods();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      await loadLuaMods();
    } finally {
      setLuaModActionId(null);
    }
  }

  /** 抓住行左侧的拖动柄开始排序：拖动期间只改前端预览顺序，松手才写 default.txt。
      抓住的那一行 1:1 跟着指针上下走（不占位时可读成「手里拿着这一行」），
      若它在多选里，则整组选中模组一起搬动，组内相对顺序保持不变。 */
  function beginLuaModReorder(event: ReactPointerEvent<HTMLElement>, mod: LuaModEntry) {
    if (event.button !== 0 || luaModSortMode !== "load") return;
    if (!luaMods || luaModActionId || luaReorderSaving || luaModBatchAction) return;
    if (!mod.enabled || mod.isLoader) return;
    const ids = movableLuaModIds();
    const key = mod.id.toLowerCase();
    const index = ids.findIndex((entry) => entry.toLowerCase() === key);
    if (index < 0) return;
    const row = (event.currentTarget as HTMLElement).closest<HTMLElement>("[data-lua-mod-key]");
    if (!row) return;
    // 选中的模组里有未启用/加载器时不参与搬动，只搬真正占加载顺序的那些。
    const rowKeyById = new Map((luaMods ?? []).map((entry) => [entry.id.toLowerCase(), luaModKey(entry)]));
    const selectedIds = ids.filter((id) => {
      const rowKey = rowKeyById.get(id.toLowerCase());
      return rowKey !== undefined && selectedLuaModKeys.has(rowKey);
    });
    const group = selectedLuaModKeys.has(luaModKey(mod)) && selectedIds.length > 1
      ? selectedIds
      : [mod.id];
    event.preventDefault();
    event.stopPropagation();
    suppressRowClickRef.current = true;
    // 保险：即使入场动画还在播，也立刻停掉，拖动过程中不能有行闪一下。
    setLuaModListSettled(true);
    // 整组（单行时就是这一行）都不走 CSS 过渡：位移由指针直接驱动，避免跟手时被过渡拖慢。
    const rowsByKey = new Map<string, HTMLElement>();
    document.querySelectorAll<HTMLElement>("[data-lua-mod-key]").forEach((element) => {
      const rowKey = element.dataset.luaModKey;
      if (rowKey) {
        rowsByKey.set(rowKey, element);
      }
    });
    const rows = group
      .map((id) => rowsByKey.get(id.toLowerCase()))
      .filter((element): element is HTMLElement => element !== undefined)
      .map((element) => {
        element.style.transition = "none";
        return {
          element,
          grabOffset: event.clientY - element.getBoundingClientRect().top,
          height: element.offsetHeight,
        };
      });
    if (rows.length === 0) return;
    // 整组会堆成一个连续块往前搬（原本分散的选中项也靠到一起）。
    // 让被抓住的那一行仍然贴着指针：块顶 = 抓住行顶 - 它上面的组员总高。
    const grabbedIndex = Math.max(0, rows.findIndex((entry) => entry.element === row));
    const aboveHeight = rows
      .slice(0, grabbedIndex)
      .reduce((sum, entry) => sum + entry.height, 0);
    const scroller = scrollParentOf(row);
    luaReorderRef.current = {
      key,
      ids,
      group,
      original: ids,
      rows,
      blockTopDelta: rows[grabbedIndex].grabOffset + aboveHeight,
      lastClientY: event.clientY,
      scroller,
      lastScrollTop: scroller?.scrollTop ?? 0,
    };
    setLuaReorder({ key, ids, group });
    // 立刻按当前指针位置把整组摞起来，然后启动逐帧循环（指针停住时也要继续自动滚动/对位）。
    luaDragUpdateRef.current();
    luaDragTickRef.current();
  }

  async function commitLuaModReorder(ids: string[]) {
    if (!luaMods) return;
    setLuaReorderSaving(true);
    try {
      const applied = await invoke<string[]>("save_lua_mod_order", {
        modIds: composeLuaModOrder(ids),
      });
      reportLuaModOrderResult(ids, applied);
      await loadLuaMods();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      await loadLuaMods();
    } finally {
      setLuaReorderSaving(false);
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

  /** 极速启动（实验性）：开关落盘并维护启动文件备份；真正的缓存生成与注入在启动/扫描流程里按这个开关走。
      `revertTo` 是失败时回退到状态：从警告弹窗确认开启时，点击瞬间还没落盘，
      失败应该回到关闭而不是回到点击后的临时状态。 */
  async function updateFastLaunch(enabled: boolean, revertTo = fastLaunch) {
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
      setFastLaunch(revertTo);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setFastLaunchSaving(false);
    }
  }

  /** 关闭极速启动的警告弹窗；confirm = 用户选了「仍要开启」。 */
  function closeFastLaunchWarning(confirm: boolean) {
    if (!fastLaunchWarningOpen || fastLaunchWarningClosing) return;
    setFastLaunchWarningClosing(true);
    window.setTimeout(() => {
      setFastLaunchWarningOpen(false);
      setFastLaunchWarningClosing(false);
      if (confirm) {
        // 点击开关时只是临时把开关状态设为开，真正落盘在这里；失败回到关闭。
        void updateFastLaunch(true, false);
      } else {
        // 先临时设为开是为了让受控开关跟着点击走；取消时退回关闭，不落盘。
        setFastLaunch(false);
      }
    }, RESOURCE_DIALOG_EXIT_MS);
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

  /**
   * 激进内存清理：只落盘 + 通知后端。之后的手动清理、静默清理、启动前清理都会带上系统级步骤
   * （全系统工作集 + 文件缓存 + 已修改页 + 待机列表），静默清理在游戏运行时仍只用常规模式。
   */
  async function updateAggressiveMemoryClean(enabled: boolean) {
    const previous = aggressiveMemoryClean;
    setAggressiveMemoryClean(enabled);
    setAggressiveMemoryCleanSaving(true);
    try {
      await invoke("save_aggressive_memory_clean", { enabled });
    } catch (error: unknown) {
      setAggressiveMemoryClean(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setAggressiveMemoryCleanSaving(false);
    }
  }

  /**
   * 以管理员身份重启启动器（激进清理的系统级步骤需要管理员特权）。
   * UAC 被取消时后端返回错误，当前实例继续运行。
   */
  async function restartLauncherAsAdmin() {
    if (restartingAsAdmin) {
      return;
    }
    setRestartingAsAdmin(true);
    try {
      await invoke("restart_as_admin");
    } catch (error: unknown) {
      setRestartingAsAdmin(false);
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /**
   * 「立即清理一次」：登记成任务中心的一条任务，边清理边采样物理内存。
   *
   * 后端清理本身只有 100–300 ms，但工作集页回到系统还要一点时间：
   * 已用物理内存会先下降、随后被系统重新占用而回升。任务行实时显示
   * 「当前已清理 X MB」（相对点击前基准的最大下降量），并在先降后升时判定完成。
   */
  async function cleanMemoryNow() {
    if (memoryCleanTaskRunningRef.current) {
      return;
    }
    memoryCleanTaskRunningRef.current = true;
    setMemoryCleaning(true);

    const taskId = "memory-task-" + (++taskIdRef.current);
    setTasks((current) => [
      {
        id: taskId,
        kind: "memory-clean",
        title: "内存清理",
        detail: "正在清理…",
        status: "running",
        downloaded: 0,
        total: null,
        createdAt: Date.now(),
        finishedAt: null,
        error: null,
        savedPath: null,
        request: null,
        freedMb: 0,
        usedPercent: null,
      },
      ...current,
    ]);
    setTaskPanelOpen(true);

    // 基准要在清理前取：之后所有「已清理」都相对它计算。
    const baseline = await invoke<MemoryStatus>("get_memory_status").catch(() => null);
    const fallbackBaseline =
      memoryStatus !== null ? memoryStatus.totalMb - memoryStatus.availableMb : 0;
    let tracker = createMemoryCleanTracker(
      baseline !== null ? baseline.totalMb - baseline.availableMb : fallbackBaseline,
    );
    const startedAt = Date.now();
    let lastDecreaseAt = startedAt;
    let lastUsedPercent: number | null = null;
    let cleanReturned = false;
    let monitorCancelled = false;

    const sampleOnce = async () => {
      const status = await invoke<MemoryStatus>("get_memory_status").catch(() => null);
      if (status === null || monitorCancelled) {
        return;
      }
      setMemoryStatus(status);
      lastUsedPercent = status.usedPercent;
      const previousPeak = tracker.peakFreedMb;
      tracker = applyMemoryCleanSample(tracker, {
        usedMb: status.totalMb - status.availableMb,
        usedPercent: status.usedPercent,
      });
      if (tracker.peakFreedMb > previousPeak) {
        lastDecreaseAt = Date.now();
      }
      updateTask(taskId, {
        detail: memoryCleanProgressDetail(tracker.peakFreedMb),
        freedMb: tracker.peakFreedMb,
        usedPercent: status.usedPercent,
      });
    };

    // 清理命令还没返回时也继续采样，但只有返回后才允许按「回升」收尾。
    const monitor = (async () => {
      while (
        !monitorCancelled &&
        !shouldFinishMemoryClean(tracker, {
          startedAt,
          lastDecreaseAt,
          now: Date.now(),
          cleanReturned,
        })
      ) {
        await new Promise((resolve) => window.setTimeout(resolve, MEMORY_CLEAN_POLL_MS));
        await sampleOnce();
      }
    })();

    try {
      const report = await invoke<MemoryCleanupReport>("clean_memory_now");
      cleanReturned = true;
      await monitor;
      // 任务行看到的是整个回落过程的最大释放量；设置页的「上次清理」用同一个数字，避免两处对不上。
      const freedMb = Math.max(report.freedMb, tracker.peakFreedMb);
      setLastCleanupReport({ ...report, freedMb });
      updateTask(taskId, {
        status: "success",
        detail: memoryCleanFinishedDetail(freedMb, report.trimmed),
        freedMb,
        usedPercent: lastUsedPercent,
        finishedAt: Date.now(),
      });
    } catch (error: unknown) {
      cleanReturned = true;
      monitorCancelled = true;
      const message = error instanceof Error ? error.message : String(error);
      updateTask(taskId, {
        status: "error",
        detail: message,
        error: message,
        finishedAt: Date.now(),
      });
      showToast(message, "error");
    } finally {
      memoryCleanTaskRunningRef.current = false;
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

  /**
   * 覆盖层背景色：只落盘（真正生效在下次启动游戏、启动器写状态文件时），
   * 拖动系统颜色选择器会连续触发，因此同样做 250ms 防抖。
   */
  function updateOverlayBackgroundColor(nextValue: string) {
    setOverlayBackgroundColor(nextValue);
    if (overlayBackgroundSaveTimerRef.current !== null) {
      window.clearTimeout(overlayBackgroundSaveTimerRef.current);
    }
    overlayBackgroundSaveTimerRef.current = window.setTimeout(() => {
      overlayBackgroundSaveTimerRef.current = null;
      void invoke("save_overlay_background_color", { value: nextValue }).catch((error: unknown) => {
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
    // 管理员状态不会中途变化，问一次即可；拿来给激进清理提示权限不足。
    invoke<boolean>("is_launcher_elevated")
      .then((elevated) => {
        if (active) {
          setLauncherElevated(elevated);
        }
      })
      .catch(() => {
        // 读不到就按"未知"处理，不显示提权引导。
      });
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
          setPrivacyConsentedVersion(settingsResult.value.privacyConsentedVersion ?? null);
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
          setOverlayBackgroundColor(
            settingsResult.value.overlayBackgroundColor ?? DEFAULT_OVERLAY_BACKGROUND_COLOR,
          );
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
          setAggressiveMemoryClean(Boolean(settingsResult.value.aggressiveMemoryClean));
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
          setVersionParallel(settingsResult.value.versionParallel ?? true);
          setVersionIncludeMods(settingsResult.value.versionIncludeMods ?? true);
          setVersionDriveNoticeDismissed(
            Boolean(settingsResult.value.versionSystemDriveNoticeDismissed),
          );
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
  // 版本操作也可能从「模组管理」右键的版本记录里发起，进设置页之前就要知道
  // 备份位置是否在 C 盘，所以挂载时先取一次实际路径。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    void invoke<string>("get_version_store_path")
      .then((path) => setVersionStorePath(path))
      .catch(() => undefined);
  }, []);
  // 「版本」顶层页：进入页面/切分页时读取对应数据（游戏版本与历史版本共用版本状态）。
  useEffect(() => {
    if (activePage !== "versions") return;
    if (versionTab === "mods") {
      void loadModVersionTargets();
      void loadModVersionPacks();
      return;
    }
    // 支持库 / 光影包 / 截图 不依赖版本状态：媒体数据启动时就已读好，不再重复刷新。
    if (versionTab === "support" || versionTab === "shaderpacks" || versionTab === "shots") {
      return;
    }
    // 还原会往游戏目录写临时文件，操作期间不扫；完成事件会触发刷新。
    if (versionOperationRunning || versionStatus?.operationRunning) return;
    void refreshVersionStatus();
  }, [activePage, versionTab]);

  // 「设置 › 日志」：进入页面时读取一次启动器日志尾部。
  useEffect(() => {
    if (settingsPage !== "logs") return;
    void refreshLauncherLog();
  }, [settingsPage]);

  // 「设置 › 关于」：读取本机提交过的反馈编号，给「已提交的反馈」列表用。
  useEffect(() => {
    if (activePage !== "settings" || settingsPage !== "about") return;
    void loadFeedbackHistory();
  }, [activePage, settingsPage]);

  // 前端未捕获错误写进启动器日志，用户可以直接导日志排查。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const onError = (event: ErrorEvent) => {
      void invoke("log_frontend_message", {
        level: "error",
        message: `window.onerror: ${event.message} (${event.filename}:${event.lineno}:${event.colno})`,
      }).catch(() => undefined);
    };
    const onRejection = (event: PromiseRejectionEvent) => {
      const reason =
        event.reason instanceof Error
          ? (event.reason.stack ?? event.reason.message)
          : String(event.reason);
      void invoke("log_frontend_message", {
        level: "error",
        message: `unhandledrejection: ${reason}`,
      }).catch(() => undefined);
    };
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);
    return () => {
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    };
  }, []);

  // 版本操作在后台线程执行，用 version-progress 推送进度；结束后自动刷新状态。
  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return;
    let unlisten: (() => void) | undefined;
    let unlistenVerified: (() => void) | undefined;
    // StrictMode 下 effect 会挂载→卸载→再挂载：listen 的 Promise 还没 resolve 时
    // cleanup 就跑过了，如果不在这里补一次 unlisten，会留下一个重复监听（Toast 会弹两次）。
    let disposed = false;
    void listen<VersionOperationUpdate>("version-progress", (event) => {
      const update = event.payload;
      setVersionOperation(update);
      const etaSeconds = updateVersionEta(versionEtaTrackerRef.current, update);
      setVersionTiming({ startedAt: versionEtaTrackerRef.current.startedAt, etaSeconds });
      // 同步到任务中心那一行（进行中 / 完成 / 失败）。
      const taskId = versionTaskIdRef.current;
      if (taskId) {
        applyVersionTaskProgress(taskId, update, etaSeconds);
        if (update.status === "completed" || update.status === "error") {
          versionTaskIdRef.current = null;
        }
      }
      if (update.status === "started" || update.status === "progress") {
        setVersionOperationRunning(true);
        return;
      }
      setVersionOperationRunning(false);
      // 任何会改变游戏目录的操作完成后，之前那次“已校验”结论就作废了。
      if (update.status === "completed" && update.operation !== "verify") {
        setVersionVerified(null);
      }
      if (update.status === "completed") {
        showToast(update.message ?? "版本操作完成", "success");
        void refreshVersionStatus();
      } else if (update.status === "error") {
        const message = update.error ?? "版本操作失败";
        showToast(message, message.includes("取消") ? "info" : "error");
        void refreshVersionStatus();
      }
      // 还原会卸载/重装 PZ Optimization，刷新检测让「支持库」里的状态同步。
      if (update.operation === "restore") {
        void refreshDetection();
      }
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => undefined);
    void listen<VersionVerifyResult>("version-verified", (event) => {
      setVersionVerified(event.payload);
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlistenVerified = cleanup;
    }).catch(() => undefined);
    // 还原完成但 PZ Optimization 未能自动重装：后端单独发警告事件，弹模态框提示。
    void listen<{ error: string }>("version-restore-pzopt-warning", (event) => {
      setPzOptRestoreWarning(event.payload.error);
    }).then((cleanup) => {
      if (disposed) cleanup();
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
      unlistenVerified?.();
    };
  }, []);
  // 模组版本操作走独立事件通道：只刷新对应模组的状态，不污染游戏分页。
  useEffect(() => {
    if (!(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<ModVersionOperationUpdate>("mod-version-progress", (event) => {
      const update = event.payload;
      setModVersionOperation(update);
      const etaSeconds = updateVersionEta(versionEtaTrackerRef.current, update);
      setVersionTiming({ startedAt: versionEtaTrackerRef.current.startedAt, etaSeconds });
      const taskId = modVersionTaskIdRef.current;
      if (taskId) {
        applyVersionTaskProgress(taskId, update, etaSeconds);
        if (update.status === "completed" || update.status === "error") {
          modVersionTaskIdRef.current = null;
        }
      }
      if (update.status === "started" || update.status === "progress") {
        setModVersionOperationRunning(true);
        return;
      }
      setModVersionOperationRunning(false);
      const rootPath = update.rootPath ?? null;
      if (rootPath) {
        void loadModVersionStatus(rootPath);
        void loadModVersionTargets();
      }
      // 整合包操作没有 rootPath：完成后刷新整合包列表和模组列表。
      if (
        update.status === "completed" &&
        (update.operation === "modSaveAll" || update.operation === "modRestorePack")
      ) {
        void loadModVersionPacks();
        void loadModVersionTargets();
        setModVersionStatuses({});
      }
      if (update.status === "completed") {
        showToast(update.message ?? "模组版本操作完成", "success");
      } else if (update.status === "error") {
        const message = update.error ?? "模组版本操作失败";
        showToast(message, message.includes("取消") ? "info" : "error");
      }
    }).then((cleanup) => {
      if (disposed) cleanup();
      else unlisten = cleanup;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlisten?.();
    };
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

  // 始终指向最新一次渲染里的提交函数，避免全局监听拿到旧的 luaMods 闭包。
  useEffect(() => {
    luaReorderCommitRef.current = (ids) => {
      void commitLuaModReorder(ids);
    };
  });

  // Lua 模组拖动排序：按「其它行中线」判断落点，实时换位；松手时一次性写回 default.txt。
  // 多选时整组一起搬：落点行只要不属于这组，就把整组插到它前/后，组内相对顺序不变。
  useEffect(() => {
    const SCROLL_EDGE = 56;
    const SCROLL_MAX_STEP = 18;

    // 指针靠近滚动框上下边缘时自动滚动；越靠边越快，最少 1px/帧。
    const autoScroll = (drag: NonNullable<typeof luaReorderRef.current>) => {
      const scroller = drag.scroller;
      if (!scroller) {
        return;
      }
      const rect = scroller.getBoundingClientRect();
      const y = drag.lastClientY;
      let step = 0;
      if (y < rect.top + SCROLL_EDGE) {
        step = -((rect.top + SCROLL_EDGE - y) / SCROLL_EDGE) * SCROLL_MAX_STEP;
      } else if (y > rect.bottom - SCROLL_EDGE) {
        step = ((y - (rect.bottom - SCROLL_EDGE)) / SCROLL_EDGE) * SCROLL_MAX_STEP;
      }
      if (step === 0) {
        return;
      }
      const clamped = Math.max(-SCROLL_MAX_STEP, Math.min(SCROLL_MAX_STEP, step));
      const delta = Math.abs(clamped) < 1 ? Math.sign(clamped) : Math.round(clamped);
      scroller.scrollTop += delta;
    };

    /** 按最后的指针位置重排手里的整组并重新判断落点。
        每帧都要跑（指针不动时滚动框也会动），所以位移完全以指针坐标为准。
        坐标一律用「内容坐标」：行的 offsetTop（相对列表，不受 transform / 滚动影响）。 */
    const update = (drag: NonNullable<typeof luaReorderRef.current>) => {
      // 内容原点 = 列表可视区顶部在视口里的位置 − 当前滚动量。
      const origin = drag.rows[0]?.element.offsetParent as HTMLElement | null;
      const contentOrigin = origin ? origin.getBoundingClientRect().top - origin.scrollTop : 0;
      const blockTop = drag.lastClientY - contentOrigin - drag.blockTopDelta;
      let stackedTop = blockTop;
      for (const entry of drag.rows) {
        entry.element.style.transform = `translateY(${stackedTop - entry.element.offsetTop}px)`;
        stackedTop += entry.height;
      }
      const groupKeys = new Set(drag.group.map((id) => id.toLowerCase()));
      const rows = Array.from(
        document.querySelectorAll<HTMLElement>('[data-lua-mod-sortable="true"]'),
      )
        .filter((row) => !groupKeys.has(row.dataset.luaModKey ?? ""))
        .map((row) => ({
          id: row.dataset.luaModId ?? "",
          top: row.offsetTop,
          height: row.offsetHeight,
        }))
        .filter((row) => row.id !== "")
        .sort((left, right) => left.top - right.top);
      if (rows.length === 0) {
        return;
      }
      const next = rows.find((row) => blockTop <= row.top + row.height / 2);
      const previous = [...rows]
        .reverse()
        .find((row) => blockTop > row.top + row.height / 2);
      const ids = previous
        ? moveModIdBlock(drag.ids, drag.group, previous.id, true)
        : next
          ? moveModIdBlock(drag.ids, drag.group, next.id, false)
          : null;
      if (!ids || ids.join("\u0000") === drag.ids.join("\u0000")) {
        return;
      }
      drag.ids = ids;
      setLuaReorder({ key: drag.key, ids, group: drag.group });
    };

    // 拖动期间逐帧跑：指针停住时靠这里继续自动滚动；滚动（自动或用户滚轮）后重新对位。
    // 没滚动就不重算什么，避免每帧扫描整个列表。
    const tick = () => {
      const drag = luaReorderRef.current;
      if (!drag) {
        return;
      }
      autoScroll(drag);
      const scrollTop = drag.scroller?.scrollTop ?? 0;
      if (scrollTop !== drag.lastScrollTop) {
        drag.lastScrollTop = scrollTop;
        update(drag);
      }
      luaDragFrameRef.current = window.requestAnimationFrame(tick);
    };
    luaDragTickRef.current = tick;
    luaDragUpdateRef.current = () => {
      const drag = luaReorderRef.current;
      if (drag) {
        update(drag);
      }
    };

    const handleMove = (event: PointerEvent) => {
      const drag = luaReorderRef.current;
      if (!drag) {
        return;
      }
      drag.lastClientY = event.clientY;
      update(drag);
    };
    const stop = (commit: boolean) => {
      const drag = luaReorderRef.current;
      luaReorderRef.current = null;
      if (luaDragFrameRef.current !== null) {
        window.cancelAnimationFrame(luaDragFrameRef.current);
        luaDragFrameRef.current = null;
      }
      if (!drag) {
        setLuaReorder(null);
        return;
      }
      const changed = drag.ids.join("\u0000") !== drag.original.join("\u0000");
      if (commit && changed) {
        // 先把预览顺序写进本地数据，再撤掉预览。否则撤预览的这一刻列表会先回弹到
        // 旧顺序（后端还没保存完），等 loadLuaMods 回来再 FLIP 到新位置——就是"转一圈"。
        applyLuaModOrder(drag.ids);
      }
      setLuaReorder(null);
      // 手里那几行的内联位移**先别清**：等这次渲染（最终顺序）提交之后再由 layout effect
      // 统一放开，动画目标就是最终槽位。在事件里直接清的话，目标还是渲染前的旧布局，
      // 一旦顺序在同一帧里变了就会先滑到旧位置再"绕"到新位置。
      luaDropRowsRef.current = drag.rows;
      setLuaDropToken((token) => token + 1);
      // 拖动结束后的那一次 click 不应被当成勾选行。
      suppressRowClickRef.current = true;
      window.setTimeout(() => {
        suppressRowClickRef.current = false;
      }, 0);
      if (commit && changed) {
        luaReorderCommitRef.current(drag.ids);
      }
    };
    const handleUp = () => stop(true);
    const handleCancel = () => stop(false);
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") stop(false);
    };
    // 任何祖先滚动（滚轮、滚动链、程序滚动）都重新对位一次，保证手里的整块一直贴在指针上。
    const handleScroll = () => {
      luaDragUpdateRef.current();
    };
    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
    window.addEventListener("pointercancel", handleCancel);
    window.addEventListener("keydown", handleKey);
    window.addEventListener("scroll", handleScroll, true);
    return () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
      window.removeEventListener("pointercancel", handleCancel);
      window.removeEventListener("keydown", handleKey);
      window.removeEventListener("scroll", handleScroll, true);
      if (luaDragFrameRef.current !== null) {
        window.cancelAnimationFrame(luaDragFrameRef.current);
        luaDragFrameRef.current = null;
      }
    };
  }, []);

  // 换位后行在文档流里的位置变了：手里的整组按新布局重新对齐到指针，
  // 其它行用 FLIP 滑到新槽位（拖动中被抓的那几行由指针直接驱动，跳过）。
  useLayoutEffect(() => {
    const snapshot = luaFlipRef.current;
    if (!snapshot) {
      return;
    }
    const draggingKeys = new Set((luaReorder?.group ?? []).map((id) => id.toLowerCase()));
    // 松手后还没放开的行由下面那个 effect 统一处理（目标必须是最终槽位），这里跳过。
    const droppingRows = new Set(luaDropRowsRef.current?.map(({ element }) => element) ?? []);
    const animated: HTMLElement[] = [];
    document.querySelectorAll<HTMLElement>("[data-lua-mod-key]").forEach((row) => {
      const key = row.dataset.luaModKey;
      if (!key || draggingKeys.has(key) || droppingRows.has(row)) {
        return;
      }
      const previous = snapshot.tops.get(key);
      if (previous === undefined) {
        return;
      }
      const delta = previous - row.offsetTop;
      if (Math.abs(delta) < 0.5) {
        return;
      }
      row.style.transition = "none";
      row.style.transform = `translateY(${delta}px)`;
      animated.push(row);
    });
    // 先把手里的整组按最后的指针位置重新对齐（换位改变了它们的流内位置）。
    if (luaReorder) {
      luaDragUpdateRef.current();
    }
    if (animated.length === 0) {
      return;
    }
    // 读一次布局把反向位移提交成过渡起点，同一帧再清掉 → 由 .pz-mod-row 的 transform 过渡滑回原位。
    void document.body.offsetHeight;
    animated.forEach((row) => {
      row.style.transition = "";
      row.style.transform = "";
    });
  }, [luaOrderSignature]);

  // 松手后的收尾：等「最终顺序」这次渲染提交之后再放开手里的行，
  // 用「当前视口位置 → 最终槽位」做一次 FLIP。在 pointerup 里直接清 transform 的话，
  // 过渡目标还是渲染前的布局，顺序一旦同帧变化就会先滑回旧位置再绕到新位置。
  useLayoutEffect(() => {
    const rows = luaDropRowsRef.current;
    luaDropRowsRef.current = null;
    if (!rows || rows.length === 0) {
      return;
    }
    const from = rows.map(({ element }) => element.getBoundingClientRect().top);
    rows.forEach(({ element }) => {
      element.style.transition = "none";
      element.style.transform = "";
    });
    void document.body.offsetHeight;
    const targets = rows.map(({ element }) => element.getBoundingClientRect().top);
    rows.forEach(({ element }, index) => {
      const delta = from[index] - targets[index];
      element.style.transform = Math.abs(delta) < 0.5 ? "" : `translateY(${delta}px)`;
    });
    void document.body.offsetHeight;
    rows.forEach(({ element }) => {
      element.style.transition = "";
      element.style.transform = "";
    });
  }, [luaDropToken]);


  // 入场滑入动画只该播一次：最长延迟 540ms + 260ms 播完后就关掉。
  // 否则之后每次换位（拖动预览、筛选、排序）React 移动 DOM 节点，
  // 浏览器都会重播「向右滑入 + 淡入」，看起来就是被挪过的行闪了一下。
  useEffect(() => {
    if (!luaMods || luaModListSettled) {
      return;
    }
    const timer = window.setTimeout(() => setLuaModListSettled(true), 1000);
    return () => window.clearTimeout(timer);
  }, [luaMods, luaModListSettled]);

  useEffect(() => {
    if (activePage !== "mods") {
      return;
    }
    // 动态加载：先让浏览器画完进入模组页的这一帧，再把扫描请求发给后端
    // （真正的 Lua/JAR 扫描在 Rust 侧的后台线程/阻塞线程池里跑，不再占用 UI 主线程）。
    const handle = window.setTimeout(() => {
      void loadModPresets();
      if (modManagementPage === "jar") {
        // 只在启动后首次进入 JAR 页面时自动扫描；之后进入直接复用已扫描结果，
        // 需要重新扫描时点页面上的「刷新 JAR 列表」。
        if (!jarModsScanStartedRef.current) {
          void loadJarMods();
        }
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
        // 光影包已并入「版本 › 光影包」分区：这两个页面状态下都接受拖放。
        const onShaderpackView =
          activePage === "shaderpacks" ||
          (activePage === "versions" && versionTab === "shaderpacks");
        if (!onShaderpackView || event.payload.type !== "drop") {
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
  }, [navActivePage, versionTab]);

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

  async function installPzOptimization(force = false) {
    setInstallingPzOptimization(true);
    try {
      const result = await invoke<PzOptimizationInstallResult>("install_pz_optimization", {
        force,
      });
      showToast(
        result.alreadyInstalled
          ? "PZ Optimization 已经安装过了。"
          : `PZ Optimization ${force ? "已通过官方脚本安装" : "安装完成"}${result.installedRevision ? `（版本 ${result.installedRevision}）` : ""}。`,
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

  // 截图页子来源：Viewpoint 启动器截图 / Steam 云截图（userdata/<账号>/760/remote/108600）。
  const [shotsSource, setShotsSource] = useState<ShotsSource>("viewpoint");
  const [steamShots, setSteamShots] = useState<ManagedDirectory | null>(null);
  const [steamShotsLoading, setSteamShotsLoading] = useState(false);

  async function loadSteamShots() {
    if (steamShotsLoading) return;
    setSteamShotsLoading(true);
    try {
      setSteamShots(await invoke<ManagedDirectory>("list_steam_screenshots"));
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      setSteamShots({ directory: "", files: [] });
    } finally {
      setSteamShotsLoading(false);
    }
  }

  function switchShotsSource(source: ShotsSource) {
    setShotsSource(source);
    if (source === "steam" && !steamShots && !steamShotsLoading) {
      void loadSteamShots();
    }
  }

  /** 关闭截图预览：先播弹窗退场动画，结束再释放 ObjectURL 并卸载。 */
  function closeShotPreview() {
    if (previewClosing) {
      return;
    }
    setPreviewClosing(true);
    if (previewCloseTimerRef.current !== null) {
      clearTimeout(previewCloseTimerRef.current);
    }
    previewCloseTimerRef.current = window.setTimeout(() => {
      previewCloseTimerRef.current = null;
      setPreview((current) => {
        if (current) {
          URL.revokeObjectURL(current.url);
        }
        return null;
      });
      setPreviewClosing(false);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  async function openShot(name: string, source: ShotsSource = "viewpoint") {
    try {
      const bytes = await invoke<ArrayBuffer>(
        source === "steam" ? "read_steam_screenshot" : "read_viewpoint_shot",
        { name },
      );
      const mime = mimeTypeFor(name);
      const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
      setPreview({ name, url, bytes });
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function agreePrivacyPolicy() {
    if (privacyBusy) return;
    // 更新源出现新版本时，同意的是「新版本」的隐私政策；否则同意当前运行版本。
    const version =
      launcherUpdate?.updateAvailable && launcherUpdate.latestVersion
        ? launcherUpdate.latestVersion
        : appManifest.version;
    setPrivacyBusy(true);
    try {
      await invoke("save_privacy_consent", { version });
      setPrivacyConsentedVersion(version);
      setShowPrivacyConsent(false);
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setPrivacyBusy(false);
    }
  }

  async function disagreePrivacyPolicy() {
    if (!window.confirm("确定不同意并退出 PZManager 吗？")) {
      return;
    }
    await exit(0);
  }

  /** 撤销隐私协议同意：清空记录，重新弹出同意框（必须重新同意或退出）。 */
  async function revokePrivacyConsent() {
    const confirmed = window.confirm(
      "确定撤销隐私协议同意吗？\n\n撤销后会立即重新弹出同意框，需要重新同意才能继续使用。",
    );
    if (!confirmed) {
      return;
    }
    try {
      await invoke("clear_privacy_consent");
      setPrivacyConsentedVersion(null);
      showToast("已撤销隐私协议同意。", "info");
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

  /** 版本操作也进任务中心：返回任务 id，进度事件按它更新。 */
  function startVersionTask(title: string, rootPath: string | null): string {
    const id = "version-task-" + (++taskIdRef.current);
    const task: TaskItem = {
      id,
      kind: "version-operation",
      title,
      detail: "准备中…",
      status: "running",
      downloaded: 0,
      total: null,
      createdAt: Date.now(),
      finishedAt: null,
      error: null,
      savedPath: null,
      request: null,
      percent: 0,
      step: 1,
      steps: null,
      doneFiles: null,
      totalFiles: null,
      rootPath,
    };
    setTasks((current) => [task, ...current]);
    return id;
  }

  /** 把 version-progress / mod-version-progress 的 payload 写进任务中心的那一行。 */
  function applyVersionTaskProgress(id: string, update: VersionOperationUpdate, etaSeconds: number | null = null) {
    const terminal = update.status === "completed" || update.status === "error";
    const etaText =
      !terminal && etaSeconds !== null ? ` · 预计剩余 ${formatVersionEta(etaSeconds) ?? "估算中"}` : "";
    updateTask(id, {
      status:
        update.status === "completed" ? "success" : update.status === "error" ? "error" : "running",
      detail: terminal
        ? (update.error ?? update.message ?? (update.status === "completed" ? "完成" : "失败"))
        : `${versionPhaseLabel(update.phase)}${etaText}`,
      percent: versionProgressPercent(update),
      step: update.step ?? null,
      steps: update.steps ?? null,
      doneFiles: update.totalFiles > 0 ? update.processedFiles : null,
      totalFiles: update.totalFiles > 0 ? update.totalFiles : null,
      error: update.status === "error" ? (update.error ?? "版本操作失败") : null,
      finishedAt: terminal ? Date.now() : null,
    });
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
    const request = task.request;
    if (!request) {
      return;
    }
    const attempt = (target: string) =>
      invoke<string>("download_shader_file", {
        downloadId: task.id,
        url: target,
        fileName: request.fileName,
        directory: request.directory,
        threads: request.threads,
      });

    try {
      let savedPath: string;
      try {
        savedPath = await attempt(request.url);
      } catch (error: unknown) {
        // PCL2 风格：镜像源失败后依次回退到备用源（官方源、CDN 域名变体）
        const fallbacks = request.fallbackUrls.filter((url) => url !== request.url);
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
    const request = task.request;
    if (!request) {
      return;
    }
    const nextTask: TaskItem = {
      ...task,
      status: "running",
      downloaded: 0,
      total: null,
      error: null,
      savedPath: null,
      finishedAt: null,
      detail: request.fileName.trim() || task.detail,
    };
    setTasks((current) => current.map((item) => (item.id === task.id ? nextTask : item)));
    void executeShaderDownloadTask(nextTask);
  }

  // 移除任务复用通知的隐藏动画：先给行挂上 closing 标记播退场，动画结束再从列表删掉，
  // 避免任务行瞬间消失、下面的行突然跳位。
  function scheduleTaskRemoval(ids: string[]) {
    if (ids.length === 0) return;
    setTaskClosingIds((current) => [...new Set([...current, ...ids])]);
    window.setTimeout(() => {
      setTaskClosingIds((current) => current.filter((id) => !ids.includes(id)));
      setTasks((current) => current.filter((task) => !ids.includes(task.id)));
    }, TASK_REMOVE_EXIT_MS);
  }

  function removeTask(id: string) {
    scheduleTaskRemoval([id]);
  }

  function clearFinishedTasks() {
    scheduleTaskRemoval(tasks.filter((task) => task.status !== "running").map((task) => task.id));
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

  /** 「设置 › 版本」：读取版本状态；只做元数据扫描，不读文件内容。 */
  async function refreshVersionStatus() {
    const requestId = ++versionStatusRequestRef.current;
    setVersionStatusLoading(true);
    setVersionStatusError(null);
    try {
      const status = await invoke<VersionStatus>("get_version_status");
      // 旧请求晚到时不要覆盖新结果（操作完成后的刷新必须赢）。
      if (requestId !== versionStatusRequestRef.current) return;
      setVersionStatus(status);
      // 以磁盘里的设置为准：设置可能在别的窗口/会话里被改过，
      // 保留旧草稿会让输入框显示一个和实际操作不一致的目录。
      setVersionStoreDraft(status.storePath);
      setVersionStorePath(status.storePath);
      setVersionOperationRunning((current) => current || status.operationRunning);
      // 版本被删除或换了存储目录后，之前那次“已校验”的结论就不再有效。
      setVersionVerified((current) => {
        if (!current?.matchedVersionId) return current;
        return status.versions.some((version) => version.id === current.matchedVersionId)
          ? current
          : null;
      });
    } catch (error: unknown) {
      if (requestId !== versionStatusRequestRef.current) return;
      setVersionStatusError(error instanceof Error ? error.message : String(error));
    } finally {
      if (requestId === versionStatusRequestRef.current) {
        setVersionStatusLoading(false);
      }
    }
  }

  /** 备份在 C 盘时，每次操作前先提醒换盘；勾过「不再弹出」就直接执行。 */
  function runWithVersionDriveNotice(run: () => void) {
    if (
      !shouldWarnVersionStoreOnSystemDrive(versionStorePath, versionDriveNoticeDismissed)
    ) {
      run();
      return;
    }
    versionDriveNoticeActionRef.current = run;
    setVersionDriveNoticeDontAsk(false);
    setVersionDriveNoticeClosing(false);
    setVersionDriveNoticeOpen(true);
  }

  async function saveVersionDriveNoticeDismissed(dismissed: boolean) {
    setVersionDriveNoticeDismissed(dismissed);
    try {
      await invoke("save_version_system_drive_notice_dismissed", { dismissed });
    } catch (error: unknown) {
      setVersionDriveNoticeDismissed(!dismissed);
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 关闭换盘提示；proceed = 用户选了「仍要继续」。 */
  function closeVersionDriveNotice(proceed: boolean) {
    if (!versionDriveNoticeOpen || versionDriveNoticeClosing) return;
    const action = proceed ? versionDriveNoticeActionRef.current : null;
    versionDriveNoticeActionRef.current = null;
    if (versionDriveNoticeDontAsk) {
      void saveVersionDriveNoticeDismissed(true);
    }
    setVersionDriveNoticeClosing(true);
    window.setTimeout(() => {
      setVersionDriveNoticeOpen(false);
      setVersionDriveNoticeClosing(false);
      action?.();
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  /** 「去换盘」：收起提示，跳到「版本」页的备份位置输入框。 */
  function goChangeVersionStoreDrive() {
    closeVersionDriveNotice(false);
    switchVersionTab("game");
    navigateToPage("versions");
    window.setTimeout(() => {
      versionStoreInputRef.current?.focus();
      versionStoreInputRef.current?.select();
    }, RESOURCE_DIALOG_EXIT_MS + PAGE_TRANSITION_MS + 80);
    showToast("把「备份保存位置」改到其它盘并保存，再继续操作。", "info");
  }

  function startVersionOperation(command: string, args: Record<string, unknown>, label: string) {
    if (versionOperationRunning || versionStatus?.operationRunning) {
      showToast("已有版本操作正在进行。", "info");
      return;
    }
    runWithVersionDriveNotice(() => beginVersionOperation(command, args, label));
  }

  function beginVersionOperation(command: string, args: Record<string, unknown>, label: string) {
    if (versionOperationRunning || versionStatus?.operationRunning) {
      showToast("已有版本操作正在进行。", "info");
      return;
    }
    if (command !== "start_version_verify") {
      setVersionVerified(null);
    }
    setVersionOperationRunning(true);
    setVersionOperation({
      operationId: 0,
      operation: command,
      status: "started",
      phase: "preparing",
      processedFiles: 0,
      totalFiles: 0,
      processedBytes: 0,
      totalBytes: 0,
      currentPath: null,
      message: null,
      error: null,
    });
    const taskId = startVersionTask(`游戏版本 · ${label}`, null);
    versionTaskIdRef.current = taskId;
    void invoke<number>(command, args).catch((error: unknown) => {
      setVersionOperationRunning(false);
      setVersionOperation(null);
      const message = error instanceof Error ? error.message : String(error);
      if (versionTaskIdRef.current === taskId) {
        versionTaskIdRef.current = null;
      }
      updateTask(taskId, {
        status: "error",
        detail: `${label}失败`,
        error: message,
        finishedAt: Date.now(),
      });
      showToast(`${label}失败：${message}`, "error");
    });
  }

  async function createVersionSnapshot() {
    const label = versionLabelDraft.trim();
    const existing = versionStatus?.versions.length ?? 0;
    const modNote = versionIncludeMods ? "\n\n当前启用的模组也会各存一份。" : "";
    // 用最近一个版本的大小估算游戏目录体积，让「占用大量空间」有具体数字。
    const versions = versionStatus?.versions ?? [];
    const latestBytes = versions.length > 0 ? versions[versions.length - 1].totalBytes : 0;
    const sizeNote = latestBytes > 0 ? `（约 ${formatVersionBytes(latestBytes)}）` : "";
    const confirmed = window.confirm(
      existing > 0
        ? `一般完整保存一份母版本就够了，日常改动用「保存变化」只记录差异、省空间。\n\n再完整保存一份会把整个游戏目录${sizeNote}原样再复制一份，占用大量空间。${modNote}\n\n确定继续吗？`
        : `这是第一次保存：会把整个游戏目录${sizeNote}完整复制一份作为母版本。${modNote}\n\n确定继续吗？`,
    );
    if (!confirmed) return;
    startVersionOperation(
      "start_version_snapshot",
      { label: label || null, includeMods: versionIncludeMods },
      "完整保存",
    );
  }

  function captureVersion() {
    startVersionOperation(
      "start_version_capture",
      { label: versionLabelDraft.trim() || null, includeMods: versionIncludeMods },
      "保存变化",
    );
  }

  function verifyVersion() {
    startVersionOperation("start_version_verify", {}, "检查当前版本");
  }

  function restoreVersion(record: VersionRecord) {
    const unknown = versionStatus?.currentMatch.kind === "unknown";
    const modCount = record.modVersions?.length ?? 0;
    const modNote =
      modCount > 0
        ? `\n\n这个版本连同 ${modCount} 个模组一起保存过，模组也会一起还原；已经卸载的会跳过。`
        : "";
    const confirmed = window.confirm(
      `确定还原到「${record.label}」吗？\n\n游戏文件会被换成这个版本的内容，当前没保存过的修改会丢失。${modNote}\n\n装了 PZ Optimization 的话，还原前会自动卸载、完成后重新安装；想保留原版状态请先到「支持库」卸载。\n请先关闭游戏。`,
    );
    if (!confirmed) return;
    if (unknown) {
      const forced = window.confirm(
        "当前游戏文件可能被 Steam 更新或手动修改过。\\n\\n继续还原会覆盖这些内容，确定吗？",
      );
      if (!forced) return;
    }
    // 用户已经在上面明确确认过覆盖风险；后端仍保留 force 门控给其它调用方。
    startVersionOperation(
      "start_version_restore",
      { versionId: record.id, force: true, restoreMods: true },
      "还原到这个版本",
    );
  }

  async function deleteVersion(record: VersionRecord) {
    const confirmed = window.confirm(
      `确定删除「${record.label}」吗？

只能删除最新的版本，删掉后不能恢复。`,
    );
    if (!confirmed) return;
    setVersionVerified(null);
    // 删除可能上 GB：走后台操作，进度进任务中心与概览的进度面板。
    startVersionOperation("delete_version", { versionId: record.id }, `删除「${record.label}」`);
  }

  async function saveVersionStore() {
    setVersionStoreSaving(true);
    try {
      await invoke("save_version_store_path", { path: versionStoreDraft });
      setVersionVerified(null);
      showToast("备份保存位置已更新", "success");
      await refreshVersionStatus();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setVersionStoreSaving(false);
    }
  }

  async function chooseVersionStoreDirectory() {
    const selected = await open({ title: "选择备份保存位置", directory: true, multiple: false });
    if (typeof selected === "string") setVersionStoreDraft(selected);
  }

  async function updateVersionParallel(enabled: boolean) {
    const previous = versionParallel;
    setVersionParallel(enabled);
    setVersionParallelSaving(true);
    try {
      await invoke("save_version_parallel", { enabled });
      showToast(enabled ? "已开启多线程加速" : "已改为逐个处理", "success");
    } catch (error: unknown) {
      setVersionParallel(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setVersionParallelSaving(false);
    }
  }

  function cancelVersionOperation() {
    void invoke("cancel_version_operation").catch(() => undefined);
    showToast("正在中断…", "info");
  }

  /** 版本操作进行中才显示的时间行：已用时 + 按进度平滑出的剩余时间估算。 */
  function versionTimingLabel(status: string): string | null {
    if (status !== "started" && status !== "progress") {
      return null;
    }
    const parts: string[] = [];
    const elapsed = formatVersionElapsed(versionTiming.startedAt);
    if (elapsed) {
      parts.push(`已用 ${elapsed}`);
    }
    const eta = formatVersionEta(versionTiming.etaSeconds);
    parts.push(`预计剩余 ${eta ?? "估算中…"}`);
    return parts.join(" · ");
  }

  async function updateVersionIncludeMods(enabled: boolean) {
    const previous = versionIncludeMods;
    setVersionIncludeMods(enabled);
    setVersionIncludeModsSaving(true);
    try {
      await invoke("save_version_include_mods", { enabled });
      showToast(
        enabled ? "保存游戏版本时会一起存下启用的模组" : "保存游戏版本时不再包含模组",
        "success",
      );
    } catch (error: unknown) {
      setVersionIncludeMods(previous);
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setVersionIncludeModsSaving(false);
    }
  }

  async function loadModVersionTargets() {
    setModVersionTargetsLoading(true);
    try {
      setModVersionTargets(await invoke<ModVersionTarget[]>("list_mod_version_targets"));
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setModVersionTargetsLoading(false);
    }
  }

  async function loadModVersionPacks() {
    try {
      setModVersionPacks(await invoke<ModPackRecord[]>("list_mod_version_packs"));
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  /** 整合包操作（保存全部 / 整套还原）也进任务中心，事件仍走 mod-version-progress。 */
  function startModPackOperation(
    command: string,
    payload: Record<string, unknown>,
    label: string,
    taskTitle: string,
  ) {
    if (modVersionOperationRunning || versionOperationRunning) {
      showToast("已有版本操作正在进行，请等待完成或先取消。", "info");
      return;
    }
    runWithVersionDriveNotice(() => {
      void beginModPackOperation(command, payload, label, taskTitle);
    });
  }

  async function beginModPackOperation(
    command: string,
    payload: Record<string, unknown>,
    label: string,
    taskTitle: string,
  ) {
    if (modVersionOperationRunning || versionOperationRunning) {
      showToast("已有版本操作正在进行，请等待完成或先取消。", "info");
      return;
    }
    setModVersionOperationRunning(true);
    setModVersionOperation({
      operationId: 0,
      operation: command,
      status: "started",
      phase: "preparing",
      processedFiles: 0,
      totalFiles: 0,
      processedBytes: 0,
      totalBytes: 0,
      currentPath: null,
      message: null,
      error: null,
      rootPath: null,
    });
    const taskId = startVersionTask(taskTitle, null);
    modVersionTaskIdRef.current = taskId;
    try {
      await invoke(command, payload);
    } catch (error: unknown) {
      setModVersionOperationRunning(false);
      setModVersionOperation(null);
      const message = error instanceof Error ? error.message : String(error);
      if (modVersionTaskIdRef.current === taskId) {
        modVersionTaskIdRef.current = null;
      }
      updateTask(taskId, {
        status: "error",
        detail: `${label}失败`,
        error: message,
        finishedAt: Date.now(),
      });
      showToast(`${label}失败：${message}`, "error");
    }
  }

  function saveAllModVersions() {
    const total = modVersionTargets?.length ?? 0;
    if (total === 0) {
      showToast("还没有读取到模组列表，先点「刷新」。", "info");
      return;
    }
    // 命名交给弹窗：留空时后端回落「整合包 N」。
    setModPackNameDialog({ mode: "save", pack: null });
  }

  function renameModVersionPack(pack: ModPackRecord) {
    setModPackNameDialog({ mode: "rename", pack });
  }

  /** 关闭整合包命名 / 重命名弹窗（取消或点遮罩）；确认走 confirmSaveModPack / confirmRenameModPack。 */
  function closeModPackNameDialog() {
    if (!modPackNameDialog || modPackNameDialogClosing) return;
    setModPackNameDialogClosing(true);
    window.setTimeout(() => {
      setModPackNameDialog(null);
      setModPackNameDialogClosing(false);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  /** 保存整合包（带可选名称）并关掉弹窗。 */
  function confirmSaveModPack(label: string) {
    if (modPackNameDialogClosing) return;
    const trimmed = label.trim();
    setModPackNameDialogClosing(true);
    window.setTimeout(() => {
      setModPackNameDialog(null);
      setModPackNameDialogClosing(false);
      void startModPackOperation(
        "start_mod_version_save_all",
        { label: trimmed || null },
        "保存全部模组",
        trimmed ? `整合包 · ${trimmed}` : "整合包 · 保存全部模组",
      );
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  /** 重命名整合包并刷新列表。 */
  async function confirmRenameModPack(pack: ModPackRecord, label: string) {
    if (modPackRenaming) return;
    const trimmed = label.trim();
    if (!trimmed) {
      showToast("整合包名称不能为空。", "error");
      return;
    }
    setModPackRenaming(true);
    try {
      const message = await invoke<string>("rename_mod_version_pack", {
        packId: pack.id,
        label: trimmed,
      });
      showToast(message, "success");
      setModPackNameDialogClosing(true);
      window.setTimeout(() => {
        setModPackNameDialog(null);
        setModPackNameDialogClosing(false);
      }, RESOURCE_DIALOG_EXIT_MS);
      await loadModVersionPacks();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setModPackRenaming(false);
    }
  }

  function restoreModVersionPack(pack: ModPackRecord) {
    const confirmed = window.confirm(
      `确定还原整合包「${pack.label}」吗？\n\n会把包里的 ${pack.mods.length} 个模组都换成记录时的版本，当前没保存过的修改会丢失；找不到的模组会跳过。\n请先关闭游戏。`,
    );
    if (!confirmed) return;
    void startModPackOperation(
      "start_mod_version_restore_pack",
      { packId: pack.id },
      "还原整合包",
      pack.label,
    );
  }

  async function deleteModVersionPack(pack: ModPackRecord) {
    const confirmed = window.confirm(
      `确定删除整合包「${pack.label}」吗？\n\n只删这条记录，各模组自己的版本历史会保留。`,
    );
    if (!confirmed) return;
    try {
      const message = await invoke<string>("delete_mod_version_pack", { packId: pack.id });
      showToast(message, "success");
      await loadModVersionPacks();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function loadModVersionStatus(rootPath: string) {
    if (modVersionDialogRootRef.current === rootPath) {
      setModVersionDialogLoading(true);
    }
    try {
      const status = await invoke<ModVersionStatus>("get_mod_version_status", { rootPath });
      setModVersionStatuses((current) => ({ ...current, [rootPath]: status }));
      return status;
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
      return null;
    } finally {
      if (modVersionDialogRootRef.current === rootPath) {
        setModVersionDialogLoading(false);
      }
    }
  }

  function startModVersionOperation(
    command: string,
    rootPath: string,
    payload: Record<string, unknown>,
    label: string,
    modName: string,
  ) {
    if (modVersionOperationRunning || versionOperationRunning) {
      showToast("已有版本操作正在进行，请等待完成或先取消。", "info");
      return;
    }
    runWithVersionDriveNotice(() => {
      void beginModVersionOperation(command, rootPath, payload, label, modName);
    });
  }

  async function beginModVersionOperation(
    command: string,
    rootPath: string,
    payload: Record<string, unknown>,
    label: string,
    modName: string,
  ) {
    if (modVersionOperationRunning || versionOperationRunning) {
      showToast("已有版本操作正在进行，请等待完成或先取消。", "info");
      return;
    }
    setModVersionOperationRunning(true);
    setModVersionOperation({
      operationId: 0,
      operation: command,
      status: "started",
      phase: "preparing",
      processedFiles: 0,
      totalFiles: 0,
      processedBytes: 0,
      totalBytes: 0,
      currentPath: null,
      message: null,
      error: null,
      rootPath,
    });
    const taskId = startVersionTask(`${modName} · ${label}`, rootPath);
    modVersionTaskIdRef.current = taskId;
    try {
      await invoke(command, { rootPath, ...payload });
    } catch (error: unknown) {
      setModVersionOperationRunning(false);
      setModVersionOperation(null);
      const message = error instanceof Error ? error.message : String(error);
      if (modVersionTaskIdRef.current === taskId) {
        modVersionTaskIdRef.current = null;
      }
      updateTask(taskId, {
        status: "error",
        detail: `${label}失败`,
        error: message,
        finishedAt: Date.now(),
      });
      showToast(`${label}失败：${message}`, "error");
    }
  }

  function recordModVersion(target: {
    rootPath: string;
    name: string;
    versionCount: number;
  }) {
    const first = target.versionCount === 0;
    const confirmed = window.confirm(
      first
        ? `第一次保存「${target.name}」：会把整个模组目录复制一份。\n\n确定继续吗？`
        : `保存「${target.name}」的当前变化：只记录和上次不同的文件。\n\n确定继续吗？`,
    );
    if (!confirmed) return;
    void startModVersionOperation(
      first ? "start_mod_version_snapshot" : "start_mod_version_capture",
      target.rootPath,
      { label: null },
      first ? "完整保存" : "保存变化",
      target.name,
    );
  }

  function restoreModVersion(rootPath: string, name: string, record: VersionRecord) {
    const confirmed = window.confirm(
      `确定把「${name}」还原到「${record.label}」吗？\n\n模组文件会被换成这个版本的内容，当前没保存过的修改会丢失。\n创意工坊的模组可能被 Steam 校验或更新覆盖掉。\n请先关闭游戏。`,
    );
    if (!confirmed) return;
    void startModVersionOperation(
      "start_mod_version_restore",
      rootPath,
      { versionId: record.id, force: true },
      "还原到这个版本",
      name,
    );
  }

  async function deleteModVersion(rootPath: string, modName: string, record: VersionRecord) {
    const confirmed = window.confirm(
      `确定删除「${record.label}」吗？

只能删除最新的版本，删掉后不能恢复。`,
    );
    if (!confirmed) return;
    // 删除走后台操作：进度进任务中心与模组版本进度面板，完成后监听器自动刷新列表。
    startModVersionOperation(
      "delete_mod_version",
      rootPath,
      { versionId: record.id },
      `删除「${record.label}」`,
      modName,
    );
  }

  function verifyModVersion(rootPath: string, name: string) {
    void startModVersionOperation(
      "start_mod_version_verify",
      rootPath,
      {},
      "检查当前版本",
      name,
    );
  }

  function openModVersionDialog(rootPath: string, name: string) {
    modVersionDialogRootRef.current = rootPath;
    setModVersionDialog({ rootPath, name });
    setModVersionDialogClosing(false);
    setModVersionDialogLoading(true);
    void loadModVersionStatus(rootPath);
  }

  function closeModVersionDialog() {
    setModVersionDialogClosing(true);
    window.setTimeout(() => {
      modVersionDialogRootRef.current = null;
      setModVersionDialog(null);
      setModVersionDialogClosing(false);
      setModVersionDialogLoading(false);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  function toggleModVersionExpansion(key: string, rootPath: string) {
    if (modVersionExpandedKey === key) {
      setModVersionClosingKey(key);
      setModVersionExpandedKey(null);
      window.setTimeout(() => {
        setModVersionClosingKey((current) => (current === key ? null : current));
      }, VERSION_DIFF_EXIT_MS);
    } else {
      setModVersionClosingKey(null);
      setModVersionExpandedKey(key);
    }
    if (!modVersionStatuses[rootPath]) {
      void loadModVersionStatus(rootPath);
    }
  }

  async function refreshLauncherLog() {
    setLauncherLogLoading(true);
    setLauncherLogError(null);
    try {
      const tail = await invoke<LauncherLogTail>("read_launcher_log", { limit: 256 * 1024 });
      setLauncherLogTail(tail);
    } catch (error: unknown) {
      setLauncherLogError(error instanceof Error ? error.message : String(error));
    } finally {
      setLauncherLogLoading(false);
    }
  }

  function copyLauncherLog() {
    const content = launcherLogTail?.content ?? "";
    if (!content) {
      showToast("启动器日志为空。", "info");
      return;
    }
    void writeText(content)
      .then(() => showToast("已复制启动器日志", "success"))
      .catch((error: unknown) => {
        showToast(error instanceof Error ? error.message : String(error), "error");
      });
  }

  async function clearLauncherLog() {
    if (!window.confirm("确定清空启动器日志吗？历史记录会被删除。")) {
      return;
    }
    try {
      await invoke("clear_launcher_log");
      showToast("启动器日志已清空", "success");
      await refreshLauncherLog();
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function openLauncherLogFolder() {
    try {
      await invoke("open_launcher_log_folder");
    } catch (error: unknown) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
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

  async function openWorkshopLink(link: ModWorkshopLink) {
    try {
      showToast("正在打开 Steam 创意工坊资源。", "info");
      await openUrl(link.url);
      return;
    } catch {
      try {
        await openUrl(link.fallbackUrl);
      } catch (error: unknown) {
        showToast(error instanceof Error ? error.message : String(error), "error");
      }
    }
  }

  async function openWorkshopItem(workshopId: string, fallbackUrl: string) {
    await openWorkshopLink({
      url: steamWorkshopUrl(workshopId),
      fallbackUrl,
    });
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
    <Box className="pz-toolbox-card">
      <Text color="gray.900" fontSize="lg" fontWeight="700">控制台</Text>
      <Text color="gray.500" fontSize="xs" mt="1">
        游戏加载状态和本次运行的通知记录。
      </Text>
      <Box className="pz-toolbox-stack" mt="3">
        {overlayConsolePanel}
        <Box className="pz-toolbox-separator" />
        <NotificationConsole entries={notificationHistory} onClear={clearNotificationHistory} />
      </Box>
    </Box>
  );

  const launcherLogSettingsContent = (
    <Box className="pz-panel pz-launcher-log-panel">
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="0">
          <Text color="gray.900" fontSize="lg" fontWeight="700">启动器日志</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            记录启动器启动、游戏检测/启动、版本操作、PZ Optimization 安装卸载以及前端错误。
          </Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            日志时间使用 UTC（行尾 Z）；日志只保存在本机，提交反馈时会一并上传最近一次启动的日志。
          </Text>
        </Box>
        <Button loading={launcherLogLoading} onClick={() => void refreshLauncherLog()} size="sm" variant="outline">
          刷新
        </Button>
      </HStack>

      {launcherLogError ? (
        <Text color="red.500" fontSize="xs" mt="3" whiteSpace="pre-wrap">读取失败：{launcherLogError}</Text>
      ) : null}

      {launcherLogTail ? (
        <>
          <Box bg="gray.50" borderColor="gray.200" borderRadius="md" borderWidth="1px" mt="4" p="3">
            <Stack gap="2">
              <HStack align="start" gap="3" justify="space-between">
                <Text color="gray.500" fontSize="xs">文件</Text>
                <Text color="gray.700" fontSize="xs" overflowWrap="anywhere" textAlign="right">{launcherLogTail.path}</Text>
              </HStack>
              <HStack align="start" gap="3" justify="space-between">
                <Text color="gray.500" fontSize="xs">大小</Text>
                <Text color="gray.700" fontSize="xs" textAlign="right">
                  {formatVersionBytes(launcherLogTail.size)}
                  {launcherLogTail.truncated ? " · 仅显示末尾 256 KB" : ""}
                </Text>
              </HStack>
            </Stack>
          </Box>

          <HStack gap="2" mt="3" wrap="wrap">
            <Button disabled={!launcherLogTail.content} onClick={copyLauncherLog} size="sm" variant="outline">复制</Button>
            <Button onClick={() => void openLauncherLogFolder()} size="sm" variant="outline">打开日志文件夹</Button>
            <Button colorPalette="red" onClick={() => void clearLauncherLog()} size="sm" variant="ghost">清空日志</Button>
          </HStack>

          <Box
            aria-label="启动器日志内容"
            as="pre"
            bg="gray.900"
            borderRadius="md"
            className="pz-launcher-log-view"
            color="gray.100"
            fontFamily="mono"
            fontSize="xs"
            lineHeight="1.5"
            maxH="360px"
            mt="3"
            overflowWrap="anywhere"
            overflowX="hidden"
            overflowY="auto"
            p="3"
            tabIndex={0}
            whiteSpace="pre-wrap"
          >
            {launcherLogTail.content || (launcherLogTail.exists ? "日志文件为空。" : "日志文件还没有创建。启动器启动或执行操作后会自动生成。")}
          </Box>
        </>
      ) : !launcherLogLoading ? (
        <Text color="gray.500" fontSize="sm" mt="4">还没有读取日志。</Text>
      ) : (
        <Text color="gray.500" fontSize="sm" mt="4">正在读取启动器日志...</Text>
      )}
    </Box>
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
  const pzOptimizationForceAvailable = Boolean(pzOptimizationStatus?.forceInstallAvailable);
  const pzOptimizationCanInstall = Boolean(
    pzOptimizationStatus?.available || pzOptimizationForceAvailable,
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
                        : pzOptimizationForceAvailable
                          ? "可强制安装"
                          : "未下载"
                }
                badgePalette={
                  pzOptimizationInstalledCurrent
                    ? "green"
                    : pzOptimizationStatus.available
                      ? pzOptimizationNeedsUpdate
                        ? "orange"
                        : "blue"
                      : pzOptimizationForceAvailable
                        ? "orange"
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
                      : pzOptimizationForceAvailable
                        ? "没有匹配当前游戏版本的类文件，但检测到官方 install.ps1。可以强制安装：脚本会从 GitHub 下载当前版本的构建。"
                        : "请先在 Steam 创意工坊订阅并等待 PZ Optimization 下载完成。"}
                </Text>
                {developerMode && (
                  <Stack gap="0.5" mt="2">
                    <Text color="gray.500" fontFamily="mono" fontSize="xs" overflowWrap="anywhere">
                      来源：{pzOptimizationStatus.sourcePath ?? "未找到"}
                    </Text>
                    <Text color="gray.500" fontFamily="mono" fontSize="xs" overflowWrap="anywhere">
                      安装脚本：{pzOptimizationStatus.installerPath ?? "未找到"}
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
                    colorPalette={
                      pzOptimizationForceAvailable && !pzOptimizationStatus.available
                        ? "orange"
                        : "blue"
                    }
                    disabled={!pzOptimizationCanInstall || pzOptimizationInstalledCurrent}
                    loading={installingPzOptimization}
                    onClick={() =>
                      void installPzOptimization(
                        pzOptimizationForceAvailable && !pzOptimizationStatus.available,
                      )
                    }
                    size="sm"
                  >
                    {pzOptimizationInstalledCurrent
                      ? "已安装"
                      : pzOptimizationNeedsUpdate
                        ? "一键更新"
                        : pzOptimizationForceAvailable && !pzOptimizationStatus.available
                          ? "强制安装（官方脚本）"
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
                {pzOptimizationForceAvailable && !pzOptimizationStatus.available && (
                  <Text color="orange.700" fontSize="xs" mt="3">
                    强制安装会运行官方脚本并从 GitHub 下载匹配当前游戏版本（revision）的构建；
                    如果作者尚未发布该版本，脚本会直接报错，不会硬装旧类文件。
                  </Text>
                )}
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
      title="截图"
      description="Viewpoint 启动器截图与 Steam 云截图，点击预览并复制到系统剪贴板。"
      directory={shotsSource === "steam" ? steamShots?.directory : shots?.directory}
      developerMode={developerMode}
    >
      <HStack className="pz-shots-source-tabs" gap="1" mb="4" wrap="wrap">
        <Button
          aria-selected={shotsSource === "viewpoint"}
          className={`pz-shots-source-tab${shotsSource === "viewpoint" ? " is-active" : ""}`}
          onClick={() => switchShotsSource("viewpoint")}
          size="xs"
          variant="ghost"
        >
          Viewpoint 截图
        </Button>
        <Button
          aria-selected={shotsSource === "steam"}
          className={`pz-shots-source-tab${shotsSource === "steam" ? " is-active" : ""}`}
          onClick={() => switchShotsSource("steam")}
          size="xs"
          variant="ghost"
        >
          Steam 截图
        </Button>
      </HStack>
      {shotsSource === "viewpoint" ? (
        <>
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
        </>
      ) : steamShotsLoading ? (
        <Box display="grid" gap="3" gridTemplateColumns="repeat(auto-fill, minmax(160px, 1fr))">
          {Array.from({ length: 8 }).map((_, index) => (
            <Box className="pz-media-card" bg="gray.50" borderColor="gray.200" borderRadius="sm" borderWidth="1px" key={index} overflow="hidden">
              <Box className="pz-skeleton-shine" style={{ aspectRatio: "16 / 10" }} />
              <Box p="2">
                <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: "70%" }} />
              </Box>
            </Box>
          ))}
        </Box>
      ) : steamShots && steamShots.files.length > 0 ? (
        <Box display="grid" gap="3" gridTemplateColumns="repeat(auto-fill, minmax(160px, 1fr))">
          {steamShots.files.map((shot) => (
            <Box
              className="pz-media-card"
              key={shot.name}
              bg="gray.50"
              borderColor="gray.200"
              borderRadius="sm"
              borderWidth="1px"
              cursor="pointer"
              onClick={() => void openShot(shot.name, "steam")}
              overflow="hidden"
              _hover={{ borderColor: "blue.400", boxShadow: "sm" }}
            >
              <ShotThumbnail name={shot.name} source="steam" />
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
        <EmptyState
          text={
            steamShots && steamShots.directory
              ? "Steam 截图目录暂无图片。"
              : "没有找到 Steam 截图目录：未检测到 Steam 安装位置或已登录的用户。"
          }
        />
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
          <SkeletonGate
            loading={workshopLoading && !workshopResponse}
            skeleton={<SkeletonList rows={6} thumb="preview" />}
          >
            {workshopResponse ? (
            <>
              {workshopResponse.items.length > 0 ? (
                <Box className="pz-workshop-list">
                  {workshopResponse.items.map((item) => (
                    <Menu.Root
                      key={item.id}
                      positioning={{ strategy: "fixed", flip: true, shift: 8, overflowPadding: 8 }}
                    >
                      {/* 整行都是右键触发区；左键默认在窗口内打开详情，
                          右侧方形按钮才走 Steam 客户端。 */}
                      <Menu.ContextTrigger asChild>
                        <Box
                          className="pz-workshop-list-item"
                          onClick={() => void openWorkshopItemInWindow(item)}
                          role="button"
                          tabIndex={0}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              void openWorkshopItemInWindow(item);
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
                          <Button
                            aria-label={`在 Steam 中打开 ${item.title || "未命名模组"}`}
                            className="pz-workshop-share-button"
                            onClick={(event) => {
                              event.stopPropagation();
                              void openWorkshopItem(item.id, item.fileUrl);
                            }}
                            size="xs"
                            title="在 Steam 中打开"
                            variant="outline"
                          >
                            {/* 斜向右上的箭头（↗）：表示“到外部打开”。
                                不用三点连线的分享图标，也不画成先上后右的折线。 */}
                            <svg
                              aria-hidden="true"
                              className="pz-workshop-share-icon"
                              fill="none"
                              viewBox="0 0 16 16"
                            >
                              <path
                                d="M3.4 12.6 12.8 3.2"
                                stroke="currentColor"
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth="1.6"
                              />
                              <path
                                d="M6.8 3.2H12.8V9.2"
                                stroke="currentColor"
                                strokeLinecap="round"
                                strokeLinejoin="round"
                                strokeWidth="1.6"
                              />
                            </svg>
                          </Button>
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
          </SkeletonGate>
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
        <SkeletonGate
          loading={developerNewsLoading && !developerNews}
          skeleton={<SkeletonNews cards={3} />}
        >
          {developerNews?.items.length ? developerNews.items.map((item, index) => {
          const engineShare = formatNewsEngineShare(item.engineTally);
          return (
            <DeveloperNewsCard key={item.gid} revealIndex={index}>
              <Text color="gray.900" fontSize="md" fontWeight="700">{item.title}</Text>
              <Text color="gray.500" fontSize="xs" mt="1">{new Date(item.date * 1000).toLocaleString()} {item.author ? `· ${item.author}` : ""} {item.feedLabel ? `· ${item.feedLabel}` : ""}</Text>
              <HtmlContentWithImageReveal className="pz-developer-news-content" html={item.contentsHtml} mt="3" />
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
        </SkeletonGate>
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
      <HStack align="center" gap="4" justify="space-between" mt="5" wrap="wrap">
        <Box flex="1" minW="240px">
          <Text color="gray.800" fontSize="sm" fontWeight="700">覆盖层背景色</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            游戏加载时覆盖层的底色（默认纯红）。文字会自动按背景深浅换成白字或深色字，
            浅色背景也能看清。改动在下次启动游戏时生效。
          </Text>
        </Box>
        <HStack gap="2" align="center" wrap="wrap">
          {OVERLAY_BACKGROUND_PRESETS.map((preset) => {
            const selected =
              overlayBackgroundColor.toUpperCase() === preset.value.toUpperCase();
            return (
              <Button
                aria-label={`覆盖层背景色：${preset.label}`}
                aria-pressed={selected}
                key={preset.value}
                onClick={() => updateOverlayBackgroundColor(preset.value)}
                size="sm"
                title={preset.label}
                variant={selected ? "solid" : "outline"}
              >
                <Box
                  bg={preset.value}
                  borderColor="gray.300"
                  borderRadius="sm"
                  borderWidth="1px"
                  h="0.7rem"
                  w="0.7rem"
                />
              </Button>
            );
          })}
          {/* 系统取色器：Chakra Input 的 color 类型，样式规则里已排除在统一控件高度之外。 */}
          <Input
            aria-label="自定义覆盖层背景色"
            onChange={(event) => updateOverlayBackgroundColor(event.target.value)}
            title="自定义颜色"
            type="color"
            value={overlayBackgroundColor}
            w="2.4rem"
          />
          <Text color="blue.700" fontFamily="mono" fontSize="xs" fontWeight="700">
            {overlayBackgroundColor.toUpperCase()}
          </Text>
          {overlayBackgroundColor.toUpperCase() !== DEFAULT_OVERLAY_BACKGROUND_COLOR && (
            <Button
              aria-label="恢复默认覆盖层背景色"
              onClick={() => updateOverlayBackgroundColor(DEFAULT_OVERLAY_BACKGROUND_COLOR)}
              size="sm"
              variant="ghost"
            >
              恢复默认
            </Button>
          )}
        </HStack>
      </HStack>
      <Box mt="5">
        <HStack align="center" gap="2" wrap="wrap">
          <Text color="gray.800" fontSize="sm" fontWeight="700">网络</Text>
          <HintDot
            label="网络中转说明"
            text={
              "大陆网络直连 Steam 创意工坊会失败：默认经中转服务器访问工坊列表、项目详情和开发者新闻，预览图加载失败时也走中转兜底。\n\n" +
              "「自动」= 先试直连，失败自动切中转；「中转优先」= 一直走中转；「仅直连」= 不用中转（直连被墙时会打不开）。\n\n" +
              "UU 这类只加速 Steam 客户端的工具不会接管启动器，开着也仍需中转；如果你装了 Clash / V2Ray 等带本地端口的代理，填在下面通常比中转更快。"
            }
          />
        </HStack>
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
            <HintDot
              label="极速启动说明"
              text={
                "让游戏跳过启动时的模组全量扫描，直接使用启动器预先整理好的模组清单，明显加快启动速度。任何一步出问题都会自动退回游戏原本的加载方式，不影响正常游玩。\n\n" +
                "需要通过本启动器的「启动游戏」按钮启动游戏。开启时会先备份游戏的启动配置（原文件保存在 .pzmanager\\fast-launch-backup），关闭时自动恢复。" +
                (modLoaderScheme === "zombieBuddy"
                  ? "\n\n清单只包含「已启用」的 JAR 模组，会一直保留：改动模组后想立即生效，到「模组管理 › JAR」点一次「刷新 JAR 列表」即可。"
                  : "") +
                "\n\n此功能会修改游戏的启动方式，建议只在单机或自建服务器环境使用。"
              }
            />
          </HStack>
          {modLoaderScheme === "zombieBuddy" ? null : (
            <Text color="orange.700" fontSize="xs" mt="1">
              仅支持 ZombieBuddy 模组加载器：当前选择的是「不使用（原版加载）」，请先到
              设置 › 模组加载器 更改。
            </Text>
          )}
          <FastLaunchCacheStatusLine
            enabled={fastLaunch && !fastLaunchWarningOpen && modLoaderScheme === "zombieBuddy"}
          />
        </Box>
        <Switch.Root
          aria-label="极速启动"
          checked={fastLaunch}
          disabled={fastLaunchSaving || modLoaderScheme !== "zombieBuddy"}
          onCheckedChange={(details) => {
            if (details.checked) {
              // 先临时设为开，让受控开关的隐藏 input 跟上点击；确认后才落盘，取消时退回。
              setFastLaunch(true);
              setFastLaunchWarningOpen(true);
            } else {
              void updateFastLaunch(false);
            }
          }}
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
              两次清理至少间隔 60 秒（打开激进清理后为 5 分钟）；正在运行的游戏不受影响。
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
                  {memoryCleanupSystemDetail(lastCleanupReport)}
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

        <HStack align="center" gap="4" justify="space-between" mt="4" wrap="wrap">
          <Box flex="1" minW="240px">
            <HStack gap="2" wrap="wrap">
              <Text color="gray.800" fontSize="sm" fontWeight="700">
                激进内存清理
              </Text>
              <HintDot
                label="激进内存清理说明"
                text={
                  "常规清理只压各进程的工作集。「已提交」是进程持有的提交配额，外部工具无法回收，清理不会让这个数值回落。\n\n" +
                  "打开后追加系统级清理：清空全系统工作集、刷新系统文件缓存、把已修改页写回磁盘，最后把待机缓存（Standby）标成空闲页——任务管理器里「可用内存」变大、「已缓存」下降靠的就是这几步。\n\n" +
                  "代价是文件缓存被清掉后，接下来读盘会慢一些，建议只在内存吃紧时临时打开。系统级步骤需要管理员权限，权限不足时只剩工作集修剪。\n\n" +
                  "静默清理在游戏运行时仍只用常规模式；手动清理在游戏运行时也会跳过全系统工作集，避免游戏卡顿。"
                }
              />
            </HStack>
            {aggressiveMemoryClean && launcherElevated === false && (
              <HStack gap="2" mt="2" wrap="wrap">
                <Text color="orange.700" fontSize="xs">
                  当前未以管理员身份运行：激进清理的系统级步骤不会生效。
                </Text>
                <Button
                  loading={restartingAsAdmin}
                  onClick={() => void restartLauncherAsAdmin()}
                  size="xs"
                  variant="outline"
                >
                  以管理员身份重启
                </Button>
              </HStack>
            )}
          </Box>
          <Switch.Root
            aria-label="激进内存清理"
            checked={aggressiveMemoryClean}
            disabled={aggressiveMemoryCleanSaving}
            onCheckedChange={(details) => void updateAggressiveMemoryClean(details.checked)}
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
            <Text color="gray.500" fontSize="xs" mt="0.5">Project Zomboid Manager 的开发者!<p>(小声:其实up才高二)</p></Text>
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
      <Box mt="4">
        <HStack gap="2" align="center">
          <Text color="gray.800" fontSize="sm" fontWeight="700">爱发电</Text>
          <Text color="gray.500" fontSize="xs">支持作者 叁葉 持续更新</Text>
        </HStack>
        <HStack gap="2" mt="1" align="center">
          <Text color="gray.500" fontSize="sm" overflowWrap="anywhere">
            https://ifdian.net/a/sanyee
          </Text>
          <Button
            onClick={() => void openUrl("https://ifdian.net/a/sanyee")}
            size="sm"
            variant="ghost"
          >
            打开 ↗
          </Button>
        </HStack>
      </Box>
      <Box mt="5">
        <HStack gap="2" wrap="wrap">
          <Text color="gray.800" fontSize="md" fontWeight="800">PZMANAGER</Text>
          <Badge colorPalette="blue" size="sm">v{APP_DEV_BUILD?.base ?? appManifest.version}</Badge>
          {APP_DEV_BUILD ? (
            <Badge colorPalette="purple" size="sm">DevBuild {APP_DEV_BUILD.buildId}</Badge>
          ) : null}
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
        <Button
          aria-expanded={feedbackHistoryOpen}
          className="pz-feedback-history-toggle pz-text-button"
          mt="3"
          onClick={() => setFeedbackHistoryOpen((open) => !open)}
          size="xs"
          variant="ghost"
        >
          {feedbackHistoryOpen ? "收起" : "已提交的反馈"}
          {feedbackHistory.length > 0 ? `（${feedbackHistory.length}）` : ""}
          <Text as="span" color="gray.400" ml="1">{feedbackHistoryOpen ? "▴" : "▾"}</Text>
        </Button>
        {feedbackHistoryOpen && (
          <Stack className="pz-feedback-history" gap="1" mt="2">
            {feedbackHistory.length === 0 ? (
              <Text color="gray.500" fontSize="xs">这台机器还没有提交过反馈。</Text>
            ) : (
              feedbackHistory.map((entry) => (
                <HStack
                  align="center"
                  className="pz-feedback-history-row"
                  gap="3"
                  key={entry.id}
                  wrap="wrap"
                >
                  <Text
                    color="gray.800"
                    fontFamily="mono"
                    fontSize="xs"
                    fontWeight="700"
                    minW="12rem"
                  >
                    {entry.id}
                  </Text>
                  <Text color="gray.500" fontSize="xs" minW="9rem">
                    {new Date(entry.sentAt * 1000).toLocaleString("zh-CN", { hour12: false })}
                  </Text>
                  <Text color="gray.500" flex="1" fontSize="xs" lineClamp={1} minW="8rem">
                    {entry.summary || "（无描述）"}
                  </Text>
                  <Button
                    aria-label={`复制反馈编号 ${entry.id}`}
                    className="pz-text-button"
                    onClick={() => copyFeedbackId(entry.id)}
                    size="xs"
                    variant="ghost"
                  >
                    复制编号
                  </Button>
                </HStack>
              ))
            )}
          </Stack>
        )}
      </Box>
      <Box mt="5" pt="4" borderTopWidth="1px" borderColor="gray.200">
        <HStack justify="space-between" align="center" gap="3" wrap="wrap">
          <Box>
            <HStack align="center" gap="2" wrap="wrap">
              <Text color="gray.800" fontSize="sm" fontWeight="700">隐私协议</Text>
              {privacyConsentedVersion ? (
                <Badge colorPalette="green" size="sm">
                  已同意 v{privacyConsentedVersion}
                </Badge>
              ) : (
                <Badge colorPalette="orange" size="sm">未同意</Badge>
              )}
            </HStack>
            <Text color="gray.500" fontSize="xs" mt="0.5">
              撤销后下一次启动会重新弹出隐私协议，需要重新同意才能继续使用启动器。
            </Text>
          </Box>
          <Button
            colorPalette="red"
            disabled={!privacyConsentedVersion}
            onClick={() => void revokePrivacyConsent()}
            size="sm"
            variant="outline"
          >
            撤销同意
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
                <HStack gap="2" wrap="wrap" align="center">
                  <Text color="gray.500" fontSize="sm">
                    {launcherUpdate.devBuild
                      ? `内测版本 DevBuild ${launcherUpdate.devBuildId}`
                      : launcherUpdate.unknownVersion
                        ? "未知版本"
                        : "已是最新版本"}（v
                    {launcherUpdate.devBuild
                      ? parseDevBuildVersion(launcherUpdate.currentVersion)?.base ??
                        launcherUpdate.currentVersion
                      : launcherUpdate.currentVersion}
                    ）。
                  </Text>
                  {launcherUpdate.devBuild ? (
                    <Badge colorPalette="purple" size="sm">
                      DevBuild {launcherUpdate.devBuildId}
                    </Badge>
                  ) : launcherUpdate.unknownVersion ? (
                    <Badge colorPalette="purple" size="sm">BETA</Badge>
                  ) : null}
                </HStack>
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
    </Box>
  );

  const versionBusy =
    versionOperationRunning ||
    (versionStatus?.operationRunning ?? false) ||
    versionParallelSaving;
  const versionChainDepth = versionStatus ? versionDeltaChainDepth(versionStatus.versions) : 0;
  /** 版本操作进度：游戏版本页和历史版本页共用，从历史页还原时也能看到。 */
  const versionOperationProgressContent = versionOperation ? (
    <Box className="pz-version-section">
      <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
        <Text color="gray.800" fontSize="sm" fontWeight="700">
          {versionOperation.status === "completed"
            ? "上次操作完成"
            : versionOperation.status === "error"
              ? "上次操作失败"
              : "正在处理…"}
        </Text>
        <Text color="gray.500" fontSize="xs">
          {versionOperation.phase !== "mods" && versionOperation.totalFiles > 0
            ? `${versionOperation.processedFiles} / ${versionOperation.totalFiles} 个文件 · `
            : ""}
          {versionProgressPercent(versionOperation)}%{versionStepLabel(versionOperation)}
        </Text>
      </HStack>
      <Box bg="gray.200" borderRadius="full" h="2" mt="2" overflow="hidden">
        <Box
          bg={versionOperation.status === "error" ? "red.400" : "blue.500"}
          h="full"
          transition="width 0.2s"
          width={`${versionProgressPercent(versionOperation)}%`}
        />
      </Box>
      {versionTimingLabel(versionOperation.status) ? (
        <Text color="gray.500" fontSize="xs" mt="2">
          {versionTimingLabel(versionOperation.status)}
        </Text>
      ) : null}
      <Text
        color={versionOperation.status === "error" ? "red.600" : "gray.600"}
        fontSize="xs"
        mt="2"
        whiteSpace="pre-wrap"
      >
        {versionOperation.status === "error"
          ? versionOperation.error ?? "版本操作失败"
          : versionOperation.message ?? `${versionPhaseLabel(versionOperation.phase)}…`}
      </Text>
      {versionOperation.currentPath && versionOperation.status !== "completed" ? (
        <Text color="gray.400" fontSize="xs" mt="1" overflowWrap="anywhere">
          {versionOperation.currentPath}
        </Text>
      ) : null}
    </Box>
  ) : null;
  // 「当前状态」紧跟在「游戏版本」标题下方：一眼就能看到当前目录和保存过的版本是否一致。
  const versionCurrentStatusContent = versionStatus ? (
    <Box className="pz-version-section pz-version-status-block">
      <HStack align="center" gap="2" wrap="wrap">
        <Text color="gray.800" fontSize="sm" fontWeight="700">当前状态</Text>
        <Badge colorPalette={versionStatus.currentMatch.kind === "unknown" ? "orange" : "blue"} size="sm">
          {versionMatchLabel(versionStatus.currentMatch, versionStatus.versions)}
        </Badge>
      </HStack>
      <Text color="gray.500" fontSize="xs" mt="1" whiteSpace="pre-wrap">{versionStatus.currentMatch.message}</Text>
      {versionVerified ? (
        <Text
          color={versionVerified.matchedVersionId ? "green.700" : "red.600"}
          fontSize="xs"
          mt="2"
        >
          上次检查：
          {versionVerified.matchedVersionId
            ? `和「${versionLabelById(versionStatus.versions, versionVerified.matchedVersionId)}」完全一致`
            : "和保存过的版本都不一样"}
          {developerMode ? `（${shortVersionHash(versionVerified.rootHash)}）` : ""}
        </Text>
      ) : null}
      {versionStatus.pendingChanges ? (
        <Text color="orange.700" fontSize="xs" mt="2">
          和最新版本相比：新增 {versionStatus.pendingChanges.added} · 删除 {versionStatus.pendingChanges.removed} · 修改 {versionStatus.pendingChanges.modified}。
        </Text>
      ) : null}
    </Box>
  ) : null;
  const gameVersionSettingsContent = (
    <>
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="0">
          <HStack align="center" gap="2" wrap="wrap">
            <Text color="gray.900" fontSize="lg" fontWeight="700">游戏版本</Text>
            <HintDot
              label="游戏版本说明"
              text={"把整个游戏目录存成一个版本，以后随时能切回来。第一次是完整保存，之后只记录改动过的文件。\n\n保存或还原前请先关闭游戏。也建议在 Steam 里把 Project Zomboid 设为「仅在我启动此游戏时更新」，免得在后台被自动更新。"}
            />
          </HStack>
        </Box>
        <Button
          disabled={versionBusy}
          loading={versionStatusLoading}
          onClick={() => void refreshVersionStatus()}
          size="sm"
          variant="outline"
        >
          刷新
        </Button>
      </HStack>

      {versionCurrentStatusContent}

      {versionStatusError ? (
        <Text color="red.500" fontSize="xs" mt="3" whiteSpace="pre-wrap">读取失败：{versionStatusError}</Text>
      ) : null}
      {/* 首次读取用骨架屏占位（复用资源中心那套）；刷新时 versionStatus 已在，不重播。 */}
      <SkeletonGate
        loading={versionStatusLoading && !versionStatus}
        skeleton={<SkeletonList rows={5} lines={2} />}
      >
      {versionStatus ? (
        <>
          <Box className="pz-version-section">
            <Text color="gray.800" fontSize="sm" fontWeight="700">游戏信息</Text>
            <Box className="pz-version-facts" mt="1">
              <Box className="pz-version-fact">
                <Text color="gray.500" fontSize="xs">游戏位置</Text>
                <Text color="gray.700" fontSize="xs" overflowWrap="anywhere">
                  {versionStatus.gamePath ?? "没找到"}
                </Text>
              </Box>
              <Box className="pz-version-fact">
                <Text color="gray.500" fontSize="xs">当前游戏版本</Text>
                <Text color="gray.700" fontSize="xs">
                  {versionStatus.steamBuildId ?? "未知"}
                  {versionStatus.steamUpdatePending ? " · 和上次保存的不同" : ""}
                </Text>
              </Box>
              <Box className="pz-version-fact">
                <Text color="gray.500" fontSize="xs">上次保存的版本</Text>
                <Text color="gray.700" fontSize="xs">{versionStatus.lastCapturedBuildId ?? "—"}</Text>
              </Box>
              <Box className="pz-version-fact">
                <Text color="gray.500" fontSize="xs">更新通道</Text>
                <Text color="gray.700" fontSize="xs">{versionStatus.branch}</Text>
              </Box>
            </Box>
          </Box>

          <Box className="pz-version-section">
            <HStack align="center" gap="2" wrap="wrap">
              <Text color="gray.800" fontSize="sm" fontWeight="700">保存版本</Text>
              <HintDot
                label="保存版本说明"
                text={"「完整保存」把当前文件全部复制一份，占空间但最稳；「保存变化」只记录和上次不同的文件，省空间。都在后台进行，进度见下方。\n\n装了 PZ Optimization 时，还原前会自动卸载、完成后重新安装；想保留原版状态请先到「支持库」卸载。"}
              />
            </HStack>
            <HStack align="center" gap="2" mt="2" wrap="wrap">
              <Input
                aria-label="版本备注"
                maxW="240px"
                onChange={(event) => setVersionLabelDraft(event.currentTarget.value)}
                placeholder="给这个版本起个名字（可选）"
                size="sm"
                value={versionLabelDraft}
              />
              <Button
                colorPalette="blue"
                disabled={!versionStatus.gamePath || versionBusy}
                onClick={() => void createVersionSnapshot()}
                size="sm"
              >
                {versionStatus.versions.length > 0 ? "再完整保存一份" : "完整保存"}
              </Button>
              <Button
                disabled={!versionStatus.gamePath || versionStatus.versions.length === 0 || versionBusy}
                onClick={captureVersion}
                size="sm"
                variant="outline"
              >
                保存变化
              </Button>
              <Button
                disabled={!versionStatus.gamePath || versionStatus.versions.length === 0 || versionBusy}
                onClick={verifyVersion}
                size="sm"
                variant="outline"
              >
                检查当前版本
              </Button>
              {versionBusy ? (
                <Button colorPalette="red" onClick={cancelVersionOperation} size="sm" variant="outline">
                  中断
                </Button>
              ) : null}
            </HStack>
            {versionChainDepth >= 5 ? (
              <Text color="orange.700" fontSize="xs" mt="1">
                已经连续保存了 {versionChainDepth} 次变化，建议再做一次「完整保存」，还原会更快。
              </Text>
            ) : null}
          </Box>

          <Box className="pz-version-section">
            <HStack align="center" gap="2" wrap="wrap">
              <Text color="gray.800" fontSize="sm" fontWeight="700">备份保存位置</Text>
              <HintDot
                label="备份保存位置说明"
                text={"默认放在用户目录的 Zomboid\\.pzmanager\\versions。\n\n换位置不会搬走已有的备份，需要自己复制过去。"}
              />
            </HStack>
            <HStack gap="2" mt="1" wrap="wrap">
              <Input
                aria-label="备份保存位置"
                flex="1"
                minW="240px"
                onChange={(event) => setVersionStoreDraft(event.currentTarget.value)}
                placeholder="留空使用默认位置"
                ref={versionStoreInputRef}
                size="sm"
                value={versionStoreDraft}
              />
              <Button onClick={() => void chooseVersionStoreDirectory()} size="sm" variant="outline">浏览</Button>
              <Button disabled={versionBusy} loading={versionStoreSaving} onClick={() => void saveVersionStore()} size="sm" variant="outline">保存</Button>
            </HStack>
            {versionStoreOnSystemDrive(versionStorePath) ? (
              <HStack align="center" gap="2" mt="1" wrap="wrap">
                <Text color="orange.700" fontSize="xs">
                  备份在 C 盘，版本文件会占用系统盘空间，建议换到其它盘。
                </Text>
                {versionDriveNoticeDismissed ? (
                  <Button
                    className="pz-text-button"
                    onClick={() => void saveVersionDriveNoticeDismissed(false)}
                    size="xs"
                    variant="ghost"
                  >
                    重新开启换盘提示
                  </Button>
                ) : null}
              </HStack>
            ) : null}
          </Box>

          <Box className="pz-version-section">
            <HStack align="center" gap="4" justify="space-between" wrap="wrap">
              <Box flex="1" minW="260px">
                <HStack align="center" gap="2" wrap="wrap">
                  <Text color="gray.800" fontSize="sm" fontWeight="700">多线程加速</Text>
                  <HintDot
                    label="多线程加速说明"
                    text="同时读写多个文件，保存和还原会快一些。机械硬盘反而变慢时可以关掉，改成逐个处理。"
                  />
                </HStack>
              </Box>
              <Switch.Root
                aria-label="多线程加速"
                checked={versionParallel}
                disabled={versionBusy}
                onCheckedChange={(details) => void updateVersionParallel(details.checked)}
              >
                <Switch.HiddenInput />
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
              </Switch.Root>
            </HStack>
          </Box>

          <Box className="pz-version-section">
            <HStack align="center" gap="4" justify="space-between" wrap="wrap">
              <Box flex="1" minW="260px">
                <HStack align="center" gap="2" wrap="wrap">
                  <Text color="gray.800" fontSize="sm" fontWeight="700">保存时同时备份模组</Text>
                  <HintDot
                    label="保存时同时备份模组说明"
                    text="保存游戏版本时，把当前启用的模组也各存一份；以后还原这个游戏版本，模组会一起回到当时的样子。没启用的模组不受影响。"
                  />
                </HStack>
              </Box>
              <Switch.Root
                aria-label="保存时同时备份模组"
                checked={versionIncludeMods}
                disabled={versionIncludeModsSaving || versionBusy}
                onCheckedChange={(details) => void updateVersionIncludeMods(details.checked)}
              >
                <Switch.HiddenInput />
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
              </Switch.Root>
            </HStack>
          </Box>

          {versionStatus.steamUpdatePending ? (
            <Box className="pz-version-notice">
              <Text color="orange.800" fontSize="sm" fontWeight="700">Steam 可能更新过游戏</Text>
              <Text color="orange.700" fontSize="xs" mt="1">
                Steam 记录的游戏版本是 {versionStatus.steamBuildId}，上次保存的是 {versionStatus.lastCapturedBuildId ?? "未知"}。
                建议先「保存变化」把更新后的文件记下来；还原旧版本不会改变 Steam 的更新状态。
              </Text>
            </Box>
          ) : null}

          {versionOperationProgressContent}
        </>
      ) : null}
      </SkeletonGate>
    </>
  );

  /** 「历史版本」分页：只展示游戏版本的版本链，保存与备份设置留在「游戏版本」。 */
  const versionHistorySettingsContent = (
    <>
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="0">
          <HStack align="center" gap="2" wrap="wrap">
            <Text color="gray.900" fontSize="lg" fontWeight="700">历史版本</Text>
            <HintDot
              label="历史版本说明"
              text={"保存过的游戏版本都在这里，最新的在最上面。\n\n可以查看每个版本改了哪些文件、还原到这个版本，或删除最新的版本。还原前请先关闭游戏。"}
            />
          </HStack>
          <Text color="gray.500" fontSize="xs" mt="1">
            {versionStatus ? `${versionStatus.versions.length} 个版本` : "正在读取版本信息…"}
          </Text>
        </Box>
        <Button
          disabled={versionBusy}
          loading={versionStatusLoading}
          onClick={() => void refreshVersionStatus()}
          size="sm"
          variant="outline"
        >
          刷新
        </Button>
      </HStack>

      {versionStatusError ? (
        <Text color="red.500" fontSize="xs" mt="3" whiteSpace="pre-wrap">读取失败：{versionStatusError}</Text>
      ) : null}
      {/* 首次读取用骨架屏占位（复用资源中心那套）；刷新时 versionStatus 已在，不重播。 */}
      <SkeletonGate
        loading={versionStatusLoading && !versionStatus}
        skeleton={<SkeletonList rows={4} lines={2} />}
      >
      {versionStatus ? (
        <>
          {versionOperationProgressContent}
          <Box className="pz-version-section">
            {versionStatus.versions.length === 0 ? (
              <Text color="gray.500" fontSize="sm" mt="3">
                还没保存过版本，先到「游戏版本」里点「完整保存」。
              </Text>
            ) : (
              <Box className="pz-version-list" mt="2">
                {versionStatus.versions
                  .slice()
                  .reverse()
                  .map((record, index) => {
                    const changeLog = record.changeLog;
                    const isLatest = index === 0;
                    const isCurrent = versionStatus.currentMatch.versionId === record.id;
                    const expanded = versionExpandedId === record.id;
                    const changeLogLabel = changeLog
                      ? `新增 ${changeLog.added.length}、删除 ${changeLog.removed.length}、修改 ${changeLog.modified.length} 个文件`
                      : "";
                    const toggleChangeLog = () => {
                      if (expanded) {
                        // 先让 grid 收拢，动画结束后再卸载内容。
                        setVersionClosingId(record.id);
                        setVersionExpandedId(null);
                        window.setTimeout(() => {
                          setVersionClosingId((current) =>
                            current === record.id ? null : current,
                          );
                        }, VERSION_DIFF_EXIT_MS);
                      } else {
                        setVersionClosingId(null);
                        setVersionExpandedId(record.id);
                      }
                    };
                    return (
                      <Box className="pz-version-entry" key={record.id}>
                        <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
                          <Box flex="1" minW="240px">
                            <HStack align="center" gap="2" justify="space-between" wrap="nowrap">
                              <Text color="gray.900" flex="1" fontSize="sm" fontWeight="700" lineClamp={1} minW="0" title={record.label}>
                                {record.label}
                              </Text>
                              <HStack align="center" flexShrink="0" gap="1" justify="flex-end" wrap="wrap">
                                <Badge colorPalette={record.baseType === "full" ? "purple" : "blue"} size="sm">
                                  {versionBaseTypeLabel(record.baseType, record.parentId)}
                                </Badge>
                                {isCurrent ? <Badge colorPalette="green" size="sm">当前</Badge> : null}
                                {record.steamBuildId && developerMode ? (
                                  <Badge size="sm" variant="subtle">build {record.steamBuildId}</Badge>
                                ) : null}
                                {(record.modVersions?.length ?? 0) > 0 ? (
                                  <Badge colorPalette="teal" size="sm" variant="subtle">
                                    含 {record.modVersions?.length} 个模组
                                  </Badge>
                                ) : null}
                              </HStack>
                            </HStack>
                            <Text color="gray.500" fontSize="xs" mt="1">
                              {formatVersionTime(record.createdAt)} · {record.fileCount} 个文件 · {formatVersionBytes(record.totalBytes)}
                              {developerMode ? ` · ${shortVersionHash(record.rootHash)}` : ""}
                            </Text>
                            <Text color="gray.500" fontSize="xs" mt="0.5">
                              {changeLog
                                ? `相比上一个版本：${versionChangeSummary(changeLog)}`
                                : "完整备份"}
                            </Text>
                          </Box>
                          <HStack align="center" gap="2" wrap="wrap">
                            <Button
                              disabled={versionBusy}
                              onClick={() => void restoreVersion(record)}
                              size="xs"
                              variant="outline"
                            >
                              还原到这个版本
                            </Button>
                            {isLatest ? (
                              <Button
                                colorPalette="red"
                                disabled={versionBusy}
                                onClick={() => void deleteVersion(record)}
                                size="xs"
                                variant="ghost"
                              >
                                删除
                              </Button>
                            ) : null}
                          </HStack>
                        </HStack>
                        {changeLog ? (
                          <Box
                            aria-hidden={!expanded}
                            className={`pz-version-expand-shell${expanded ? " is-expanded" : ""}`}
                          >
                            <Box className="pz-version-expand">
                              {expanded || versionClosingId === record.id ? (
                                <Box className="pz-version-entry-diff">
                            <Stack gap="2">
                              {changeLog.added.length > 0 ? (
                                <Box>
                                  <Text color="green.700" fontSize="xs" fontWeight="700">新增 {changeLog.added.length}</Text>
                                  {changeLog.added.slice(0, 200).map((entry) => (
                                    <Text
                                      color="gray.600"
                                      fontFamily="mono"
                                      fontSize="xs"
                                      key={`added-${entry.path}`}
                                      overflowWrap="anywhere"
                                      pl="2"
                                    >
                                      {entry.path} · {formatVersionBytes(entry.size)}
                                    </Text>
                                  ))}
                                  {changeLog.added.length > 200 ? (
                                    <Text color="gray.400" fontSize="xs" pl="2">还有 {changeLog.added.length - 200} 项…</Text>
                                  ) : null}
                                </Box>
                              ) : null}
                              {changeLog.modified.length > 0 ? (
                                <Box>
                                  <Text color="orange.700" fontSize="xs" fontWeight="700">修改 {changeLog.modified.length}</Text>
                                  {changeLog.modified.slice(0, 200).map((entry) => (
                                    <Text
                                      color="gray.600"
                                      fontFamily="mono"
                                      fontSize="xs"
                                      key={`modified-${entry.path}`}
                                      overflowWrap="anywhere"
                                      pl="2"
                                    >
                                      {entry.path} · {formatVersionBytes(entry.oldSize)} → {formatVersionBytes(entry.newSize)}
                                    </Text>
                                  ))}
                                  {changeLog.modified.length > 200 ? (
                                    <Text color="gray.400" fontSize="xs" pl="2">还有 {changeLog.modified.length - 200} 项…</Text>
                                  ) : null}
                                </Box>
                              ) : null}
                              {changeLog.removed.length > 0 ? (
                                <Box>
                                  <Text color="red.600" fontSize="xs" fontWeight="700">删除 {changeLog.removed.length}</Text>
                                  {changeLog.removed.slice(0, 200).map((path) => (
                                    <Text
                                      color="gray.600"
                                      fontFamily="mono"
                                      fontSize="xs"
                                      key={`removed-${path}`}
                                      overflowWrap="anywhere"
                                      pl="2"
                                    >
                                      {path}
                                    </Text>
                                  ))}
                                  {changeLog.removed.length > 200 ? (
                                    <Text color="gray.400" fontSize="xs" pl="2">还有 {changeLog.removed.length - 200} 项…</Text>
                                  ) : null}
                                </Box>
                              ) : null}
                              {developerMode && (changeLog.fromBuildId || changeLog.toBuildId) ? (
                                <Text color="gray.400" fontSize="xs">
                                  build {changeLog.fromBuildId ?? "未知"} → {changeLog.toBuildId ?? "未知"}
                                </Text>
                              ) : null}
                            </Stack>
                                </Box>
                              ) : null}
                            </Box>
                          </Box>
                        ) : null}
                        {changeLog ? (
                          <HStack justify="flex-end" mt="1.5">
                            <Button
                              aria-expanded={expanded}
                              aria-label={`${expanded ? "收起改动" : "查看改动"}：${changeLogLabel}`}
                              className={`pz-version-diff-button${expanded ? " is-expanded" : ""}`}
                              onClick={toggleChangeLog}
                              size="xs"
                              title={`${expanded ? "收起改动" : "查看改动"}：${changeLogLabel}`}
                              variant="ghost"
                            >
                              <Text as="span" className="pz-version-diff-add">
                                +{changeLog.added.length}
                              </Text>
                              <Text as="span" className="pz-version-diff-remove">
                                −{changeLog.removed.length}
                              </Text>
                              <Text as="span" className="pz-version-diff-modify">
                                ~{changeLog.modified.length}
                              </Text>
                              <Text aria-hidden="true" as="span" className="pz-version-diff-chevron">
                                {expanded ? "▴" : "▾"}
                              </Text>
                            </Button>
                          </HStack>
                        ) : null}
                      </Box>
                    );
                  })}
              </Box>
            )}
          </Box>
        </>
      ) : null}
      </SkeletonGate>
    </>
  );

  const modVersionBusy =
    modVersionOperationRunning || versionOperationRunning || (versionStatus?.operationRunning ?? false);
  const filteredModVersionTargets = filterModVersionTargets(
    modVersionTargets ?? [],
    modVersionSearch,
    modVersionFilter,
  );
  const isolatedModCount = (modVersionTargets ?? []).filter((target) => target.versionCount > 0).length;
  const modVersionFilters: Array<{ label: string; value: ModVersionFilter }> = [
    { label: "全部", value: "all" },
    { label: "已记录", value: "isolated" },
    { label: "创意工坊", value: "workshop" },
    { label: "本地", value: "local" },
  ];
  const modVersionSettingsContent = (
    <>
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="0">
          <HStack align="center" gap="2" wrap="wrap">
            <Text color="gray.900" fontSize="lg" fontWeight="700">模组版本</Text>
            <HintDot
              label="模组版本说明"
              text={"每个模组单独保存版本：第一次是完整保存，之后只记录改动的文件。也可以到「模组管理」里右键某个模组，选「版本记录」。\n\n保存和检查随时都能做；还原前请先关闭游戏。创意工坊的模组可能被 Steam 校验或更新覆盖掉。"}
            />
          </HStack>
        </Box>
        <HStack gap="1" wrap="wrap">
          <Button
            colorPalette="blue"
            disabled={modVersionBusy || !modVersionTargets?.length}
            onClick={saveAllModVersions}
            size="sm"
            title="把所有模组各存一个版本，记成一条可整套还原的整合包"
          >
            一键保存全部模组
          </Button>
          {/* 保存整合包耗时可能很长，中断入口放在页首按钮排，不用滚到底部进度框找。 */}
          {modVersionOperationRunning ? (
            <Button colorPalette="red" onClick={cancelVersionOperation} size="sm" variant="outline">
              中断
            </Button>
          ) : null}
          <Button
            disabled={modVersionBusy}
            loading={modVersionTargetsLoading}
            onClick={() => void loadModVersionTargets()}
            size="sm"
            variant="outline"
          >
            刷新
          </Button>
        </HStack>
      </HStack>

      <Box className="pz-version-section">
        <HStack align="center" gap="3" justify="space-between" wrap="wrap">
          <Text color="gray.800" fontSize="sm" fontWeight="700">整合包</Text>
          <Text color="gray.500" fontSize="xs">{modVersionPacks?.length ?? 0} 个</Text>
        </HStack>
        {/* 首次读取用骨架屏占位；加载完（哪怕 0 条）才显示空状态文案。 */}
        <SkeletonGate loading={!modVersionPacks} skeleton={<SkeletonList rows={2} lines={1} />}>
        {!modVersionPacks || modVersionPacks.length === 0 ? (
          <Text color="gray.500" fontSize="sm" mt="2">
            还没有整合包。点「一键保存全部模组」把当前所有模组存成一份，之后可以整套还原。
          </Text>
        ) : (
          <Box className="pz-version-mod-list" mt="2">
            {[...modVersionPacks].reverse().map((pack) => (
              <Box className="pz-version-mod-row" key={pack.id}>
                <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
                  <Box flex="1" minW="220px">
                    <HStack align="center" gap="2" justify="space-between" wrap="nowrap">
                      <Text color="gray.900" flex="1" fontSize="sm" fontWeight="700" lineClamp={1} minW="0" title={pack.label}>{pack.label}</Text>
                      <HStack align="center" flexShrink="0" gap="1" justify="flex-end" wrap="wrap">
                        <Badge size="sm" variant="subtle">{pack.mods.length} 个模组</Badge>
                      </HStack>
                    </HStack>
                    <Text color="gray.400" fontSize="xs" mt="0.5">
                      创建于 {formatVersionTime(pack.createdAt)}
                    </Text>
                  </Box>
                  <HStack gap="1" wrap="wrap">
                    <Button
                      disabled={modVersionBusy}
                      onClick={() => restoreModVersionPack(pack)}
                      size="xs"
                      variant="outline"
                    >
                      还原整套
                    </Button>
                    <Button
                      disabled={modVersionBusy}
                      onClick={() => renameModVersionPack(pack)}
                      size="xs"
                      variant="ghost"
                    >
                      重命名
                    </Button>
                    <Button
                      disabled={modVersionBusy}
                      onClick={() => void deleteModVersionPack(pack)}
                      size="xs"
                      variant="ghost"
                    >
                      删除
                    </Button>
                  </HStack>
                </HStack>
              </Box>
            ))}
          </Box>
        )}
        </SkeletonGate>
      </Box>

      <Box className="pz-version-section">
        <HStack gap="2" wrap="wrap">
          <Input
            aria-label="搜索模组版本"
            flex="1"
            minW="220px"
            onChange={(event) => setModVersionSearch(event.currentTarget.value)}
            placeholder="搜索模组名称、ID 或路径"
            size="sm"
            value={modVersionSearch}
          />
          <HStack gap="1" wrap="wrap">
            {modVersionFilters.map((item) => (
              <Button
                aria-pressed={modVersionFilter === item.value}
                className={`pz-version-filter${modVersionFilter === item.value ? " is-active" : ""}`}
                key={item.value}
                onClick={() => setModVersionFilter(item.value)}
                size="xs"
                variant="ghost"
              >
                {item.label}
              </Button>
            ))}
          </HStack>
        </HStack>
        <Text color="gray.500" fontSize="xs" mt="1">
          共 {modVersionTargets?.length ?? 0} 个模组，{isolatedModCount} 个保存过版本
          {modVersionTargets ? `；当前显示 ${filteredModVersionTargets.length} 个` : ""}。
        </Text>
      </Box>

      <Box className="pz-version-section">
        {/* 首次读取用骨架屏占位（模组行带海报占位）；刷新时 targets 已在，不重播。 */}
        <SkeletonGate
          loading={modVersionTargetsLoading && !modVersionTargets}
          skeleton={<SkeletonList rows={6} thumb="poster" />}
        >
        {!modVersionTargets ? (
          <Text color="gray.500" fontSize="sm">尚未读取模组列表。</Text>
        ) : filteredModVersionTargets.length === 0 ? (
          <Text color="gray.500" fontSize="sm">没有符合条件的模组。</Text>
        ) : (
          <Box className="pz-version-mod-list">
            {filteredModVersionTargets.map((target) => {
              const expanded = modVersionExpandedKey === target.key;
              const status = modVersionStatuses[target.rootPath];
              return (
                <Box className={`pz-version-mod-row${expanded ? " is-expanded" : ""}`} key={target.key}>
                  <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
                    <Box flex="1" minW="220px">
                      <HStack align="center" gap="2" justify="space-between" wrap="nowrap">
                        <Text color="gray.900" flex="1" fontSize="sm" fontWeight="700" lineClamp={1} minW="0" title={target.name}>{target.name}</Text>
                        <HStack align="center" flexShrink="0" gap="1" justify="flex-end" wrap="wrap">
                          <Badge size="sm" variant="subtle">{modSourceLabel(target.source)}</Badge>
                          <Badge
                            colorPalette={target.versionCount > 0 ? "blue" : "gray"}
                            size="sm"
                            variant={target.versionCount > 0 ? "solid" : "subtle"}
                          >
                            {target.versionCount > 0 ? `${target.versionCount} 个版本` : "还没保存过"}
                          </Badge>
                        </HStack>
                      </HStack>
                      <HStack align="center" gap="1" mt="0.5" wrap="wrap">
                        <Text color="gray.400" fontSize="xs">ID · {target.modId || "未知"}</Text>
                        <HintDot label={`${target.name} 的模组目录`} text={target.rootPath} />
                        {target.latestCreatedAt ? (
                          <Text color="gray.400" fontSize="xs">
                            · 上次保存 {formatVersionTime(target.latestCreatedAt)}
                          </Text>
                        ) : null}
                      </HStack>
                    </Box>
                    <HStack gap="1" wrap="wrap">
                      <Button
                        disabled={modVersionBusy}
                        onClick={() => recordModVersion(target)}
                        size="xs"
                        variant="outline"
                      >
                        {target.versionCount > 0 ? "保存变化" : "完整保存"}
                      </Button>
                      <Button
                        aria-expanded={expanded}
                        className="pz-text-button"
                        disabled={modVersionBusy}
                        onClick={() => toggleModVersionExpansion(target.key, target.rootPath)}
                        size="xs"
                        variant="ghost"
                      >
                        {expanded ? "收起" : "详情"}
                      </Button>
                      <Button
                        className="pz-text-button"
                        onClick={() => openModVersionDialog(target.rootPath, target.name)}
                        size="xs"
                        variant="ghost"
                      >
                        版本记录
                      </Button>
                    </HStack>
                  </HStack>

                  <Box
                    aria-hidden={!expanded}
                    className={`pz-version-expand-shell${expanded ? " is-expanded" : ""}`}
                  >
                    <Box className="pz-version-expand">
                      {expanded || modVersionClosingKey === target.key ? (
                        <Box className="pz-version-mod-detail">
                      {/* 首次展开按需读取状态，用骨架占位；读完（含失败提示后）显示正文。 */}
                      <SkeletonGate loading={!status} skeleton={<SkeletonList rows={2} lines={2} />}>
                      {!status ? null : (
                        <>
                          {status.versions.length > 0 ? (
                            <HStack align="center" gap="2" wrap="wrap">
                              <Badge
                                colorPalette={status.currentMatch.kind === "unknown" ? "orange" : "blue"}
                                size="sm"
                              >
                                {versionMatchLabel(status.currentMatch, status.versions)}
                              </Badge>
                              <Text color="gray.500" fontSize="xs">{status.currentMatch.message}</Text>
                            </HStack>
                          ) : null}
                          {status.pendingChanges ? (
                            <Text color="orange.700" fontSize="xs" mt="1">
                              与最新版本相比：新增 {status.pendingChanges.added} · 删除 {status.pendingChanges.removed} · 修改 {status.pendingChanges.modified}。
                            </Text>
                          ) : null}
                          <HStack gap="1" mt="2" wrap="wrap">
                            <Button
                              disabled={modVersionBusy}
                              onClick={() => verifyModVersion(target.rootPath, target.name)}
                              size="xs"
                              variant="outline"
                            >
                              检查当前版本
                            </Button>
                          </HStack>
                          <ModVersionTimeline
                            busy={modVersionBusy}
                            currentMatch={status.currentMatch}
                            onDelete={(record) => void deleteModVersion(target.rootPath, target.name, record)}
                            onRestore={(record) => restoreModVersion(target.rootPath, target.name, record)}
                            versions={status.versions}
                          />
                        </>
                      )}
                      </SkeletonGate>
                        </Box>
                      ) : null}
                    </Box>
                  </Box>
                </Box>
              );
            })}
          </Box>
        )}
        </SkeletonGate>
      </Box>

      {modVersionOperation ? (
        <Box className="pz-version-section">
          <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
            <Text color="gray.800" fontSize="sm" fontWeight="700">
              {modVersionOperation.status === "completed"
                ? "上次操作完成"
                : modVersionOperation.status === "error"
                  ? "上次操作失败"
                  : "正在保存模组版本"}
            </Text>
            <Text color="gray.500" fontSize="xs">
              {modVersionOperation.phase !== "mods" && modVersionOperation.totalFiles > 0
                ? `${modVersionOperation.processedFiles} / ${modVersionOperation.totalFiles} 个文件 · `
                : ""}
              {versionProgressPercent(modVersionOperation)}%{versionStepLabel(modVersionOperation)}
            </Text>
          </HStack>
          <Box bg="gray.200" borderRadius="full" h="2" mt="2" overflow="hidden">
            <Box
              bg={modVersionOperation.status === "error" ? "red.400" : "blue.500"}
              h="full"
              transition="width 0.2s"
              width={`${versionProgressPercent(modVersionOperation)}%`}
            />
          </Box>
          {versionTimingLabel(modVersionOperation.status) ? (
            <Text color="gray.500" fontSize="xs" mt="2">
              {versionTimingLabel(modVersionOperation.status)}
            </Text>
          ) : null}
          <Text
            color={modVersionOperation.status === "error" ? "red.600" : "gray.600"}
            fontSize="xs"
            mt="2"
            whiteSpace="pre-wrap"
          >
            {modVersionOperation.status === "error"
              ? modVersionOperation.error ?? "模组版本操作失败"
              : modVersionOperation.message ?? `${versionPhaseLabel(modVersionOperation.phase)}…`}
          </Text>
          {modVersionOperation.currentPath && modVersionOperation.status !== "completed" ? (
            <Text color="gray.400" fontSize="xs" mt="1" overflowWrap="anywhere">
              {modVersionOperation.currentPath}
            </Text>
          ) : null}
          {modVersionOperationRunning ? (
            <Button colorPalette="red" onClick={cancelVersionOperation} size="xs" mt="2" variant="outline">
              中断
            </Button>
          ) : null}
        </Box>
      ) : null}
    </>
  );

  const versionSections: Array<{ value: VersionTab; label: string; icon: typeof faHouse }> = [
    { value: "game", label: "概览", icon: faHouse },
    { value: "mods", label: "模组版本", icon: faCubes },
    { value: "history", label: "历史版本", icon: faClockRotateLeft },
    { value: "support", label: "支持库", icon: faScrewdriverWrench },
    { value: "shaderpacks", label: "光影包", icon: faSun },
    { value: "shots", label: "截图", icon: faImages },
  ];

  // 「版本」页：左侧分区列表 + 右侧内容（支持库 / 光影包 / 截图 已并入同一页）。
  const versionSettingsContent = (
    <Box className="pz-version-hub">
      <Box as="nav" aria-label="版本管理分类" className="pz-version-hub-sidebar">
        <Text className="pz-version-hub-sidebar-title">版本管理</Text>
        <Stack gap="1">
          {versionSections.map((section) => (
            <Button
              aria-current={versionTab === section.value ? "page" : undefined}
              className={`pz-version-hub-item${versionTab === section.value ? " is-active" : ""}`}
              key={section.value}
              onClick={() => switchVersionTab(section.value)}
              size="sm"
              variant="ghost"
            >
              <FontAwesomeIcon aria-hidden="true" className="pz-version-hub-icon" icon={section.icon} />
              <Box as="span">{section.label}</Box>
            </Button>
          ))}
        </Stack>
      </Box>
      <Box
        className="pz-version-hub-main"
        data-direction={versionTransitionDirection}
        key={versionTab}
      >
        {versionTab === "game" || versionTab === "mods" || versionTab === "history" ? (
          <Box className="pz-panel pz-version-settings-panel">
            {versionTab === "game"
              ? gameVersionSettingsContent
              : versionTab === "mods"
                ? modVersionSettingsContent
                : versionHistorySettingsContent}
          </Box>
        ) : versionTab === "support" ? (
          overviewContent
        ) : versionTab === "shaderpacks" ? (
          shaderpacksContent
        ) : (
          shotsContent
        )}
      </Box>
    </Box>
  );

  const toolboxContent = (
    <Box className="pz-panel pz-toolbox-page">
      <Box>
        <Text color="gray.900" fontSize="lg" fontWeight="700">百宝箱</Text>
        <Text color="gray.500" fontSize="xs" mt="1">
          翻译、下载和运行状态集中在这里，随时可用。卡片按单列全宽排列。
        </Text>
      </Box>
      <Box className="pz-toolbox-grid">
        <Box className="pz-toolbox-card">{translatorSettingsContent}</Box>
        <Box className="pz-toolbox-card">{downloaderSettingsContent}</Box>
        {consoleContent}
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
            onClick={() => switchSettingsPage("about")}
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
            onClick={() => switchSettingsPage("general")}
            size="sm"
            variant="ghost"
          >
            通用
          </Button>
          <Button
            aria-current={settingsPage === "toolbox" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "toolbox" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => switchSettingsPage("toolbox")}
            size="sm"
            variant="ghost"
          >
            百宝箱
          </Button>
          <Button
            aria-current={settingsPage === "logs" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "logs" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => switchSettingsPage("logs")}
            size="sm"
            variant="ghost"
          >
            日志
          </Button>
          <Button
            aria-current={settingsPage === "modLoader" ? "page" : undefined}
            className={`pz-resource-center-sidebar-item ${
              settingsPage === "modLoader" ? "pz-resource-center-sidebar-item-active" : ""
            }`}
            onClick={() => switchSettingsPage("modLoader")}
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
            onClick={() => switchSettingsPage("startup")}
            size="sm"
            variant="ghost"
          >
            启动
          </Button>
        </Stack>
      </Box>
      {/* key=settingsPage：切分页时强制重挂正文，换页动画才会重播；
          data-direction 决定从右滑入还是从左滑入。 */}
      <Box
        key={settingsPage}
        className="pz-resource-center-main"
        data-direction={settingsTransitionDirection}
      >
        {settingsPage === "about"
          ? aboutSettingsContent
          : settingsPage === "general"
            ? generalSettingsContent
            : settingsPage === "toolbox"
              ? toolboxContent
            : settingsPage === "logs"
              ? launcherLogSettingsContent
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
        onAutoSort={() => void autoSortLuaMods()}
        onClear={() => startTransition(() => setSelectedLuaModKeys(new Set()))}
        onInvert={invertVisibleLuaMods}
        onNegative={() => void setLuaModsEnabled(selectedLuaMods, false)}
        onPositive={() => void setLuaModsEnabled(selectedLuaMods, true)}
        onSelectAll={selectAllVisibleLuaMods}
        positiveLabel="启用"
        selectedCount={selectedLuaModCount}
        totalCount={filteredLuaMods?.length ?? 0}
      />
      <Box className={`pz-lua-mod-list${luaModListSettled ? " is-settled" : ""}`}>
        <SkeletonGate
          loading={luaModsLoading && !luaMods}
          skeleton={<SkeletonList rows={8} thumb="poster" />}
        >
        {filteredLuaMods && filteredLuaMods.length > 0 ? (
          filteredLuaMods.map((mod) => {
            const key = luaModKey(mod);
            return (
              <SleepingModRow
                key={key}
                render={() => {
                  const selected = selectedLuaModKeys.has(key);
                  const expanded = expandedLuaModKeys.has(key);
                  const busy =
                    luaModActionId === key || luaModBatchAction !== null || luaReorderSaving;
                  // 只统计真正参与加载顺序的模组：加载器固定在最前、不显示在列表里，不能算进置顶/置底的边界。
                  const orderIds = movableLuaModIds();
                  const orderIndex = orderIds.findIndex(
                    (entry) => entry.toLowerCase() === mod.id.toLowerCase(),
                  );
                  const reorderable = mod.enabled && luaModSortMode === "load";
                  const inReorderGroup =
                    luaReorder?.group.some(
                      (id) => id.toLowerCase() === mod.id.toLowerCase(),
                    ) ?? false;
                  const grabbed = luaReorder?.key === mod.id.toLowerCase();
                  const multiDrag = selected && selectedEnabledLuaModCount > 1;
                  const dragTitle = multiDrag
                    ? `拖动调整选中的 ${selectedEnabledLuaModCount} 个模组的加载顺序`
                    : "拖动调整加载顺序";
                  return (
              <ModRowContextMenu
                busy={busy}
                canEnable={mod.hasB42}
                enabled={mod.enabled}
                onDisable={() => void setLuaModsEnabled([mod], false)}
                onEnable={() => void setLuaModsEnabled([mod], true)}
                onOpenVersionHistory={() => openModVersionDialog(mod.path, mod.name)}
                onOpenWorkshop={(link) => void openWorkshopLink(link)}
                workshopId={mod.workshopId}
              >
              <Box
                aria-checked={selected}
                aria-label={`选择 ${mod.name}`}
                className={`pz-mod-row pz-lua-mod-row${selected ? " is-selected" : ""}${mod.enabled ? " is-enabled" : ""}${expanded ? " pz-jar-mod-row-expanded" : ""}${inReorderGroup ? " is-reordering" : ""}${grabbed ? " is-reordering-grabbed" : ""}`}
                data-lua-mod-id={mod.id}
                data-lua-mod-key={mod.id.toLowerCase()}
                data-lua-mod-sortable={reorderable ? "true" : undefined}
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
                    {reorderable && (
                      <Box
                        aria-label={`${multiDrag ? `拖动调整选中的 ${selectedEnabledLuaModCount} 个模组` : `拖动调整 ${mod.name}`} 的加载顺序`}
                        className={`pz-lua-mod-drag-handle${inReorderGroup ? " is-dragging" : ""}`}
                        onClick={(event) => event.stopPropagation()}
                        onPointerDown={(event) => beginLuaModReorder(event, mod)}
                        role="button"
                        title={dragTitle}
                      />
                    )}
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
                        aria-label={`置顶 ${mod.name}`}
                        className="pz-lua-mod-order-button"
                        disabled={busy || orderIndex <= 0}
                        onClick={(event) => {
                          event.stopPropagation();
                          void moveLuaModToEdge(mod, "top");
                        }}
                        size="xs"
                        title="置顶（移到加载顺序最前）"
                        variant="ghost"
                      >
                        <FontAwesomeIcon icon={faAnglesUp} />
                      </Button>
                      <Button
                        aria-label={`置底 ${mod.name}`}
                        className="pz-lua-mod-order-button"
                        disabled={busy || orderIndex < 0 || orderIndex >= orderIds.length - 1}
                        onClick={(event) => {
                          event.stopPropagation();
                          void moveLuaModToEdge(mod, "bottom");
                        }}
                        size="xs"
                        title="置底（移到加载顺序最后）"
                        variant="ghost"
                      >
                        <FontAwesomeIcon icon={faAnglesDown} />
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
              </ModRowContextMenu>
                  );
                }}
              />
            );
          })
        ) : (
          <EmptyState text={luaMods && luaMods.length > 0 ? "没有符合当前筛选条件的模组。" : "未发现本地或创意工坊 Lua 模组。"} />
        )}
        </SkeletonGate>
      </Box>
    </Box>
  );
  // 首次进入 JAR 页面、扫描还没发起（120ms 延迟窗口内）时也按加载态处理，
  // 避免先闪一帧「尚未扫描 JAR 模组」。
  const jarModsAwaitingFirstScan =
    modManagementPage === "jar" && jarMods === null && !jarModsScanStartedRef.current;
  const jarModsLoadingView = jarModsLoading || jarModsAwaitingFirstScan;
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
        ) : jarMods || jarModsLoadingView ? (
          <Stack className="pz-jar-mod-list" gap="2">
            <HStack justify="space-between" color="gray.500" fontSize="sm" mb="1" wrap="wrap">
              <Text>
                {jarModsLoadingView
                  ? developerMode
                    ? jarScanStatusLabel(jarScanFrame, jarScanProgress.scanned, jarScanProgress.total)
                    : "正在扫描模组…"
                  : developerMode
                    ? "扫描 mod.info 与 mod.info.disabled 中的 ZombieBuddy 注解"
                    : "已读取 JAR 模组"}
              </Text>
              <Text>{jarModsLoadingView ? `已发现 ${filteredJarMods?.length ?? 0} 个 JAR 模组` : `发现 ${filteredJarMods?.length ?? 0} 个 JAR 模组`}</Text>
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
            )}
            <SkeletonGate
              loading={jarModsLoadingView}
              skeleton={<SkeletonList rows={8} thumb="poster" />}
            >
            {filteredJarMods && filteredJarMods.length > 0 ? (
              filteredJarMods.map((mod) => {
                const modKey = jarModKey(mod);
                return (
                  <SleepingModRow
                    key={modKey}
                    render={() => {
                      const selected = selectedJarModKeys.has(modKey);
                      const expanded = expandedJarModKeys.has(modKey);
                      const enabled = mod.decision === true && !mod.disabled;
                      const busy = luaModBatchAction !== null || jarModBatchAction !== null;
                      const toggleExpanded = () => {
                        setExpandedJarModKeys((current) => toggleJarModExpansion(current, modKey));
                      };

                      return (
                  <ModRowContextMenu
                    busy={busy}
                    canEnable
                    enabled={enabled}
                    onDisable={() => void setJarModDecisions([mod], false)}
                    onEnable={() => void setJarModDecisions([mod], true)}
                    onOpenVersionHistory={() => openModVersionDialog(mod.modRoot, mod.id)}
                    onOpenWorkshop={(link) => void openWorkshopLink(link)}
                    workshopId={mod.workshopId}
                  >
                  <Box
                    aria-checked={selected}
                    aria-label={`选择 ${mod.id}`}
                    className={`pz-mod-row pz-jar-mod-row${selected ? " is-selected" : ""}${enabled ? " is-enabled" : ""}${mod.disabled || mod.decision === false ? " is-rejected" : ""}${expanded ? " pz-jar-mod-row-expanded" : ""}`}
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
                            aria-label={`${enabled ? "禁用" : "启用"} ${mod.id}`}
                            checked={enabled}
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
                  </ModRowContextMenu>
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
            </SkeletonGate>
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
                <ScrollingLabel text={preset.name} />
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
        onToggleTasks={() => (taskPanelOpen ? closeTaskPanel() : setTaskPanelOpen(true))}
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
          closing={taskPanelClosing}
          closingTaskIds={taskClosingIds}
          onClearFinished={clearFinishedTasks}
          onClose={closeTaskPanel}
          onInterruptVersion={cancelVersionOperation}
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
              activePage === "workshop" || activePage === "mods" || activePage === "settings" || activePage === "versions" ? "pz-app-content-resource-center" : ""
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
              {activePage === "versions" && versionSettingsContent}
              {activePage === "workshop" && resourceCenterContent}
              {activePage === "mods" && modsContent}
              {activePage === "settings" && settingsContent}
            </Stack>
          </Box>
        </Box>
      </Box>

      {fastLaunchWarningOpen ? (
        <FastLaunchWarningDialog
          closing={fastLaunchWarningClosing}
          onCancel={() => closeFastLaunchWarning(false)}
          onConfirm={() => closeFastLaunchWarning(true)}
        />
      ) : null}

      {pzOptRestoreWarning ? (
        <PzOptRestoreWarningDialog
          error={pzOptRestoreWarning}
          onClose={() => setPzOptRestoreWarning(null)}
          onGoSupport={() => {
            setPzOptRestoreWarning(null);
            navigateToPage("overview");
          }}
        />
      ) : null}

      {modPackNameDialog ? (
        <ModPackNameDialog
          busy={
            modPackNameDialog.mode === "save"
              ? modVersionOperationRunning || versionOperationRunning
              : modPackRenaming
          }
          closing={modPackNameDialogClosing}
          initialLabel={modPackNameDialog.pack?.label ?? ""}
          mode={modPackNameDialog.mode}
          onCancel={closeModPackNameDialog}
          onConfirm={(label) => {
            if (modPackNameDialog.mode === "save") {
              confirmSaveModPack(label);
            } else if (modPackNameDialog.pack) {
              void confirmRenameModPack(modPackNameDialog.pack, label);
            }
          }}
        />
      ) : null}

      {versionDriveNoticeOpen ? (
        <VersionStoreDriveDialog
          closing={versionDriveNoticeClosing}
          dontAskAgain={versionDriveNoticeDontAsk}
          onCheckDontAskAgain={setVersionDriveNoticeDontAsk}
          onClose={() => closeVersionDriveNotice(false)}
          onContinue={() => closeVersionDriveNotice(true)}
          onGoChange={goChangeVersionStoreDrive}
          storePath={versionStorePath}
        />
      ) : null}

      {modVersionDialog ? (
        <ModVersionDialog
          closing={modVersionDialogClosing}
          loading={modVersionDialogLoading}
          name={modVersionDialog.name}
          onClose={closeModVersionDialog}
          onDelete={(record) => void deleteModVersion(modVersionDialog.rootPath, modVersionDialog.name, record)}
          onRecord={() =>
            recordModVersion({
              rootPath: modVersionDialog.rootPath,
              name: modVersionDialog.name,
              versionCount: modVersionStatuses[modVersionDialog.rootPath]?.versions.length ?? 0,
            })
          }
          onRestore={(record) =>
            restoreModVersion(modVersionDialog.rootPath, modVersionDialog.name, record)
          }
          onVerify={() => verifyModVersion(modVersionDialog.rootPath, modVersionDialog.name)}
          status={modVersionStatuses[modVersionDialog.rootPath] ?? null}
        />
      ) : null}

      {/* 截图预览：亚克力弹窗配方与光影下载/工坊详情一致（Portal + 进出场动画）。
          尺寸收窄：预览图限高 52vh，弹窗宽度 min(560px, 100%)。 */}
      {preview && (
        <Portal>
          <Box
            className={`pz-resource-file-overlay ${previewClosing ? "is-closing" : ""}`}
            onClick={closeShotPreview}
          >
            <Box
              aria-label="截图预览"
              aria-modal="true"
              className="pz-resource-file-dialog pz-shot-preview-dialog"
              onClick={(event) => event.stopPropagation()}
              role="dialog"
            >
              <HStack justify="space-between" gap="2">
                <Text color="gray.900" fontWeight="700" overflowWrap="anywhere">
                  {preview.name}
                </Text>
                <Button aria-label="关闭截图预览" onClick={closeShotPreview} size="sm" variant="ghost">
                  ×
                </Button>
              </HStack>
              <Box className="pz-shot-preview-body" mt="3" textAlign="center">
                <FallbackImage
                  alt={preview.name}
                  src={preview.url}
                  style={{ display: "block", margin: "0 auto", maxHeight: "52vh", maxWidth: "100%" }}
                />
              </Box>
              <HStack justify="end" mt="3">
                <Button colorPalette="blue" loading={copyingShot} onClick={() => void copyPreview()}>
                  复制图片
                </Button>
              </HStack>
            </Box>
          </Box>
        </Portal>
      )}

      {/* 隐私协议同意框：内嵌官网 policy 页（wall=true 只渲染正文）。
          不可点遮罩关闭——不同意只能退出应用。 */}
      {showPrivacyConsent && (
        <Portal>
          <Box
            aria-modal="true"
            className="pz-privacy-overlay"
            position="fixed"
            role="dialog"
            zIndex="tooltip"
          >
            <Box
              aria-label="隐私协议"
              className="pz-resource-file-dialog pz-privacy-dialog"
              onClick={(event) => event.stopPropagation()}
              role="dialog"
            >
              <Text color="gray.900" fontSize="lg" fontWeight="700">隐私协议与使用条款</Text>
              <Text color="gray.500" fontSize="xs" mt="1">
                使用 PZManager 前请阅读并同意以下协议。页面加载不出来时，也可以直接看下方的要点。
              </Text>
              <Box className="pz-privacy-frame-wrap" mt="3">
                <iframe
                  className="pz-privacy-frame"
                  src="https://pzm.sanyec.top/policy.html?wall=true"
                  title="PZManager 隐私协议"
                />
              </Box>
              <Button
                className="pz-privacy-open-browser"
                onClick={() => void openUrl("https://pzm.sanyec.top/policy.html")}
                size="xs"
                variant="ghost"
              >
                在浏览器中打开完整页面 ↗
              </Button>
              <Stack gap="2" mt="3">
                <Checkbox.Root
                  checked={privacyAgreeCollect}
                  onCheckedChange={(details) => setPrivacyAgreeCollect(Boolean(details.checked))}
                >
                  <Checkbox.HiddenInput />
                  <Checkbox.Control>
                    <Checkbox.Indicator />
                  </Checkbox.Control>
                  <Checkbox.Label>同意信息收集（仅限协议中列出的本地数据与反馈内容）</Checkbox.Label>
                </Checkbox.Root>
                <Checkbox.Root
                  checked={privacyAgreeSend}
                  onCheckedChange={(details) => setPrivacyAgreeSend(Boolean(details.checked))}
                >
                  <Checkbox.HiddenInput />
                  <Checkbox.Control>
                    <Checkbox.Indicator />
                  </Checkbox.Control>
                  <Checkbox.Label>同意信息发送（仅在检查更新与你主动反馈时联网）</Checkbox.Label>
                </Checkbox.Root>
                <Checkbox.Root
                  checked={privacyTrapChecked}
                  onCheckedChange={(details) => setPrivacyTrapChecked(Boolean(details.checked))}
                >
                  <Checkbox.HiddenInput />
                  <Checkbox.Control>
                    <Checkbox.Indicator />
                  </Checkbox.Control>
                  <Checkbox.Label>我真的有认真看这个页面，所以我不会勾选此项</Checkbox.Label>
                </Checkbox.Root>
              </Stack>
              {privacyTrapChecked ? (
                <Text color="orange.600" fontSize="xs" mt="2">
                  欸，说好了不勾选的——把它去掉，再认真看一遍上面的协议。
                </Text>
              ) : null}
              <HStack justify="end" gap="2" mt="3">
                <Button colorPalette="red" onClick={() => void disagreePrivacyPolicy()} size="sm" variant="outline">
                  不同意并退出
                </Button>
                <Button
                  colorPalette="blue"
                  disabled={!(privacyAgreeCollect && privacyAgreeSend && !privacyTrapChecked)}
                  loading={privacyBusy}
                  onClick={() => void agreePrivacyPolicy()}
                  size="sm"
                >
                  同意并进入
                </Button>
              </HStack>
            </Box>
          </Box>
        </Portal>
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
                    将上传启动器日志（%USERPROFILE%\Zomboid\.pzmanager\logs\launcher.log，最近 1 MB）、
                    %TEMP%\overlay.log（最近一次游戏启动的部分）与 %USERPROFILE%\Zomboid\console.txt。
                    日志可能包含游戏路径、Steam ID 等信息，仅用于排查问题。
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
                  <Box mt="2">
                    <Text color="gray.500" fontSize="xs">
                      联系方式（选填，会附在描述末尾，方便回访）
                    </Text>
                    <Input
                      aria-label="联系方式"
                      disabled={feedbackSubmitting}
                      maxLength={200}
                      mt="1"
                      onChange={(event) => setFeedbackContact(event.currentTarget.value)}
                      placeholder="邮箱 / QQ / 群昵称，留空则不附"
                      size="sm"
                      value={feedbackContact}
                    />
                  </Box>
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