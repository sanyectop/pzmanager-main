import {
  DEFAULT_SEASON_FEATURES,
  SEASONAL_BACKGROUNDS,
  analyzeWallpaperPixels,
  curveTableValues,
  type SeasonFeatures,
  type SeasonId,
  type WallpaperEffect,
} from "./builtinWallpaper";

/** 分析用的采样尺寸，和 a(1).html 的 AI 分析一致（64×64）。 */
const ANALYSIS_SIZE = 64;

/** 读一张季节图，分析光照方向 / 冷暖 / 对比度；读不了就按默认值处理。 */
function analyzeSeasonImage(url: string) {
  return new Promise<SeasonFeatures[SeasonId] | null>((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = ANALYSIS_SIZE;
        canvas.height = ANALYSIS_SIZE;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          resolve(null);
          return;
        }
        ctx.drawImage(img, 0, 0, ANALYSIS_SIZE, ANALYSIS_SIZE);
        const data = ctx.getImageData(0, 0, ANALYSIS_SIZE, ANALYSIS_SIZE).data;
        resolve(analyzeWallpaperPixels(data, ANALYSIS_SIZE));
      } catch {
        resolve(null);
      }
    };
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

/** 分析四张季节图，得到每个季节的光照方向、冷暖和对比度。 */
export async function loadSeasonFeatures(): Promise<SeasonFeatures> {
  const results = await Promise.all(SEASONAL_BACKGROUNDS.map((item) => analyzeSeasonImage(item.url)));
  const features = { ...DEFAULT_SEASON_FEATURES };
  SEASONAL_BACKGROUNDS.forEach((item, index) => {
    const result = results[index];
    if (result) {
      features[item.id] = result;
    }
  });
  return features;
}

/**
 * 内置四季壁纸的绘制层（移植自 a(1).html 的渲染管线）：
 *  1. 四张季节图按权重叠放（权重即透明度，换季时交叉淡化），整体套 SVG 逐通道曲线 + 亮度 / 饱和度 / 对比度；
 *  2. 每张季节图各自的光照方向用 soft-light 叠在上面，按权重淡入淡出；
 *  3. 中心死黑提亮用 screen 叠在最上层。
 * 放在 .pz-app-shell 的最底层（z-index: -1），内容都在它上面。
 */
export function SeasonalWallpaper({ effect, idSuffix = "" }: { effect: WallpaperEffect; idSuffix?: string }) {
  const { centerBoost } = effect;
  // 同一个页面上可能同时挂两份（真壁纸 + 启动页亚克力的副本），SVG filter id 必须可区分。
  const filterId = `pz-seasonal-curve${idSuffix}`;
  return (
    <div aria-hidden="true" className="pz-seasonal-wallpaper">
      <svg className="pz-seasonal-wallpaper-defs" focusable="false">
        <defs>
          <filter id={filterId} colorInterpolationFilters="sRGB">
            <feComponentTransfer>
              <feFuncR type="table" tableValues={curveTableValues(effect.curves.r)} />
              <feFuncG type="table" tableValues={curveTableValues(effect.curves.g)} />
              <feFuncB type="table" tableValues={curveTableValues(effect.curves.b)} />
            </feComponentTransfer>
          </filter>
        </defs>
      </svg>
      <div
        className="pz-seasonal-wallpaper-images"
        style={{
          filter: `url(#${filterId}) brightness(${effect.brightness.toFixed(2)}) contrast(${effect.filterContrast.toFixed(2)}) saturate(${effect.saturation.toFixed(2)})`,
        }}
      >
        {SEASONAL_BACKGROUNDS.map((background) => (
          <img
            key={background.id}
            alt=""
            className="pz-seasonal-wallpaper-layer"
            src={background.url}
            style={{ opacity: effect.weights[background.id] }}
          />
        ))}
      </div>
      {/* 光照层不进亮度 / 对比度滤镜，和 a(1).html 的 .lighting-overlay 一样是滤镜容器的兄弟 */}
      <div className="pz-seasonal-wallpaper-lights">
        {SEASONAL_BACKGROUNDS.map((background) => (
          <div
            key={background.id}
            className="pz-seasonal-wallpaper-light"
            style={{
              opacity: effect.weights[background.id],
              background: lightGradient(effect.lightDirections[background.id], effect.light),
            }}
          />
        ))}
      </div>
      <div
        className="pz-seasonal-wallpaper-boost"
        style={{
          opacity: centerBoost.opacity,
          background: `radial-gradient(circle at 50% 50%, rgba(${Math.round(centerBoost.r)}, ${Math.round(centerBoost.g)}, ${Math.round(centerBoost.b)}, ${centerBoost.alpha.toFixed(3)}) 0%, transparent ${centerBoost.stop.toFixed(1)}%)`,
        }}
      />
    </div>
  );
}

/** 光照方向的径向渐变（与 a(1).html 第 5 步一致）。 */
function lightGradient(dir: string, light: [number, number, number]): string {
  const color = `rgba(${light[0]}, ${light[1]}, ${light[2]}, 0.8)`;
  switch (dir) {
    case "left":
      return `radial-gradient(circle at 0% 50%, ${color} 0%, transparent 60%)`;
    case "right":
      return `radial-gradient(circle at 100% 50%, ${color} 0%, transparent 60%)`;
    case "top":
      return `radial-gradient(circle at 50% 0%, ${color} 0%, transparent 60%)`;
    default:
      return `radial-gradient(circle at 50% 50%, ${color} 0%, transparent 70%)`;
  }
}
