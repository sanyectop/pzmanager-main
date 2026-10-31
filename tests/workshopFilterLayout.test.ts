import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("shows only the Project Zomboid Workshop title", () => {
  assert.match(appSource, /Project Zomboid 创意工坊/);
  assert.doesNotMatch(appSource, /浏览 Steam 创意工坊模组，点击资源行打开详情页。/);
});

test("shader download filters stay on one line at the minimum window width", () => {
  // 840px 最小窗口下筛选行只有 623px：四处下拉收窄到文字仍放得下的宽度，
  // 游戏版本 / 排序是主要压缩对象，下载源按 CurseForge 的最长文案定宽。
  assert.match(appSource, /width=\{\{ base: "76px", md: "80px" \}\}/);
  assert.match(appSource, /width=\{\{ base: "116px", md: "124px" \}\}/);
  assert.match(appSource, /width=\{\{ base: "92px", md: "100px" \}\}/);
  assert.match(
    cssSource,
    /\.pz-resource-center-main \.pz-resource-filter-row \{\s*column-gap: 0\.3rem !important;/,
  );
});
