import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const glassSource = readFileSync(
  fileURLToPath(new URL("../src/AcrylicGlass.tsx", import.meta.url)),
  "utf8",
);
const seasonalSource = readFileSync(
  fileURLToPath(new URL("../src/SeasonalWallpaper.tsx", import.meta.url)),
  "utf8",
);

/** 注释里会提到 backdrop-filter（解释为什么不用），比较前先去掉。 */
const cssCode = cssSource.replace(/\/\*[\s\S]*?\*\//g, "");

/** 取出某条规则（选择器到下一个空行前的 }）。 */
function rule(selector: string): string {
  const start = cssCode.indexOf(selector + " {");
  if (start < 0) {
    return "";
  }
  const end = cssCode.indexOf("\n}", start);
  return end < 0 ? "" : cssCode.slice(start, end + 2);
}

test("keeps backdrop-filter off the launch page's persistent panels", () => {
  // 启动页常驻面板（账户 / 启动 / 工坊胶囊 / 图标按钮）原有的 6 处 backdrop-filter
  // 已按 docs/acrylic-panel-options.md 换成"克隆壁纸 + 裁剪"。这些是整页固定层，
  // 一旦加回 backdrop-filter，就会连带带回圆角光晕、切换时丢模糊、fixed 包含块。
  const panels = [
    ".pz-launch-account",
    ".pz-launch-command",
    ".pz-launch-workshop-info-toggle",
    ".pz-launch-workshop-info-details",
    ".pz-launch-command-toolbar .pz-launch-icon-button",
  ];
  for (const selector of panels) {
    const block = rule(selector);
    assert.notEqual(block, "", selector);
    assert.doesNotMatch(block, /backdrop-filter/, selector);
  }
});

test("draws acrylic as a clipped copy of the wallpaper", () => {
  // 半径吃全局模糊倍率（设置 › 通用 › 模糊度）
  assert.match(cssCode, /--pz-acrylic-blur:\s*calc\(\d+px \* var\(--pz-blur-scale, 1\)\);/);
  assert.match(cssCode, /--pz-acrylic-saturate:\s*[\d.]+;/);

  const glass = rule(".pz-acrylic-glass");
  assert.match(glass, /position:\s*absolute;/);
  // 几何算出来之前必须整块裁掉，否则会露出一块没裁的模糊层
  assert.match(glass, /clip-path:\s*inset\(100% 0 0 100%\);/);

  const copy = rule(".pz-acrylic-glass-copy");
  assert.match(
    copy,
    /filter:\s*blur\(var\(--pz-acrylic-blur\)\)\s*saturate\(var\(--pz-acrylic-saturate\)\);/,
  );
  // 副本要比可视区大一圈，否则边缘 blur 采样到透明区，面板贴边处发暗
  assert.match(copy, /inset:\s*calc\(var\(--pz-acrylic-blur\) \* -1\.6\);/);
});

test("registers every acrylic panel and clips to their union", () => {
  assert.match(appSource, /data-pz-acrylic="account"/);
  assert.match(appSource, /data-pz-acrylic="command"/);
  assert.match(appSource, /data-pz-acrylic="workshop"/);
  assert.match(
    appSource,
    /<AcrylicGlass backgroundUrl=\{backgroundUrl\} seasonal=\{wallpaperSeasonal\} effect=\{wallpaperEffect\} \/>/,
  );

  assert.match(glassSource, /const PANEL_SELECTOR = "\[data-pz-acrylic\]";/);
  // 直接写行内 clip-path；CSS 里那条只是兜底
  assert.match(glassSource, /root\.style\.clipPath = /);
  // 多条圆角矩形子路径拼成一个 path()，靠 nonzero 填充规则取并集
  assert.match(glassSource, /path\("\$\{paths\.join\(" "\)\}"\)/);
  // 尺寸变化后要重算：窗口缩放 + 面板自身尺寸变化
  assert.match(glassSource, /new ResizeObserver\(\(\) => measure\(\)\)/);
  assert.match(glassSource, /window\.addEventListener\("resize", onResize\)/);
});

test("renders the seasonal wallpaper twice without clashing filter ids", () => {
  assert.match(seasonalSource, /idSuffix = ""/);
  assert.match(seasonalSource, /pz-seasonal-curve\$\{idSuffix\}/);
  assert.match(glassSource, /idSuffix="-glass"/);
});
