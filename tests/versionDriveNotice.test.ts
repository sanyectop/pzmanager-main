import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  shouldWarnVersionStoreOnSystemDrive,
  versionStoreOnSystemDrive,
} from "../src/versioning.ts";

import { appSource } from "./sourceText.ts";
import { rustSource as backendSource } from "./sourceText.ts";

test("only C: counts as the system drive", () => {
  assert.equal(versionStoreOnSystemDrive("C:\\Users\\me\\Zomboid\\.pzmanager\\versions"), true);
  assert.equal(versionStoreOnSystemDrive("c:/Users/me/versions"), true);
  assert.equal(versionStoreOnSystemDrive("  C:\\PZ备份  "), true);
  assert.equal(versionStoreOnSystemDrive("\\\\?\\C:\\PZ备份"), true);
  assert.equal(versionStoreOnSystemDrive("D:\\PZ备份"), false);
  assert.equal(versionStoreOnSystemDrive("E:/versions"), false);
  assert.equal(versionStoreOnSystemDrive("\\\\nas\\backup"), false);
  assert.equal(versionStoreOnSystemDrive(""), false);
  assert.equal(versionStoreOnSystemDrive(null), false);
});

test("the notice is skipped once '不再弹出' is saved", () => {
  assert.equal(shouldWarnVersionStoreOnSystemDrive("C:\\PZ备份", false), true);
  assert.equal(shouldWarnVersionStoreOnSystemDrive("C:\\PZ备份", true), false);
  assert.equal(shouldWarnVersionStoreOnSystemDrive("D:\\PZ备份", false), false);
});

test("every version operation goes through the drive notice first", () => {
  assert.match(appSource, /function runWithVersionDriveNotice\(run: \(\) => void\)/);
  // 游戏版本、单个模组、整合包三类入口都先经过提示，再由 begin 实际开始。
  assert.match(appSource, /runWithVersionDriveNotice\(\(\) => beginVersionOperation\(/);
  assert.match(
    appSource,
    /runWithVersionDriveNotice\(\(\) => \{\s*void beginModVersionOperation\(/,
  );
  assert.match(
    appSource,
    /runWithVersionDriveNotice\(\(\) => \{\s*void beginModPackOperation\(/,
  );
  // 弹窗里的复选框与两个出口。
  assert.match(appSource, /不再弹出该提示/);
  assert.match(appSource, /去换盘/);
  assert.match(appSource, /仍要继续/);
  assert.match(appSource, /save_version_system_drive_notice_dismissed/);
  assert.match(appSource, /versionSystemDriveNoticeDismissed/);
  // C 盘时「备份保存位置」下方常驻警告，并允许重新开启提示。
  assert.match(appSource, /备份在 C 盘，版本文件会占用系统盘空间，建议换到其它盘。/);
  assert.match(appSource, /重新开启换盘提示/);
});

test("backend persists the dismissal and exposes the effective store path", () => {
  assert.match(backendSource, /version_system_drive_notice_dismissed: bool/);
  assert.match(
    backendSource,
    /fn save_version_system_drive_notice_dismissed\(dismissed: bool\)/,
  );
  assert.match(backendSource, /fn get_version_store_path\(\) -> Result<String, String>/);
  assert.match(backendSource, /save_version_system_drive_notice_dismissed,/);
  assert.match(backendSource, /get_version_store_path,/);
});
