/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { NotificationKind } from "../developerMode";

export type SteamProfile = {
  steamId: string;
  accountName: string;
  personaName: string;
};

export type ProjectZomboidPaths = {
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
    /** 没匹配到当前版本的类文件，但找到了官方 install.ps1，可尝试强制安装。 */
    forceInstallAvailable: boolean;
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

export type DetectionState =
  | { status: "checking" }
  | { status: "found"; paths: ProjectZomboidPaths }
  | { status: "notFound" }
  | { status: "error"; message: string };

export type InstallResult = {
  changed: boolean;
  alreadyInstalled: boolean;
  backupPath: string | null;
};

export type PzOptimizationInstallResult = {
  changed: boolean;
  alreadyInstalled: boolean;
  sourcePath: string;
  installedRevision: string | null;
};

export type JarModBatchDecisionResult = {
  updated: number;
  luaModsEnabled: string[];
};

export type ModPresetJarMod = {
  modDir: string;
  modId: string;
  workshopId: number | null;
  packageName: string;
  enabled: boolean;
};

export type ModPreset = {
  id: string;
  name: string;
  luaModIds: string[];
  jarMods: ModPresetJarMod[];
};

export type ModPresetApplyResult = {
  luaEnabled: number;
  luaSkipped: string[];
  jarChanged: number;
};

export type AutoSortLuaModsResult = {
  backupPreset: string | null;
  moved: number;
  total: number;
};

export type LuaModBatchToggleResult = {
  enabled: boolean;
  changed: string[];
  added: string[];
  missingRequires: string[];
  dependents: string[];
  skipped: string[];
};

export type ManagedFile = {
  name: string;
  sizeBytes: number;
  modifiedAt: number | null;
};

export type ManagedDirectory = {
  directory: string;
  files: ManagedFile[];
};

export type LuaModEntry = {
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

export type ScannedJarMod = {
  id: string;
  infoId: string;
  modInfoPath: string;
  /** 整个模组目录（mod.info 在 common/ 或 42.20/ 时也指模组根），版本记录按它定位。 */
  modRoot: string;
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
  /** 模组字节码引用到的游戏类（zombie.* / fmod.*）。「注入项搜索」的数据源。 */
  classReferences: string[];
  decision: boolean | null;
};

export type JarModScanUpdate = {
  scanId: number;
  status: "started" | "item" | "completed" | "error";
  scanned: number;
  total: number;
  item: ScannedJarMod | null;
  error: string | null;
};

export type WorkshopBackgroundScanUpdate = {
  scanId: number;
  status: "started" | "item" | "completed" | "error";
  scanned: number;
  total: number;
  item: BackgroundEntry | null;
  error: string | null;
};

export type OverlaySessionUpdate = {
  phase: string;
  message: string;
  pid: number | null;
  processName: string | null;
  zbLines: number;
  elapsedMs: number;
  /** 游戏停止原因（与后端 StopReason 的 serde 序列化一致），仅「游戏进程已退出」时给出。 */
  stopReason?: string | null;
  /** 游戏进程退出码；进程消亡后拿不到时缺省。 */
  exitCode?: number | null;
};

export type OverlayZbProgress = {
  zbLines: number;
  phase: string;
  lastLine: string;
  elapsedMs: number;
};

export type StartupTiming = {
  lastMs: number | null;
  previousMs: number | null;
};

/** 「已提交的反馈」列表里的一条（本机记录，见 feedback.rs）。 */
export type FeedbackHistoryEntry = {
  id: string;
  /** Unix 秒。 */
  sentAt: number;
  /** 描述摘要，方便认出是哪次反馈。 */
  summary: string;
};

export type LauncherUpdateStatus = {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  /** 自身版本比更新源还新（未发布的开发构建）：标注为「未知版本 + BETA」。 */
  unknownVersion: boolean;
  /** 当前版本是内测构建（`x.y.z-devbuild.<构建号>`）。 */
  devBuild: boolean;
  /** 内测构建号；正式版为 null。 */
  devBuildId: string | null;
  pubDate: string | null;
  notes: string | null;
  /** 「下载更新」按钮地址:后端已做 123 云盘 → GitHub 安装包 → 便携版回落。 */
  downloadUrl: string | null;
  releasePageUrl: string | null;
};

export type ActivePage = "launch" | "overview" | "shots" | "shaderpacks" | "versions" | "workshop" | "mods" | "settings";

export type ModManagementPage = "lua" | "jar";

export type ResourceCenterPage = "workshop" | "shaderDownload" | "developerNews";

export type SettingsPage =
  | "about"
  | "general"
  | "toolbox"
  | "logs"
  | "modLoader"
  | "startup"
  | "developer";

/** 「版本」页左侧分区：概览（游戏版本）/ 模组版本 / 历史版本，以及并入的支持库、光影包、截图。 */
export type VersionTab = "game" | "mods" | "history" | "support" | "shaderpacks" | "shots";

export type PreviewState = {
  name: string;
  url: string;
  bytes: ArrayBuffer;
};

export type WorkshopFilters = {
  browsesort: string;
  days: number;
  searchText: string;
  page: number;
  numPerPage: number;
  requiredTags: string[];
  excludedTags: string[];
};

export type WorkshopTagMode = "requiredTags" | "excludedTags";

export type WorkshopItem = {
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

export type WorkshopResponse = {
  items: WorkshopItem[];
  meta: {
    currentPage: number;
    totalPages: number;
    totalCount: number;
  };
  steamUrl: string;
};

export type WorkshopItemDetail = {
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

export type WorkshopOptions = {
  sortOptions: { value: string; label: string }[];
  tags: string[];
};

export type ResourceFilters = {
  searchQuery: string;
  gameVersion: string;
  selectedTag: string;
  sortBy: string;
  downloadSource: string;
  page: number;
  pageSize: number;
};

export type ResourceItem = {
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

export type ShaderpackIcon = {
  url: string;
  fallbackUrl: string | null;
};

export type ResourceSearchResponse = {
  items: ResourceItem[];
  total: number;
  page: number;
  pageSize: number;
};

export type DeveloperNewsItem = {
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

export type DeveloperNewsResponse = {
  appId: number;
  items: DeveloperNewsItem[];
  language: string;
  translationJobs: Array<{ sid: string; text: string }>;
};

export type ResourceFile = {
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

export type ResourceVersionPack = {
  name: string;
  items: ResourceFile[];
};

export type ResourceOptions = {
  sources: WorkshopMenuOption[];
  tags: Record<string, string[]>;
  sortOptions: Record<string, WorkshopMenuOption[]>;
};

export type MinecraftVersionOption = {
  value: string;
  label: string;
};

export type AppSettings = {
  developerMode: boolean;
  steamAccountHidden: boolean;
  backgroundPath: string | null;
  wallpaperEffect: unknown;
  shaderDownloadUrl: string | null;
  shaderDownloadDirectory: string | null;
  shaderDownloadThreads: number | null;
  memoryLimitMb: number | null;
  translationLanguage: string | null;
  /** 「通用 › 翻译引擎」：auto = 默认链（Google → 必应 → MyMemory）；其余为手动指定的优先引擎。 */
  translationEngine: string | null;
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
  /** 「窗口 › 原生窗口标题栏」：true = 用系统标题栏，隐藏自绘标题栏两侧的 LOGO 与窗口按钮。 */
  nativeTitlebar: boolean;
  /** 「启动详情 › 启动参数」：用户编辑过的 JVM 参数列表；空 = 用默认。 */
  launchArguments: string[];
  /** 覆盖层背景色（#RRGGBB）；null = 覆盖层默认纯红。 */
  overlayBackgroundColor: string | null;
  modLoaderScheme: string | null;
  fastLaunch: boolean;
  /** 「启动时保留 JAR 模组启用状态」：true = 以磁盘当前状态为基准，不改写 mod.info。 */
  jarModUseDiskStateOnStartup: boolean;
  silentMemoryCleanup: boolean;
  silentMemoryCleanupThreshold: number;
  cleanMemoryOnLaunch: boolean;
  /** 「激进内存清理」：常规清理之外再清系统工作集、文件缓存、已修改页与待机缓存。 */
  aggressiveMemoryClean: boolean;
  /** 「Steam 网络加速」模式：auto / relay / direct；null = 没写过，按 auto。 */
  steamRelayMode: string | null;
  /** 「Steam 网络加速」的中转地址；null/空 = 内置默认。 */
  steamRelayBase: string | null;
  /** 「Steam 网络加速 › 本地代理」；null/空 = 不用显式代理。 */
  steamProxy: string | null;
  /** 「设置 › 版本」的版本存储目录；null/空 = 默认 .pzmanager/versions。 */
  versionStorePath: string | null;
  /** 「设置 › 版本」：哈希/复制是否使用多线程；缺失 = 默认开启。 */
  versionParallel: boolean;
  /** 「设置 › 版本 › 游戏版本」：保存游戏版本时是否同时保存已启用的模组。 */
  versionIncludeMods: boolean;
  /** 隐私协议：用户最后同意的版本号；null = 从未同意过（首次启动弹同意框）。 */
  privacyConsentedVersion: string | null;
  /** 「设置 › 版本」：备份在 C 盘时，操作前的换盘提示是否已勾选「不再弹出」。 */
  versionSystemDriveNoticeDismissed: boolean;
};

export type LauncherLogTail = {
  path: string;
  content: string;
  size: number;
  exists: boolean;
  truncated: boolean;
};

export type LauncherUserDataClearResult = {
  path: string;
  removedEntries: number;
  freedBytes: number;
};

export type FastLaunchSaveResult = {
  backupPath: string | null;
  restored: boolean;
  warning: string | null;
};

export type MemoryStatus = {
  totalMb: number;
  availableMb: number;
  /** 已用物理内存百分比（0–100）。 */
  usedPercent: number;
};

export type MemoryCleanupReport = {
  trimmed: number;
  skipped: number;
  failed: number;
  /** 清理前后可用物理内存差（MB），口径与 PCL2 的「内存优化」一致。 */
  freedMb: number;
  usedPercentBefore: number;
  usedPercentAfter: number;
  elapsedMs: number;
  /** 这次是否跑了激进模式（系统工作集 / 文件缓存 / 已修改页 / 待机列表）。 */
  aggressive: boolean;
  /** 清空全系统工作集是否成功；游戏运行时会主动跳过。 */
  emptiedWorkingSets: boolean;
  /** 系统文件缓存是否成功刷新。 */
  flushedFileCache: boolean;
  /** 已修改页是否成功刷回磁盘。 */
  flushedModified: boolean;
  /** 待机列表是否成功转成空闲页。 */
  purgedStandby: boolean;
  /** 清理时游戏在运行，因此跳过了清空全系统工作集。 */
  protectedRunning: boolean;
};

export type BackgroundEntry = {
  id: string;
  name: string;
  path: string;
  source: string;
  workshopId: string | null;
  modName: string | null;
};

export type BuiltinBackground = { id: string; name: string; url: string };

export type WorkshopMenuOption = {
  value: string;
  label: string;
};

export type ToastKind = NotificationKind;

export type ToastItem = {
  id: number;
  message: string;
  kind: ToastKind;
  closing?: boolean;
};

export type NotificationEntry = {
  id: number;
  message: string;
  kind: ToastKind;
  createdAt: number;
  visible: boolean;
};

export type TaskStatus = "running" | "success" | "error";

export type ShaderDownloadRequest = {
  url: string;
  fallbackUrls: string[];
  fileName: string;
  directory: string;
  threads: number;
};

export type TaskItem = {
  id: string;
  /** 光影下载、「设置 › 版本」的后台版本操作，或「设置 › 启动」的内存清理。 */
  kind: "shader-download" | "version-operation" | "memory-clean";
  title: string;
  detail: string;
  status: TaskStatus;
  /** 光影下载：已下载字节；版本操作不用。 */
  downloaded: number;
  /** 光影下载：总字节；版本操作为 null。 */
  total: number | null;
  createdAt: number;
  finishedAt: number | null;
  error: string | null;
  savedPath: string | null;
  /** 只有光影下载任务有。 */
  request: ShaderDownloadRequest | null;
  /** 版本操作：当前这一步的百分比。 */
  percent?: number | null;
  step?: number | null;
  steps?: number | null;
  doneFiles?: number | null;
  totalFiles?: number | null;
  /** 版本操作：模组任务的目标目录；游戏任务为 null。 */
  rootPath?: string | null;
  /** 内存清理：当前已清理 MB（观察到的最大下降量）。 */
  freedMb?: number | null;
  /** 内存清理：最近一次采样时系统的物理内存占用百分比。 */
  usedPercent?: number | null;
};

export type FastLaunchCacheStatus = {
  exists: boolean;
  generatedAt: number;
  modCount: number;
  modSetHash: string;
  matches: boolean;
};

export type DeveloperNewsTranslationResult = { sid: string; text: string; engine: string };

export type CustomTranslationLine =
  | { kind: "blank" }
  | { kind: "raw"; text: string }
  | { kind: "translatable"; chunks: string[]; sids: string[]; separators: string[] };

/** Viewpoint 光影包配置：shaders.properties 解析结果 + viewpoint-live.properties 当前值。 */

/** 预设里的一个参数 token（`opt=value` / `opt` 开 / `!opt` 关）。 */
export type ShaderProfileToken = {
  option: string;
  value: string;
};

/** shaders.properties 里的一个参数：默认值、是否滑块、观测到的取值、live 文件当前值。 */
export type ShaderpackOptionInfo = {
  name: string;
  /** lang 文件里的显示名（option.<name>）；没有 = null（前端回退参数名）。 */
  label: string | null;
  /** lang 文件里的说明（option.<name>.comment）。 */
  comment: string | null;
  defaultValue: string | null;
  slider: boolean;
  values: string[];
  /** 有本地化标签的观测取值。 */
  valueLabels: ShaderValueLabel[];
  liveValue: string | null;
};

/** 一个取值的本地化显示名（value.<name>.<value> / 通用 value.<value>）。 */
export type ShaderValueLabel = {
  value: string;
  label: string;
};

/** shaders.properties 里的一个预设（profile.*）。 */
export type ShaderProfileMeta = {
  name: string;
  /** lang 文件里的预设显示名（profile.<name>）；没有 = null。 */
  label: string | null;
  tokens: ShaderProfileToken[];
};

/** 一个光影包的覆写配置（shaderpack-overrides.json 里的一条）。 */
export type ShaderpackOverrideEntry = {
  packName: string;
  /** 选用的预设；空 = 不覆写预设。 */
  profile: string | null;
  /** 参数覆写值（键 = shaders.properties 的参数名）。 */
  values: Record<string, string>;
};

/** read_shaderpack_config 返回的弹窗数据。 */
/** 分页界面里的一个格子（OptiFine / Iris 的 screen 布局 token）。 */
export type ShaderScreenToken = {
  /** "option"（参数）| "screen"（[子屏] 引用）| "empty" | "profile"（<profile> 预设按钮）。 */
  kind: "option" | "screen" | "empty" | "profile";
  /** option 名或子屏名；empty 没有。 */
  name: string | null;
};

/** 一个分页界面（主页 = "screen"，子页 = screen.<name> 的 name）。 */
export type ShaderScreenInfo = {
  name: string;
  /** lang 的 screen.<name> 显示名。 */
  label: string | null;
  /** screen.<name>.columns，OptiFine 默认 2。 */
  columns: number;
  tokens: ShaderScreenToken[];
};

export type ShaderpackConfigInfo = {
  packName: string;
  /** 推导 / 反查出的 packId（写入键用）。 */
  packId: string;
  /** packId 是否与 live 文件里已出现的键对上；false = 推导值。 */
  packIdMatched: boolean;
  /** 实际使用的 lang 文件名字干（如 "zh_CN"）；包里没有 lang = null。 */
  lang: string | null;
  /** 包里可用的全部语言（文件名字干，去重）。 */
  availableLangs: string[];
  /** 分页界面（Iris 同款布局）；第一个是主页。 */
  screens: ShaderScreenInfo[];
  profiles: ShaderProfileMeta[];
  options: ShaderpackOptionInfo[];
  /** live 文件里当前的 minecraftProfile.<packId>。 */
  liveProfile: string | null;
  saved: ShaderpackOverrideEntry | null;
};
