import assert from "node:assert/strict";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

test("restore continues and warns when PZ Optimization fails to reinstall", () => {
  // 重装失败只追加提示，不能把已完成的还原判成失败（不 return Err）。
  const reinstallMatch = rustSource.match(
    /match install_pz_optimization_impl\(true\) \{[\s\S]*?Err\(error\) => \{([\s\S]*?)\}/,
  );
  assert.ok(reinstallMatch, "找不到重装失败分支");
  assert.match(reinstallMatch[1], /message\.push_str/);
  assert.doesNotMatch(reinstallMatch[1], /return Err/);

  // 失败时单独发警告事件，供前端在还原完成后弹模态框。
  assert.match(rustSource, /version-restore-pzopt-warning/);
  assert.match(rustSource, /serde_json::json!\(\{ "error": error \}\)/);
});

test("frontend opens a modal warning after restore when reinstall failed", () => {
  assert.match(appSource, /listen<\{ error: string \}>\("version-restore-pzopt-warning"/);
  assert.match(appSource, /pzOptRestoreWarning \? \(/);
  assert.match(appSource, /<PzOptRestoreWarningDialog/);
  // 「去支持库安装」跳转到支持库页（overview）。
  assert.match(
    appSource,
    /onGoSupport=\{\(\) => \{[\s\S]*?setPzOptRestoreWarning\(null\);[\s\S]*?navigateToPage\("overview"\);/,
  );
});
