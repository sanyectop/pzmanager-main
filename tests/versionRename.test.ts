import assert from "node:assert/strict";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

const history = appSource.slice(
  appSource.indexOf("const versionHistorySettingsContent = ("),
  appSource.indexOf("const settingsContent = ("),
);

test("历史版本每行都有「重命名」入口，且走专用弹窗", () => {
  assert.ok(history.length > 0, "缺少历史版本分页");
  assert.match(history, /onClick=\{\(\) => renameVersion\(record\)\}/);
  assert.match(history, />\s*重命名\s*<\/Button>/);
  // 弹窗在应用根部渲染，带退场动画与提交中状态。
  assert.match(appSource, /<VersionRenameDialog/);
  assert.match(appSource, /closing=\{versionRenameClosing\}/);
  assert.match(appSource, /busy=\{versionRenaming\}/);
  // 提交走 rename_version 命令并刷新版本状态。
  assert.match(appSource, /invoke<string>\("rename_version", \{\s*versionId: record\.id,\s*label: trimmed,/s);
  assert.match(appSource, /await refreshVersionStatus\(\)/);
});

test("重命名弹窗：必填、回车确认、Esc 取消、限长 60", () => {
  const dialog = appSource.slice(
    appSource.indexOf("export function VersionRenameDialog("),
    appSource.indexOf("/** 整合包命名 / 重命名弹窗"),
  );
  assert.ok(dialog.length > 0, "缺少 VersionRenameDialog");
  assert.match(dialog, /aria-label="重命名版本"/);
  assert.match(dialog, /maxLength=\{60\}/);
  assert.match(dialog, /const confirmDisabled = busy \|\| !trimmed;/);
  assert.match(dialog, /event\.key === "Enter" && !confirmDisabled/);
  assert.match(dialog, /event\.key === "Escape"/);
  assert.match(dialog, /只改这条版本的显示名/);
});

test("后端 rename_version 只改 label，并统一去空白 / 限长", () => {
  assert.match(rustSource, /pub fn rename_version\(store: &Path, version_id: &str, label: &str\)/);
  // 只写 meta.json，不碰快照文件。
  assert.match(rustSource, /record\.label = label\.clone\(\);\s*save_meta\(store, &meta\)\?;/s);
  assert.match(rustSource, /RENAME_LABEL_MAX_CHARS/);
  assert.match(rustSource, /fn rename_version\(version_id: String, label: String\)/);
  assert.match(rustSource, /rename_version,/);
});
