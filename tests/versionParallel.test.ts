import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
import { rustSource } from "./sourceText.ts";
const versioningSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/versioning.rs", import.meta.url)),
  "utf8",
);
const cargoSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/Cargo.toml", import.meta.url)),
  "utf8",
);

test("settings › 版本 exposes a parallel toggle defaulting on", () => {
  assert.match(appSource, /const \[versionParallel, setVersionParallel\] = useState\(true\);/);
  assert.match(appSource, /settingsResult\.value\.versionParallel \?\? true/);
  assert.match(appSource, /invoke\("save_version_parallel", \{ enabled \}\)/);
  assert.match(appSource, /多线程加速/);
  assert.match(appSource, /aria-label="多线程加速"/);
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
  // 全新安装还没有 settings.json 时，derive 的 Default 全是 false；
  // load_app_settings_at 必须按 serde 默认值把版本保存相关的开关补成开启。
  assert.match(
    rustSource,
    /settings\.version_parallel = crate::default_version_parallel_enabled\(\);/,
  );
  assert.match(
    rustSource,
    /settings\.version_include_mods = crate::default_version_parallel_enabled\(\);/,
  );
});

test("versioning keeps a sequential fallback and supports parallel workers", () => {
  assert.match(versioningSource, /fn worker_count\(file_count: usize, parallel: bool\) -> usize/);
  assert.match(versioningSource, /pub fn scan_directory\(\s*root: &Path,\s*parallel: bool,/s);
  assert.match(versioningSource, /fn scan_entries\(/);
  assert.match(versioningSource, /fn copy_snapshot_files\(/);
  assert.match(versioningSource, /fn restore_target_files\(/);
  assert.match(versioningSource, /if workers <= 1/);
});

test("scanning reuses read buffers and only hashes changed files", () => {
  // 并行度按逻辑核心数，上限 16（旧的 6 太小）。
  assert.match(versioningSource, /const MAX_PARALLEL_WORKERS: usize = 16;/);
  // 读缓冲每个 worker 只分配一次，不再每文件 vec![0u8; 1 MiB]。
  assert.match(versioningSource, /fn hash_file_with_buffer\(/);
  assert.match(versioningSource, /let mut buffer = vec!\[0u8; HASH_BUFFER_SIZE\];/);
  // 遍历时用 DirEntry::metadata() 顺手拿大小/mtime，不再逐个 symlink_metadata。
  assert.match(versioningSource, /item\s*\n?\s*\.metadata\(\)/);
  // 增量捕获：大小 + 修改时间都没变就沿用旧哈希，不打开文件。
  assert.match(versioningSource, /old\.size == entry\.size && old\.mtime == entry\.mtime/);
});

test("snapshot copying runs on a bounded rayon pool", () => {
  assert.match(cargoSource, /rayon = "1"/);
  assert.match(versioningSource, /rayon::ThreadPoolBuilder::new\(\)/);
  assert.match(versioningSource, /\.num_threads\(workers\)/);
  // 文件之间并行 + 每线程复用一份复制缓冲。
  assert.match(versioningSource, /par_iter\(\)\.for_each_init\(/);
  assert.match(versioningSource, /fn copy_file_streaming\(/);
  // 大文件按块响应取消，并校验最终大小。
  assert.match(versioningSource, /if cancelled\.load\(Ordering::Relaxed\) \{\s*return Err\("操作已取消"\.to_string\(\)\);/);
  assert.match(versioningSource, /if written != expected_size \{/);
});
