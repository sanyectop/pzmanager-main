import { useEffect, useLayoutEffect, useRef } from "react";
import { SeasonalWallpaper } from "./SeasonalWallpaper";
import type { WallpaperEffect } from "./builtinWallpaper";

/** 亚克力面板用这个属性登记自己，AcrylicGlass 按它们的圆角矩形并集裁剪模糊层。 */
const PANEL_SELECTOR = "[data-pz-acrylic]";

/**
 * 几何还没算出来（或浏览器不支持 path()）时的兜底：整块裁掉，只剩面板自己的底色。
 * 与 .pz-acrylic-glass 里那条 CSS 兜底保持一致。
 */
const HIDDEN_CLIP = "inset(100% 0 0 100%)";

/** `clip-path: path()` 从 Chromium 88 起支持；不支持时干脆不渲染这层。 */
const SUPPORTS_PATH_CLIP =
  typeof CSS !== "undefined" &&
  typeof CSS.supports === "function" &&
  CSS.supports("clip-path", 'path("M0 0h1v1Z")');

export type AcrylicGlassProps = {
  /** 当前背景图（自定义 / 创意工坊 / 内置）；四季壁纸时是 null。 */
  backgroundUrl: string | null;
  /** 是否四季动态壁纸：它由 DOM 图层逐秒重绘，靠再渲染一次 <SeasonalWallpaper> 复制。 */
  seasonal: boolean;
  effect: WallpaperEffect;
};

/** 保留两位小数，路径字符串短一点。 */
function num(value: number): string {
  return (Math.round(value * 100) / 100).toString();
}

/** 圆角矩形的 path：四个角半径独立，0 的角退化成直角。 */
function roundedRectPath(
  x: number,
  y: number,
  width: number,
  height: number,
  [topLeft, topRight, bottomRight, bottomLeft]: [number, number, number, number],
): string {
  const clamp = (radius: number) => Math.min(Math.max(radius, 0), width / 2, height / 2);
  const tl = clamp(topLeft);
  const tr = clamp(topRight);
  const br = clamp(bottomRight);
  const bl = clamp(bottomLeft);

  const parts = [`M${num(x + tl)} ${num(y)}`];
  parts.push(`H${num(x + width - tr)}`);
  if (tr > 0) parts.push(`A${num(tr)} ${num(tr)} 0 0 1 ${num(x + width)} ${num(y + tr)}`);
  parts.push(`V${num(y + height - br)}`);
  if (br > 0) parts.push(`A${num(br)} ${num(br)} 0 0 1 ${num(x + width - br)} ${num(y + height)}`);
  parts.push(`H${num(x + bl)}`);
  if (bl > 0) parts.push(`A${num(bl)} ${num(bl)} 0 0 1 ${num(x)} ${num(y + height - bl)}`);
  parts.push(`V${num(y + tl)}`);
  if (tl > 0) parts.push(`A${num(tl)} ${num(tl)} 0 0 1 ${num(x + tl)} ${num(y)}`);
  parts.push("Z");
  return parts.join(" ");
}

/** 取某个角的圆角半径；getComputedStyle 在多值（椭圆角）时给的是 "a b"，取第一个即可。
    注意横线归属：回调给每个大写字母补 "-"，前缀不能再写 "border-"，
    否则拼出 "border--top-left-radius"（双横线）→ 查无此属性 → 圆角恒为 0。 */
function cornerRadius(
  style: CSSStyleDeclaration,
  side: "TopLeft" | "TopRight" | "BottomRight" | "BottomLeft",
): number {
  const property = `border${side.replace(/[A-Z]/g, (letter) => `-${letter.toLowerCase()}`)}-radius`;
  return Number.parseFloat(style.getPropertyValue(property)) || 0;
}

/**
 * 启动页亚克力的"克隆壁纸层"（方案 C，见 docs/acrylic-panel-options.md）：
 * 把当前壁纸原样再渲染一份，整层 blur + 提饱和，再用 `clip-path: path()` 裁成
 * 各亚克力面板的圆角矩形并集，垫在面板下面。观感等价于 backdrop-filter，
 * 但不会踩它的那些坑（圆角光晕、过渡期丢模糊、每个面板都成为 fixed 包含块）。
 *
 * 几何在挂载时算一次，之后跟着面板尺寸变化 / 窗口缩放 / 字体加载重算。
 * 启动页不滚动，面板位置只由这三件事改变。
 */
export function AcrylicGlass({ backgroundUrl, seasonal, effect }: AcrylicGlassProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const measureRef = useRef<() => void>(() => {});

  useLayoutEffect(() => {
    const root = rootRef.current;
    const host = root?.parentElement;
    if (!root || !host || !SUPPORTS_PATH_CLIP) {
      return;
    }

    let lastClip = "";

    const measure = () => {
      const box = root.getBoundingClientRect();
      if (box.width < 1 || box.height < 1) {
        root.style.clipPath = HIDDEN_CLIP;
        return;
      }

      const paths: string[] = [];
      host.querySelectorAll<HTMLElement>(PANEL_SELECTOR).forEach((panel) => {
        const rect = panel.getBoundingClientRect();
        if (rect.width < 1 || rect.height < 1) {
          return;
        }
        // 面板隐藏（如游戏运行时用户面板下滑淡出）时不裁出它的补丁，
        // 否则裁剪层会在空位上留下一块没有面板的模糊壁纸。
        const style = getComputedStyle(panel);
        if (style.visibility === "hidden") {
          return;
        }
        paths.push(
          roundedRectPath(rect.left - box.left, rect.top - box.top, rect.width, rect.height, [
            cornerRadius(style, "TopLeft"),
            cornerRadius(style, "TopRight"),
            cornerRadius(style, "BottomRight"),
            cornerRadius(style, "BottomLeft"),
          ]),
        );
      });

      // 直接写 style.clipPath（不是 CSS 变量）：这一层的裁剪完全由 JS 决定，
      // CSS 里那条只是算出来之前的兜底。多条子路径用 nonzero 填充规则自然并集。
      const next = paths.length > 0 ? `path("${paths.join(" ")}")` : HIDDEN_CLIP;
      if (next === lastClip) {
        return;
      }
      lastClip = next;
      root.style.clipPath = next;
    };

    measureRef.current = measure;
    measure();

    // ResizeObserver 的回调本身就在布局之后、绘制之前，直接量，不再排到下一帧：
    // 排帧会让面板已经变大、裁出来的模糊层还停在旧尺寸，边缘露出一条没模糊的壁纸。
    const observer = new ResizeObserver(() => measure());
    observer.observe(root);
    host.querySelectorAll<HTMLElement>(PANEL_SELECTOR).forEach((panel) => observer.observe(panel));

    const onResize = () => measure();
    window.addEventListener("resize", onResize);
    // 字体换入后账户名、按钮文案的宽度会变，面板跟着变宽 → 重新量一次。
    void document.fonts?.ready.then(measure).catch(() => {});

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", onResize);
    };
    // 换背景只有两种变化：图片 URL 变、或切进/切出四季壁纸；
    // 创意工坊胶囊的出现/消失也随背景变化，所以这两项就够了。
  }, [backgroundUrl, seasonal]);

  // 每次渲染后再量一次，作为 ResizeObserver 的兜底：四季壁纸每秒都会重渲染一次，
  // 即使窗口被遮挡 / 帧被节流导致 RO 通知迟到，几何也最多晚一秒收敛。
  // measure 内部有"结果没变就不写"的判断，正常情况这里几乎不产生布局开销。
  useEffect(() => {
    measureRef.current();
  });

  // 没有壁纸可复制（纯色画布）时不必占一层。
  if (!SUPPORTS_PATH_CLIP || (!backgroundUrl && !seasonal)) {
    return null;
  }

  return (
    <div aria-hidden="true" className="pz-acrylic-glass" ref={rootRef}>
      <div
        className="pz-acrylic-glass-copy"
        style={backgroundUrl ? { backgroundImage: `url("${backgroundUrl}")` } : undefined}
      >
        {seasonal ? <SeasonalWallpaper effect={effect} idSuffix="-glass" /> : null}
      </div>
    </div>
  );
}
