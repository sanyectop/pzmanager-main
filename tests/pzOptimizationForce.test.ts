import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
import { rustSource } from "./sourceText.ts";

test("backend exposes force install when only install.ps1 is present", () => {
  assert.match(rustSource, /force_install_available: bool/);
  assert.match(rustSource, /fn pz_optimization_installer_from_libraries/);
  assert.match(rustSource, /fn install_pz_optimization_impl\(force: bool\)/);
  assert.match(rustSource, /fn install_pz_optimization\(force: Option<bool>\)/);
  assert.match(rustSource, /pzmanager-pzopt-/);
  assert.match(rustSource, /install_pz_optimization_impl\(true\)/);
});

test("support UI offers the official-script force install", () => {
  assert.match(appSource, /forceInstallAvailable/);
  assert.match(appSource, /强制安装（官方脚本）/);
  assert.match(appSource, /installPzOptimization\(\s*pzOptimizationForceAvailable/);
  assert.match(appSource, /invoke<PzOptimizationInstallResult>\("install_pz_optimization", \{\s*force,/s);
});
