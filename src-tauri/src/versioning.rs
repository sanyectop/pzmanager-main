//! PZ 版本隔离（方案 D：母文件 + 增量链）。
//!
//! 存储布局（`store` 默认是 `<用户 Zomboid>/.pzmanager/versions`）：
//!
//! ```text
//! versions/
//! ├── base/<id>/                第一个完整快照（母文件）
//! ├── checkpoints/<id>/         后续的完整快照（检查点）
//! ├── deltas/<from>__<to>/      每次捕获的增量包
//! ├── manifests/<id>.json       每个版本的文件清单
//! ├── trash/restore-*/          还原时被移出的“目标版本里没有”的文件
//! ├── tmp/                      操作过程中的临时文件
//! └── meta.json                 版本链元数据
//! ```
//!
//! 版本身份 = 根哈希（所有文件的相对路径 + 大小 + SHA-256 排序后再哈希）。
//! 增量包使用内容定义分块（CDC，Gear hash）+ COPY/ADD 指令：只有真正变化的
//! 分块才写入增量包，插入/删除一段数据不会让后面所有分块整体错位。
//!
//! 还原任意版本时从最近的完整快照出发，按父链逐个应用增量包；每个文件写出
//! 后都会用清单里的 SHA-256 校验，校验不过就中止并删除临时文件。

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MANIFEST_DIR: &str = "manifests";
const BASE_DIR: &str = "base";
const CHECKPOINT_DIR: &str = "checkpoints";
const DELTA_DIR: &str = "deltas";
const TRASH_DIR: &str = "trash";
const TMP_DIR: &str = "tmp";
const META_FILE: &str = "meta.json";
const META_FORMAT: u32 = 1;

const DELTA_MAGIC: &[u8; 4] = b"PZD1";
const CHUNK_MIN: usize = 16 * 1024;
const CHUNK_MAX: usize = 128 * 1024;
const CHUNK_MASK: u64 = 0xFFFF;
const MAX_CHAIN_DEPTH: u32 = 64;
/// 并行哈希/复制最多开多少个 worker。按逻辑核心数取，SSD 上多开能跑满 CPU；
/// 机械硬盘如果反而更慢，可以在设置里关掉「多线程加速」退回单线程。
const MAX_PARALLEL_WORKERS: usize = 16;
/// 每个 worker 复用的读缓冲大小；1 MiB 对 SHA-NI 和普通磁盘都够用。
const HASH_BUFFER_SIZE: usize = 1024 * 1024;
/// 复制文件时每个 worker 复用的读写缓冲（rayon 的 for_each_init 保证每线程一份）。
const COPY_BUFFER_SIZE: usize = 1024 * 1024;

/// 一次扫描 / 快照 / 还原的进度。回调返回 `Err` 表示用户取消。
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub phase: String,
    pub processed_files: u64,
    pub total_files: u64,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub current_path: Option<String>,
}

type Progress<'a> = &'a mut dyn FnMut(ProgressEvent) -> Result<(), String>;

/// 并行度：关闭多线程或只有一个文件时退回单线程；否则按逻辑核心数取 2–6。
fn worker_count(file_count: usize, parallel: bool) -> usize {
    if !parallel || file_count <= 1 {
        return 1;
    }
    let cores = thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    cores.clamp(1, MAX_PARALLEL_WORKERS).min(file_count)
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    /// 相对游戏目录，统一用 `/` 分隔。
    pub path: String,
    pub size: u64,
    /// 文件修改时间（Unix 毫秒）。增量捕获用它判断文件是否没动过，
    /// 毫秒粒度可以避免“同一秒内改了两次、大小又没变”被漏掉。
    pub mtime: u64,
    /// 十六进制 SHA-256（不带 `sha256:` 前缀）。
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionManifest {
    pub version_id: String,
    pub parent_version_id: Option<String>,
    pub steam_build_id: Option<String>,
    pub branch: String,
    /// `full` = 完整快照（母文件/检查点）；`delta` = 增量版本。
    pub base_type: String,
    pub created_at: u64,
    pub root_hash: String,
    pub total_bytes: u64,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionAddedFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// 相对版本存储目录的增量包路径。
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionModifiedFile {
    pub path: String,
    pub old_size: u64,
    pub new_size: u64,
    pub old_sha256: String,
    pub new_sha256: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionChangeLog {
    pub from: String,
    pub to: String,
    pub from_build_id: Option<String>,
    pub to_build_id: Option<String>,
    pub added: Vec<VersionAddedFile>,
    pub removed: Vec<String>,
    pub modified: Vec<VersionModifiedFile>,
}

/// 游戏大版本里关联的单个模组版本。每个模组仍是一条独立版本链，这里只存引用，
/// 不把模组文件并进游戏快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionRef {
    /// 模组版本库的目录名（`workshop-<id>-<modId>` / `local-<modId>`）。
    pub key: String,
    pub mod_id: String,
    pub name: String,
    /// `local` / `workshop`
    pub source: String,
    pub workshop_id: Option<String>,
    pub version_id: String,
    /// 捕获时的模组根目录；当前目录里找不到同一模组时用它兜底。
    pub root_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionRecord {
    pub id: String,
    pub parent_id: Option<String>,
    pub steam_build_id: Option<String>,
    pub branch: String,
    pub base_type: String,
    pub created_at: u64,
    pub root_hash: String,
    /// 只含“相对路径 + 大小”的指纹，用于不读清单就快速识别当前目录。
    #[serde(default)]
    pub metadata_hash: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub label: String,
    pub change_log: Option<VersionChangeLog>,
    /// 只有游戏大版本会填；模组自己的链里为空。
    #[serde(default)]
    pub mod_versions: Vec<ModVersionRef>,
}

impl VersionRecord {
    fn is_full(&self) -> bool {
        self.base_type == "full"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionStoreMeta {
    #[serde(default = "meta_format")]
    format: u32,
    #[serde(default)]
    versions: Vec<VersionRecord>,
    #[serde(default)]
    last_seen_build_id: Option<String>,
    #[serde(default)]
    active_version_id: Option<String>,
}

fn meta_format() -> u32 {
    META_FORMAT
}

impl Default for VersionStoreMeta {
    fn default() -> Self {
        Self {
            format: META_FORMAT,
            versions: Vec::new(),
            last_seen_build_id: None,
            active_version_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionCurrentMatch {
    /// `hash` / `metadata` / `unknown` / `none` / `missing`
    pub kind: String,
    pub version_id: Option<String>,
    pub candidates: Vec<String>,
    pub checked_hashes: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPendingChanges {
    pub added: u64,
    pub removed: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionStatus {
    pub game_path: Option<String>,
    pub store_path: String,
    pub store_exists: bool,
    pub steam_build_id: Option<String>,
    pub branch: String,
    pub last_captured_build_id: Option<String>,
    pub steam_update_pending: bool,
    pub versions: Vec<VersionRecord>,
    pub current_match: VersionCurrentMatch,
    pub pending_changes: Option<VersionPendingChanges>,
    pub active_version_id: Option<String>,
    pub operation_running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOutcome {
    pub created: bool,
    pub version: Option<VersionRecord>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureOutcome {
    pub created: bool,
    pub version: Option<VersionRecord>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub replaced: u64,
    pub kept: u64,
    pub removed: u64,
    pub bytes_written: u64,
    pub trash_path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutcome {
    pub matched_version_id: Option<String>,
    pub root_hash: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// 通用工具
// ---------------------------------------------------------------------------

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn normalize_key(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
}

fn display_relative(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024.0 && index + 1 < UNITS.len() {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[index])
    }
}

/// UTC 的 `YYYYMMDD-HHMMSS`，只用于版本 ID（展示时间由前端按本地时区渲染）。
fn utc_stamp(seconds: u64) -> String {
    let days = seconds / 86_400;
    let remainder = seconds % 86_400;
    let hour = remainder / 3_600;
    let minute = (remainder % 3_600) / 60;
    let second = remainder % 60;

    // Howard Hinnant 的 civil_from_days。
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    if month <= 2 {
        year += 1;
    }

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

fn mtime_millis(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn io_error(action: &str, path: &Path, error: io::Error) -> String {
    format!("{action}失败（{}）：{error}", path.display())
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn path_compare_key(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| absolute_path(path));
    resolved
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// 版本存储目录不能放在目标目录里，否则快照会把存储区自己也扫进去。
/// 游戏目录和模组目录都调用这一条。
pub fn ensure_store_outside_root(store: &Path, root: &Path) -> Result<(), String> {
    let store_key = path_compare_key(store);
    let root_key = path_compare_key(root);
    if store_key == root_key || store_key.starts_with(&format!("{root_key}\\")) {
        return Err("备份位置不能放在要备份的目录里面，请换一个位置。".to_string());
    }
    Ok(())
}

fn remove_if_exists(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else if path.exists() {
        let _ = fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// 扫描 / 清单
// ---------------------------------------------------------------------------

/// 目录遍历时顺手拿到的元数据。Windows 的目录枚举（FindNextFile）本来就带
/// 大小 / 时间，`DirEntry::metadata()` 不会再多一次 stat；旧实现里每个文件要
/// `symlink_metadata` 三次（统计总大小一次、哈希前一次、校验时一次），在几万个
/// 小文件上光元数据就要跑很久。
#[derive(Debug, Clone)]
struct WalkEntry {
    relative: PathBuf,
    size: u64,
    mtime: u64,
}

fn collect_walk_entries(root: &Path) -> Result<Vec<WalkEntry>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(directory) = stack.pop() {
        let items = fs::read_dir(&directory)
            .map_err(|error| io_error("读取目录", &directory, error))?;
        for item in items {
            let item = item.map_err(|error| io_error("读取目录项", &directory, error))?;
            let path = item.path();
            let file_type = item
                .file_type()
                .map_err(|error| io_error("读取文件类型", &path, error))?;
            // 跳过符号链接 / junction，避免递归环和“链接指向目标目录外”的意外。
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| format!("无法计算相对路径：{}", path.display()))?;
                let metadata = item
                    .metadata()
                    .map_err(|error| io_error("读取文件信息", &path, error))?;
                files.push(WalkEntry {
                    relative: relative.to_path_buf(),
                    size: metadata.len(),
                    mtime: mtime_millis(&metadata),
                });
            }
        }
    }

    files.sort_by(|left, right| {
        let left_key = normalize_key(&display_relative(&left.relative));
        let right_key = normalize_key(&display_relative(&right.relative));
        left_key
            .cmp(&right_key)
            .then_with(|| display_relative(&left.relative).cmp(&display_relative(&right.relative)))
    });
    Ok(files)
}

fn hash_file(path: &Path) -> Result<String, String> {
    hash_file_with_size(path).map(|(hash, _)| hash)
}

/// 每个 worker 复用同一个读缓冲：旧实现每哈希一个文件就 `vec![0u8; 1 MiB]`，
/// 几万个文件等于反复 memset 几十 GB，纯属白烧 CPU / 内存带宽。
fn hash_file_with_buffer(path: &Path, buffer: &mut [u8]) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| io_error("打开文件", path, error))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    loop {
        let read = file
            .read(buffer)
            .map_err(|error| io_error("读取文件", path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((hex(&hasher.finalize()), size))
}

fn hash_file_with_size(path: &Path) -> Result<(String, u64), String> {
    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    hash_file_with_buffer(path, &mut buffer)
}

pub fn scan_directory(
    root: &Path,
    parallel: bool,
    progress: Progress<'_>,
) -> Result<Vec<ManifestFile>, String> {
    let entries = collect_walk_entries(root)?;
    if entries.is_empty() {
        return Err(format!(
            "目录里没有可扫描的文件：{}，请确认路径是否正确。",
            root.display()
        ));
    }
    let cached = vec![None; entries.len()];
    let workers = worker_count(entries.len(), parallel);
    scan_entries(root, &entries, &cached, workers, progress)
}

/// 按需哈希：`cached_hashes[i]` 有值表示这个文件的大小和修改时间都没变，
/// 直接沿用上次清单里的 SHA-256，不打开文件。增量捕获主要省在这一步，
/// 也避开 Windows Defender 对每个被打开文件的实时扫描。
fn scan_entries(
    root: &Path,
    entries: &[WalkEntry],
    cached_hashes: &[Option<String>],
    workers: usize,
    progress: Progress<'_>,
) -> Result<Vec<ManifestFile>, String> {
    let pending: Vec<usize> = (0..entries.len())
        .filter(|&index| cached_hashes.get(index).and_then(Option::as_ref).is_none())
        .collect();
    let total_files = pending.len() as u64;
    let total_bytes: u64 = pending.iter().map(|&index| entries[index].size).sum();

    let mut files: Vec<ManifestFile> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| ManifestFile {
            path: display_relative(&entry.relative),
            size: entry.size,
            mtime: entry.mtime,
            sha256: cached_hashes
                .get(index)
                .and_then(Option::as_ref)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    // 全部命中缓存：没有要读的文件（entries 已经排好序）。
    if pending.is_empty() {
        return Ok(files);
    }

    // 单线程路径：机械硬盘可以在设置里关掉多线程。
    if workers <= 1 {
        let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
        let mut processed_bytes = 0u64;
        for (done, &index) in pending.iter().enumerate() {
            let path = root.join(&entries[index].relative);
            let (sha256, size) = hash_file_with_buffer(&path, &mut buffer)?;
            processed_bytes += size;
            files[index].sha256 = sha256;
            files[index].size = size;
            progress(ProgressEvent {
                phase: "hashing".to_string(),
                processed_files: done as u64 + 1,
                total_files,
                processed_bytes,
                total_bytes,
                current_path: Some(files[index].path.clone()),
            })?;
        }
        return Ok(files);
    }

    let next = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let pending_ref = &pending;
    let (sender, receiver) = mpsc::channel::<(usize, Result<(String, u64), String>)>();
    let mut first_error: Option<String> = None;
    let mut processed_files = 0u64;
    let mut processed_bytes = 0u64;

    thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            let cancelled = &cancelled;
            let pending = pending_ref;
            scope.spawn(move || {
                // 读缓冲每个 worker 只分配一次，循环里复用。
                let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
                loop {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    let slot = next.fetch_add(1, Ordering::Relaxed);
                    if slot >= pending.len() {
                        break;
                    }
                    let index = pending[slot];
                    let path = root.join(&entries[index].relative);
                    let result = hash_file_with_buffer(&path, &mut buffer);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for (index, result) in receiver {
            match result {
                Ok((sha256, size)) => {
                    processed_files += 1;
                    processed_bytes += size;
                    files[index].sha256 = sha256;
                    files[index].size = size;
                    let current_path = files[index].path.clone();
                    if first_error.is_none() && !cancelled.load(Ordering::SeqCst) {
                        if let Err(error) = progress(ProgressEvent {
                            phase: "hashing".to_string(),
                            processed_files,
                            total_files,
                            processed_bytes,
                            total_bytes,
                            current_path: Some(current_path),
                        }) {
                            first_error = Some(error);
                            cancelled.store(true, Ordering::SeqCst);
                        }
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
        }
    });

    if let Some(error) = first_error {
        return Err(error);
    }
    if processed_files != total_files {
        let missing = pending
            .iter()
            .map(|&index| files[index].path.clone())
            .find(|path| !path.is_empty())
            .unwrap_or_default();
        return Err(format!("并行扫描未完成文件：{missing}"));
    }
    Ok(files)
}

fn scan_metadata(root: &Path) -> Result<Vec<(String, u64)>, String> {
    // 遍历时已经带回大小，不用再逐个 stat（页面每次进「版本」都会走这条）。
    let entries = collect_walk_entries(root)?;
    let mut pairs = Vec::with_capacity(entries.len());
    for entry in entries {
        pairs.push((normalize_key(&display_relative(&entry.relative)), entry.size));
    }
    pairs.sort();
    Ok(pairs)
}

fn compute_metadata_hash_from_pairs(pairs: &[(String, u64)]) -> String {
    let mut sorted = pairs.to_vec();
    sorted.sort();
    let mut hasher = Sha256::new();
    for (path, size) in sorted {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(size.to_le_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

pub fn compute_metadata_hash(files: &[ManifestFile]) -> String {
    let pairs: Vec<(String, u64)> = files
        .iter()
        .map(|file| (normalize_key(&file.path), file.size))
        .collect();
    compute_metadata_hash_from_pairs(&pairs)
}

pub fn compute_root_hash(files: &[ManifestFile]) -> String {
    let mut sorted: Vec<&ManifestFile> = files.iter().collect();
    sorted.sort_by(|left, right| {
        normalize_key(&left.path)
            .cmp(&normalize_key(&right.path))
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut hasher = Sha256::new();
    for file in sorted {
        hasher.update(normalize_key(&file.path).as_bytes());
        hasher.update(b"\0");
        hasher.update(file.size.to_le_bytes());
        hasher.update(b"\0");
        hasher.update(file.sha256.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{}", hex(&hasher.finalize()))
}

// ---------------------------------------------------------------------------
// 存储读写
// ---------------------------------------------------------------------------

fn meta_path(store: &Path) -> PathBuf {
    store.join(META_FILE)
}

fn manifest_path(store: &Path, version_id: &str) -> PathBuf {
    store.join(MANIFEST_DIR).join(format!("{version_id}.json"))
}

fn full_snapshot_root(store: &Path, record: &VersionRecord) -> PathBuf {
    if record.parent_id.is_none() {
        store.join(BASE_DIR).join(&record.id)
    } else {
        store.join(CHECKPOINT_DIR).join(&record.id)
    }
}

fn load_meta(store: &Path) -> Result<VersionStoreMeta, String> {
    let path = meta_path(store);
    if !path.is_file() {
        return Ok(VersionStoreMeta::default());
    }
    let content = fs::read_to_string(&path).map_err(|error| io_error("读取版本元数据", &path, error))?;
    if content.trim().is_empty() {
        return Ok(VersionStoreMeta::default());
    }
    serde_json::from_str(&content).map_err(|error| format!("解析版本元数据失败：{error}"))
}

fn save_meta(store: &Path, meta: &VersionStoreMeta) -> Result<(), String> {
    fs::create_dir_all(store).map_err(|error| io_error("创建版本存储目录", store, error))?;
    let content = serde_json::to_string_pretty(meta)
        .map_err(|error| format!("序列化版本元数据失败：{error}"))?;
    let path = meta_path(store);
    let temp = store.join("meta.json.tmp");
    fs::write(&temp, format!("{content}\n"))
        .map_err(|error| io_error("写入版本元数据", &temp, error))?;
    fs::rename(&temp, &path).map_err(|error| io_error("保存版本元数据", &path, error))
}

/// 只读版本链元数据。模组列表用它统计版本数，不会触发目录扫描。
pub fn read_versions(store: &Path) -> Result<Vec<VersionRecord>, String> {
    Ok(load_meta(store)?.versions)
}

/// 把模组版本引用写进某个游戏大版本的记录里。游戏快照/捕获完成后再回填，
/// 所以不需要改 `create_full_snapshot` / `capture_version` 的签名。
pub fn attach_mod_versions(
    store: &Path,
    version_id: &str,
    mod_versions: Vec<ModVersionRef>,
) -> Result<(), String> {
    let mut meta = load_meta(store)?;
    let Some(record) = meta.versions.iter_mut().find(|record| record.id == version_id) else {
        return Err(format!("找不到版本记录：{version_id}"));
    };
    record.mod_versions = mod_versions;
    save_meta(store, &meta)
}

fn load_manifest(store: &Path, version_id: &str) -> Result<VersionManifest, String> {
    let path = manifest_path(store, version_id);
    let content =
        fs::read_to_string(&path).map_err(|error| io_error("读取版本清单", &path, error))?;
    serde_json::from_str(&content).map_err(|error| format!("解析版本清单失败：{error}"))
}

fn save_manifest(store: &Path, manifest: &VersionManifest) -> Result<(), String> {
    let directory = store.join(MANIFEST_DIR);
    fs::create_dir_all(&directory)
        .map_err(|error| io_error("创建版本清单目录", &directory, error))?;
    let path = manifest_path(store, &manifest.version_id);
    let content =
        serde_json::to_string(manifest).map_err(|error| format!("序列化版本清单失败：{error}"))?;
    fs::write(&path, format!("{content}\n")).map_err(|error| io_error("写入版本清单", &path, error))
}

fn new_version_id(store: &Path, meta: &VersionStoreMeta) -> String {
    let stamp = utc_stamp(now_seconds());
    let mut candidate = format!("v{stamp}");
    let mut suffix = 2;
    while meta.versions.iter().any(|version| version.id == candidate)
        || manifest_path(store, &candidate).exists()
    {
        candidate = format!("v{stamp}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn default_label(first: bool, full: bool, build_id: Option<&str>) -> String {
    // 面向用户的默认备注：只说这次是完整保存还是只存了变化，build 号留给开发者视角。
    let _ = build_id;
    if full {
        if first {
            "第一次完整备份".to_string()
        } else {
            "完整备份".to_string()
        }
    } else {
        "保存变化".to_string()
    }
}

// ---------------------------------------------------------------------------
// 增量格式（CDC + COPY/ADD）
// ---------------------------------------------------------------------------

fn gear_table() -> &'static [u64; 256] {
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u64; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut value = (index as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
            value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            *slot = value ^ (value >> 31);
        }
        table
    })
}

/// 带 64KB 缓冲的逐字节读取器：CDC 需要按字节推进 Gear hash，
/// 直接 `File::read` 每字节一次系统调用会非常慢。
struct ByteReader<R: Read> {
    inner: R,
    buffer: Box<[u8; 65_536]>,
    position: usize,
    length: usize,
}

impl<R: Read> ByteReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buffer: Box::new([0u8; 65_536]),
            position: 0,
            length: 0,
        }
    }

    fn next_byte(&mut self) -> io::Result<Option<u8>> {
        if self.position == self.length {
            self.length = self.inner.read(&mut self.buffer[..])?;
            self.position = 0;
            if self.length == 0 {
                return Ok(None);
            }
        }
        let byte = self.buffer[self.position];
        self.position += 1;
        Ok(Some(byte))
    }
}

fn read_chunk<R: Read>(reader: &mut ByteReader<R>, chunk: &mut Vec<u8>) -> io::Result<bool> {
    chunk.clear();
    let table = gear_table();
    let mut hash = 0u64;
    while chunk.len() < CHUNK_MAX {
        let Some(byte) = reader.next_byte()? else {
            break;
        };
        chunk.push(byte);
        hash = (hash << 1).wrapping_add(table[byte as usize]);
        if chunk.len() >= CHUNK_MIN && (hash & CHUNK_MASK) == 0 {
            break;
        }
    }
    Ok(!chunk.is_empty())
}

fn digest_array(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

fn write_u64(writer: &mut impl Write, value: u64) -> Result<(), String> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(|error| format!("写入增量包失败：{error}"))
}

fn read_u64(reader: &mut impl Read) -> Result<u64, String> {
    let mut buffer = [0u8; 8];
    reader
        .read_exact(&mut buffer)
        .map_err(|error| format!("读取增量包失败：{error}"))?;
    Ok(u64::from_le_bytes(buffer))
}

fn normalize_hash(value: &str) -> String {
    if value.starts_with("sha256:") {
        value.to_string()
    } else {
        format!("sha256:{value}")
    }
}

/// 生成 `base -> target` 的增量包。`base` 为 `None` 表示新增文件（空基准）。
/// 返回目标文件大小。
fn create_delta(
    base_path: Option<&Path>,
    target_path: &Path,
    delta_path: &Path,
) -> Result<u64, String> {
    let result = (|| -> Result<u64, String> {
        let mut index: HashMap<[u8; 32], (u64, u64)> = HashMap::new();
        let mut base_len = 0u64;
        let mut base_hasher = Sha256::new();

        if let Some(path) = base_path {
            let file = File::open(path).map_err(|error| io_error("打开基准文件", path, error))?;
            let mut reader = ByteReader::new(file);
            let mut chunk = Vec::with_capacity(CHUNK_MAX);
            let mut offset = 0u64;
            while read_chunk(&mut reader, &mut chunk)
                .map_err(|error| io_error("读取基准文件", path, error))?
            {
                let digest = digest_array(&chunk);
                index.entry(digest).or_insert((offset, chunk.len() as u64));
                base_hasher.update(&chunk);
                offset += chunk.len() as u64;
            }
            base_len = offset;
        }

        let base_hash: [u8; 32] = base_hasher.finalize().into();
        let target_file =
            File::open(target_path).map_err(|error| io_error("打开目标文件", target_path, error))?;
        let mut target_reader = ByteReader::new(target_file);
        let mut target_hasher = Sha256::new();
        let mut target_len = 0u64;

        if let Some(parent) = delta_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("创建增量目录", parent, error))?;
        }
        let output =
            File::create(delta_path).map_err(|error| io_error("创建增量包", delta_path, error))?;
        let mut writer = BufWriter::new(output);

        writer
            .write_all(DELTA_MAGIC)
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        write_u64(&mut writer, base_len)?;
        writer
            .write_all(&base_hash)
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        // target_len / target_hash 先占位，流式写完指令后再回填。
        writer
            .write_all(&[0u8; 40])
            .map_err(|error| format!("写入增量包失败：{error}"))?;

        let mut chunk = Vec::with_capacity(CHUNK_MAX);
        loop {
            if !read_chunk(&mut target_reader, &mut chunk)
                .map_err(|error| io_error("读取目标文件", target_path, error))?
            {
                break;
            }
            target_len += chunk.len() as u64;
            target_hasher.update(&chunk);
            let digest = digest_array(&chunk);
            match index.get(&digest) {
                Some((offset, length)) if *length == chunk.len() as u64 => {
                    writer
                        .write_all(&[0u8])
                        .map_err(|error| format!("写入增量包失败：{error}"))?;
                    write_u64(&mut writer, *offset)?;
                    write_u64(&mut writer, *length)?;
                }
                _ => {
                    writer
                        .write_all(&[1u8])
                        .map_err(|error| format!("写入增量包失败：{error}"))?;
                    write_u64(&mut writer, chunk.len() as u64)?;
                    writer
                        .write_all(&chunk)
                        .map_err(|error| format!("写入增量包失败：{error}"))?;
                }
            }
        }

        let target_hash: [u8; 32] = target_hasher.finalize().into();
        writer
            .seek(SeekFrom::Start(44))
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        writer
            .write_all(&target_len.to_le_bytes())
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        writer
            .write_all(&target_hash)
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        writer
            .flush()
            .map_err(|error| format!("写入增量包失败：{error}"))?;
        Ok(target_len)
    })();

    if result.is_err() {
        remove_if_exists(delta_path);
    }
    result
}

/// 应用增量包，并校验目标 SHA-256（增量包自带的 trailer + 清单里的期望值）。
fn apply_delta(
    base_path: Option<&Path>,
    delta_path: &Path,
    output_path: &Path,
    expected_hash: &str,
) -> Result<(), String> {
    let result = (|| -> Result<(), String> {
        let input = File::open(delta_path)
            .map_err(|error| io_error("打开增量包", delta_path, error))?;
        let mut reader = BufReader::new(input);

        let mut magic = [0u8; 4];
        reader
            .read_exact(&mut magic)
            .map_err(|error| format!("读取增量包失败：{error}"))?;
        if &magic != DELTA_MAGIC {
            return Err("增量包格式不受支持（magic 不匹配）。".to_string());
        }
        let base_len = read_u64(&mut reader)?;
        let mut base_hash = [0u8; 32];
        reader
            .read_exact(&mut base_hash)
            .map_err(|error| format!("读取增量包失败：{error}"))?;
        let target_len = read_u64(&mut reader)?;
        let mut target_hash = [0u8; 32];
        reader
            .read_exact(&mut target_hash)
            .map_err(|error| format!("读取增量包失败：{error}"))?;

        let base_file = match base_path {
            Some(path) => {
                let metadata = fs::metadata(path)
                    .map_err(|error| io_error("读取基准文件信息", path, error))?;
                if metadata.len() != base_len {
                    return Err(format!(
                        "增量包与基准文件大小不一致（期望 {base_len}，实际 {}）。",
                        metadata.len()
                    ));
                }
                Some(File::open(path).map_err(|error| io_error("打开基准文件", path, error))?)
            }
            None => {
                if base_len != 0 {
                    return Err("增量包需要基准文件，但本次没有提供。".to_string());
                }
                None
            }
        };
        let mut base_reader = base_file.map(BufReader::new);

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("创建目标目录", parent, error))?;
        }
        let output =
            File::create(output_path).map_err(|error| io_error("创建目标文件", output_path, error))?;
        let mut writer = BufWriter::new(output);

        let mut written = 0u64;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];

        while written < target_len {
            let mut opcode = [0u8; 1];
            reader
                .read_exact(&mut opcode)
                .map_err(|error| format!("读取增量包失败：{error}"))?;
            match opcode[0] {
                0 => {
                    let offset = read_u64(&mut reader)?;
                    let length = read_u64(&mut reader)?;
                    if written + length > target_len {
                        return Err("增量包指令越界（COPY）。".to_string());
                    }
                    let base = base_reader
                        .as_mut()
                        .ok_or_else(|| "增量包引用了基准文件，但本次没有提供。".to_string())?;
                    base.seek(SeekFrom::Start(offset))
                        .map_err(|error| format!("定位基准文件失败：{error}"))?;
                    let mut remaining = length;
                    while remaining > 0 {
                        let take = remaining.min(buffer.len() as u64) as usize;
                        base.read_exact(&mut buffer[..take])
                            .map_err(|error| format!("读取基准文件失败：{error}"))?;
                        writer
                            .write_all(&buffer[..take])
                            .map_err(|error| format!("写入目标文件失败：{error}"))?;
                        hasher.update(&buffer[..take]);
                        remaining -= take as u64;
                        written += take as u64;
                    }
                }
                1 => {
                    let length = read_u64(&mut reader)?;
                    if written + length > target_len {
                        return Err("增量包指令越界（ADD）。".to_string());
                    }
                    let mut remaining = length;
                    while remaining > 0 {
                        let take = remaining.min(buffer.len() as u64) as usize;
                        reader
                            .read_exact(&mut buffer[..take])
                            .map_err(|error| format!("读取增量包失败：{error}"))?;
                        writer
                            .write_all(&buffer[..take])
                            .map_err(|error| format!("写入目标文件失败：{error}"))?;
                        hasher.update(&buffer[..take]);
                        remaining -= take as u64;
                        written += take as u64;
                    }
                }
                other => return Err(format!("增量包包含未知指令：{other}")),
            }
        }
        writer
            .flush()
            .map_err(|error| format!("写入目标文件失败：{error}"))?;

        let actual = format!("sha256:{}", hex(&hasher.finalize()));
        let trailer = format!("sha256:{}", hex(&target_hash));
        if actual != trailer {
            return Err("增量包自校验失败（输出哈希与 trailer 不一致）。".to_string());
        }
        if !expected_hash.is_empty() && actual != normalize_hash(expected_hash) {
            return Err("增量还原结果与版本清单的 SHA-256 不一致。".to_string());
        }
        Ok(())
    })();

    if result.is_err() {
        remove_if_exists(output_path);
    }
    result
}

fn delta_for_file(record: &VersionRecord, key: &str) -> Option<String> {
    let change_log = record.change_log.as_ref()?;
    if let Some(entry) = change_log
        .added
        .iter()
        .find(|entry| normalize_key(&entry.path) == key)
    {
        return Some(entry.delta.clone());
    }
    if let Some(entry) = change_log
        .modified
        .iter()
        .find(|entry| normalize_key(&entry.path) == key)
    {
        return Some(entry.delta.clone());
    }
    None
}

// ---------------------------------------------------------------------------
// 重建器：从完整快照 + 增量链还原单个文件
// ---------------------------------------------------------------------------

struct Materializer<'a> {
    store: &'a Path,
    meta: &'a VersionStoreMeta,
    manifests: HashMap<String, Rc<VersionManifest>>,
    file_maps: HashMap<String, Rc<HashMap<String, ManifestFile>>>,
}

impl<'a> Materializer<'a> {
    fn new(store: &'a Path, meta: &'a VersionStoreMeta) -> Self {
        Self {
            store,
            meta,
            manifests: HashMap::new(),
            file_maps: HashMap::new(),
        }
    }

    fn record(&self, version_id: &str) -> Result<VersionRecord, String> {
        self.meta
            .versions
            .iter()
            .find(|record| record.id == version_id)
            .cloned()
            .ok_or_else(|| format!("版本不存在：{version_id}"))
    }

    fn manifest(&mut self, version_id: &str) -> Result<Rc<VersionManifest>, String> {
        if let Some(manifest) = self.manifests.get(version_id) {
            return Ok(manifest.clone());
        }
        let manifest = Rc::new(load_manifest(self.store, version_id)?);
        self.manifests
            .insert(version_id.to_string(), manifest.clone());
        Ok(manifest)
    }

    fn file_map(&mut self, version_id: &str) -> Result<Rc<HashMap<String, ManifestFile>>, String> {
        if let Some(map) = self.file_maps.get(version_id) {
            return Ok(map.clone());
        }
        let manifest = self.manifest(version_id)?;
        let map: HashMap<String, ManifestFile> = manifest
            .files
            .iter()
            .cloned()
            .map(|file| (normalize_key(&file.path), file))
            .collect();
        let map = Rc::new(map);
        self.file_maps.insert(version_id.to_string(), map.clone());
        Ok(map)
    }

    fn materialize(
        &mut self,
        version_id: &str,
        relative_path: &str,
        destination: &Path,
        depth: u32,
    ) -> Result<(), String> {
        if depth > MAX_CHAIN_DEPTH {
            return Err("版本关联得太长，请先做一次「完整保存」再还原。".to_string());
        }
        let record = self.record(version_id)?;
        let key = normalize_key(relative_path);
        let file_map = self.file_map(&record.id)?;
        let entry = file_map
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("版本「{}」的清单里没有文件：{relative_path}", record.label))?;

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("创建还原目标目录", parent, error))?;
        }

        if record.is_full() {
            let source = full_snapshot_root(self.store, &record).join(&entry.path);
            if !source.is_file() {
                return Err(format!("完整快照缺少文件：{}", source.display()));
            }
            if destination.exists() {
                fs::remove_file(destination)
                    .map_err(|error| io_error("覆盖还原目标", destination, error))?;
            }
            fs::copy(&source, destination)
                .map_err(|error| io_error("复制完整快照文件", &source, error))?;
            let copied = fs::metadata(destination)
                .map_err(|error| io_error("读取还原目标信息", destination, error))?
                .len();
            if copied != entry.size {
                remove_if_exists(destination);
                return Err(format!("完整快照文件大小不一致：{}", entry.path));
            }
            return Ok(());
        }

        let parent_id = record
            .parent_id
            .clone()
            .ok_or_else(|| format!("增量版本「{}」缺少父版本。", record.label))?;
        let parent_map = self.file_map(&parent_id)?;
        let parent_entry = parent_map.get(&key).cloned();

        // 这个文件在父版本里没变：直接沿父链继续往上游找完整快照。
        if let Some(parent) = parent_entry.as_ref() {
            if parent.sha256 == entry.sha256 {
                // 用父版本清单里的路径大小写，避免 Windows 大小写不敏感之外的文件系统找不到文件。
                let parent_path = parent.path.clone();
                return self.materialize(&parent_id, &parent_path, destination, depth + 1);
            }
        }

        let delta_relative = delta_for_file(&record, &key).ok_or_else(|| {
            format!(
                "版本「{}」缺少文件 {} 的增量包，版本链可能已损坏。",
                record.label, entry.path
            )
        })?;
        let delta_path = self.store.join(&delta_relative);

        match parent_entry {
            Some(parent) => {
                let file_name = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("file");
                let base_temp =
                    destination.with_file_name(format!(".{file_name}.pzbase{depth}"));
                let parent_path = parent.path.clone();
                let result = (|| -> Result<(), String> {
                    self.materialize(&parent_id, &parent_path, &base_temp, depth + 1)?;
                    apply_delta(Some(&base_temp), &delta_path, destination, &entry.sha256)
                })();
                remove_if_exists(&base_temp);
                result
            }
            None => apply_delta(None, &delta_path, destination, &entry.sha256),
        }
    }
}

// ---------------------------------------------------------------------------
// 快照 / 捕获
// ---------------------------------------------------------------------------

fn copy_snapshot_files(
    game_path: &Path,
    snapshot_root: &Path,
    files: &[ManifestFile],
    total_bytes: u64,
    workers: usize,
    progress: Progress<'_>,
) -> Result<(), String> {
    let total_files = files.len() as u64;

    // 快照根目录始终先建好：全部文件都在根层级（parent 为空）时，
    // 下面的父目录收集循环不会创建它，File::create 会报「找不到路径」。
    fs::create_dir_all(snapshot_root)
        .map_err(|error| io_error("创建快照目录", snapshot_root, error))?;

    // 单线程路径：保留原来的逐文件复制行为。
    if workers <= 1 {
        let mut processed_bytes = 0u64;
        for (index, file) in files.iter().enumerate() {
            let source = game_path.join(&file.path);
            let destination = snapshot_root.join(&file.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| io_error("创建快照目录", parent, error))?;
            }
            let copied = fs::copy(&source, &destination)
                .map_err(|error| io_error("复制快照文件", &source, error))?;
            if copied != file.size {
                return Err(format!("复制快照文件大小不一致：{}", file.path));
            }
            processed_bytes += file.size;
            progress(ProgressEvent {
                phase: "copying".to_string(),
                processed_files: index as u64 + 1,
                total_files,
                processed_bytes,
                total_bytes,
                current_path: Some(file.path.clone()),
            })?;
        }
        return Ok(());
    }

    // 并行复制前先串行创建所有目录，避免多个 worker 同时 create_dir_all。
    let mut parents = HashSet::new();
    for file in files {
        if let Some(parent) = Path::new(&file.path).parent() {
            if !parent.as_os_str().is_empty() {
                parents.insert(snapshot_root.join(parent));
            }
        }
    }
    for parent in parents {
        fs::create_dir_all(&parent)
            .map_err(|error| io_error("创建快照目录", &parent, error))?;
    }

    // 用 rayon 的有界线程池并发复制：文件之间并行，每个 rayon 线程复用一份读写缓冲。
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .thread_name(|index| format!("pz-version-copy-{index}"))
        .build()
        .map_err(|error| format!("创建复制线程池失败：{error}"))?;
    let processed_files = AtomicU64::new(0);
    let processed_bytes = AtomicU64::new(0);
    let cancelled = AtomicBool::new(false);
    let finished = AtomicBool::new(false);
    let first_error: Mutex<Option<String>> = Mutex::new(None);
    let last_path: Mutex<Option<String>> = Mutex::new(None);
    let mut progress_error: Option<String> = None;

    thread::scope(|scope| {
        let handle = scope.spawn(|| {
            pool.install(|| {
                files.par_iter().for_each_init(
                    || vec![0u8; COPY_BUFFER_SIZE],
                    |buffer, file| {
                        if cancelled.load(Ordering::Relaxed) {
                            return;
                        }
                        let source = game_path.join(&file.path);
                        let destination = snapshot_root.join(&file.path);
                        match copy_file_streaming(
                            buffer,
                            &source,
                            &destination,
                            file.size,
                            &cancelled,
                        ) {
                            Ok(()) => {
                                processed_files.fetch_add(1, Ordering::Relaxed);
                                processed_bytes.fetch_add(file.size, Ordering::Relaxed);
                                if let Ok(mut slot) = last_path.lock() {
                                    *slot = Some(file.path.clone());
                                }
                            }
                            Err(error) => {
                                if let Ok(mut slot) = first_error.lock() {
                                    if slot.is_none() {
                                        *slot = Some(error);
                                    }
                                }
                                cancelled.store(true, Ordering::SeqCst);
                            }
                        }
                    },
                );
            });
            finished.store(true, Ordering::SeqCst);
        });

        // progress 不是 Send，只能留在调用线程：每 120ms 读一次原子量上报。
        let mut last_emit: Option<Instant> = None;
        loop {
            if finished.load(Ordering::SeqCst) {
                break;
            }
            let should_emit =
                last_emit.is_none_or(|last| last.elapsed() >= Duration::from_millis(120));
            if should_emit && !cancelled.load(Ordering::SeqCst) {
                last_emit = Some(Instant::now());
                let copied = processed_files.load(Ordering::Relaxed);
                let current_path = last_path.lock().ok().and_then(|slot| slot.clone());
                if let Err(error) = progress(ProgressEvent {
                    phase: "copying".to_string(),
                    processed_files: copied,
                    total_files,
                    processed_bytes: processed_bytes.load(Ordering::Relaxed),
                    total_bytes,
                    current_path,
                }) {
                    progress_error = Some(error);
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        let _ = handle.join();
    });

    if let Some(error) = progress_error {
        return Err(error);
    }
    if let Some(error) = first_error.into_inner().unwrap_or(None) {
        return Err(error);
    }
    let copied_files = processed_files.load(Ordering::Relaxed);
    if copied_files != total_files {
        return Err(format!(
            "并行复制未完成全部文件（{copied_files}/{total_files}）。"
        ));
    }
    // 收尾事件：把「复制文件」这一步推到 100%。
    let _ = progress(ProgressEvent {
        phase: "copying".to_string(),
        processed_files: total_files,
        total_files,
        processed_bytes: total_bytes,
        total_bytes,
        current_path: None,
    });
    Ok(())
}

/// 流式复制单个文件：每块都检查取消，大文件也能在块边界中断；
/// 结束后按清单里的 size 校验，避免拷到一半的文件被当成成功。
fn copy_file_streaming(
    buffer: &mut [u8],
    source: &Path,
    destination: &Path,
    expected_size: u64,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let mut reader =
        File::open(source).map_err(|error| io_error("打开源文件", source, error))?;
    let mut writer =
        File::create(destination).map_err(|error| io_error("创建目标文件", destination, error))?;
    let mut written = 0u64;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("操作已取消".to_string());
        }
        let read = reader
            .read(buffer)
            .map_err(|error| io_error("读取源文件", source, error))?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| io_error("写入目标文件", destination, error))?;
        written += read as u64;
    }
    writer
        .flush()
        .map_err(|error| io_error("写入目标文件", destination, error))?;
    if written != expected_size {
        return Err(format!(
            "复制快照文件大小不一致：{}（期望 {expected_size}，实际 {written}）",
            source.display()
        ));
    }
    Ok(())
}

pub fn create_full_snapshot(
    game_path: &Path,
    store: &Path,
    steam_build_id: Option<&str>,
    branch: &str,
    label: Option<&str>,
    parallel: bool,
    progress: Progress<'_>,
) -> Result<SnapshotOutcome, String> {
    ensure_store_outside_root(store, game_path)?;
    let mut meta = load_meta(store)?;
    let files = scan_directory(game_path, parallel, progress)?;
    let root_hash = compute_root_hash(&files);
    let total_bytes: u64 = files.iter().map(|file| file.size).sum();

    // 最新版本已经是同样内容的完整快照时没必要再复制一份；
    // 但如果最新版本是增量版本，仍然允许补一个检查点来缩短重建链。
    let duplicate_full = meta
        .versions
        .last()
        .filter(|latest| latest.is_full() && latest.root_hash == root_hash)
        .cloned();
    if let Some(latest) = duplicate_full {
        // 内容没变但 Steam buildid 可能变了（例如只改了 depot 清单）：
        // 记下新的 buildid，避免页面上永远挂着「Steam 可能已经更新」。
        if let Some(build) = steam_build_id {
            if meta.last_seen_build_id.as_deref() != Some(build) {
                meta.last_seen_build_id = Some(build.to_string());
                save_meta(store, &meta)?;
            }
        }
        return Ok(SnapshotOutcome {
            created: false,
            version: Some(latest.clone()),
            message: format!("当前文件和「{}」完全一样，不用再存一次。", latest.label),
        });
    }

    let id = new_version_id(store, &meta);
    let parent_id = meta.versions.last().map(|record| record.id.clone());
    let base_type = "full";
    let label = label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_label(meta.versions.is_empty(), true, steam_build_id));

    let snapshot_root = if parent_id.is_none() {
        store.join(BASE_DIR).join(&id)
    } else {
        store.join(CHECKPOINT_DIR).join(&id)
    };

    let workers = worker_count(files.len(), parallel);
    if let Err(error) =
        copy_snapshot_files(game_path, &snapshot_root, &files, total_bytes, workers, progress)
    {
        remove_if_exists(&snapshot_root);
        return Err(error);
    }

    let now = now_seconds();
    let manifest = VersionManifest {
        version_id: id.clone(),
        parent_version_id: parent_id.clone(),
        steam_build_id: steam_build_id.map(str::to_string),
        branch: branch.to_string(),
        base_type: base_type.to_string(),
        created_at: now,
        root_hash: root_hash.clone(),
        total_bytes,
        files,
    };
    let record = VersionRecord {
        id: id.clone(),
        parent_id,
        steam_build_id: steam_build_id.map(str::to_string),
        branch: branch.to_string(),
        base_type: base_type.to_string(),
        created_at: now,
        root_hash,
        metadata_hash: compute_metadata_hash(&manifest.files),
        file_count: manifest.files.len() as u64,
        total_bytes,
        label: label.clone(),
        change_log: None,
        mod_versions: Vec::new(),
    };

    if let Err(error) = save_manifest(store, &manifest) {
        remove_if_exists(&snapshot_root);
        return Err(error);
    }
    meta.versions.push(record.clone());
    if let Some(build) = steam_build_id {
        meta.last_seen_build_id = Some(build.to_string());
    }
    meta.active_version_id = Some(id);
    if let Err(error) = save_meta(store, &meta) {
        remove_if_exists(&snapshot_root);
        let _ = fs::remove_file(manifest_path(store, &record.id));
        return Err(error);
    }

    Ok(SnapshotOutcome {
        created: true,
        version: Some(record.clone()),
        message: format!(
            "已完整保存为「{}」：{} 个文件，{}。",
            label,
            manifest.files.len(),
            format_bytes(total_bytes)
        ),
    })
}

pub fn capture_version(
    game_path: &Path,
    store: &Path,
    steam_build_id: Option<&str>,
    branch: &str,
    label: Option<&str>,
    parallel: bool,
    progress: Progress<'_>,
) -> Result<CaptureOutcome, String> {
    ensure_store_outside_root(store, game_path)?;
    let mut meta = load_meta(store)?;
    let latest = meta
        .versions
        .last()
        .cloned()
        .ok_or_else(|| "还没有基准版本，请先创建一个完整快照。".to_string())?;
    let latest_manifest = load_manifest(store, &latest.id)?;

    // 大小和修改时间都没变的文件直接沿用上次的 SHA-256，不打开文件：
    // Steam 更新通常只重写少量文件，增量捕获因此能快很多。
    let entries = collect_walk_entries(game_path)?;
    if entries.is_empty() {
        return Err(format!(
            "目录里没有可扫描的文件：{}，请确认路径是否正确。",
            game_path.display()
        ));
    }
    let latest_by_path: HashMap<String, &ManifestFile> = latest_manifest
        .files
        .iter()
        .map(|file| (normalize_key(&file.path), file))
        .collect();
    let cached: Vec<Option<String>> = entries
        .iter()
        .map(|entry| {
            latest_by_path
                .get(&normalize_key(&display_relative(&entry.relative)))
                .filter(|old| old.size == entry.size && old.mtime == entry.mtime)
                .map(|old| old.sha256.clone())
        })
        .collect();
    let workers = worker_count(entries.len(), parallel);
    let files = scan_entries(game_path, &entries, &cached, workers, progress)?;
    let root_hash = compute_root_hash(&files);
    if root_hash == latest.root_hash {
        if let Some(build) = steam_build_id {
            if meta.last_seen_build_id.as_deref() != Some(build) {
                meta.last_seen_build_id = Some(build.to_string());
                save_meta(store, &meta)?;
            }
        }
        return Ok(CaptureOutcome {
            created: false,
            version: Some(latest.clone()),
            message: format!("当前文件和「{}」完全一样，没有需要保存的变化。", latest.label),
        });
    }

    let latest_map: HashMap<String, ManifestFile> = latest_manifest
        .files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();
    let next_map: HashMap<String, ManifestFile> = files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();

    let added: Vec<ManifestFile> = files
        .iter()
        .filter(|file| !latest_map.contains_key(&normalize_key(&file.path)))
        .cloned()
        .collect();
    let removed: Vec<String> = latest_manifest
        .files
        .iter()
        .filter(|file| !next_map.contains_key(&normalize_key(&file.path)))
        .map(|file| file.path.clone())
        .collect();
    let modified: Vec<(ManifestFile, ManifestFile)> = files
        .iter()
        .filter_map(|file| {
            let key = normalize_key(&file.path);
            let old = latest_map.get(&key)?;
            (old.sha256 != file.sha256).then(|| (old.clone(), file.clone()))
        })
        .collect();

    if added.is_empty() && removed.is_empty() && modified.is_empty() {
        return Ok(CaptureOutcome {
            created: false,
            version: Some(latest.clone()),
            message: format!("当前文件和「{}」没有区别。", latest.label),
        });
    }

    let id = new_version_id(store, &meta);
    let label = label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_label(false, false, steam_build_id));
    let delta_folder = format!("{}__{}", latest.id, id);
    let delta_root = store.join(DELTA_DIR).join(&delta_folder);
    let temp_root = store.join(TMP_DIR).join(&id);

    let result = (|| -> Result<(Vec<VersionAddedFile>, Vec<VersionModifiedFile>, u64), String> {
        fs::create_dir_all(&delta_root)
            .map_err(|error| io_error("创建增量目录", &delta_root, error))?;
        fs::create_dir_all(&temp_root)
            .map_err(|error| io_error("创建临时目录", &temp_root, error))?;

        let total_changes = (modified.len() + added.len()) as u64;
        let mut processed = 0u64;
        let mut delta_bytes = 0u64;
        let mut modified_entries = Vec::with_capacity(modified.len());
        let mut added_entries = Vec::with_capacity(added.len());

        {
            let mut materializer = Materializer::new(store, &meta);

            for (index, (old, new)) in modified.iter().enumerate() {
                let old_temp = temp_root.join(format!("{index:04}.old"));
                materializer.materialize(&latest.id, &new.path, &old_temp, 0)?;
                let delta_relative = format!("{DELTA_DIR}/{delta_folder}/{index:04}.pzd");
                let delta_path = store.join(&delta_relative);
                let new_path = game_path.join(&new.path);
                create_delta(Some(&old_temp), &new_path, &delta_path)?;
                delta_bytes += fs::metadata(&delta_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                remove_if_exists(&old_temp);
                processed += 1;
                progress(ProgressEvent {
                    phase: "delta".to_string(),
                    processed_files: processed,
                    total_files: total_changes,
                    processed_bytes: delta_bytes,
                    total_bytes: 0,
                    current_path: Some(new.path.clone()),
                })?;
                modified_entries.push(VersionModifiedFile {
                    path: new.path.clone(),
                    old_size: old.size,
                    new_size: new.size,
                    old_sha256: old.sha256.clone(),
                    new_sha256: new.sha256.clone(),
                    delta: delta_relative,
                });
            }

            for (index, new) in added.iter().enumerate() {
                let delta_relative = format!("{DELTA_DIR}/{delta_folder}/add-{index:04}.pzd");
                let delta_path = store.join(&delta_relative);
                create_delta(None, &game_path.join(&new.path), &delta_path)?;
                delta_bytes += fs::metadata(&delta_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                processed += 1;
                progress(ProgressEvent {
                    phase: "delta".to_string(),
                    processed_files: processed,
                    total_files: total_changes,
                    processed_bytes: delta_bytes,
                    total_bytes: 0,
                    current_path: Some(new.path.clone()),
                })?;
                added_entries.push(VersionAddedFile {
                    path: new.path.clone(),
                    size: new.size,
                    sha256: new.sha256.clone(),
                    delta: delta_relative,
                });
            }
        }

        Ok((added_entries, modified_entries, delta_bytes))
    })();

    remove_if_exists(&temp_root);

    let (added_entries, modified_entries, delta_bytes) = match result {
        Ok(value) => value,
        Err(error) => {
            remove_if_exists(&delta_root);
            return Err(error);
        }
    };

    let now = now_seconds();
    let total_bytes: u64 = files.iter().map(|file| file.size).sum();
    let added_count = added_entries.len();
    let removed_count = removed.len();
    let modified_count = modified_entries.len();
    let change_log = VersionChangeLog {
        from: latest.id.clone(),
        to: id.clone(),
        from_build_id: latest.steam_build_id.clone(),
        to_build_id: steam_build_id.map(str::to_string),
        added: added_entries,
        removed: removed.clone(),
        modified: modified_entries,
    };
    let manifest = VersionManifest {
        version_id: id.clone(),
        parent_version_id: Some(latest.id.clone()),
        steam_build_id: steam_build_id.map(str::to_string),
        branch: branch.to_string(),
        base_type: "delta".to_string(),
        created_at: now,
        root_hash: root_hash.clone(),
        total_bytes,
        files,
    };
    let record = VersionRecord {
        id: id.clone(),
        parent_id: Some(latest.id.clone()),
        steam_build_id: steam_build_id.map(str::to_string),
        branch: branch.to_string(),
        base_type: "delta".to_string(),
        created_at: now,
        root_hash,
        metadata_hash: compute_metadata_hash(&manifest.files),
        file_count: manifest.files.len() as u64,
        total_bytes,
        label: label.clone(),
        change_log: Some(change_log.clone()),
        mod_versions: Vec::new(),
    };

    // 额外落一份独立的 changelog 到增量目录，便于离线查看/排障；
    // 页面展示仍用 meta 里的记录，避免每次展开都读磁盘。
    let changelog_result = (|| -> Result<(), String> {
        let changelog_path = delta_root.join("changelog.json");
        let content = serde_json::to_string_pretty(&change_log)
            .map_err(|error| format!("序列化变更清单失败：{error}"))?;
        fs::write(&changelog_path, format!("{content}\n"))
            .map_err(|error| io_error("写入变更清单", &changelog_path, error))
    })();
    if let Err(error) = changelog_result {
        remove_if_exists(&delta_root);
        return Err(error);
    }

    if let Err(error) = save_manifest(store, &manifest) {
        remove_if_exists(&delta_root);
        return Err(error);
    }
    meta.versions.push(record.clone());
    if let Some(build) = steam_build_id {
        meta.last_seen_build_id = Some(build.to_string());
    }
    meta.active_version_id = Some(id);
    if let Err(error) = save_meta(store, &meta) {
        let _ = fs::remove_file(manifest_path(store, &record.id));
        remove_if_exists(&delta_root);
        return Err(error);
    }

    Ok(CaptureOutcome {
        created: true,
        version: Some(record),
        message: format!(
            "已保存「{}」：新增 {}，删除 {}，修改 {}，变化部分共 {}。",
            label,
            added_count,
            removed_count,
            modified_count,
            format_bytes(delta_bytes)
        ),
    })
}

// ---------------------------------------------------------------------------
// 转换为独立版本（重挂到基准快照）
// ---------------------------------------------------------------------------

/// 一个版本自己的增量包目录。新版本写在 `deltas/<from>__<to>/`；
/// 更早的版本曾把 `.pzd` 直接写在存储根目录，删除/重写时两个位置都要清。
fn delta_dirs(store: &Path, parent_id: &str, version_id: &str) -> Vec<PathBuf> {
    let folder = format!("{parent_id}__{version_id}");
    vec![store.join(DELTA_DIR).join(&folder), store.join(&folder)]
}

/// 沿父链（含 `start_id`）找到最近的完整快照。
fn nearest_full_ancestor<'a>(
    meta: &'a VersionStoreMeta,
    start_id: &str,
) -> Option<&'a VersionRecord> {
    let mut cursor = Some(start_id.to_string());
    let mut hops = 0u32;
    while let Some(id) = cursor {
        let record = meta.versions.iter().find(|record| record.id == id)?;
        if record.is_full() {
            return Some(record);
        }
        hops += 1;
        if hops > MAX_CHAIN_DEPTH {
            return None;
        }
        cursor = record.parent_id.clone();
    }
    None
}

/// 把 `record` 的增量包重建成相对 `base`（完整快照）的独立增量，
/// 并把它的父版本改成 `base`。之后删除 `base` 与它之间的旧版本不再影响它。
fn rebase_onto(
    store: &Path,
    record: &VersionRecord,
    base: &VersionRecord,
    progress: Progress,
) -> Result<String, String> {
    if !base.is_full() {
        return Err("转换基准必须是完整快照。".to_string());
    }
    if record.is_full() {
        return Ok(format!("「{}」是完整备份，本身就是独立的。", record.label));
    }
    if record.parent_id.as_deref() == Some(base.id.as_str()) {
        return Ok(format!("「{}」已经是独立版本。", record.label));
    }

    let mut meta = load_meta(store)?;
    let target_manifest = load_manifest(store, &record.id)?;
    let base_manifest = load_manifest(store, &base.id)?;
    let base_root = full_snapshot_root(store, base);

    let base_map: HashMap<String, ManifestFile> = base_manifest
        .files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();
    let target_map: HashMap<String, ManifestFile> = target_manifest
        .files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();

    let added: Vec<ManifestFile> = target_manifest
        .files
        .iter()
        .filter(|file| !base_map.contains_key(&normalize_key(&file.path)))
        .cloned()
        .collect();
    let removed: Vec<String> = base_manifest
        .files
        .iter()
        .filter(|file| !target_map.contains_key(&normalize_key(&file.path)))
        .map(|file| file.path.clone())
        .collect();
    let modified: Vec<(ManifestFile, ManifestFile)> = target_manifest
        .files
        .iter()
        .filter_map(|file| {
            let key = normalize_key(&file.path);
            let old = base_map.get(&key)?;
            (old.sha256 != file.sha256).then(|| (old.clone(), file.clone()))
        })
        .collect();

    let delta_folder = format!("{}__{}", base.id, record.id);
    let delta_root = store.join(DELTA_DIR).join(&delta_folder);
    let temp_root = store.join(TMP_DIR).join(format!("{}-rebase", record.id));
    // 上一次转换失败可能留下同名目录；当前记录的父版本不是这个基准时才清。
    remove_if_exists(&delta_root);
    remove_if_exists(&temp_root);

    let result = (|| -> Result<(Vec<VersionAddedFile>, Vec<VersionModifiedFile>, u64), String> {
        fs::create_dir_all(&delta_root)
            .map_err(|error| io_error("创建增量目录", &delta_root, error))?;
        fs::create_dir_all(&temp_root)
            .map_err(|error| io_error("创建临时目录", &temp_root, error))?;

        let total_changes = (modified.len() + added.len()) as u64;
        let mut processed = 0u64;
        let mut delta_bytes = 0u64;
        let mut modified_entries = Vec::with_capacity(modified.len());
        let mut added_entries = Vec::with_capacity(added.len());

        {
            let mut materializer = Materializer::new(store, &meta);

            for (index, (old, new)) in modified.iter().enumerate() {
                let target_temp = temp_root.join(format!("{index:04}.new"));
                materializer.materialize(&record.id, &new.path, &target_temp, 0)?;
                let base_path = base_root.join(&old.path);
                if !base_path.is_file() {
                    remove_if_exists(&target_temp);
                    return Err(format!("完整快照缺少文件：{}", base_path.display()));
                }
                let delta_relative = format!("{DELTA_DIR}/{delta_folder}/{index:04}.pzd");
                let delta_path = store.join(&delta_relative);
                let delta_result = create_delta(Some(&base_path), &target_temp, &delta_path);
                remove_if_exists(&target_temp);
                delta_result?;
                delta_bytes += fs::metadata(&delta_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                processed += 1;
                progress(ProgressEvent {
                    phase: "rebase".to_string(),
                    processed_files: processed,
                    total_files: total_changes,
                    processed_bytes: delta_bytes,
                    total_bytes: 0,
                    current_path: Some(new.path.clone()),
                })?;
                modified_entries.push(VersionModifiedFile {
                    path: new.path.clone(),
                    old_size: old.size,
                    new_size: new.size,
                    old_sha256: old.sha256.clone(),
                    new_sha256: new.sha256.clone(),
                    delta: delta_relative,
                });
            }

            for (index, new) in added.iter().enumerate() {
                let target_temp = temp_root.join(format!("add-{index:04}.new"));
                materializer.materialize(&record.id, &new.path, &target_temp, 0)?;
                let delta_relative =
                    format!("{DELTA_DIR}/{delta_folder}/add-{index:04}.pzd");
                let delta_path = store.join(&delta_relative);
                let delta_result = create_delta(None, &target_temp, &delta_path);
                remove_if_exists(&target_temp);
                delta_result?;
                delta_bytes += fs::metadata(&delta_path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                processed += 1;
                progress(ProgressEvent {
                    phase: "rebase".to_string(),
                    processed_files: processed,
                    total_files: total_changes,
                    processed_bytes: delta_bytes,
                    total_bytes: 0,
                    current_path: Some(new.path.clone()),
                })?;
                added_entries.push(VersionAddedFile {
                    path: new.path.clone(),
                    size: new.size,
                    sha256: new.sha256.clone(),
                    delta: delta_relative,
                });
            }
        }

        Ok((added_entries, modified_entries, delta_bytes))
    })();

    remove_if_exists(&temp_root);

    let (added_entries, modified_entries, delta_bytes) = match result {
        Ok(value) => value,
        Err(error) => {
            remove_if_exists(&delta_root);
            return Err(error);
        }
    };

    let added_count = added_entries.len();
    let removed_count = removed.len();
    let modified_count = modified_entries.len();
    let change_log = VersionChangeLog {
        from: base.id.clone(),
        to: record.id.clone(),
        from_build_id: base.steam_build_id.clone(),
        to_build_id: record.steam_build_id.clone(),
        added: added_entries,
        removed,
        modified: modified_entries,
    };

    let changelog_path = delta_root.join("changelog.json");
    if let Err(error) = (|| -> Result<(), String> {
        let content = serde_json::to_string_pretty(&change_log)
            .map_err(|error| format!("序列化变更清单失败：{error}"))?;
        fs::write(&changelog_path, format!("{content}\n"))
            .map_err(|error| io_error("写入变更清单", &changelog_path, error))
    })() {
        remove_if_exists(&delta_root);
        return Err(error);
    }

    let old_parent = record.parent_id.clone();
    let mut manifest = target_manifest;
    manifest.parent_version_id = Some(base.id.clone());
    if let Err(error) = save_manifest(store, &manifest) {
        remove_if_exists(&delta_root);
        return Err(error);
    }

    let Some(slot) = meta
        .versions
        .iter_mut()
        .find(|candidate| candidate.id == record.id)
    else {
        manifest.parent_version_id = old_parent;
        let _ = save_manifest(store, &manifest);
        remove_if_exists(&delta_root);
        return Err(format!("版本不存在：{}", record.id));
    };
    slot.parent_id = Some(base.id.clone());
    slot.change_log = Some(change_log);
    if let Err(error) = save_meta(store, &meta) {
        // 回滚清单，避免 parent_version_id 和 meta 指向不同的父版本。
        manifest.parent_version_id = old_parent;
        let _ = save_manifest(store, &manifest);
        remove_if_exists(&delta_root);
        return Err(error);
    }

    // 旧父版本的增量包已经没人引用；清掉，失败只留下孤儿文件。
    if let Some(old_parent) = old_parent.as_deref() {
        for dir in delta_dirs(store, old_parent, &record.id) {
            remove_if_exists(&dir);
        }
    }

    Ok(format!(
        "已把「{}」转换为独立版本（基准「{}」）：新增 {}，删除 {}，修改 {}，增量包 {}。",
        record.label,
        base.label,
        added_count,
        removed_count,
        modified_count,
        format_bytes(delta_bytes)
    ))
}

/// 把某个增量版本重挂到父链上最近的完整快照。已经是独立版本时直接返回提示。
pub fn rebase_version_progress(
    store: &Path,
    version_id: &str,
    progress: Progress,
) -> Result<String, String> {
    let meta = load_meta(store)?;
    let Some(record) = meta
        .versions
        .iter()
        .find(|record| record.id == version_id)
        .cloned()
    else {
        return Err(format!("版本不存在：{version_id}"));
    };
    if record.is_full() {
        return Ok(format!("「{}」是完整备份，本身就是独立的。", record.label));
    }
    let Some(parent_id) = record.parent_id.clone() else {
        return Ok(format!("「{}」已经是独立版本。", record.label));
    };
    let Some(parent) = meta
        .versions
        .iter()
        .find(|candidate| candidate.id == parent_id)
    else {
        return Err(format!(
            "版本「{}」的父版本不存在，版本链可能已损坏。",
            record.label
        ));
    };
    if parent.is_full() {
        return Ok(format!("「{}」已经是独立版本，不需要转换。", record.label));
    }
    let Some(base) = nearest_full_ancestor(&meta, &parent_id).cloned() else {
        return Err(format!(
            "「{}」找不到可用的完整快照，无法转换为独立版本。",
            record.label
        ));
    };
    rebase_onto(store, &record, &base, progress)
}

/// 完整备份的内容自带，只是挂靠位置变了：把 `parent_id` 指到新的上一个版本即可，
/// 不需要重建任何文件。
fn reattach_full_snapshot(
    store: &Path,
    record: &VersionRecord,
    new_parent: &str,
) -> Result<(), String> {
    let mut meta = load_meta(store)?;
    let mut manifest = load_manifest(store, &record.id)?;
    manifest.parent_version_id = Some(new_parent.to_string());
    save_manifest(store, &manifest)?;
    let Some(slot) = meta
        .versions
        .iter_mut()
        .find(|candidate| candidate.id == record.id)
    else {
        return Err(format!("版本不存在：{}", record.id));
    };
    slot.parent_id = Some(new_parent.to_string());
    save_meta(store, &meta)
}

// ---------------------------------------------------------------------------
// 还原 / 校验
// ---------------------------------------------------------------------------

struct RestoreFileOutcome {
    replaced: bool,
    size: u64,
}

fn restore_file_is_current(
    entry: &ManifestFile,
    current_map: &HashMap<String, ManifestFile>,
) -> bool {
    current_map
        .get(&normalize_key(&entry.path))
        .is_some_and(|current| current.size == entry.size && current.sha256 == entry.sha256)
}

fn restore_one_file(
    game_path: &Path,
    record: &VersionRecord,
    entry: &ManifestFile,
    index: usize,
    materializer: &mut Materializer<'_>,
) -> Result<RestoreFileOutcome, String> {
    let target_path = game_path.join(&entry.path);
    let file_name = target_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp_path = target_path.with_file_name(format!(".{file_name}.pzrestore{index}"));

    let result = (|| -> Result<(), String> {
        materializer.materialize(&record.id, &entry.path, &temp_path, 0)?;
        let actual = hash_file(&temp_path)?;
        if actual != entry.sha256 {
            return Err(format!(
                "还原文件校验失败：{}（期望 {}，实际 {}）",
                entry.path, entry.sha256, actual
            ));
        }
        if target_path.exists() {
            fs::remove_file(&target_path)
                .map_err(|error| io_error("覆盖游戏文件", &target_path, error))?;
        }
        fs::rename(&temp_path, &target_path).or_else(|_| {
            fs::copy(&temp_path, &target_path)
                .map(|_| ())
                .and_then(|_| fs::remove_file(&temp_path))
                .map_err(|error| io_error("写入游戏文件", &target_path, error))
        })?;
        Ok(())
    })();

    if let Err(error) = result {
        remove_if_exists(&temp_path);
        return Err(error);
    }
    Ok(RestoreFileOutcome {
        replaced: true,
        size: entry.size,
    })
}

fn restore_target_files(
    game_path: &Path,
    store: &Path,
    meta: &VersionStoreMeta,
    record: &VersionRecord,
    target_files: &[ManifestFile],
    current_map: &HashMap<String, ManifestFile>,
    total_files: u64,
    total_bytes: u64,
    workers: usize,
    progress: Progress<'_>,
) -> Result<(u64, u64, u64), String> {
    // 单线程路径：保留原来的逐文件还原行为。
    if workers <= 1 {
        let mut materializer = Materializer::new(store, meta);
        let mut replaced = 0u64;
        let mut kept = 0u64;
        let mut bytes_written = 0u64;

        for (index, entry) in target_files.iter().enumerate() {
            if restore_file_is_current(entry, current_map) {
                kept += 1;
                // 还原写入阶段不再响应取消，避免留下“改了一半”的游戏目录。
                let _ = progress(ProgressEvent {
                    phase: "restoring".to_string(),
                    processed_files: index as u64 + 1,
                    total_files,
                    processed_bytes: bytes_written,
                    total_bytes,
                    current_path: Some(entry.path.clone()),
                });
                continue;
            }

            let outcome =
                restore_one_file(game_path, record, entry, index, &mut materializer)?;
            replaced += 1;
            bytes_written += outcome.size;
            let _ = progress(ProgressEvent {
                phase: "restoring".to_string(),
                processed_files: index as u64 + 1,
                total_files,
                processed_bytes: bytes_written,
                total_bytes,
                current_path: Some(entry.path.clone()),
            });
        }
        return Ok((replaced, kept, bytes_written));
    }

    let next = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let (sender, receiver) = mpsc::channel::<(usize, Result<RestoreFileOutcome, String>)>();
    let mut first_error: Option<String> = None;
    let mut replaced = 0u64;
    let mut kept = 0u64;
    let mut bytes_written = 0u64;
    let mut completed = 0u64;

    thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let next = &next;
            let cancelled = &cancelled;
            scope.spawn(move || {
                // Materializer 内部的清单缓存用 Rc，不能跨线程共享；每个 worker 自己持有一个。
                let mut materializer = Materializer::new(store, meta);
                loop {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= target_files.len() {
                        break;
                    }
                    let entry = &target_files[index];
                    let result = if restore_file_is_current(entry, current_map) {
                        Ok(RestoreFileOutcome {
                            replaced: false,
                            size: 0,
                        })
                    } else {
                        restore_one_file(game_path, record, entry, index, &mut materializer)
                    };
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);

        for (index, result) in receiver {
            completed += 1;
            match result {
                Ok(outcome) => {
                    if outcome.replaced {
                        replaced += 1;
                        bytes_written += outcome.size;
                    } else {
                        kept += 1;
                    }
                    let entry = &target_files[index];
                    // 写入阶段忽略回调错误：取消请求不会把还原切成半截。
                    let _ = progress(ProgressEvent {
                        phase: "restoring".to_string(),
                        processed_files: completed,
                        total_files,
                        processed_bytes: bytes_written,
                        total_bytes,
                        current_path: Some(entry.path.clone()),
                    });
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
        }
    });

    if let Some(error) = first_error {
        return Err(error);
    }
    if completed != target_files.len() as u64 {
        return Err("并行还原未完成全部文件。".to_string());
    }
    Ok((replaced, kept, bytes_written))
}

pub fn restore_version(
    game_path: &Path,
    store: &Path,
    version_id: &str,
    force: bool,
    parallel: bool,
    progress: Progress<'_>,
) -> Result<RestoreOutcome, String> {
    ensure_store_outside_root(store, game_path)?;
    let mut meta = load_meta(store)?;
    let record = meta
        .versions
        .iter()
        .find(|record| record.id == version_id)
        .cloned()
        .ok_or_else(|| format!("版本不存在：{version_id}"))?;
    let target_manifest = load_manifest(store, &record.id)?;
    let target_map: HashMap<String, ManifestFile> = target_manifest
        .files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();

    // 先完整校验当前目录：不匹配任何已记录版本时，除非用户明确选择强制还原，
    // 否则不覆盖（方案 D 的“未知修改不自动覆盖”原则）。
    let current_files = scan_directory(game_path, parallel, progress)?;
    let current_root = compute_root_hash(&current_files);
    let known = meta
        .versions
        .iter()
        .any(|version| version.root_hash == current_root);
    if !known && !force {
        return Err(
            "当前文件和保存过的版本都对不上（可能被 Steam 更新或手动改过）。请先「保存变化」，或者确认强制还原。"
                .to_string(),
        );
    }

    let current_map: HashMap<String, ManifestFile> = current_files
        .iter()
        .cloned()
        .map(|file| (normalize_key(&file.path), file))
        .collect();
    let total_files = target_manifest.files.len() as u64 + current_files.len() as u64;
    let total_bytes = target_manifest.total_bytes;
    let workers = worker_count(target_manifest.files.len(), parallel);
    let (replaced, kept, bytes_written) = restore_target_files(
        game_path,
        store,
        &meta,
        &record,
        &target_manifest.files,
        &current_map,
        total_files,
        total_bytes,
        workers,
        progress,
    )?;
    let mut removed = 0u64;
    let mut trash_root: Option<PathBuf> = None;

    for (index, current) in current_files.iter().enumerate() {
        let key = normalize_key(&current.path);
        if target_map.contains_key(&key) {
            continue;
        }
        let source = game_path.join(&current.path);
        let root = trash_root.get_or_insert_with(|| {
            store
                .join(TRASH_DIR)
                .join(format!("restore-{}", now_seconds()))
        });
        let destination = root.join(&current.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| io_error("创建回收目录", parent, error))?;
        }
        if fs::rename(&source, &destination).is_err() {
            fs::copy(&source, &destination)
                .map_err(|error| io_error("移动多余文件到回收区", &source, error))?;
            fs::remove_file(&source)
                .map_err(|error| io_error("移除多余文件", &source, error))?;
        }
        removed += 1;
        let _ = progress(ProgressEvent {
            phase: "cleaning".to_string(),
            processed_files: index as u64 + 1,
            total_files,
            processed_bytes: bytes_written,
            total_bytes,
            current_path: Some(current.path.clone()),
        });
    }

    meta.active_version_id = Some(record.id.clone());
    save_meta(store, &meta)?;

    let trash_path = trash_root.map(|path| path.to_string_lossy().into_owned());
    let mut message = format!(
        "已还原到「{}」：替换 {} 个文件，{} 个保持不变，{} 个多余文件移到了回收区（写入 {}）。",
        record.label,
        replaced,
        kept,
        removed,
        format_bytes(bytes_written)
    );
    if let Some(path) = &trash_path {
        message.push_str(&format!(" 多余文件在：{path}"));
    }

    Ok(RestoreOutcome {
        replaced,
        kept,
        removed,
        bytes_written,
        trash_path,
        message,
    })
}

pub fn verify_current_version(
    game_path: &Path,
    store: &Path,
    parallel: bool,
    progress: Progress<'_>,
) -> Result<VerifyOutcome, String> {
    let meta = load_meta(store)?;
    let files = scan_directory(game_path, parallel, progress)?;
    let root_hash = compute_root_hash(&files);
    let matched = meta
        .versions
        .iter()
        .rev()
        .find(|record| record.root_hash == root_hash)
        .cloned();

    let message = match &matched {
        Some(record) => format!("当前文件和「{}」完全一致。", record.label),
        None => "当前文件和保存过的版本都不一样。".to_string(),
    };

    Ok(VerifyOutcome {
        matched_version_id: matched.map(|record| record.id),
        root_hash,
        message,
    })
}

// ---------------------------------------------------------------------------
// 状态 / 删除
// ---------------------------------------------------------------------------

fn metadata_matches(record: &VersionRecord, metadata_hash: &str) -> bool {
    !record.metadata_hash.is_empty() && record.metadata_hash == metadata_hash
}

pub fn get_status(
    game_path: Option<&Path>,
    store: &Path,
    steam_build_id: Option<&str>,
    branch: &str,
    operation_running: bool,
) -> Result<VersionStatus, String> {
    let meta = load_meta(store)?;
    let versions = meta.versions.clone();
    let latest = versions.last().cloned();
    let last_captured_build_id = meta
        .last_seen_build_id
        .clone()
        .or_else(|| latest.as_ref().and_then(|record| record.steam_build_id.clone()));
    let steam_update_pending = match (steam_build_id, last_captured_build_id.as_deref()) {
        (Some(current), Some(previous)) => current != previous,
        _ => false,
    };

    let mut current_match = VersionCurrentMatch {
        kind: "none".to_string(),
        version_id: None,
        candidates: Vec::new(),
        checked_hashes: false,
        message: "还没保存过版本。".to_string(),
    };
    let mut pending_changes = None;

    match game_path {
        None => {
            current_match = VersionCurrentMatch {
                kind: "missing".to_string(),
                version_id: None,
                candidates: Vec::new(),
                checked_hashes: false,
                message: "没找到 Project Zomboid 的安装目录。".to_string(),
            };
        }
        Some(game) if !game.is_dir() => {
            current_match = VersionCurrentMatch {
                kind: "missing".to_string(),
                version_id: None,
                candidates: Vec::new(),
                checked_hashes: false,
                message: "Project Zomboid 的安装目录不存在。".to_string(),
            };
        }
        // 还没有任何版本时不用扫游戏目录：页面只需要显示“先创建基准版本”。
        Some(_) if versions.is_empty() => {}
        Some(game) => {
            let pairs = scan_metadata(game)?;
            let metadata_hash = compute_metadata_hash_from_pairs(&pairs);
            if !versions.is_empty() {
                let candidates: Vec<String> = versions
                    .iter()
                    .filter(|record| metadata_matches(record, &metadata_hash))
                    .map(|record| record.id.clone())
                    .collect();
                if let Some(version_id) = candidates.last().cloned() {
                    let label = versions
                        .iter()
                        .find(|record| record.id == version_id)
                        .map(|record| record.label.clone())
                        .unwrap_or_else(|| version_id.clone());
                    current_match = VersionCurrentMatch {
                        kind: "metadata".to_string(),
                        version_id: Some(version_id),
                        candidates,
                        checked_hashes: false,
                        message: format!(
                            "文件和「{label}」的路径、大小都一样，可能就是这个版本；点「检查当前版本」可以再确认。"
                        ),
                    };
                } else {
                    current_match = VersionCurrentMatch {
                        kind: "unknown".to_string(),
                        version_id: None,
                        candidates: Vec::new(),
                        checked_hashes: false,
                        message:
                            "当前文件和保存过的版本都对不上，可能被 Steam 更新或手动改过。"
                                .to_string(),
                    };
                }

                if let Some(latest) = &latest {
                    if let Ok(manifest) = load_manifest(store, &latest.id) {
                        let latest_map: HashMap<String, u64> = manifest
                            .files
                            .iter()
                            .map(|file| (normalize_key(&file.path), file.size))
                            .collect();
                        let current_map: HashMap<String, u64> = pairs.into_iter().collect();
                        let added = current_map
                            .keys()
                            .filter(|key| !latest_map.contains_key(*key))
                            .count() as u64;
                        let removed = latest_map
                            .keys()
                            .filter(|key| !current_map.contains_key(*key))
                            .count() as u64;
                        let modified = current_map
                            .iter()
                            .filter(|(key, size)| {
                                latest_map.get(*key).is_some_and(|old| old != *size)
                            })
                            .count() as u64;
                        if added + removed + modified > 0 {
                            pending_changes = Some(VersionPendingChanges {
                                added,
                                removed,
                                modified,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(VersionStatus {
        game_path: game_path.map(|path| path.to_string_lossy().into_owned()),
        store_path: store.to_string_lossy().into_owned(),
        store_exists: store.is_dir(),
        steam_build_id: steam_build_id.map(str::to_string),
        branch: branch.to_string(),
        last_captured_build_id,
        steam_update_pending,
        versions,
        current_match,
        pending_changes,
        active_version_id: meta.active_version_id,
        operation_running,
    })
}

#[allow(dead_code)]
pub fn delete_version(store: &Path, version_id: &str) -> Result<String, String> {
    delete_version_progress(store, version_id, &mut |_| Ok(()))
}

/// 幸存版本的父链是否伸进删除范围。增量沿 parent_id 向上找，撞到删除范围返回真、
/// 撞到范围外的完整快照返回假；完整备份本身自包含，只看直接父版本。
fn depends_on_doomed(
    meta: &VersionStoreMeta,
    record: &VersionRecord,
    doomed: &HashSet<String>,
) -> bool {
    if record.is_full() {
        return record
            .parent_id
            .as_deref()
            .is_some_and(|parent| doomed.contains(parent));
    }
    let mut cursor = record.parent_id.clone();
    let mut hops = 0u32;
    while let Some(id) = cursor {
        if doomed.contains(&id) {
            return true;
        }
        let Some(parent) = meta.versions.iter().find(|candidate| candidate.id == id) else {
            return false;
        };
        if parent.is_full() {
            return false;
        }
        hops += 1;
        if hops > MAX_CHAIN_DEPTH {
            return true;
        }
        cursor = parent.parent_id.clone();
    }
    false
}

/// 让所有还依赖删除范围的幸存版本摆脱依赖：
/// - 完整备份：父版本改成 None，目录从 `checkpoints/<id>` 挪到 `base/<id>`；
/// - 增量版本：整个物化成新的完整快照（`base/<id>`），清掉 change_log。
/// 按时间序处理，物化时父链仍然完整。返回被降级前记录的 (旧父版本, 版本) 对，
/// 供调用方在删除完成后清理它们的旧增量包。
fn promote_dependents_of_doomed(
    store: &Path,
    doomed: &HashSet<String>,
    progress: Progress,
) -> Result<Vec<(String, String)>, String> {
    let meta = load_meta(store)?;
    let mut promoted: Vec<(String, String)> = Vec::new();
    for record in meta.versions.iter().cloned() {
        if doomed.contains(&record.id) || !depends_on_doomed(&meta, &record, doomed) {
            continue;
        }
        let old_parent = record.parent_id.clone();
        let new_root = store.join(BASE_DIR).join(&record.id);
        if new_root.exists() {
            return Err(format!("完整快照目录已存在：{}", new_root.display()));
        }
        if record.is_full() {
            // 只挪位置：完整快照的内容不依赖父版本。
            let old_root = full_snapshot_root(store, &record);
            fs::rename(&old_root, &new_root)
                .map_err(|error| io_error("移动完整快照", &old_root, error))?;
        } else {
            // 物化成完整快照：逐文件沿旧链重建，写完才改 meta，中途失败可整体放弃。
            let manifest = load_manifest(store, &record.id)?;
            fs::create_dir_all(&new_root)
                .map_err(|error| io_error("创建完整快照目录", &new_root, error))?;
            let result = (|| -> Result<(), String> {
                let mut materializer = Materializer::new(store, &meta);
                let total = manifest.files.len() as u64;
                for (index, file) in manifest.files.iter().enumerate() {
                    let destination = new_root.join(&file.path);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent)
                            .map_err(|error| io_error("创建快照子目录", parent, error))?;
                    }
                    materializer.materialize(&record.id, &file.path, &destination, 0)?;
                    progress(ProgressEvent {
                        phase: "copying".to_string(),
                        processed_files: index as u64 + 1,
                        total_files: total,
                        processed_bytes: 0,
                        total_bytes: 0,
                        current_path: Some(file.path.clone()),
                    })?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                remove_if_exists(&new_root);
                return Err(error);
            }
            let mut manifest = manifest;
            manifest.base_type = "full".to_string();
            manifest.parent_version_id = None;
            if let Err(error) = save_manifest(store, &manifest) {
                remove_if_exists(&new_root);
                return Err(error);
            }
        }
        let mut meta = load_meta(store)?;
        let Some(slot) = meta
            .versions
            .iter_mut()
            .find(|candidate| candidate.id == record.id)
        else {
            return Err(format!("版本不存在：{}", record.id));
        };
        slot.parent_id = None;
        slot.base_type = "full".to_string();
        slot.change_log = None;
        save_meta(store, &meta)?;
        if let Some(old_parent) = old_parent {
            promoted.push((old_parent, record.id.clone()));
        }
    }
    Ok(promoted)
}

/// 「删除该版本及更早」：先让后面的版本摆脱对删除范围的依赖（重建成新的完整基准），
/// 再从新到旧逐个删除。phase：重建 = "copying"，删除 = "deleting"。
pub fn delete_versions_through_progress(
    store: &Path,
    version_id: &str,
    progress: Progress,
) -> Result<String, String> {
    let meta = load_meta(store)?;
    let Some(index) = meta
        .versions
        .iter()
        .position(|record| record.id == version_id)
    else {
        return Err(format!("版本不存在：{version_id}"));
    };
    let doomed: Vec<String> = meta.versions[..=index]
        .iter()
        .map(|record| record.id.clone())
        .collect();
    let doomed_set: HashSet<String> = doomed.iter().cloned().collect();

    let promoted = promote_dependents_of_doomed(store, &doomed_set, &mut *progress)?;
    let mut deleted = 0u64;
    for id in doomed.iter().rev() {
        delete_version_progress(store, id, progress)?;
        deleted += 1;
    }
    // 被降级版本的旧增量包已经没人引用，清掉；失败只留下孤儿文件。
    for (old_parent, id) in &promoted {
        for dir in delta_dirs(store, old_parent, id) {
            remove_if_exists(&dir);
        }
    }
    Ok(format!("已删除 {deleted} 个版本。"))
}

/// 删除任意版本；如果它后面还有版本依赖它，先把依赖者重挂到基准快照：
/// 增量依赖者重建增量包（phase = "rebase"），完整备份依赖者只改挂靠位置。
/// 随后交给严格的 `delete_version_progress` 落盘删除。
pub fn delete_version_with_rebase_progress(
    store: &Path,
    version_id: &str,
    progress: Progress,
) -> Result<String, String> {
    let meta = load_meta(store)?;
    let Some(target) = meta
        .versions
        .iter()
        .find(|record| record.id == version_id)
        .cloned()
    else {
        return Err(format!("版本不存在：{version_id}"));
    };
    let dependents: Vec<VersionRecord> = meta
        .versions
        .iter()
        .filter(|record| record.parent_id.as_deref() == Some(version_id))
        .cloned()
        .collect();
    if !dependents.is_empty() {
        let Some(parent_id) = target.parent_id.clone() else {
            return Err(format!(
                "「{}」是基础版本，后面还有版本依赖它，不能删除。",
                target.label
            ));
        };
        let Some(base) = nearest_full_ancestor(&meta, &parent_id).cloned() else {
            return Err(format!(
                "找不到「{}」的完整快照，无法重挂后面的版本。",
                target.label
            ));
        };
        for dependent in &dependents {
            if dependent.is_full() {
                reattach_full_snapshot(store, dependent, &parent_id)?;
            } else {
                rebase_onto(store, dependent, &base, &mut *progress)?;
            }
        }
    }
    delete_version_progress(store, version_id, progress)
}

/// 删除一个没有被其他版本依赖的版本，并按文件粒度上报进度（phase = "deleting"）。
/// 先落盘新的 meta 再删文件：即使删文件中途失败，也只是留下孤儿文件，
/// 不会出现「meta 里还有版本、manifest 已经被删掉」的不可还原状态。
pub fn delete_version_progress(
    store: &Path,
    version_id: &str,
    progress: Progress,
) -> Result<String, String> {
    let mut meta = load_meta(store)?;
    let Some(index) = meta
        .versions
        .iter()
        .position(|record| record.id == version_id)
    else {
        return Err(format!("版本不存在：{version_id}"));
    };
    if let Some(dependent) = meta
        .versions
        .iter()
        .find(|record| record.parent_id.as_deref() == Some(version_id))
    {
        return Err(format!(
            "「{}」还被「{}」依赖，请先把后面的版本转换为独立版本。",
            version_id, dependent.label
        ));
    }

    let was_last = index + 1 == meta.versions.len();
    let record = meta.versions.remove(index);
    if meta.active_version_id.as_deref() == Some(record.id.as_str()) {
        meta.active_version_id = meta.versions.last().map(|record| record.id.clone());
    }
    if was_last {
        meta.last_seen_build_id = meta
            .versions
            .last()
            .and_then(|record| record.steam_build_id.clone());
    }
    save_meta(store, &meta)?;

    // 收集要删的文件（含大小），按文件粒度删除并上报进度。
    let mut targets: Vec<PathBuf> = vec![manifest_path(store, &record.id)];
    if record.is_full() {
        targets.push(full_snapshot_root(store, &record));
    }
    if let Some(parent_id) = &record.parent_id {
        targets.extend(delta_dirs(store, parent_id, &record.id));
    }
    targets.push(store.join(TMP_DIR).join(&record.id));

    let mut files: Vec<(PathBuf, u64)> = Vec::new();
    let mut total_bytes: u64 = 0;
    for target in &targets {
        collect_deletion_files(target, &mut files, &mut total_bytes);
    }
    let total = files.len() as u64;
    let mut processed: u64 = 0;
    let mut processed_bytes: u64 = 0;
    // 文件很多时逐个发事件太密：按 1% 或 64 个文件至少发一次。
    let report_every = (total / 100).clamp(1, 64);
    for (file, size) in &files {
        let _ = fs::remove_file(file);
        processed += 1;
        processed_bytes += size;
        if processed % report_every == 0 || processed == total {
            progress(ProgressEvent {
                phase: "deleting".to_string(),
                processed_files: processed,
                total_files: total,
                processed_bytes,
                total_bytes,
                current_path: Some(file.to_string_lossy().into_owned()),
            })?;
        }
    }
    // 剩下的空目录自顶向下删掉。
    for target in &targets {
        remove_if_exists(target);
    }

    Ok(format!("已删除版本「{}」。", record.label))
}

/// 递归收集目录下的普通文件与总大小；文件直接删除时按路径列出。
fn collect_deletion_files(path: &Path, files: &mut Vec<(PathBuf, u64)>, total_bytes: &mut u64) {
    if !path.exists() {
        return;
    }
    if path.is_file() {
        let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        files.push((path.to_path_buf(), size));
        *total_bytes += size;
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if child.is_dir() {
            collect_deletion_files(&child, files, total_bytes);
        } else if child.is_file() {
            let size = fs::metadata(&child).map(|meta| meta.len()).unwrap_or(0);
            files.push((child, size));
            *total_bytes += size;
        }
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(path).unwrap();
        file.write_all(content).unwrap();
    }

    fn read_file(root: &Path, relative: &str) -> Vec<u8> {
        fs::read(root.join(relative)).unwrap()
    }

    fn ignore_progress(_: ProgressEvent) -> Result<(), String> {
        Ok(())
    }

    fn pseudo_random(length: usize, seed: u64) -> Vec<u8> {
        let mut state = seed.max(1);
        let mut output = Vec::with_capacity(length);
        for _ in 0..length {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            output.push(state as u8);
        }
        output
    }

    #[test]
    fn root_hash_is_stable_and_case_insensitive() {
        let files = vec![
            ManifestFile {
                path: "Media/Core.pak".to_string(),
                size: 10,
                mtime: 1,
                sha256: "aa".to_string(),
            },
            ManifestFile {
                path: "ProjectZomboid64.exe".to_string(),
                size: 20,
                mtime: 2,
                sha256: "bb".to_string(),
            },
        ];
        let mut reordered = files.clone();
        reordered.reverse();
        reordered[0].path = reordered[0].path.to_lowercase();
        assert_eq!(compute_root_hash(&files), compute_root_hash(&reordered));

        let mut changed = files.clone();
        changed[0].size = 11;
        assert_ne!(compute_root_hash(&files), compute_root_hash(&changed));
    }

    #[test]
    fn delta_roundtrip_handles_insertions_with_cdc() {
        let directory = tempfile::tempdir().unwrap();
        let old_path = directory.path().join("old.bin");
        let new_path = directory.path().join("new.bin");
        let delta_path = directory.path().join("change.pzd");
        let output_path = directory.path().join("output.bin");

        let old = pseudo_random(1024 * 1024, 7);
        let mut new = Vec::with_capacity(old.len() + 160 * 1024);
        new.extend_from_slice(&pseudo_random(64 * 1024, 11));
        new.extend_from_slice(&old);
        new.extend_from_slice(&pseudo_random(96 * 1024, 13));
        // 中间再改 4KB，确保 COPY/ADD 混合。
        let middle = old.len() / 2;
        for (index, byte) in new[middle + 64 * 1024..middle + 68 * 1024]
            .iter_mut()
            .enumerate()
        {
            *byte = (index as u8).wrapping_mul(31);
        }

        fs::write(&old_path, &old).unwrap();
        fs::write(&new_path, &new).unwrap();
        let target_size = create_delta(Some(&old_path), &new_path, &delta_path).unwrap();
        assert_eq!(target_size, new.len() as u64);
        let delta_size = fs::metadata(&delta_path).unwrap().len();
        assert!(
            delta_size < new.len() as u64 / 2,
            "delta should only store changed chunks, got {delta_size} for {} bytes",
            new.len()
        );

        let target_hash = hex(&Sha256::digest(&new));
        apply_delta(Some(&old_path), &delta_path, &output_path, &target_hash).unwrap();
        assert_eq!(fs::read(&output_path).unwrap(), new);
    }

    #[test]
    fn parallel_scan_matches_sequential_and_forced_workers() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        fs::create_dir_all(&game).unwrap();
        for index in 0..32u64 {
            write_file(
                &game,
                &format!("media/{index:02}.bin"),
                &pseudo_random(2048 + index as usize * 17, index + 1),
            );
        }

        let sequential = scan_directory(&game, false, &mut ignore_progress).unwrap();
        let parallel = scan_directory(&game, true, &mut ignore_progress).unwrap();
        assert_eq!(sequential, parallel);
        assert_eq!(compute_root_hash(&sequential), compute_root_hash(&parallel));

        let entries = collect_walk_entries(&game).unwrap();
        // 强制 4 个 worker：结果必须和串行完全一致。
        let cached = vec![None; entries.len()];
        let forced = scan_entries(&game, &entries, &cached, 4, &mut ignore_progress).unwrap();
        assert_eq!(sequential, forced);
    }

    #[test]
    fn cached_scan_reuses_hashes_without_reading_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("root");
        fs::create_dir_all(&root).unwrap();
        write_file(&root, "a.bin", &pseudo_random(4096, 1));
        write_file(&root, "b.bin", &pseudo_random(4096, 2));

        let entries = collect_walk_entries(&root).unwrap();
        assert_eq!(entries.len(), 2);
        let uncached = vec![None; entries.len()];
        let first = scan_entries(&root, &entries, &uncached, 2, &mut ignore_progress).unwrap();

        // 全部命中缓存：一个文件都不用打开，进度回调也不会触发。
        let all_cached: Vec<Option<String>> = first
            .iter()
            .map(|file| Some(file.sha256.clone()))
            .collect();
        let mut progress_calls = 0usize;
        let mut counting = |_: ProgressEvent| {
            progress_calls += 1;
            Ok(())
        };
        let reused = scan_entries(&root, &entries, &all_cached, 2, &mut counting).unwrap();
        assert_eq!(first, reused);
        assert_eq!(progress_calls, 0, "no file should be hashed when every entry is cached");

        // 只缓存第二个文件：第一个必须重算，第二个直接沿用缓存值（不读文件）。
        let partial: Vec<Option<String>> = entries
            .iter()
            .enumerate()
            .map(|(index, _)| (index == 1).then(|| "stale-hash".to_string()))
            .collect();
        let mixed = scan_entries(&root, &entries, &partial, 2, &mut ignore_progress).unwrap();
        assert_eq!(mixed[0].sha256, first[0].sha256);
        assert_eq!(mixed[1].sha256, "stale-hash");
    }

    #[test]
    fn streaming_copy_verifies_size_and_honours_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.bin");
        let destination = directory.path().join("copy.bin");
        let data = pseudo_random(300 * 1024, 7);
        fs::write(&source, &data).unwrap();

        let mut buffer = vec![0u8; 64 * 1024];
        let active = AtomicBool::new(false);
        copy_file_streaming(
            &mut buffer,
            &source,
            &destination,
            data.len() as u64,
            &active,
        )
        .unwrap();
        assert_eq!(fs::read(&destination).unwrap(), data);

        // 期望大小对不上时不能当成复制成功。
        let mismatch = copy_file_streaming(
            &mut buffer,
            &source,
            &destination,
            data.len() as u64 + 1,
            &active,
        )
        .unwrap_err();
        assert!(mismatch.contains("大小不一致"));

        // 已经取消时直接返回，不再读文件。
        let cancelled = AtomicBool::new(true);
        let error =
            copy_file_streaming(&mut buffer, &source, &destination, data.len() as u64, &cancelled)
                .unwrap_err();
        assert_eq!(error, "操作已取消");
    }

    #[test]
    fn parallel_scan_propagates_cancellation() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        fs::create_dir_all(&game).unwrap();
        for index in 0..24u64 {
            write_file(&game, &format!("{index:02}.bin"), &pseudo_random(4096, index + 1));
        }

        let mut calls = 0;
        let result = scan_directory(&game, true, &mut |_: ProgressEvent| {
            calls += 1;
            if calls >= 3 {
                Err("stop".to_string())
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert!(calls >= 3);
    }

    fn snapshot(game: &Path, store: &Path) -> SnapshotOutcome {
        create_full_snapshot(
            game,
            store,
            Some("100"),
            "stable",
            None,
            true,
            &mut ignore_progress,
        )
        .unwrap()
    }

    fn capture(game: &Path, store: &Path, build: &str) -> CaptureOutcome {
        capture_version(
            game,
            store,
            Some(build),
            "stable",
            None,
            true,
            &mut ignore_progress,
        )
        .unwrap()
    }

    #[test]
    fn full_capture_restore_roundtrip_with_checkpoint() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "ProjectZomboid64.exe", b"exe-v1");
        write_file(&game, "media/core.pak", &pseudo_random(300 * 1024, 21));
        write_file(&game, "old.txt", b"will be removed");

        let first = snapshot(&game, &store);
        assert!(first.created);
        assert_eq!(first.version.as_ref().unwrap().base_type, "full");

        // 改一个文件、加一个文件、删一个文件。
        write_file(&game, "media/core.pak", &{
            let mut data = pseudo_random(300 * 1024, 21);
            data[1024] ^= 0xFF;
            data
        });
        write_file(&game, "new.txt", b"new file");
        fs::remove_file(game.join("old.txt")).unwrap();

        let second = capture(&game, &store, "200");
        assert!(second.created);
        let second_record = second.version.unwrap();
        let change_log = second_record.change_log.as_ref().unwrap();
        assert_eq!(change_log.added.len(), 1);
        assert_eq!(change_log.removed, vec!["old.txt".to_string()]);
        assert_eq!(change_log.modified.len(), 1);
        assert_eq!(second_record.base_type, "delta");

        let changelog_path = store
            .join(DELTA_DIR)
            .join(format!(
                "{}__{}",
                first.version.as_ref().unwrap().id,
                second_record.id
            ))
            .join("changelog.json");
        let changelog_text = fs::read_to_string(&changelog_path).unwrap();
        let changelog_on_disk: VersionChangeLog = serde_json::from_str(&changelog_text).unwrap();
        assert_eq!(changelog_on_disk.added.len(), 1);
        assert_eq!(changelog_on_disk.modified.len(), 1);

        let status = get_status(Some(&game), &store, Some("200"), "stable", false).unwrap();
        assert_eq!(status.versions.len(), 2);
        assert_eq!(status.current_match.kind, "metadata");
        assert_eq!(
            status.current_match.version_id.as_deref(),
            Some(second_record.id.as_str())
        );
        assert!(!status.steam_update_pending);

        // 还原到 v1：当前目录是已知的 v2，允许直接还原。
        let restore_first = restore_version(
            &game,
            &store,
            &first.version.as_ref().unwrap().id,
            false,
            true,
            &mut ignore_progress,
        )
        .unwrap();
        assert_eq!(restore_first.removed, 1);
        assert!(restore_first.replaced >= 1);
        assert_eq!(read_file(&game, "ProjectZomboid64.exe"), b"exe-v1");
        assert_eq!(
            read_file(&game, "media/core.pak"),
            pseudo_random(300 * 1024, 21)
        );
        assert!(game.join("old.txt").is_file());
        assert!(!game.join("new.txt").exists());

        // 再还原到 v2：新增文件要能用“空基准增量”恢复。
        restore_version(&game, &store, &second_record.id, false, true, &mut ignore_progress).unwrap();
        assert!(!game.join("old.txt").exists());
        assert_eq!(read_file(&game, "new.txt"), b"new file");

        let verified = verify_current_version(&game, &store, true, &mut ignore_progress).unwrap();
        assert_eq!(
            verified.matched_version_id.as_deref(),
            Some(second_record.id.as_str())
        );

        // 检查点：完整快照后继续捕获增量，再从检查点 + 增量重建。
        let checkpoint = snapshot(&game, &store);
        assert!(checkpoint.created);
        assert_eq!(checkpoint.version.as_ref().unwrap().base_type, "full");
        write_file(&game, "ProjectZomboid64.exe", b"exe-v3");
        let third = capture(&game, &store, "300");
        assert!(third.created);
        let third_record = third.version.unwrap();
        restore_version(&game, &store, &third_record.id, false, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "ProjectZomboid64.exe"), b"exe-v3");

        // 只能删最新版本。
        assert!(delete_version(&store, &first.version.as_ref().unwrap().id).is_err());
        delete_version(&store, &third_record.id).unwrap();
        let status = get_status(Some(&game), &store, Some("200"), "stable", false).unwrap();
        assert_eq!(status.versions.len(), 3);
    }

    #[test]
    fn rebase_makes_delta_version_independent_and_restorable() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        write_file(&game, "b.txt", b"bee");
        let first = snapshot(&game, &store).version.unwrap();

        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();

        write_file(&game, "a.txt", b"three");
        write_file(&game, "c.txt", b"new");
        let third = capture(&game, &store, "300").version.unwrap();
        assert_eq!(third.parent_id.as_deref(), Some(second.id.as_str()));

        let message = rebase_version_progress(&store, &third.id, &mut ignore_progress).unwrap();
        assert!(message.contains("独立版本"));
        // 旧链路（second → third）已经被重写掉。
        assert!(!delta_dirs(&store, &second.id, &third.id)
            .iter()
            .any(|path| path.exists()));

        let versions = read_versions(&store).unwrap();
        let rebased = versions.iter().find(|version| version.id == third.id).unwrap();
        assert_eq!(rebased.parent_id.as_deref(), Some(first.id.as_str()));
        let change_log = rebased.change_log.as_ref().unwrap();
        assert_eq!(change_log.from, first.id);
        assert_eq!(change_log.added.len(), 1);
        assert_eq!(change_log.modified.len(), 1);
        assert_eq!(change_log.removed.len(), 0);

        restore_version(&game, &store, &third.id, false, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "a.txt"), b"three");
        assert_eq!(read_file(&game, "c.txt"), b"new");

        // 中间版本已经没有依赖者，可以单独删掉；转换后的版本仍能还原。
        delete_version(&store, &second.id).unwrap();
        assert_eq!(read_versions(&store).unwrap().len(), 2);
        restore_version(&game, &store, &third.id, false, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "a.txt"), b"three");
    }

    #[test]
    fn rebase_is_noop_when_parent_is_already_full() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        let first = snapshot(&game, &store).version.unwrap();
        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();
        assert_eq!(second.parent_id.as_deref(), Some(first.id.as_str()));

        let message = rebase_version_progress(&store, &second.id, &mut ignore_progress).unwrap();
        assert!(message.contains("已经是独立版本"));
        let versions = read_versions(&store).unwrap();
        let unchanged = versions.iter().find(|version| version.id == second.id).unwrap();
        assert_eq!(unchanged.parent_id.as_deref(), Some(first.id.as_str()));
    }

    #[test]
    fn deleting_middle_version_rebases_its_successor() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        let first = snapshot(&game, &store).version.unwrap();
        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();
        write_file(&game, "a.txt", b"three");
        let third = capture(&game, &store, "300").version.unwrap();

        delete_version_with_rebase_progress(&store, &second.id, &mut ignore_progress).unwrap();

        let versions = read_versions(&store).unwrap();
        assert!(versions.iter().all(|version| version.id != second.id));
        let successor = versions.iter().find(|version| version.id == third.id).unwrap();
        assert_eq!(successor.parent_id.as_deref(), Some(first.id.as_str()));

        restore_version(&game, &store, &third.id, false, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "a.txt"), b"three");
        restore_version(&game, &store, &first.id, false, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "a.txt"), b"one");
    }

    #[test]
    fn refuses_deleting_root_base_with_dependents() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        let first = snapshot(&game, &store).version.unwrap();
        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();

        let error =
            delete_version_with_rebase_progress(&store, &first.id, &mut ignore_progress).unwrap_err();
        assert!(error.contains("基础版本"));
        // 严格版本也不允许绕过依赖检查。
        assert!(delete_version(&store, &first.id).is_err());
        // 最新版本没有依赖者，可以直接删。
        delete_version(&store, &second.id).unwrap();
        assert_eq!(read_versions(&store).unwrap().len(), 1);
    }

    #[test]
    fn delta_files_live_under_the_deltas_directory() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        let first = snapshot(&game, &store).version.unwrap();
        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();

        let delta_dir = store.join(DELTA_DIR).join(format!("{}__{}", first.id, second.id));
        let has_pzd = fs::read_dir(&delta_dir)
            .unwrap()
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "pzd"));
        assert!(has_pzd);
        // 旧布局（存储根目录）不应再生成增量包。
        assert!(!store.join(format!("{}__{}", first.id, second.id)).exists());

        delete_version(&store, &second.id).unwrap();
        assert!(!delta_dir.exists());
    }

    #[test]
    fn delete_through_promotes_successor_to_a_new_base() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();

        write_file(&game, "a.txt", b"one");
        let first = snapshot(&game, &store).version.unwrap();
        write_file(&game, "a.txt", b"two");
        let second = capture(&game, &store, "200").version.unwrap();
        write_file(&game, "a.txt", b"three");
        write_file(&game, "b.txt", b"bee");
        let third = capture(&game, &store, "300").version.unwrap();

        // 删除前两个版本：第三个必须重建成不依赖任何人的新完整基准。
        delete_versions_through_progress(&store, &second.id, &mut ignore_progress).unwrap();

        let versions = read_versions(&store).unwrap();
        assert_eq!(versions.len(), 1);
        let successor = &versions[0];
        assert_eq!(successor.id, third.id);
        assert_eq!(successor.base_type, "full");
        assert_eq!(successor.parent_id, None);
        assert!(successor.change_log.is_none());
        // 物化出的完整快照落在 base/（parent 为 None 的约定位置），旧增量包已清。
        assert!(store.join(BASE_DIR).join(&third.id).is_dir());
        for parent in [&first.id, &second.id] {
            assert!(!store
                .join(DELTA_DIR)
                .join(format!("{parent}__{}", third.id))
                .exists());
        }

        // 新基准能独立还原，内容就是删除时的状态。
        write_file(&game, "a.txt", b"corrupted");
        fs::remove_file(game.join("b.txt")).unwrap();
        restore_version(&game, &store, &third.id, true, true, &mut ignore_progress).unwrap();
        assert_eq!(read_file(&game, "a.txt"), b"three");
        assert_eq!(read_file(&game, "b.txt"), b"bee");

        // 只剩一个版本时「删除及更早」等于清空整个版本库。
        delete_versions_through_progress(&store, &third.id, &mut ignore_progress).unwrap();
        assert!(read_versions(&store).unwrap().is_empty());
    }

    #[test]
    fn restore_refuses_unknown_directory_unless_forced() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();
        write_file(&game, "a.txt", b"a");
        let first = snapshot(&game, &store);
        write_file(&game, "unknown.txt", b"not captured");

        let error = restore_version(
            &game,
            &store,
            first.version.as_ref().unwrap().id.as_str(),
            false,
            false,
            &mut ignore_progress,
        )
        .unwrap_err();
        assert!(error.contains("都对不上"));

        restore_version(
            &game,
            &store,
            first.version.as_ref().unwrap().id.as_str(),
            true,
            true,
            &mut ignore_progress,
        )
        .unwrap();
        assert!(!game.join("unknown.txt").exists());
    }

    #[test]
    fn metadata_hash_tracks_size_changes() {
        let files = vec![
            ManifestFile {
                path: "a.txt".to_string(),
                size: 1,
                mtime: 1,
                sha256: "aa".to_string(),
            },
            ManifestFile {
                path: "B.txt".to_string(),
                size: 2,
                mtime: 1,
                sha256: "bb".to_string(),
            },
        ];
        let first = compute_metadata_hash(&files);
        let mut changed = files.clone();
        changed[1].size = 3;
        assert_ne!(first, compute_metadata_hash(&changed));
        let mut renamed = files.clone();
        renamed[0].path = "A.TXT".to_string();
        assert_eq!(first, compute_metadata_hash(&renamed));
    }

    #[test]
    fn attaches_mod_version_references_to_a_game_version() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();
        write_file(&game, "a.txt", b"a");

        let outcome = snapshot(&game, &store);
        let version_id = outcome.version.unwrap().id;
        let reference = ModVersionRef {
            key: "local-alpha".to_string(),
            mod_id: "Alpha".to_string(),
            name: "Alpha".to_string(),
            source: "local".to_string(),
            workshop_id: None,
            version_id: "v1".to_string(),
            root_path: "C:\\mods\\Alpha".to_string(),
        };
        attach_mod_versions(&store, &version_id, vec![reference.clone()]).unwrap();

        let versions = read_versions(&store).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].mod_versions, vec![reference]);
        // 旧版本记录没有该字段时按空列表反序列化（向后兼容）。
        let empty = serde_json::from_str::<VersionRecord>(
            r#"{"id":"v0","parentId":null,"steamBuildId":null,"branch":"stable","baseType":"full","createdAt":1,"rootHash":"sha256:0","fileCount":0,"totalBytes":0,"label":"旧版本"}"#,
        )
        .unwrap();
        assert!(empty.mod_versions.is_empty());
    }

    #[test]
    fn delete_version_progress_reports_and_removes_files() {
        let directory = tempfile::tempdir().unwrap();
        let game = directory.path().join("game");
        let store = directory.path().join("store");
        fs::create_dir_all(&game).unwrap();
        // 多写几个文件让删除有真实的文件数与字节量
        write_file(&game, "ProjectZomboid64.exe", b"exe-v1");
        for index in 0..5 {
            write_file(&game, &format!("data{index}.bin"), &[index as u8; 512]);
        }
        let outcome = snapshot(&game, &store);
        let version_id = outcome.version.as_ref().unwrap().id.clone();

        let mut events: Vec<ProgressEvent> = Vec::new();
        let message = delete_version_progress(&store, &version_id, &mut |event: ProgressEvent| {
            events.push(event);
            Ok(())
        })
        .unwrap();

        assert!(message.contains("已删除版本"));
        assert!(events.len() >= 2, "进度事件太少：{:?}", events.len());
        let last = events.last().unwrap();
        assert_eq!(last.phase, "deleting");
        assert_eq!(last.processed_files, last.total_files);
        assert_eq!(last.processed_bytes, last.total_bytes);
        assert!(last.total_files >= 6, "manifest + 6 个数据文件");
        // store 里不再有任何版本文件（meta 索引除外）
        let mut leftovers: Vec<(PathBuf, u64)> = Vec::new();
        let mut leftover_bytes: u64 = 0;
        collect_deletion_files(&store, &mut leftovers, &mut leftover_bytes);
        let leftovers: Vec<_> = leftovers
            .into_iter()
            .filter(|(path, _)| !path.to_string_lossy().contains("meta"))
            .collect();
        assert!(leftovers.is_empty(), "还有残留文件：{:?}", leftovers);

        // 删除后再删同一个版本：报「版本不存在」
        let error = delete_version_progress(&store, &version_id, &mut ignore_progress).unwrap_err();
        assert!(error.contains("版本不存在"));
    }
}
