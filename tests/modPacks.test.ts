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

test("backend commands save, list, restore and delete packs", () => {
  assert.match(backendSource, /fn list_mod_version_packs\(\)/);
  assert.match(backendSource, /fn delete_mod_version_pack\(pack_id: String\)/);
  assert.match(backendSource, /fn start_mod_version_save_all\(\s*app: tauri::AppHandle,\s*label: Option<String>,/s);
  assert.match(backendSource, /fn start_mod_version_restore_pack\(/);
  for (const command of [
    "list_mod_version_packs",
    "delete_mod_version_pack",
    "start_mod_version_save_all",
    "start_mod_version_restore_pack",
  ]) {
    assert.ok(
      backendSource.includes(`            ${command},`),
      `missing handler registration: ${command}`,
    );
  }
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
  assert.match(backendSource, /"modSaveAll" \| "modRestorePack" =>/);
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
