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

test("update check labels this build as an internal DevBuild", () => {
  // 内测构建不与公开版本号对比：界面一律标注「内测版本 DevBuild <构建号>」。
  assert.match(rustSource, /unknown_version: bool/);
  assert.match(
    rustSource,
    /unknown_version: version_is_newer\(&current_version, &latest_version\)/,
  );
  assert.match(appSource, /const DEV_BUILD_INFO = \{/);
  assert.match(appSource, /buildId: "1793461756"/);
  assert.match(appSource, /distributor: "PZManager官方交流群"/);
  // 检查更新结果与 toast 都使用内测标注。
  assert.match(appSource, /内测版本 DevBuild \{DEV_BUILD_INFO\.buildId\}。/);
  assert.match(appSource, /<Badge colorPalette="purple" size="sm">\{DEV_BUILD_INFO\.channel\}<\/Badge>/);
  assert.match(appSource, /发行:\{DEV_BUILD_INFO\.distributor\}/);
  assert.match(appSource, /当前是内测版本 DevBuild \$\{DEV_BUILD_INFO\.buildId\}。/);
  // 关于页徽标同步。
  assert.match(appSource, /<Badge colorPalette="purple" size="sm">DevBuild \{DEV_BUILD_INFO\.buildId\}<\/Badge>/);
});

test("about page lists the Afdian link below the project repository", () => {
  const repoAt = appSource.indexOf("项目仓库");
  const afdianAt = appSource.indexOf("https://ifdian.net/a/sanyee");
  assert.ok(repoAt > 0 && afdianAt > repoAt, "爱发电应在项目仓库之后");
  assert.match(appSource, /<Text color="gray\.800" fontSize="sm" fontWeight="700">爱发电<\/Text>/);
  assert.match(appSource, /openUrl\("https:\/\/ifdian\.net\/a\/sanyee"\)/);
});
