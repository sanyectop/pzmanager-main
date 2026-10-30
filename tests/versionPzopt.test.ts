import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const rustSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);
const versioningTsSource = readFileSync(
  fileURLToPath(new URL("../src/versioning.ts", import.meta.url)),
  "utf8",
);

test("restore uninstalls PZ Optimization before switching and reinstalls after", () => {
  const start = rustSource.indexOf("fn start_version_restore");
  assert.ok(start >= 0, "missing start_version_restore");
  const end = rustSource.indexOf("fn start_version_verify", start);
  assert.ok(end > start, "missing start_version_verify boundary");
  const section = rustSource.slice(start, end);

  const uninstall = section.indexOf("uninstall_pz_optimization()");
  const restore = section.indexOf("versioning::restore_version(");
  // 注意：uninstall_pz_optimization() 里也包含 install_pz_optimization() 子串，
  // 这里用 match 前缀 + impl 调用精确定位真正的“还原后重装”调用。
  const install = section.indexOf("match install_pz_optimization_impl(true)");
  assert.ok(uninstall >= 0, "restore must call uninstall_pz_optimization");
  assert.ok(restore >= 0, "restore must call versioning::restore_version");
  assert.ok(install >= 0, "restore must call install_pz_optimization");
  assert.ok(uninstall < restore, "PZ Optimization must be uninstalled before restore");
  assert.ok(restore < install, "PZ Optimization must be reinstalled after restore");
  assert.match(section, /PZ Optimization 已重新安装/);
  assert.match(section, /未能自动重装/);
});

test("version page documents PZ Optimization handling and refreshes detection", () => {
  assert.match(appSource, /PZ Optimization 的话，还原前会自动卸载/);
  assert.match(appSource, /装了 PZ Optimization 时，还原前会自动卸载/);
  assert.match(appSource, /update\.operation === "restore"/);
  assert.match(appSource, /refreshDetection\(\)/);
  assert.match(versioningTsSource, /pzopt: "正在处理 PZ Optimization"/);
});
