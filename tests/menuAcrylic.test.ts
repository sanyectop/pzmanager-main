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

/** 注释里会解释"为什么不用 backdrop-filter"，比较前先去掉。 */
const cssCode = cssSource.replace(/\/\*[\s\S]*?\*\//g, "");

/** 取出以 selector 开头的那条规则（到下一个单独一行的 } 为止）。 */
function rule(selector: string): string {
  const start = cssCode.indexOf(selector + " {");
  if (start < 0) {
    return "";
  }
  const end = cssCode.indexOf("\n}", start);
  return end < 0 ? "" : cssCode.slice(start, end + 2);
}

test("every menu surface carries the shared acrylic class", () => {
  // 所有下拉 / 多级子菜单 / 右键菜单都是 Menu.Content，外观统一由
  // .pz-workshop-tag-menu-content 提供；新加菜单漏了这个类就吃不到亚克力。
  const menuLines = appSource
    .split("\n")
    .filter((line) => line.includes("<Menu.Content"));
  assert.ok(menuLines.length >= 10, "菜单数量异常，检查 App.tsx 是否改动过大");
  for (const line of menuLines) {
    // 右键菜单不带滚动限制那一套，只挂自己的类；两者都在亚克力规则的选择器列表里。
    assert.match(line, /pz-workshop-tag-menu-content|pz-workshop-context-menu/, line.trim());
  }
  // 多级菜单（背景菜单里的子菜单）存在且共用同一套表面
  const layered = appSource.split("\n").filter((line) => line.includes("<Menu.Content") && line.includes("pz-background-menu-layer"));
  assert.ok(layered.length >= 4);
  for (const line of layered) {
    assert.match(line, /pz-workshop-tag-menu-content/);
  }
});

test("menu acrylic is blur + tint + grain", () => {
  // 半径吃全局模糊倍率（设置 › 通用 › 模糊度）
  assert.match(cssCode, /--pz-menu-blur:\s*calc\(\d+px \* var\(--pz-blur-scale, 1\)\);/);
  assert.match(cssCode, /--pz-menu-saturate:\s*[\d.]+;/);
  assert.match(cssCode, /--pz-menu-tint:\s*rgba\(255,\s*255,\s*255,\s*0\.8\);/);
  assert.match(cssCode, /--pz-menu-highlight:\s*rgba\(255,\s*255,\s*255,\s*0\.6\);/);
  // 颗粒的强度烘在 SVG 里（背景层没法单独调 opacity）。0.3 是白底上 std≈3 的量级，
  // 0.08 只有 0.87、肉眼看不见，别往下调。
  assert.match(cssCode, /--pz-menu-noise-image:\s*url\("data:image\/svg\+xml[^"]*opacity='0\.3'/);

  const menu = rule(".pz-workshop-tag-menu-content,\n.pz-workshop-context-menu");
  assert.notEqual(menu, "");
  assert.match(menu, /background-color:\s*var\(--pz-menu-tint\);/);
  assert.match(menu, /background-image:\s*var\(--pz-menu-noise-image\);/);
  assert.match(menu, /box-shadow:[\s\S]*?inset 0 1px 0 var\(--pz-menu-highlight\);/);
  // 浮层叠在任意内容上，只能用真的背景模糊；参数全部走令牌，要关只改 --pz-menu-blur
  assert.match(
    menu,
    /backdrop-filter:\s*blur\(var\(--pz-menu-blur\)\)\s*saturate\(var\(--pz-menu-saturate\)\);/,
  );

  const popovers = rule(".pz-launch-settings-popover,\n.pz-launch-details-popover");
  assert.notEqual(popovers, "");
  assert.match(popovers, /background-color:\s*var\(--pz-menu-tint\);/);
  assert.match(popovers, /background-image:\s*var\(--pz-menu-noise-image\);/);
  // 上弹面板与菜单同一套材质（它们只在打开期间存在，不参与页面过渡）
  assert.match(
    popovers,
    /backdrop-filter:\s*blur\(var\(--pz-menu-blur\)\)\s*saturate\(var\(--pz-menu-saturate\)\);/,
  );

  // 任务中心浮层同样叠在任意页面上，套同一套菜单亚克力。
  const taskPanel = rule(".pz-task-panel");
  assert.notEqual(taskPanel, "");
  assert.match(taskPanel, /background-color:\s*var\(--pz-menu-tint\);/);
  assert.match(taskPanel, /background-image:\s*var\(--pz-menu-noise-image\);/);
  assert.match(taskPanel, /box-shadow:[\s\S]*?inset 0 1px 0 var\(--pz-menu-highlight\);/);
  assert.match(
    taskPanel,
    /backdrop-filter:\s*blur\(var\(--pz-menu-blur\)\)\s*saturate\(var\(--pz-menu-saturate\)\);/,
  );
});

test("launch page content is not masked, otherwise overlays cannot blur the wallpaper", () => {
  // .pz-app-content 上有滚动渐隐的 mask-image。mask 会让元素成为 **backdrop root**：
  // 它内部的 backdrop-filter 只能采到自己子树里的内容，背后的壁纸完全采不到，
  // 表现就是"写了模糊但一点效果都没有"。启动页因此单独去掉遮罩。
  const sharedMask = cssCode.indexOf("mask-image: linear-gradient(");
  assert.ok(sharedMask >= 0, "找不到滚动渐隐的共享 mask 规则");
  const override = cssCode.search(/\.pz-app-content-launch\s*\{\s*mask-image:\s*none;/);
  assert.ok(override > sharedMask, "启动页的 mask-image: none 必须写在共享遮罩规则之后才能覆盖");
});

test("startup panels reuse the same noise tile instead of duplicating the data URI", () => {
  // 同一张 feTurbulence 只写一份（--pz-noise-image），面板靠 opacity 调强度
  assert.equal((cssSource.match(/feTurbulence/g) ?? []).length, 2);
  assert.match(rule(".pz-launch-account::after,\n.pz-launch-command::after"), /background-image:\s*var\(--pz-noise-image\);/);
});
