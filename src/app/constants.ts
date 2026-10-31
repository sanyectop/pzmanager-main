/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { ActivePage, BuiltinBackground, ResourceFilters, WorkshopFilters, WorkshopMenuOption } from "./types";

export const TRANSLATION_LANGUAGE_OPTIONS = [
  { label: "简体中文", value: "zh-CN" },
  { label: "繁體中文", value: "zh-TW" },
  { label: "English", value: "en" },
  { label: "日本語", value: "ja" },
  { label: "한국어", value: "ko" },
  { label: "Русский", value: "ru" },
];

export const DEFAULT_TRANSLATION_LANGUAGE = "zh-CN";

export const MOD_LOADER_SCHEME_OPTIONS: WorkshopMenuOption[] = [
  { value: "zombieBuddy", label: "ZombieBuddy" },
  { value: "none", label: "不使用（原版加载）" },
];

export const DEFAULT_MOD_LOADER_SCHEME = "zombieBuddy";

export const BUILTIN_SEASONAL_ID = "seasonal";

export const BUILTIN_BACKGROUNDS: BuiltinBackground[] = [
  { id: BUILTIN_SEASONAL_ID, name: "四季轮转", url: "/backgrounds/4.jpg" },
];

export const BUILTIN_BACKGROUND_PREFIX = "builtin:";

export const LEGACY_DEFAULT_BUILTIN_ID = "default";

export const MINUTES_PER_DAY = 1440;

export const initialWorkshopFilters: WorkshopFilters = {
  browsesort: "trend",
  days: 7,
  searchText: "",
  page: 1,
  numPerPage: 30,
  requiredTags: [],
  excludedTags: [],
};

export const initialShaderDownloadFilters: ResourceFilters = {
  searchQuery: "",
  gameVersion: "All",
  selectedTag: "All",
  sortBy: "relevance",
  downloadSource: "Modrinth",
  page: 1,
  pageSize: 20,
};

export const MEMORY_STEP_MB = 512;

export const MEMORY_WARNING_MB = 16384;

export const MIN_SILENT_MEMORY_PERCENT = 30;

export const MAX_SILENT_MEMORY_PERCENT = 95;

export const SILENT_MEMORY_PERCENT_STEP = 5;

export const DEFAULT_SILENT_MEMORY_PERCENT = 80;

export const MIN_BLUR_STRENGTH = 0;

export const MAX_BLUR_STRENGTH = 200;

export const BLUR_STRENGTH_STEP = 10;

export const DEFAULT_BLUR_STRENGTH = 100;

export const MIN_FONT_SCALE = 80;

export const MAX_FONT_SCALE = 130;

export const FONT_SCALE_STEP = 5;

export const DEFAULT_FONT_SCALE = 100;

export const DEFAULT_WINDOW_WIDTH = 840;

export const DEFAULT_WINDOW_HEIGHT = 560;

export const FONT_FAMILY_PRESETS: WorkshopMenuOption[] = [
  { value: "default", label: "默认（Inter / 系统字体）" },
  { value: "yahei", label: "微软雅黑" },
  { value: "segoe", label: "Segoe UI" },
  { value: "simsun", label: "宋体（SimSun）" },
  { value: "consolas", label: "等宽（Consolas）" },
  { value: "custom", label: "自定义…" },
];

export const FONT_FAMILY_PRESET_CSS: Record<string, string> = {
  yahei: '"Microsoft YaHei UI", "Microsoft YaHei", sans-serif',
  segoe: '"Segoe UI Variable Text", "Segoe UI", sans-serif',
  simsun: 'SimSun, "Microsoft YaHei", serif',
  consolas: 'Consolas, "Courier New", monospace',
};

export const DEFAULT_OVERLAY_BACKGROUND_COLOR = "#FF0000";

export const OVERLAY_BACKGROUND_PRESETS: Array<{ value: string; label: string }> = [
  { value: "#FF0000", label: "经典红" },
  { value: "#1E3A5F", label: "深夜蓝" },
  { value: "#14532D", label: "森林绿" },
  { value: "#2E1065", label: "暗紫" },
  { value: "#1C1C1E", label: "纯黑" },
  { value: "#F5F5F7", label: "浅灰" },
];

export const PAGE_TRANSITION_MS = 200;

export const RESOURCE_DIALOG_EXIT_MS = 170;

export const TASK_REMOVE_EXIT_MS = 180;

export const VERSION_DIFF_EXIT_MS = 280;

export const ZOMBIE_BUDDY_WORKSHOP_ID = "3619862853";

export const PZ_OPTIMIZATION_WORKSHOP_ID = "3805285544";

export const navigationItems = [
  { value: "launch", label: "启动", icon: "play", description: "选择账户、调整内存并启动 Project Zomboid" },
  { value: "versions", label: "版本", icon: "cube", description: "游戏与模组版本隔离、历史版本与备份设置" },
  { value: "workshop", label: "资源中心", icon: "package-open", description: "浏览创意工坊与光影包下载资源" },
  { value: "mods", label: "模组管理", icon: "file-cog", description: "管理 Lua 与 JAR 模组" },
  { value: "settings", label: "设置", icon: "sliders-horizontal", description: "关于、通用、百宝箱与模组加载器等设置" },
] as const satisfies Array<{ value: ActivePage; label: string; icon: string; description: string }>;

export const CUSTOM_TRANSLATION_MAX_CHARS = 20000;

export const CUSTOM_TRANSLATION_CHUNK_CHARS = 1800;

export const TRANSLATION_BATCH_SIZE = 15;

export const newsEngineLabels: Record<string, string> = {
  google: "Google",
  mymemory: "MyMemory",
  none: "未翻译",
};
