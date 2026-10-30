import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const guideSource = readFileSync(
  fileURLToPath(new URL("../docs/ui-style-guide.md", import.meta.url)),
  "utf8",
);

function rule(selector: string): string {
  const start = cssSource.indexOf(`${selector} {`);
  assert.ok(start >= 0, `missing rule: ${selector}`);
  return cssSource.slice(start, cssSource.indexOf("}", start) + 1);
}

test("查看改动 expands with the same grid animation as mod details", () => {
  const shell = rule(".pz-version-expand-shell");
  assert.match(shell, /display:\s*grid/);
  assert.match(shell, /grid-template-rows:\s*0fr/);
  assert.match(shell, /overflow:\s*hidden/);
  assert.match(shell, /transition:\s*grid-template-rows var\(--pz-dur-slower\)/);

  const expanded = rule(".pz-version-expand-shell.is-expanded");
  assert.match(expanded, /grid-template-rows:\s*1fr/);

  const inner = rule(".pz-version-expand");
  assert.match(inner, /min-height:\s*0/);
  assert.match(inner, /opacity:\s*0/);
  assert.match(inner, /transform:\s*translateY/);

  // 收起后等动画结束再卸载内容
  assert.match(appSource, /const VERSION_DIFF_EXIT_MS = 280;/);
  assert.match(appSource, /versionClosingId/);
  // 游戏版本「查看改动」和模组版本「详情」共用同一个展开壳
  const shells = appSource.match(/className=\{`pz-version-expand-shell\$\{expanded \? " is-expanded" : ""\}`\}/g) ?? [];
  assert.ok(shells.length >= 2, `expected both expand areas to use the shell, got ${shells.length}`);
  const hidden = appSource.match(/aria-hidden=\{!expanded\}/g) ?? [];
  assert.ok(hidden.length >= 2);
  assert.match(appSource, /modVersionClosingKey/);
});

test("view changes is a git-style diff button at the entry's bottom right", () => {
  // 计数字段：+新增、−删除、~修改，配展开箭头。
  assert.match(appSource, /className="pz-version-diff-add"/);
  assert.match(appSource, /className="pz-version-diff-remove"/);
  assert.match(appSource, /className="pz-version-diff-modify"/);
  assert.match(appSource, /className="pz-version-diff-chevron"/);
  // 右下角：统计按钮独占一行右对齐，放在展开内容之后（条目最底部）。
  assert.match(appSource, /<HStack justify="flex-end" mt="1\.5">/);
  const gameExpandAt = appSource.indexOf("pz-version-expand-shell");
  const diffButtonAt = appSource.indexOf("pz-version-diff-button", gameExpandAt);
  assert.ok(gameExpandAt >= 0 && diffButtonAt > gameExpandAt);
  // 旧文字按钮不再出现在右侧操作区；动作说明放进 aria-label / title。
  assert.match(appSource, /title=\{`\$\{expanded \? "收起改动" : "查看改动"\}：\$\{changeLogLabel\}`\}/);
  assert.match(appSource, /aria-expanded=\{expanded\}/);
  // git 风格配色 + 等宽数字。
  assert.match(rule(".pz-version-diff-add"), /color:\s*var\(--pz-success\)/);
  assert.match(rule(".pz-version-diff-remove"), /color:\s*var\(--pz-danger\)/);
  assert.match(rule(".pz-version-diff-modify"), /color:\s*var\(--pz-warning\)/);
  const button = rule(".pz-version-diff-button");
  assert.match(button, /font-variant-numeric:\s*tabular-nums/);
  assert.match(button, /font-family:\s*ui-monospace/);
  assert.match(button, /font-weight:\s*600/);
});

test("version and mod row badges align to the right of the title", () => {
  // 标题左、标签右：右簇用 flex-end 且不参与压缩。
  const rightClusters = appSource.match(/flexShrink="0" gap="1" justify="flex-end"/g) ?? [];
  assert.ok(
    rightClusters.length >= 3,
    `expected at least 3 right-aligned badge clusters, got ${rightClusters.length}`,
  );
  // 标题单行省略，标签不被挤到下一行；完整名称放进 title。
  assert.match(appSource, /lineClamp=\{1\} minW="0" title=\{record\.label\}/);
  assert.match(appSource, /lineClamp=\{1\} minW="0" title=\{target\.name\}/);
  assert.match(appSource, /lineClamp=\{1\} minW="0" title=\{pack\.label\}/);
  // 标题 + 标签同一行、不换行
  assert.match(appSource, /justify="space-between" wrap="nowrap"/);
  // 时间线：日期框在左，类型/当前标签右对齐
  const timelineStart = appSource.indexOf("className=\"pz-version-timeline-body\"");
  const timelineBlock = appSource.slice(timelineStart, appSource.indexOf("</HStack>", timelineStart) + 10);
  assert.match(timelineBlock, /justify="space-between"/);
  assert.match(timelineBlock, /justify="flex-end"/);
});

test("save controls sit above the backup path setting", () => {
  // 游戏版本页顺序：游戏信息 → 保存版本 → 备份保存位置 → 其它开关。
  const saveAt = appSource.indexOf(">保存版本</Text>");
  const backupAt = appSource.indexOf(">备份保存位置</Text>");
  assert.ok(saveAt >= 0 && backupAt > saveAt, "保存版本必须排在备份保存位置之前");
});

test("the design rules are written into the style guide", () => {
  assert.match(guideSource, /右对齐到标题行右侧/);
  assert.match(guideSource, /\.pz-version-expand-shell/);
  assert.match(guideSource, /VERSION_DIFF_EXIT_MS/);
});
