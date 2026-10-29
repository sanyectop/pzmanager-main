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
const versioningSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/versioning.rs", import.meta.url)),
  "utf8",
);

test("settings › 版本 exposes a parallel toggle defaulting on", () => {
  assert.match(appSource, /const \[versionParallel, setVersionParallel\] = useState\(true\);/);
  assert.match(appSource, /settingsResult\.value\.versionParallel \?\? true/);
  assert.match(appSource, /invoke\("save_version_parallel", \{ enabled \}\)/);
  assert.match(appSource, /多线程扫描与复制/);
  assert.match(appSource, /aria-label="多线程扫描与复制"/);
  assert.match(
    appSource,
    /onCheckedChange=\{\(details\) => void updateVersionParallel\(details\.checked\)\}/,
  );
});

test("backend stores the toggle and registers the command", () => {
  assert.match(rustSource, /version_parallel: bool/);
  assert.match(rustSource, /fn default_version_parallel_enabled\(\) -> bool/);
  assert.match(rustSource, /fn save_version_parallel\(enabled: bool\)/);
  assert.match(rustSource, /save_version_parallel,/);
  assert.match(rustSource, /context\.parallel/);
});

test("versioning keeps a sequential fallback and supports parallel workers", () => {
  assert.match(versioningSource, /fn worker_count\(file_count: usize, parallel: bool\) -> usize/);
  assert.match(versioningSource, /pub fn scan_directory\(\s*root: &Path,\s*parallel: bool,/s);
  assert.match(versioningSource, /fn scan_directory_with_workers\(/);
  assert.match(versioningSource, /fn copy_snapshot_files\(/);
  assert.match(versioningSource, /fn restore_target_files\(/);
  assert.match(versioningSource, /if workers <= 1/);
});
