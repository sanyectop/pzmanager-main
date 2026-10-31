import assert from "node:assert/strict";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

test("shots page has a Viewpoint / Steam source switch wired to the backend", () => {
  assert.match(appSource, /const \[shotsSource, setShotsSource\] = useState<ShotsSource>\("viewpoint"\);/);
  assert.match(appSource, /function switchShotsSource\(source: ShotsSource\)/);
  assert.match(appSource, /"read_steam_screenshot"/);
  assert.match(appSource, /invoke<ManagedDirectory>\("list_steam_screenshots"\)/);
  // Steam 截图缩略图与预览都带来源参数。
  assert.match(appSource, /<ShotThumbnail name=\{shot\.name\} source="steam" \/>/);
  assert.match(appSource, /openShot\(shot\.name, "steam"\)/);
  // 未检测到目录时给明确提示，而不是空泛的「暂无图片」。
  assert.match(appSource, /没有找到 Steam 截图目录/);
});

test("backend resolves the Steam screenshots directory from the logged-in user", () => {
  // <Steam 根>/userdata/<32 位账号 ID>/760/remote/108600/screenshots
  assert.match(rustSource, /fn steam_screenshots_directory_at\(steam_root: &Path, account_id: &str\)/);
  assert.match(rustSource, /\.join\("userdata"\)/);
  assert.match(rustSource, /\.join\("760"\)/);
  assert.match(rustSource, /\.join\("remote"\)/);
  assert.match(rustSource, /\.join\("108600"\)/);
  assert.match(rustSource, /\.join\("screenshots"\)/);
  assert.match(rustSource, /fn list_steam_screenshots\(\)/);
  assert.match(rustSource, /fn read_steam_screenshot\(name: String\)/);
});

test("update check labels a self-newer build as unknown version with a BETA tag", () => {
  // 后端在自身版本 > 更新源版本时返回 unknownVersion（开发自编译构建）。
  assert.match(rustSource, /unknown_version: bool/);
  assert.match(
    rustSource,
    /unknown_version: version_is_newer\(&current_version, &latest_version\)/,
  );
  // 前端把「已是最新版本」替换为「未知版本 + BETA 徽标」，toast 同步提示。
  assert.match(appSource, /\{launcherUpdate\.unknownVersion \? "未知版本" : "已是最新版本"\}/);
  assert.match(appSource, /\{launcherUpdate\.unknownVersion \? \(\s*<Badge colorPalette="purple" size="sm">BETA<\/Badge>/);
  assert.match(appSource, /当前是未知版本（v\$\{status\.currentVersion\}），比更新源发布的还新。/);
});
