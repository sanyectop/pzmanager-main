import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("five list pages load behind a skeleton instead of loading text", () => {
  // 创意工坊 / 光影包下载 / 开发者新闻 / Lua 模组 / JAR 模组各自的加载门控。
  assert.match(appSource, /loading=\{workshopLoading && !workshopResponse\}/);
  assert.match(appSource, /loading=\{loading && !response\}/);
  assert.match(appSource, /loading=\{developerNewsLoading && !developerNews\}/);
  assert.match(appSource, /loading=\{luaModsLoading && !luaMods\}/);
  // JAR 列表扫描时 jarMods 已被清空成 []（真值），不能再用 !jarMods 判断加载态；
  // 首次进入的 120ms 窗口也按加载态算，避免闪一帧「尚未扫描」。
  assert.match(appSource, /const jarModsLoadingView = jarModsLoading \|\| jarModsAwaitingFirstScan;/);
  assert.match(appSource, /loading=\{jarModsLoadingView\}/);
  assert.match(appSource, /\) : jarMods \|\| jarModsLoadingView \? \(/);

  // 原来的文字占位必须全部撤掉。
  for (const text of [
    "正在读取 Steam 创意工坊",
    "正在读取 Minecraft 光影源",
    "正在读取开发者新闻",
    "正在读取 Lua 模组列表",
    "正在读取版本文件",
  ]) {
    assert.ok(!appSource.includes(text), `还留着文字加载提示：${text}`);
  }
});

test("skeleton gate plays an exit animation before unmounting", () => {
  // 骨架先挂 .is-exiting，等退场动画播完才卸载并渲染正文。
  assert.match(appSource, /export function SkeletonGate\(/);
  assert.match(appSource, /const SKELETON_EXIT_MS = 180;/);
  assert.match(appSource, /setExiting\(true\)/);
  assert.match(appSource, /pz-skeleton-stage\$\{exiting \? " is-exiting" : ""\}/);
  assert.match(appSource, /window\.setTimeout\(\(\) => \{\s*setVisible\(false\);\s*setExiting\(false\);\s*\}, SKELETON_EXIT_MS\)/);

  // 退场是过渡（不是直接 display none）：淡出 + 轻微下移。
  assert.match(cssSource, /\.pz-skeleton-stage \{[\s\S]*?transition:\s*opacity var\(--pz-dur-exit\) ease,/);
  assert.match(cssSource, /\.pz-skeleton-stage\.is-exiting \{\s*opacity: 0;\s*transform: translateY\(0\.35rem\);/);
  // 减少动态效果时骨架直接替换、扫光停止。
  assert.match(
    cssSource,
    /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\.pz-skeleton-stage\.is-exiting \{\s*opacity: 1;\s*transform: none;/,
  );
  assert.match(cssSource, /@media \(prefers-reduced-motion: reduce\) \{[\s\S]*?\.pz-skeleton-shine::after \{\s*animation: none;/);
});

test("skeleton rows match the list row geometry", () => {
  // 三种缩略图占位尺寸与真实列表一致（模组封面 / 光影图标 / 工坊预览）。
  assert.match(cssSource, /\.pz-skeleton-thumb-poster \{\s*width: 1\.9rem;\s*height: 1\.9rem;/);
  assert.match(cssSource, /\.pz-skeleton-thumb-icon \{\s*width: 42px;\s*height: 42px;/);
  assert.match(cssSource, /\.pz-skeleton-thumb-preview \{\s*width: 64px;\s*height: 64px;/);
  // 扫光动画存在且是可复用的循环。
  assert.match(cssSource, /@keyframes pz-skeleton-shine \{/);
  assert.match(cssSource, /animation: pz-skeleton-shine 1\.4s ease-in-out infinite;/);
});

test("JAR scan runs on first entry only and shows skeleton instead of empty text", () => {
  // 启动后首次进入 JAR 页才自动扫描；之后进入复用已扫描结果。
  assert.match(appSource, /const jarModsScanStartedRef = useRef\(false\);/);
  assert.match(
    appSource,
    /if \(!jarModsScanStartedRef\.current\) \{\s*void loadJarMods\(\);\s*\}/,
  );
  // 标记在扫描真正发起后置位：加载器没装时提前返回不算“已扫描”，装好后仍会自动扫描一次。
  assert.match(
    appSource,
    /async function loadJarMods\(refreshCache = false\) \{[\s\S]{0,900}?jarScanIdRef\.current = scanId;[\s\S]{0,300}?jarModsScanStartedRef\.current = true;/,
  );
  // 扫描期间不渲染「尚未扫描 JAR 模组」：该文案只在非加载态出现。
  assert.match(appSource, /\) : jarMods \|\| jarModsLoadingView \? \(/);
  assert.match(appSource, /尚未扫描 JAR 模组/);
});

test("skeleton stage is excluded from list row slide-in animations", () => {
  // 列表容器的 .pz-x-list > * 逐条滑入动画会命中骨架屏，导致骨架先透明再淡入。
  assert.match(
    cssSource,
    /\.pz-jar-mod-list > \.pz-skeleton-stage \{[\s\S]*?animation: none;/,
  );
  for (const list of ["pz-resource-download-list", "pz-developer-news-list", "pz-lua-mod-list"]) {
    assert.match(cssSource, new RegExp(`\.${list} > \.pz-skeleton-stage`));
  }
});
