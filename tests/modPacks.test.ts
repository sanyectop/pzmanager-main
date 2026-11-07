import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
import { rustSource as backendSource } from "./sourceText.ts";
const modVersioningSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/mod_versioning.rs", import.meta.url)),
  "utf8",
);
const versioningTsSource = readFileSync(
  fileURLToPath(new URL("../src/versioning.ts", import.meta.url)),
  "utf8",
);

test("整合包 records live next to the per-mod version stores", () => {
  assert.match(modVersioningSource, /pub struct ModPackRecord \{/);
  assert.match(modVersioningSource, /pub mods: Vec<versioning::ModVersionRef>/);
  assert.match(modVersioningSource, /pub fn read_packs\(store_base: &Path\)/);
  assert.match(modVersioningSource, /pub fn write_packs\(store_base: &Path/);
  assert.match(modVersioningSource, /store_base\.join\("modpacks\.json"\)/);
});

test("backend commands save, list, rename, restore and delete packs", () => {
  assert.match(backendSource, /fn list_mod_version_packs\(\)/);
  assert.match(backendSource, /fn delete_mod_version_pack\(pack_id: String\)/);
  assert.match(backendSource, /fn rename_mod_version_pack\(pack_id: String, label: String\)/);
  assert.match(backendSource, /fn start_mod_version_save_all\(\s*app: tauri::AppHandle,\s*label: Option<String>,/s);
  assert.match(backendSource, /fn start_mod_version_restore_pack\(/);
  for (const command of [
    "list_mod_version_packs",
    "delete_mod_version_pack",
    "rename_mod_version_pack",
    "start_mod_version_save_all",
    "start_mod_version_restore_pack",
  ]) {
    assert.ok(
      backendSource.includes(`            ${command},`),
      `missing handler registration: ${command}`,
    );
  }
  // 重命名：操作期间锁定、名称去空格且必填、超长拒绝。
  assert.match(
    backendSource,
    /fn rename_mod_version_pack\(pack_id: String, label: String\) -> Result<String, String> \{\s*if VERSION_OPERATION_ACTIVE/s,
  );
  assert.match(backendSource, /fn normalize_mod_pack_label\(label: &str\) -> Result<String, String>/);
  assert.match(backendSource, /整合包名称不能为空。/);
  assert.match(backendSource, /const MOD_PACK_LABEL_MAX_LEN: usize = 60;/);
});

test("save-all reuses every mod's own chain and records the refs", () => {
  // 一键保存 = 对全部物理模组根各记一个版本，再把引用写进整合包。
  assert.match(backendSource, /let roots = collect_mod_roots\(\);/);
  assert.match(backendSource, /capture_mod_versions\(&roots, &pack_label/);
  assert.match(backendSource, /"modSaveAll"/);
  // 整套还原复用游戏大版本那条还原逻辑。
  assert.match(backendSource, /restore_mod_refs\(&pack\.mods/);
  assert.match(backendSource, /"modRestorePack"/);
  // 只有一步，进度条隐藏计数。
  assert.match(backendSource, /"modSaveAll" \| "modSaveChanges" \| "modRestorePack" =>/);
});

test("save-changes picks only mods with pending changes and skips the pack", () => {
  // 11-06 新增「一键保存模组变更」：逐模组元数据比对（不哈希）挑出有变化的，
  // 无变化跳过；不创建整合包。完成后刷新模组状态（待捕获变化清零）。
  assert.match(backendSource, /fn start_mod_version_save_changes\(app: tauri::AppHandle\)/);
  assert.match(backendSource, /fn mod_root_has_pending_changes\(/);
  assert.match(backendSource, /"modSaveChanges"/);
  assert.match(backendSource, /所有模组都没有变化，无需保存。/);
  assert.match(backendSource, /capture_mod_versions\(&pending, "保存变化"/);
  assert.match(backendSource, /            start_mod_version_save_changes,/);
  assert.match(appSource, /一键保存模组变更/);
  assert.match(appSource, /"start_mod_version_save_changes"/);
  assert.match(appSource, /update\.operation === "modSaveChanges"/);
});

test("mod version tab exposes the save-all button and the pack list", () => {
  assert.match(appSource, />\s*一键保存全部模组\s*</);
  assert.match(appSource, /function saveAllModVersions\(/);
  assert.match(appSource, /function restoreModVersionPack\(/);
  assert.match(appSource, /function deleteModVersionPack\(/);
  assert.match(appSource, /invoke<ModPackRecord\[\]>\("list_mod_version_packs"\)/);
  assert.match(appSource, /"start_mod_version_save_all"/);
  assert.match(appSource, /"start_mod_version_restore_pack"/);
  assert.match(appSource, /"delete_mod_version_pack"/);
  // 整合包操作也有任务中心条目。
  assert.match(appSource, /整合包 · 保存全部模组/);
  assert.match(
    appSource,
    /"start_mod_version_restore_pack",\s*\{ packId: pack\.id \},\s*"还原整合包",\s*pack\.label,/s,
  );
  // 完成后刷新整合包列表 + 模组列表。
  assert.match(appSource, /update\.operation === "modSaveAll" \|\| update\.operation === "modRestorePack"/);
  assert.match(versioningTsSource, /export type ModPackRecord = \{/);
});

test("saving a pack opens the naming dialog and rename edits the label in place", () => {
  // 保存前先弹命名框，不再直接 window.confirm；留空时后端自动编号。
  assert.match(appSource, /setModPackNameDialog\(\{ mode: "save", pack: null \}\)/);
  const saveFnStart = appSource.indexOf("function saveAllModVersions()");
  const saveFnEnd = appSource.indexOf("function renameModVersionPack(", saveFnStart);
  assert.ok(saveFnStart >= 0 && saveFnEnd > saveFnStart);
  assert.doesNotMatch(appSource.slice(saveFnStart, saveFnEnd), /window\.confirm/);
  assert.match(
    appSource,
    /"start_mod_version_save_all",\s*\{ label: trimmed \|\| null \},/s,
  );
  // 命名后的任务标题带上整合包名。
  assert.match(appSource, /trimmed \? `整合包 · \$\{trimmed\}` : "整合包 · 保存全部模组"/);
  // 重命名：每条整合包有「重命名」按钮，确认后调用 rename_mod_version_pack。
  assert.match(appSource, /function renameModVersionPack\(pack: ModPackRecord\)/);
  assert.match(appSource, /setModPackNameDialog\(\{ mode: "rename", pack \}\)/);
  assert.match(appSource, /invoke<string>\("rename_mod_version_pack"/);
  assert.match(appSource, />\s*重命名\s*</s);
  // 弹窗组件：save 可留空，rename 必填；Escape / 遮罩点击关闭。
  assert.match(appSource, /<ModPackNameDialog/);
  assert.match(
    appSource,
    /modPackNameDialog\.mode === "save"\s*\?\s*modVersionOperationRunning \|\| versionOperationRunning\s*:\s*modPackRenaming/s,
  );
});
