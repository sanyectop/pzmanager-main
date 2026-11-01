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
  // 后端：普通构建比更新源新才返回 unknownVersion；内测构建（-devbuild.N）不算未知版本。
  assert.match(rustSource, /unknown_version: bool/);
  assert.match(
    rustSource,
    /unknown_version: dev_build\.is_none\(\) && version_is_newer\(&compare_version, &latest_version\)/,
  );
  assert.match(rustSource, /pub\(crate\) dev_build: bool,/);
  // 前端：内测构建显示「内测版本 DevBuild N + 紫色徽标」，其余才落到「未知版本 + BETA」。
  assert.match(appSource, /\? `内测版本 DevBuild \$\{launcherUpdate\.devBuildId\}`/);
  assert.match(appSource, /launcherUpdate\.unknownVersion\s*\? "未知版本"\s*:\s*"已是最新版本"/);
  assert.match(appSource, /DevBuild \{launcherUpdate\.devBuildId\}/);
  assert.match(appSource, /\) : launcherUpdate\.unknownVersion \? \(\s*<Badge colorPalette="purple" size="sm">BETA<\/Badge>/);
  assert.match(appSource, /当前是内测版本 DevBuild \$\{status\.devBuildId\}。/);
  assert.match(appSource, /当前是未知版本（v\$\{status\.currentVersion\}），比更新源发布的还新。/);
  // 关于页顶部：内测构建显示基础版本 + DevBuild 徽标。
  assert.match(appSource, /v\{APP_DEV_BUILD\?\.base \?\? appManifest\.version\}/);
});

test("about page lists the Afdian link below the project repository", () => {
  const repoAt = appSource.indexOf("项目仓库");
  const afdianAt = appSource.indexOf("https://ifdian.net/a/sanyee");
  assert.ok(repoAt > 0 && afdianAt > repoAt, "爱发电应在项目仓库之后");
  assert.match(appSource, /<Text color="gray\.800" fontSize="sm" fontWeight="700">爱发电<\/Text>/);
  assert.match(appSource, /openUrl\("https:\/\/ifdian\.net\/a\/sanyee"\)/);
});
