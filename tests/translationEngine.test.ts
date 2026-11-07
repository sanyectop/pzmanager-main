import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource, rustSource as backendSource } from "./sourceText.ts";

const constantsSource = readFileSync(
  fileURLToPath(new URL("../src/app/constants.ts", import.meta.url)),
  "utf8",
);

test("default translation engine is Google", () => {
  // 11-06：默认从「自动链首选必应」改为 Google（设置下拉框也直接显示 Google 翻译）。
  assert.match(constantsSource, /export const DEFAULT_TRANSLATION_ENGINE = "google";/);
  assert.match(backendSource, /const DEFAULT_TRANSLATION_ENGINE: &str = "google";/);
});

test("auto chain tries Google first, then Bing, then MyMemory", () => {
  assert.match(
    backendSource,
    /const DEFAULT_ENGINE_CHAIN: \[&str; 3\] = \["google", "bing", "mymemory"\];/,
  );
});

test("settings copy and backoff follow the Google-first order", () => {
  assert.match(appSource, /默认顺序：Google → 必应 → MyMemory/);
  assert.match(appSource, /默认 Google 优先，失败自动回退/);
  // 手动切换引擎时 Google 的 10 分钟熔断也要清掉，否则刚选了 Google 仍然不走它。
  assert.match(backendSource, /pub\(crate\) fn clear_google_backoff\(\)/);
  assert.match(
    backendSource,
    /clear_bing_backoff\(\);\s*crate::news_translation::clear_google_backoff\(\);/s,
  );
});
