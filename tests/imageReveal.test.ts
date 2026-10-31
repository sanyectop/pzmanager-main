import assert from "node:assert/strict";
import test from "node:test";

import { appSource, cssSource } from "./sourceText.ts";

test("images stay hidden until their own load completes", () => {
  // 记录的是当前显示地址的加载结果，换图（含退回备用源）会重新等一次
  assert.match(appSource, /const loaded = loadedSrc === displaySrc;/);
  assert.match(appSource, /onLoad=\{\(event\) => \{\s*setLoadedSrc\(displaySrc\);/);
  // 缓存命中时 onLoad 可能不触发，必须补一次 complete 检查
  assert.match(appSource, /img\.complete && img\.naturalWidth > 0/);
  assert.match(appSource, /pz-image-reveal is-loaded/);
});

test("revealed images fade in without moving layout", () => {
  assert.match(cssSource, /\.pz-image-reveal \{\s*opacity: 0;/);
  assert.match(cssSource, /\.pz-image-reveal\.is-loaded \{\s*animation: pz-image-reveal-in/);
  assert.match(cssSource, /@keyframes pz-image-reveal-in \{[\s\S]*?opacity: 0;[\s\S]*?opacity: 1;/);
  // 模糊半径同样要服从全局「模糊度」设置
  assert.match(cssSource, /pz-image-reveal-in \{[\s\S]*?blur\(calc\(6px \* var\(--pz-blur-scale, 1\)\)\)/);
});

test("developer news images use the same load-gated reveal", () => {
  // 新闻正文是后端拼的富文本：图片由 HtmlContentWithImageReveal 挂加载门控渐显。
  assert.match(appSource, /HtmlContentWithImageReveal/);
  assert.match(
    appSource,
    /<HtmlContentWithImageReveal className="pz-developer-news-content" html=\{item\.contentsHtml\} mt="3" \/>/,
  );
  // 富文本里的图片同样要处理缓存命中（complete 检查）与加载失败回退。
  assert.match(appSource, /img\.complete && img\.naturalWidth > 0/);
  assert.match(appSource, /img\.classList\.remove\("pz-image-reveal", "is-loaded"\)/);
  // 条目弹出动画不再挂到 img 上，避免「弹出 → 消失 → 再淡入」的叠帧。
  assert.doesNotMatch(
    cssSource,
    /\.pz-developer-news-content[^{}]*\{[^}]*\}[^@]*@keyframes pz-news-reveal-pop/,
  );
  assert.doesNotMatch(
    appSource,
    /\.pz-developer-news-content li, \.pz-developer-news-content img,/,
  );
});
