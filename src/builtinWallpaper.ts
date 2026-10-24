/**
 * 内置「四季」壁纸的渲染模型（纯函数，不依赖 DOM / React）。
 *
 * 内置壁纸是一组同机位的四季图，渲染管线移植自 a(1).html：
 *  - 按日期把一年映射成相邻两个季节的混合权重，换季前后交叉淡化；
 *  - 每张季节图分析一次（光照方向、冷暖、亮度分布），再按季节权重加权；
 *  - 按一天中的小时数，从 24 小时关键帧插值出 RGB 曲线、光照颜色、亮度 / 饱和度 / 时段对比度；
 *  - 再按「自动 / 手动 / 固定」把真实时间或用户指定的值折算成最终效果。
 *
 * 界面每秒调用一次 wallpaperEffectAt()，所以效果全年全天连续变化；
 * 固定（pinned）时改用 pinnedAt 那一刻的时间，等于把壁纸定格。
 */

export type SeasonId = "spring" | "summer" | "autumn" | "winter";

export type SeasonalBackground = {
  id: SeasonId;
  name: string;
  caption: string;
  url: string;
};

/** 图片随前端打包在 public/backgrounds/ 下：1 冬、2 夏、3 秋、4 春（素材原顺序 97 39 00 15）。 */
export const SEASONAL_BACKGROUNDS: SeasonalBackground[] = [
  { id: "spring", name: "春", caption: "新芽 · 晨雾", url: "/backgrounds/4.jpg" },
  { id: "summer", name: "夏", caption: "浓荫 · 积水", url: "/backgrounds/2.jpg" },
  { id: "autumn", name: "秋", caption: "落叶 · 暖光", url: "/backgrounds/3.jpg" },
  { id: "winter", name: "冬", caption: "积雪 · 寒雾", url: "/backgrounds/1.jpg" },
];

export const SEASON_ORDER: SeasonId[] = ["spring", "summer", "autumn", "winter"];

export const SEASON_LABELS: Record<SeasonId, string> = {
  spring: "春",
  summer: "夏",
  autumn: "秋",
  winter: "冬",
};

export type WallpaperMode = "auto" | "manual";

export type WallpaperEffectSettings = {
  /** auto：完全跟随系统日期与时间。manual：日期 / 时刻由用户指定，没指定的部分仍跟随系统。 */
  mode: WallpaperMode;
  /** 手动指定的一年中的第几天（0 = 1 月 1 日，最大 364），由季节滑块设置；null 表示跟随日期。 */
  dayOfYear: number | null;
  timeMinutes: number | null;
  /** 固定：时间冻结在 pinnedAt，壁纸不再随真实时间变化。 */
  pinned: boolean;
  pinnedAt: number | null;
  /** 自动对比度：按每张季节图的亮度分布算出对比度系数，再按季节混合权重加权。 */
  autoContrast: boolean;
};

/** 季节滑块的取值范围：0–364，即 1 月 1 日到 12 月 31 日（平年）。 */
export const WALLPAPER_DAY_MAX = 364;

/** 中心死黑提亮的强度（a(1).html 里「中心死黑提亮」滑块的默认值 30%）。 */
export const WALLPAPER_CENTER_BOOST = 0.3;

export const DEFAULT_WALLPAPER_EFFECT: WallpaperEffectSettings = {
  mode: "auto",
  dayOfYear: null,
  timeMinutes: null,
  pinned: false,
  pinnedAt: null,
  autoContrast: true,
};

export type SeasonWeights = Record<SeasonId, number>;

/** 光照方向：与 a(1).html 的 analyzeImage 输出一致。 */
export type LightDirection = "center" | "left" | "right" | "top" | "bottom";

/** 一张季节图的分析结果。 */
export type SeasonFeature = {
  dir: LightDirection;
  isWarm: boolean;
  /** 亮度分布对应的自动对比度系数。 */
  contrast: number;
};

export type SeasonFeatures = Record<SeasonId, SeasonFeature>;

export const DEFAULT_SEASON_FEATURE: SeasonFeature = { dir: "center", isWarm: false, contrast: 1 };

export const DEFAULT_SEASON_FEATURES: SeasonFeatures = {
  spring: DEFAULT_SEASON_FEATURE,
  summer: DEFAULT_SEASON_FEATURE,
  autumn: DEFAULT_SEASON_FEATURE,
  winter: DEFAULT_SEASON_FEATURE,
};

/** RGB 曲线的两个控制点：x=0.25 处的 y（mid）与 x=0.75 处的 y（high）。 */
export type CurvePoints = { mid: number; high: number };

export type WallpaperEffect = {
  mode: WallpaperMode;
  pinned: boolean;
  /** 当前生效的一年中的第几天（0 = 1 月 1 日），滑块据此显示。 */
  dayOfYear: number;
  /** 当前占比最大的季节（手动指定季节时就是它本身）。 */
  season: SeasonId;
  seasonName: string;
  phaseLabel: string;
  clockLabel: string;
  /** 例如「秋 100%」或「秋 62% + 冬 38%」。 */
  blendLabel: string;
  weights: SeasonWeights;
  /** 生效的当天分钟数（0–1440，含秒的小数部分）。 */
  minutes: number;
  /** 自动对比度是否开启，以及季节图加权后的自动对比度系数（关闭时为 1）。 */
  autoContrast: boolean;
  contrast: number;
  /** 最终 CSS 对比度 = 时段对比度 × 自动对比度。 */
  filterContrast: number;
  brightness: number;
  saturation: number;
  /** 三个通道的 RGB 曲线（SVG 滤镜用）。 */
  curves: { r: CurvePoints; g: CurvePoints; b: CurvePoints };
  /** 光照颜色 [r, g, b]，叠在图片上的 soft-light 层用。 */
  light: [number, number, number];
  /** 每张季节图各自的光照方向（各季分别绘制，按权重淡入淡出）。 */
  lightDirections: Record<SeasonId, LightDirection>;
  /** 中心死黑提亮（screen 层）。 */
  centerBoost: {
    r: number;
    g: number;
    b: number;
    alpha: number;
    /** 渐变透明位置（百分比）。 */
    stop: number;
    opacity: number;
  };
};

const MS_PER_DAY = 86_400_000;
const MINUTES_PER_DAY = 1440;

/**
 * 季节锚点：这几个日期是各季节"最纯粹"的时刻。
 * 锚点之间按 smoothstep 交叉淡化，所以整年都只有相邻两个季节同时出现在画面里。
 */
const SEASON_ANCHORS: Array<{ season: SeasonId; month: number; day: number }> = [
  { season: "winter", month: 1, day: 15 },
  { season: "spring", month: 4, day: 15 },
  { season: "summer", month: 7, day: 15 },
  { season: "autumn", month: 10, day: 15 },
];

/** 换季交叉淡化只发生在锚点间隔的 35%–65%（约一个月），其余时间保持当季画面。 */
const SEASON_BLEND_START = 0.35;
const SEASON_BLEND_END = 0.65;

/**
 * 24 小时光照关键帧，数值与 a(1).html 的 keyframes 一致。
 * r / g / b 只保留 [x=0.25, x=0.75] 两个控制点（html 里的首位 0 在生成曲线时并不参与计算）。
 */
type LightKeyframe = {
  hour: number;
  r: [number, number];
  g: [number, number];
  b: [number, number];
  light: [number, number, number];
};

const LIGHT_KEYFRAMES: LightKeyframe[] = [
  { hour: 0, r: [0.06, 0.32], g: [0.09, 0.38], b: [0.16, 0.52], light: [30, 50, 120] },
  { hour: 2, r: [0.03, 0.25], g: [0.05, 0.28], b: [0.1, 0.4], light: [10, 20, 60] },
  { hour: 4, r: [0.08, 0.35], g: [0.12, 0.42], b: [0.2, 0.6], light: [40, 70, 130] },
  { hour: 5, r: [0.15, 0.5], g: [0.15, 0.5], b: [0.2, 0.65], light: [150, 100, 150] },
  { hour: 6, r: [0.22, 0.65], g: [0.18, 0.55], b: [0.12, 0.45], light: [255, 120, 80] },
  { hour: 8, r: [0.26, 0.74], g: [0.27, 0.77], b: [0.3, 0.84], light: [180, 220, 255] },
  { hour: 12, r: [0.3, 0.8], g: [0.3, 0.8], b: [0.3, 0.8], light: [255, 250, 240] },
  { hour: 15, r: [0.26, 0.82], g: [0.26, 0.78], b: [0.22, 0.7], light: [255, 230, 150] },
  { hour: 17, r: [0.22, 0.85], g: [0.18, 0.65], b: [0.1, 0.4], light: [255, 150, 60] },
  { hour: 18, r: [0.18, 0.85], g: [0.12, 0.6], b: [0.05, 0.3], light: [255, 100, 50] },
  { hour: 19, r: [0.12, 0.6], g: [0.08, 0.45], b: [0.05, 0.3], light: [200, 80, 100] },
  { hour: 20, r: [0.1, 0.5], g: [0.08, 0.45], b: [0.12, 0.55], light: [100, 80, 150] },
  { hour: 22, r: [0.08, 0.38], g: [0.1, 0.4], b: [0.15, 0.5], light: [50, 60, 130] },
  { hour: 24, r: [0.06, 0.32], g: [0.09, 0.38], b: [0.16, 0.52], light: [30, 50, 120] },
];

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function lerp(from: number, to: number, progress: number): number {
  return from + (to - from) * progress;
}

function smoothstep(edge0: number, edge1: number, value: number): number {
  if (edge0 === edge1) {
    return value < edge0 ? 0 : 1;
  }
  const t = clamp((value - edge0) / (edge1 - edge0), 0, 1);
  return t * t * (3 - 2 * t);
}

function isLeapYear(year: number): boolean {
  return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
}

function dayOfYearOf(year: number, month: number, day: number): number {
  return Math.round((Date.UTC(year, month - 1, day) - Date.UTC(year, 0, 1)) / MS_PER_DAY);
}

export function isSeasonId(value: unknown): value is SeasonId {
  return value === "spring" || value === "summer" || value === "autumn" || value === "winter";
}

/** 某一天（本地时区）在一年里是第几天，0 = 1 月 1 日。 */
export function dayOfYear(date: Date): number {
  return dayOfYearOf(date.getFullYear(), date.getMonth() + 1, date.getDate());
}

/** 一年中的第 dayOfYear 天（0 = 1 月 1 日）对应的本地日期，越界按 Date 规则顺延。 */
export function dateForDayOfYear(year: number, dayOfYear: number): Date {
  return new Date(year, 0, 1 + dayOfYear);
}

/**
 * 把日期折算成相邻两个季节的混合权重，权重之和恒为 1。
 * 例：1 月 15 日是纯冬天；2 月底开始向春天过渡；4 月 15 日是纯春天。
 */
export function seasonWeightsForDate(date: Date): SeasonWeights {
  const year = date.getFullYear();
  const doy = dayOfYearOf(year, date.getMonth() + 1, date.getDate());
  const yearLength = isLeapYear(year) ? 366 : 365;
  const anchors = SEASON_ANCHORS.map((anchor) => ({
    season: anchor.season,
    doy: dayOfYearOf(year, anchor.month, anchor.day),
  }));

  // 默认落在「去年秋天 → 今年冬天」这一段（1 月 15 日之前）。
  let index = anchors.length - 1;
  let start = anchors[anchors.length - 1].doy - yearLength;
  let end = anchors[0].doy;
  if (doy >= anchors[0].doy) {
    for (let i = 0; i < anchors.length; i += 1) {
      const segmentStart = anchors[i].doy;
      const segmentEnd =
        i + 1 < anchors.length ? anchors[i + 1].doy : anchors[0].doy + yearLength;
      if (doy >= segmentStart && doy < segmentEnd) {
        index = i;
        start = segmentStart;
        end = segmentEnd;
        break;
      }
    }
  }

  const progress = smoothstep(
    SEASON_BLEND_START,
    SEASON_BLEND_END,
    (doy - start) / Math.max(1, end - start),
  );
  const current = anchors[index].season;
  const next = anchors[(index + 1) % anchors.length].season;
  const weights: SeasonWeights = { spring: 0, summer: 0, autumn: 0, winter: 0 };
  weights[current] += 1 - progress;
  weights[next] += progress;
  return weights;
}

/** pinned 时用冻结时刻，否则就是当前时刻。 */
export function resolveWallpaperDate(now: Date, settings: WallpaperEffectSettings): Date {
  if (settings.pinned && settings.pinnedAt !== null) {
    return new Date(settings.pinnedAt);
  }
  return now;
}

/** 0–1439 分钟 →「HH:MM」。 */
export function formatClock(minutes: number): string {
  const total = clamp(Math.round(minutes), 0, MINUTES_PER_DAY - 1);
  const hours = Math.floor(total / 60);
  const mins = total % 60;
  return String(hours).padStart(2, "0") + ":" + String(mins).padStart(2, "0");
}

/** 固定时刻用的「M月D日 HH:MM」。 */
export function formatWallpaperMoment(timestamp: number): string {
  const date = new Date(timestamp);
  return (
    String(date.getMonth() + 1) +
    "月" +
    String(date.getDate()) +
    "日 " +
    String(date.getHours()).padStart(2, "0") +
    ":" +
    String(date.getMinutes()).padStart(2, "0")
  );
}

/** 一年中的第几天 →「M月D日」（用平年计算，2 月最多 29 日）。 */
export function formatWallpaperDate(dayOfYear: number): string {
  const date = dateForDayOfYear(2025, clamp(Math.round(dayOfYear), 0, WALLPAPER_DAY_MAX));
  return String(date.getMonth() + 1) + "月" + String(date.getDate()) + "日";
}

export function daylightPhaseLabel(minutes: number): string {
  const value = ((minutes % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
  if (value < 270) return "深夜";
  if (value < 330) return "破晓";
  if (value < 420) return "清晨";
  if (value < 660) return "上午";
  if (value < 780) return "正午";
  if (value < 960) return "午后";
  if (value < 1080) return "傍晚";
  if (value < 1200) return "黄昏";
  if (value < 1320) return "入夜";
  return "深夜";
}

function formatBlendLabel(weights: SeasonWeights): string {
  const active = SEASON_ORDER.filter((season) => weights[season] >= 0.005).sort(
    (left, right) => weights[right] - weights[left],
  );
  if (active.length === 0) {
    return SEASON_LABELS.spring;
  }
  if (active.length === 1 || weights[active[0]] >= 0.995) {
    return SEASON_LABELS[active[0]];
  }
  return active
    .map((season) => SEASON_LABELS[season] + " " + Math.round(weights[season] * 100) + "%")
    .join(" + ");
}

/** 亮度标准差 → 自动对比度系数（阈值与 a(1).html 的 AI 对比度一致）：画面越灰越加对比，反差大就收一点。 */
export function contrastForLuminanceStdDev(stdDev: number): number {
  if (stdDev < 30) return 1.35;
  if (stdDev < 45) return 1.18;
  if (stdDev < 60) return 1.0;
  if (stdDev < 80) return 0.92;
  return 0.82;
}

/**
 * 分析一张季节图的像素（RGBA，size×size），与 a(1).html 的 analyzeImage 同一套算法：
 * 左右 / 上下亮度差决定光照方向，R−B 决定冷暖，亮度标准差决定自动对比度。
 */
export function analyzeWallpaperPixels(rgba: Uint8ClampedArray, size: number): SeasonFeature {
  let leftB = 0;
  let rightB = 0;
  let topB = 0;
  let bottomB = 0;
  let rSum = 0;
  let bSum = 0;
  let sumLum = 0;
  let sumLumSq = 0;
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const i = (y * size + x) * 4;
      const r = rgba[i];
      const g = rgba[i + 1];
      const b = rgba[i + 2];
      const lum = 0.299 * r + 0.587 * g + 0.114 * b;
      rSum += r;
      bSum += b;
      sumLum += lum;
      sumLumSq += lum * lum;
      if (x < size / 2) leftB += lum;
      else rightB += lum;
      if (y < size / 2) topB += lum;
      else bottomB += lum;
    }
  }
  const pixels = size * size;
  const xDiff = leftB - rightB;
  const yDiff = topB - bottomB;
  const threshold = 3000;
  let dir: LightDirection = "center";
  if (Math.abs(xDiff) > threshold) dir = xDiff > 0 ? "left" : "right";
  else if (Math.abs(yDiff) > threshold) dir = yDiff > 0 ? "top" : "bottom";

  const isWarm = rSum / pixels - bSum / pixels > 20;
  const mean = sumLum / pixels;
  const stdDev = Math.sqrt(Math.max(0, sumLumSq / pixels - mean * mean));
  return { dir, isWarm, contrast: contrastForLuminanceStdDev(stdDev) };
}

/** 按当前季节混合权重，把各季的自动对比度系数加权成这一刻的系数。 */
export function blendedContrast(weights: SeasonWeights, features: SeasonFeatures): number {
  return SEASON_ORDER.reduce((total, season) => total + weights[season] * features[season].contrast, 0);
}

/** 一条 RGB 曲线 → SVG feFuncX 用的 256 个采样值（与 a(1).html 的 generateTableValues 一致）。 */
export function curveTableValues({ mid, high }: CurvePoints): string {
  const values: string[] = [];
  for (let i = 0; i < 256; i += 1) {
    const x = i / 255;
    let y: number;
    if (x <= 0.25) y = (x / 0.25) * mid;
    else if (x <= 0.75) y = mid + ((x - 0.25) / 0.5) * (high - mid);
    else y = high + ((x - 0.75) / 0.25) * (1 - high);
    values.push(clamp(y, 0, 1).toFixed(4));
  }
  return values.join(" ");
}

function keyframeAt(hour: number): { from: LightKeyframe; to: LightKeyframe; progress: number } {
  let from = LIGHT_KEYFRAMES[0];
  let to = LIGHT_KEYFRAMES[0];
  for (let i = 0; i < LIGHT_KEYFRAMES.length - 1; i += 1) {
    if (hour >= LIGHT_KEYFRAMES[i].hour && hour <= LIGHT_KEYFRAMES[i + 1].hour) {
      from = LIGHT_KEYFRAMES[i];
      to = LIGHT_KEYFRAMES[i + 1];
      break;
    }
  }
  return { from, to, progress: (hour - from.hour) / (to.hour - from.hour || 1) };
}

/**
 * 一天中某个小时的曲线、光照颜色（移植自 a(1).html 的 updateLightingAndCurves 前半段）。
 * warm 是各季冷暖的加权值（0–1）：暖色原图在 16:00–19:30 会额外压暖。
 */
function lightingAt(hour: number, warm: number, boost: number): {
  curves: { r: CurvePoints; g: CurvePoints; b: CurvePoints };
  light: [number, number, number];
} {
  const { from, to, progress } = keyframeAt(hour);
  const channel = (key: "r" | "g" | "b"): CurvePoints => ({
    mid: lerp(from[key][0], to[key][0], progress),
    high: lerp(from[key][1], to[key][1], progress),
  });
  const curves = { r: channel("r"), g: channel("g"), b: channel("b") };
  let light: [number, number, number] = [
    Math.round(lerp(from.light[0], to.light[0], progress)),
    Math.round(lerp(from.light[1], to.light[1], progress)),
    Math.round(lerp(from.light[2], to.light[2], progress)),
  ];

  // 暖色原图的黄昏补偿：高光处偏冷、光色偏橙，越接近 17:30 越明显。
  const inWarmWindow = hour >= 16 && hour <= 19.5;
  const peak = inWarmWindow ? Math.max(0, 1 - Math.abs(hour - 17.5) / 1.5) * warm : 0;
  if (peak > 0) {
    curves.r.high = Math.max(0, curves.r.high - 0.1 * peak);
    curves.g.high = Math.max(0, curves.g.high - 0.08 * peak);
    curves.b.high = Math.min(1, curves.b.high + 0.18 * peak);
    light = [
      Math.round(lerp(light[0], 220, 0.5 * peak)),
      Math.round(lerp(light[1], 70, 0.5 * peak)),
      Math.round(lerp(light[2], 130, 0.5 * peak)),
    ];
  }

  // 中心死黑提亮：把曲线中段整体抬起来（html 的 lift 只作用于 x=0.25 的控制点）。
  const lift = boost * 0.08;
  curves.r.mid = Math.min(1, curves.r.mid + lift * 0.6);
  curves.g.mid = Math.min(1, curves.g.mid + lift * 0.6);
  curves.b.mid = Math.min(1, curves.b.mid + lift * 0.6);

  return { curves, light };
}

/** 一天中某个小时的亮度 / 时段对比度 / 饱和度（移植自 a(1).html 的分段函数）。 */
function timeFiltersAt(hour: number): { brightness: number; timeContrast: number; saturate: number } {
  if (hour < 2) {
    const p = hour / 2;
    return { brightness: lerp(0.75, 0.65, p), timeContrast: lerp(1.15, 1.25, p), saturate: lerp(0.6, 0.4, p) };
  }
  if (hour < 4) {
    const p = (hour - 2) / 2;
    return { brightness: lerp(0.65, 0.75, p), timeContrast: lerp(1.25, 1.15, p), saturate: lerp(0.4, 0.6, p) };
  }
  if (hour < 6) {
    const p = (hour - 4) / 2;
    return { brightness: lerp(0.75, 0.9, p), timeContrast: lerp(1.15, 1.0, p), saturate: lerp(0.6, 0.9, p) };
  }
  if (hour < 8) {
    const p = (hour - 6) / 2;
    return { brightness: lerp(0.9, 1.0, p), timeContrast: lerp(1.0, 1.05, p), saturate: lerp(0.9, 1.0, p) };
  }
  if (hour < 16) {
    const p = (hour - 8) / 8;
    return { brightness: lerp(1.0, 1.05, p), timeContrast: lerp(1.05, 1.0, p), saturate: lerp(1.0, 1.15, p) };
  }
  if (hour < 19) {
    const p = (hour - 16) / 3;
    return { brightness: lerp(1.05, 0.9, p), timeContrast: lerp(1.0, 1.1, p), saturate: lerp(1.15, 1.2, p) };
  }
  if (hour < 22) {
    const p = (hour - 19) / 3;
    return { brightness: lerp(0.9, 0.75, p), timeContrast: lerp(1.1, 1.2, p), saturate: lerp(1.2, 0.6, p) };
  }
  const p = (hour - 22) / 2;
  return { brightness: 0.75, timeContrast: lerp(1.2, 1.15, p), saturate: 0.6 };
}

/** 中心死黑提亮层（移植自 a(1).html 的第 6 步）：夜间强度加到至少 0.5。 */
function centerBoostAt(hour: number, light: [number, number, number], boost: number): WallpaperEffect["centerBoost"] {
  const isDarkTime = hour < 6 || hour > 18;
  return {
    r: Math.min(255, light[0] + 80),
    g: Math.min(255, light[1] + 80),
    b: Math.min(255, light[2] + 80),
    alpha: 0.1 + boost * 0.5,
    stop: 60 + boost * 20,
    opacity: isDarkTime ? Math.min(1, 0.5 + boost) : boost,
  };
}

/**
 * 把来自 settings.json / 用户操作的值收拢成合法状态：
 * 未知模式回落 auto，未知季节与越界时刻丢弃，没有 pinnedAt 的固定视为未固定。
 */
export function normalizeWallpaperEffectSettings(raw: unknown): WallpaperEffectSettings {
  const source =
    raw !== null && typeof raw === "object"
      ? (raw as Partial<Record<keyof WallpaperEffectSettings, unknown>>)
      : {};
  const mode: WallpaperMode = source.mode === "manual" ? "manual" : "auto";
  const dayOfYear =
    typeof source.dayOfYear === "number" && Number.isFinite(source.dayOfYear)
      ? clamp(Math.round(source.dayOfYear), 0, WALLPAPER_DAY_MAX)
      : null;
  const rawMinutes =
    typeof source.timeMinutes === "number" && Number.isFinite(source.timeMinutes)
      ? Math.round(source.timeMinutes)
      : null;
  const timeMinutes =
    rawMinutes === null ? null : clamp(rawMinutes, 0, MINUTES_PER_DAY - 1);
  const rawPinnedAt =
    typeof source.pinnedAt === "number" && Number.isFinite(source.pinnedAt) && source.pinnedAt > 0
      ? source.pinnedAt
      : null;
  const pinned = source.pinned === true && rawPinnedAt !== null;
  return {
    mode,
    dayOfYear,
    timeMinutes,
    pinned,
    pinnedAt: pinned ? rawPinnedAt : null,
    autoContrast: source.autoContrast !== false,
  };
}

/**
 * 给定「现在」和效果设置，算出这一刻要画的壁纸效果。
 * 自动模式用真实时间；手动模式覆盖季节 / 时刻；固定模式把时间换成 pinnedAt。
 */
export function wallpaperEffectAt(
  now: Date,
  settings: WallpaperEffectSettings,
  seasonFeatures: SeasonFeatures = DEFAULT_SEASON_FEATURES,
): WallpaperEffect {
  const date = resolveWallpaperDate(now, settings);
  const manual = settings.mode === "manual";
  const weights: SeasonWeights = { spring: 0, summer: 0, autumn: 0, winter: 0 };
  if (manual && settings.dayOfYear !== null) {
    // 手动日期：把滑块上的第几天换成当年的日期，再走和自动模式一样的换季计算。
    Object.assign(weights, seasonWeightsForDate(dateForDayOfYear(date.getFullYear(), settings.dayOfYear)));
  } else {
    Object.assign(weights, seasonWeightsForDate(date));
  }

  const liveMinutes = date.getHours() * 60 + date.getMinutes() + date.getSeconds() / 60;
  const minutes = manual && settings.timeMinutes !== null ? settings.timeMinutes : liveMinutes;
  const hour = minutes / 60;

  const warm = SEASON_ORDER.reduce(
    (total, season) => total + weights[season] * (seasonFeatures[season].isWarm ? 1 : 0),
    0,
  );
  const lighting = lightingAt(hour, warm, WALLPAPER_CENTER_BOOST);
  const filters = timeFiltersAt(hour);
  const contrast = settings.autoContrast ? blendedContrast(weights, seasonFeatures) : 1;

  const lightDirections = { spring: "center", summer: "center", autumn: "center", winter: "center" } as Record<
    SeasonId,
    LightDirection
  >;
  SEASON_ORDER.forEach((season) => {
    lightDirections[season] = seasonFeatures[season].dir;
  });

  const dominant = SEASON_ORDER.reduce(
    (best, season) => (weights[season] > weights[best] ? season : best),
    SEASON_ORDER[0],
  );
  const pinned = settings.pinned && settings.pinnedAt !== null;

  return {
    mode: settings.mode,
    pinned,
    dayOfYear: settings.dayOfYear !== null && manual ? settings.dayOfYear : dayOfYear(date),
    season: dominant,
    seasonName: SEASON_LABELS[dominant],
    phaseLabel: daylightPhaseLabel(minutes),
    clockLabel: formatClock(minutes),
    blendLabel: formatBlendLabel(weights),
    weights,
    minutes,
    autoContrast: settings.autoContrast,
    contrast,
    filterContrast: filters.timeContrast * contrast,
    brightness: filters.brightness,
    saturation: filters.saturate,
    curves: lighting.curves,
    light: lighting.light,
    lightDirections,
    centerBoost: centerBoostAt(hour, lighting.light, WALLPAPER_CENTER_BOOST),
  };
}
