import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

// 注释里会出现 scrollbar-width / scrollbar-color 这些字样，先剥掉注释再断言。
const css = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
).replace(/\/\*[\s\S]*?\*\//g, "");

test("vertical scrollbar hugs the container edge", () => {
  const block = css.match(/\*::\-webkit\-scrollbar\-thumb:vertical\s*\{([^}]*)\}/);
  assert.ok(block, "缺少纵向滚动条滑块规则");
  // 不再留左右透明边，铺满轨道并贴住右缘。
  assert.match(block[1], /border:\s*0;/);
  assert.match(block[1], /background-clip:\s*border-box;/);
});

test("scrollbars are rectangular", () => {
  // 轨道和滑块都不做圆角：胶囊形已废弃，横纵向统一矩形。
  const track = css.match(/\*::\-webkit\-scrollbar\-track\s*\{([^}]*)\}/);
  assert.ok(track, "缺少滚动条轨道规则");
  assert.doesNotMatch(track[1], /border-radius/);

  for (const name of ["thumb", "thumb:hover", "thumb:active", "thumb:vertical"]) {
    const block = css.match(new RegExp("\\*::-webkit-scrollbar-" + name.replace(":", ":") + "\\s*\\{([^}]*)\\}"));
    assert.ok(block, `缺少滚动条 ${name} 规则`);
    assert.doesNotMatch(block[1], /--pz-radius-pill/);
  }
  const thumb = css.match(/\*::\-webkit\-scrollbar\-thumb\s*\{([^}]*)\}/);
  assert.match(thumb[1], /border-radius:\s*0;/);
});

test("nested resource-center scrollbars sit on the panel edge", () => {
  // 资源中心由内部区域自己滚动，外层不该预留用不到的 gutter，否则右侧空 8px。
  const appContent = css.match(/\.pz-app-content-resource-center\s*\{([^}]*)\}/);
  assert.ok(appContent, "缺少资源中心正文规则");
  assert.match(appContent[1], /scrollbar-gutter:\s*auto;/);

  // 光影下载列表抵消正文 1rem 右内边距，再在滚动容器内补回，行宽不变、滚动条贴边。
  const list = css.match(/\.pz-resource-download-list\s*\{([^}]*)\}/);
  assert.ok(list, "缺少光影下载列表规则");
  assert.match(list[1], /margin-right:\s*-1rem;/);
  assert.match(list[1], /padding-right:\s*1rem;/);
});

test("defines a shared custom scrollbar theme", () => {
  // 不能设 scrollbar-width / scrollbar-color：Chromium 一旦看到这两个标准属性，
  // 就会忽略 ::-webkit-scrollbar*、改用原生主题滚动条，而原生滚动条两端带上下箭头。
  assert.doesNotMatch(css, /scrollbar-width\s*:/);
  assert.doesNotMatch(css, /scrollbar-color\s*:/);
  assert.match(css, /\*::\-webkit\-scrollbar\s*\{/);
  assert.match(css, /\*::\-webkit\-scrollbar\s*\{[^}]*width:\s*8px;/);
  assert.match(css, /\*::\-webkit\-scrollbar\-track\s*\{/);
  assert.match(css, /\*::\-webkit\-scrollbar\-thumb\s*\{/);
  assert.match(css, /\*::\-webkit\-scrollbar\-button\s*\{/);
  assert.match(
    css,
    /\*::\-webkit\-scrollbar\-button\s*\{[\s\S]*?display:\s*none\s*!important;[\s\S]*?visibility:\s*hidden;[\s\S]*?width:\s*0;[\s\S]*?height:\s*0;/,
  );
});

test("fades vertical scroll containers at both ends with a mask", () => {
  // 两端渐隐靠 scroll(self block) 时间线驱动两个注册过的长度值：
  // 贴边时为 0（不淡出），不溢出时时间线不激活、停在初始值 0px。
  assert.match(css, /@property --pz-scroll-fade-top\s*\{\s*syntax:\s*"<length>"/);
  assert.match(css, /@property --pz-scroll-fade-bottom\s*\{\s*syntax:\s*"<length>"/);
  assert.match(css, /@keyframes pz-scroll-fade-top\s*\{/);
  assert.match(css, /@keyframes pz-scroll-fade-bottom\s*\{/);

  const block = css.match(/([^{}]+)\{[^{}]*animation-timeline:\s*scroll\(self block\)[^{}]*\}/);
  assert.ok(block, "找不到滚动渐隐规则");
  const selectors = block[1]
    .split(",")
    .map((selector) => selector.trim())
    .filter(Boolean);
  assert.match(block[0], /mask-image:\s*linear-gradient\(/);
  assert.match(block[0], /--pz-scroll-fade-size:\s*1\.75rem;/);

  // 工具类式的浮层遮罩不算：遮罩必须挂在滚动容器自身，子元素才会一起淡出。
  for (const selector of selectors) {
    assert.match(selector, /^\.pz-/);
  }
  // 带自身底色和描边的浮层不能进清单，否则底色和圆角会被一起淡掉。
  assert.ok(!selectors.includes(".pz-workshop-tag-menu-content"));
  assert.ok(selectors.includes(".pz-app-content"));
  assert.ok(selectors.includes(".pz-workshop-list"));
  assert.ok(selectors.includes(".pz-task-list"));

  // 清单里的每个选择器都必须真的是纵向滚动区（同名选择器会出现在渐隐清单里，
  // 所以要遍历所有同名规则，只要有一条带 overflow-y: auto 即可）。
  for (const selector of selectors) {
    const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const rules = [...css.matchAll(new RegExp(escaped + "\\s*\\{([^}]*)\\}", "g"))];
    assert.ok(rules.length > 0, `${selector} 在 App.css 里找不到规则`);
    assert.ok(
      rules.some((rule) => /overflow-y:\s*auto/.test(rule[1])),
      `${selector} 不是纵向滚动容器`,
    );
  }
});
