use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::Emitter;
use zip::ZipArchive;

mod downloader;
mod feedback;
mod launcher_log;
mod memclean;
mod mod_versioning;
mod news;
mod news_translation;
mod overlay;
mod overlay_session;
mod resource;
mod steam_relay;
mod update;
mod versioning;
mod workshop;
mod backgrounds;
pub(crate) use backgrounds::*;
mod fast_launch;
pub(crate) use fast_launch::*;
mod fs_utils;
pub(crate) use fs_utils::*;
mod jar_mods;
pub(crate) use jar_mods::*;
mod launch_options;
pub(crate) use launch_options::*;
mod lua_mods;
pub(crate) use lua_mods::*;
mod settings;
pub(crate) use settings::*;
mod steam;
pub(crate) use steam::*;
mod support;
pub(crate) use support::*;
mod viewpoint_media;
pub(crate) use viewpoint_media::*;

// 领域模块通过 `pub(crate) use` 重新导出到 crate 根：原有命令注册、测试里的
// `super::{...}` 和未限定调用都不需要改路径。

const PROJECT_ZOMBOID_APP_ID: &str = "108600";
const VIEWPOINT_MOD_ID: &str = "3809306528";
const ZOMBIE_BUDDY_MOD_ID: &str = "3619862853";
const PZ_OPTIMIZATION_MOD_ID: &str = "3805285544";
const ZOMBIE_BUDDY_WORKSHOP_ID: u64 = 3_619_862_853;
const ZOMBIE_BUDDY_AGENT: &str = "-agentlib:zbNative";
const ZOMBIE_BUDDY_POLICY_AGENT: &str = "-agentlib:zbNative=policy=allow-all";

/// 「模组加载器方案」的取值（设置 › 模组加载器 › 模组加载器方案）。
/// 目前只有 ZombieBuddy 和"不使用"；将来加新加载器时：
/// 在这里加一项 + 在 `mod_loader_policy_agent` 里给出它的启动期 agent
/// + 在 `is_zombie_buddy_agent_token` 旁加一个同类的识别函数。
const MOD_LOADER_SCHEMES: [&str; 2] = ["zombieBuddy", "none"];
const DEFAULT_MOD_LOADER_SCHEME: &str = "zombieBuddy";

/// 方案对应的启动期 JVM agent；`None` = 不使用加载器（不往启动配置里写 agent）。
fn mod_loader_policy_agent(scheme: &str) -> Option<&'static str> {
    match scheme {
        "zombieBuddy" => Some(ZOMBIE_BUDDY_POLICY_AGENT),
        _ => None,
    }
}

/// 大小写不敏感地归一化到上面的取值；不在列表里返回 None。
fn normalize_mod_loader_scheme(scheme: &str) -> Option<String> {
    let trimmed = scheme.trim();
    MOD_LOADER_SCHEMES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}


// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/


#[cfg(windows)]
fn steam_is_running() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`：避免 GUI 程序里弹出一闪而过的控制台窗口。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("检查 Steam 进程失败: {error}"))
        .and_then(|output| {
            if !output.status.success() {
                return Err("检查 Steam 进程失败".to_string());
            }
            Ok(String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("steam.exe"))
        })
}

#[cfg(not(windows))]
fn steam_is_running() -> Result<bool, String> {
    Err("ZombieBuddy 安装只支持 Windows".to_string())
}


#[cfg(windows)]
fn steam_roots() -> Vec<PathBuf> {
    use std::env;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut roots = Vec::new();
    let mut add_root = |root: PathBuf| {
        if !roots.contains(&root) {
            roots.push(root);
        }
    };

    let registry_locations = [
        (HKEY_CURRENT_USER, "Software\\Valve\\Steam"),
        (HKEY_CURRENT_USER, "Software\\Wow6432Node\\Valve\\Steam"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Wow6432Node\\Valve\\Steam"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam"),
    ];

    for (hive, key_path) in registry_locations {
        let hive = RegKey::predef(hive);
        if let Ok(key) = hive.open_subkey(key_path) {
            let value = key
                .get_value::<String, _>("SteamPath")
                .or_else(|_| key.get_value::<String, _>("InstallPath"));
            if let Ok(path) = value {
                add_root(PathBuf::from(path));
            }
        }
    }

    for variable in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(program_files) = env::var_os(variable) {
            add_root(PathBuf::from(program_files).join("Steam"));
        }
    }

    roots
}

#[cfg(not(windows))]
fn steam_roots() -> Vec<PathBuf> {
    Vec::new()
}


#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedFile {
    name: String,
    size_bytes: u64,
    modified_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedDirectory {
    directory: String,
    files: Vec<ManagedFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LuaModEntry {
    id: String,
    name: String,
    path: String,
    source: String,
    workshop_id: Option<String>,
    enabled: bool,
    load_order: Option<usize>,
    requires: Vec<String>,
    load_after: Vec<String>,
    load_before: Vec<String>,
    load_first: bool,
    load_last: bool,
    has_common: bool,
    has_b42: bool,
    is_loader: bool,
    poster_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    background_path: Option<String>,
    shader_download_url: Option<String>,
    shader_download_directory: Option<String>,
    shader_download_threads: Option<usize>,
    memory_limit_mb: Option<u64>,
    translation_language: Option<String>,
    font_contrast: Option<u8>,
    /// 「模糊度」百分比：0 = 关掉模糊，100 = 设计基准，上限 200。没写过按 100 处理。
    blur_strength: Option<u8>,
    /// 「界面字体」：完整的 CSS font-family 列表；空 / 缺失 = App.css 的默认字体栈。
    font_family: Option<String>,
    /// 「界面字号」缩放百分比：80–130，100 = 设计基准。没写过按 100 处理。
    font_scale: Option<u8>,
    /// 记住的窗口尺寸（逻辑像素）。缺失 = 用 tauri.conf.json 的默认 840×560。
    window_width: Option<u32>,
    window_height: Option<u32>,
    /// 覆盖层背景色（`#RRGGBB`）。缺失 = 默认纯红；启动器写状态文件时下发给 DLL。
    overlay_background_color: Option<String>,
    /// 「模组加载器方案」（`zombieBuddy` / `none`），决定启动前要不要挂加载器 agent。
    mod_loader_scheme: Option<String>,
    /// 「极速启动（实验性）」：把启动器预扫描出的模组/补丁清单喂给 ZombieBuddy，
    /// 短路它内部的 ClassGraph 扫描。只在方案 = zombieBuddy 时生效；任何失败都回退原生扫描。
    #[serde(default)]
    fast_launch: bool,
    #[serde(default)]
    developer_mode: bool,
    #[serde(default)]
    steam_account_hidden: bool,
    /// 「静默内存清理」（设置 › 启动）：系统内存占用达到阈值时在后台自动清理一次。
    #[serde(default)]
    silent_memory_cleanup: bool,
    /// 静默清理的触发阈值（百分比）。0 视为"没设置过"，按默认值处理。
    #[serde(default)]
    silent_memory_cleanup_threshold: u8,
    /// 「启动时清理内存」：点「启动游戏」时先清理一次，再把游戏拉起来。
    #[serde(default)]
    clean_memory_on_launch: bool,
    /// 内置四季壁纸的手动 / 固定设置，前端 builtinWallpaper.ts 的 WallpaperEffectSettings 原样存取。
    #[serde(default)]
    wallpaper_effect: Option<serde_json::Value>,
    /// 「Steam 网络加速」模式：auto（直连失败自动切中转）/ relay（中转优先）/ direct（仅直连）。
    steam_relay_mode: Option<String>,
    /// 「Steam 网络加速」的中转地址；空 = 内置默认（steam_relay::DEFAULT_RELAY_BASE）。
    steam_relay_base: Option<String>,
    /// 「Steam 网络加速 › 本地代理」：如 `http://127.0.0.1:7897` 或
    /// `socks5://127.0.0.1:1080`；空 / 无效 = 不用显式代理（走系统代理）。
    steam_proxy: Option<String>,
    /// 「设置 › 版本」的版本存储目录；空 / 缺失 = `.pzmanager/versions`。
    version_store_path: Option<String>,
    /// 「设置 › 版本」：哈希/复制是否使用多线程；缺失 = 默认开启。
    #[serde(default = "default_version_parallel_enabled")]
    version_parallel: bool,
    /// 「设置 › 版本 › 游戏版本」：创建/捕获大版本时是否同时记录已启用模组的版本。
    #[serde(default = "default_version_parallel_enabled")]
    version_include_mods: bool,
    /// 「设置 › 版本」：备份位置在 C 盘时，每次操作前的换盘提醒是否已被「不再弹出」关闭。
    #[serde(default)]
    version_system_drive_notice_dismissed: bool,
}

fn default_version_parallel_enabled() -> bool {
    true
}


fn shader_download_directory() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?.join("viewpoint-shaderpacks"))
}


impl Default for JarModStateFile {
    fn default() -> Self {
        Self {
            format_version: 1,
            mods: Vec::new(),
        }
    }
}


/// 统计一个 jar 里所有 class 声明的注入方法(advice)数量。
///
/// 启动界面进度条用它作为"本次要注入的总数"分母；与运行期
/// `[ZB] patching … with N advice(s)` 的 ΣN 高度吻合（实测 614 vs 611）。
pub(crate) fn count_jar_advice_methods(jar_path: &Path) -> usize {
    let Ok(file) = fs::File::open(jar_path) else {
        return 0;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return 0;
    };
    let mut total = 0;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        if !entry.name().ends_with(".class") {
            continue;
        }
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        if let Some(annotations) = class_annotations(&bytes) {
            total += annotations.patch_methods.len();
        }
    }
    total
}


fn find_lua_mod_id(info_id: Option<&str>, fallback_name: &str) -> Option<String> {
    pick_lua_mod_id(&lua_mod_entries().ok()?, info_id, fallback_name)
}

/// 从 `mod.info` 路径推导创意工坊 id（形如 `.../<appid>/<workshop_id>/.../mod.info`）。
fn workshop_id_from_info_path(info_path: &Path) -> Option<u64> {
    info_path.ancestors().find_map(|ancestor| {
        let parent = ancestor.parent()?;
        (parent.file_name()?.to_str()? == PROJECT_ZOMBOID_APP_ID)
            .then(|| ancestor.file_name()?.to_str()?.parse().ok())
            .flatten()
    })
}


fn sort_scanned_jar_mods(scanned: &mut [ScannedJarMod]) {
    scanned.sort_by(|left, right| {
        left.id
            .to_lowercase()
            .cmp(&right.id.to_lowercase())
            .then_with(|| left.package_name.cmp(&right.package_name))
    });
}

fn scan_jar_mods_at_with_callbacks<Start, Item>(
    roots: &[PathBuf],
    mut on_start: Start,
    mut on_item: Item,
) -> Vec<ScannedJarMod>
where
    Start: FnMut(usize),
    Item: FnMut(usize, usize, Option<&ScannedJarMod>),
{
    let candidates = jar_scan_candidates(roots);
    let total = candidates.len();
    on_start(total);
    let mut scanned = Vec::new();

    for (index, (info_path, info, workshop_id)) in candidates.into_iter().enumerate() {
        if let Some(mod_info) = scan_java_mod_jar(&info_path, &info, workshop_id) {
            scanned.push(mod_info);
            on_item(index + 1, total, scanned.last());
        } else {
            on_item(index + 1, total, None);
        }
    }

    sort_scanned_jar_mods(&mut scanned);
    scanned
}

fn scan_jar_mods_at_with_callback<Item>(roots: &[PathBuf], mut on_item: Item) -> Vec<ScannedJarMod>
where
    Item: FnMut(usize, usize, Option<&ScannedJarMod>),
{
    scan_jar_mods_at_with_callbacks(
        roots,
        |_| {},
        |scanned, total, item| {
            on_item(scanned, total, item);
        },
    )
}

fn scan_jar_mods_at(roots: &[PathBuf]) -> Vec<ScannedJarMod> {
    scan_jar_mods_at_with_callback(roots, |_, _, _| {})
}


/// 最近一次「窗口出现 → 移除覆盖层」的启动时长（含上一次，供前端对比）。
#[tauri::command]
fn startup_timing() -> overlay_session::StartupTiming {
    overlay_session::read_startup_timing()
}


#[allow(dead_code)]
fn scan_zombie_buddy_jar_mods() -> Result<Vec<ScannedJarMod>, String> {
    #[cfg(not(windows))]
    {
        return Err("JAR 模组扫描只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = jar_scan_inputs();
        let state = read_jar_mod_state(&jar_mod_state_path()?)?;
        apply_jar_mod_state_to_roots(&roots, &state)?;
        Ok(scan_jar_mods_at(&roots))
    }
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
            )
        })
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn list_managed_files(
    directory: &Path,
    predicate: fn(&Path) -> bool,
) -> Result<Vec<ManagedFile>, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("创建目录 {} 失败: {error}", directory.display()))?;
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("读取目录 {} 失败: {error}", directory.display()))?;
    let mut files = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !is_regular_file(&path) || !predicate(&path) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs());
            Some(ManagedFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                size_bytes: metadata.len(),
                modified_at,
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(files)
}


fn managed_file_path(
    directory: &Path,
    name: &str,
    predicate: fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if name.is_empty()
        || path.file_name().and_then(|file_name| file_name.to_str()) != Some(name)
        || !predicate(path)
    {
        return Err("无效文件名或文件格式".to_string());
    }
    Ok(directory.join(path))
}


// ---------------------------------------------------------------------------
// PZ 版本隔离（设置 › 版本）
// ---------------------------------------------------------------------------

const VERSION_STORE_DIR: &str = "versions";

static VERSION_OPERATION_ACTIVE: AtomicBool = AtomicBool::new(false);
static VERSION_OPERATION_CANCELLED: AtomicBool = AtomicBool::new(false);
static NEXT_VERSION_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionProgressUpdate {
    operation_id: u64,
    operation: String,
    status: String,
    phase: String,
    processed_files: u64,
    total_files: u64,
    processed_bytes: u64,
    total_bytes: u64,
    current_path: Option<String>,
    message: Option<String>,
    error: Option<String>,
    /// 只有模组版本事件会带；游戏大版本事件里为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    root_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_path: Option<String>,
    /// 当前是第几个步骤（1 起）与总步骤数；前端在百分比后显示 `2/3`。
    step: u32,
    steps: u32,
}

struct VersionContext {
    game_path: PathBuf,
    store_path: PathBuf,
    steam_build_id: Option<String>,
    branch: String,
    parallel: bool,
}

fn configured_version_options() -> Result<(PathBuf, bool), String> {
    let settings = load_app_settings_at(&app_settings_path()?)?;
    let store_path = if let Some(path) = settings
        .version_store_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        PathBuf::from(path)
    } else {
        pzmanager_directory()?.join(VERSION_STORE_DIR)
    };
    Ok((store_path, settings.version_parallel))
}

fn configured_version_store_path() -> Result<PathBuf, String> {
    configured_version_options().map(|(path, _)| path)
}

fn detected_game_path() -> Option<PathBuf> {
    find_project_zomboid_paths()
        .ok()
        .and_then(|paths| paths.install_path)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

fn version_path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn steam_manifest_path_for_game(game_path: &Path) -> Option<PathBuf> {
    let mut current = Some(game_path);
    while let Some(directory) = current {
        if directory
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
        {
            return Some(
                directory.join(format!("appmanifest_{PROJECT_ZOMBOID_APP_ID}.acf")),
            );
        }
        current = directory.parent();
    }

    // 游戏目录不在常规的 `<库>/steamapps/common/<installdir>` 下时，退回到
    // 遍历 Steam 库并比对 appmanifest 里解析出的安装路径。
    let game_key = version_path_key(game_path);
    for root in steam_roots() {
        for library in library_paths_from_config(&root) {
            if let Some(candidate) = game_path_from_library(&library) {
                if version_path_key(Path::new(&candidate)) == game_key {
                    return Some(
                        library
                            .join("steamapps")
                            .join(format!("appmanifest_{PROJECT_ZOMBOID_APP_ID}.acf")),
                    );
                }
            }
        }
    }
    None
}

fn steam_manifest_value_for_game(game_path: &Path, key: &str) -> Option<String> {
    let path = steam_manifest_path_for_game(game_path)?;
    let content = fs::read_to_string(path).ok()?;
    vdf_value(&content, key).filter(|value| !value.trim().is_empty())
}

fn version_context() -> Result<VersionContext, String> {
    let game_path = detected_game_path().ok_or_else(|| {
        "未检测到 Project Zomboid 安装目录，请先确认 Steam 已安装游戏。".to_string()
    })?;
    let (store_path, parallel) = configured_version_options()?;
    versioning::ensure_store_outside_root(&store_path, &game_path)?;
    let steam_build_id = steam_manifest_value_for_game(&game_path, "buildid");
    let branch = steam_manifest_value_for_game(&game_path, "betakey")
        .unwrap_or_else(|| "stable".to_string());
    Ok(VersionContext {
        game_path,
        store_path,
        steam_build_id,
        branch,
        parallel,
    })
}

#[cfg(windows)]
fn project_zomboid_running() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq ProjectZomboid64.exe", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("projectzomboid64.exe")
        })
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn project_zomboid_running() -> bool {
    false
}

fn ensure_game_closed() -> Result<(), String> {
    if project_zomboid_running() {
        return Err("请先关闭 Project Zomboid，再执行版本操作。".to_string());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 模组版本隔离（设置 › 版本 › 模组版本 + 模组管理右键「版本记录」）
// ---------------------------------------------------------------------------

/// `mod.info` 可能在 `<mod>/common/` 或 `<mod>/42.20/`；版本隔离要的是整个
/// 模组目录，所以向上找到父目录名为 `mods` 的那一层。
fn mod_root_for_info_path(info_path: &Path) -> PathBuf {
    let mut current = info_path.parent().unwrap_or(info_path).to_path_buf();
    loop {
        let Some(parent) = current.parent() else {
            break;
        };
        let is_mods = parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("mods"));
        if is_mods {
            return current;
        }
        if parent.parent().is_none() {
            break;
        }
        current = parent.to_path_buf();
    }
    info_path.parent().unwrap_or(info_path).to_path_buf()
}

fn push_mod_root(
    roots: &mut Vec<mod_versioning::ModRoot>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: &str,
    workshop_id: Option<String>,
) {
    if !seen.insert(version_path_key(&path)) {
        return;
    }
    let (mod_id, name, _, _) = lua_mod_metadata(&path);
    roots.push(mod_versioning::ModRoot {
        path,
        source: source.to_string(),
        workshop_id,
        mod_id,
        name,
    });
}

/// 所有物理模组根目录（本地 + 创意工坊）。刻意**不按模组 ID 合并**：
/// 同一个模组的本地副本和工坊副本各自有一条版本链。
fn collect_mod_roots() -> Vec<mod_versioning::ModRoot> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    if let Ok(zomboid) = user_zomboid_directory() {
        if let Ok(items) = fs::read_dir(zomboid.join("mods")) {
            for entry in items
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            {
                push_mod_root(&mut roots, &mut seen, entry.path(), "local", None);
            }
        }
    }
    for workshop_root in workshop_background_roots() {
        let Ok(items) = fs::read_dir(&workshop_root) else {
            continue;
        };
        for item in items
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let workshop_id = item.file_name().to_string_lossy().into_owned();
            if workshop_id.parse::<u64>().is_err() {
                continue;
            }
            for mod_root in discover_mod_roots(&item.path()) {
                push_mod_root(
                    &mut roots,
                    &mut seen,
                    mod_root,
                    "workshop",
                    Some(workshop_id.clone()),
                );
            }
        }
    }
    roots
}

/// 大版本里要跟着记录的模组：Lua 在 `default.txt` 里启用，或 JAR 已批准启用。
fn enabled_mod_roots() -> Vec<mod_versioning::ModRoot> {
    let mut lua_enabled: HashSet<String> = HashSet::new();
    if let Ok(zomboid) = user_zomboid_directory() {
        if let Ok(ids) = read_default_mod_ids(&zomboid.join("mods").join("default.txt")) {
            lua_enabled.extend(ids.into_iter().map(|id| id.to_lowercase()));
        }
    }
    let jar_state = jar_mod_state_path()
        .and_then(|path| read_jar_mod_state(&path))
        .unwrap_or_default();

    collect_mod_roots()
        .into_iter()
        .filter(|root| {
            if lua_enabled.contains(&root.mod_id.to_lowercase()) {
                return true;
            }
            let root_key = version_path_key(&root.path);
            jar_state.mods.iter().any(|entry| {
                !entry.disabled
                    && (version_path_key(Path::new(&entry.mod_dir)) == root_key
                        || (entry.mod_id.eq_ignore_ascii_case(&root.mod_id)
                            && entry.workshop_id.map(|id| id.to_string()) == root.workshop_id))
            })
        })
        .collect()
}

fn workshop_id_for_mod_root(root: &Path) -> Option<String> {
    workshop_id_from_info_path(&root.join("mod.info")).map(|id| id.to_string())
}

/// 前端只传一个目录：能匹配到已知模组就用那条记录，否则按目录现读 `mod.info`。
fn resolve_mod_root(root_path: &str) -> Result<mod_versioning::ModRoot, String> {
    let path = PathBuf::from(root_path.trim());
    if !path.is_dir() {
        return Err("模组目录不存在，可能已被删除或移动。".to_string());
    }
    let wanted = version_path_key(&path);
    if let Some(root) = collect_mod_roots()
        .into_iter()
        .find(|root| version_path_key(&root.path) == wanted)
    {
        return Ok(root);
    }
    let (mod_id, name, _, _) = lua_mod_metadata(&path);
    let workshop_id = workshop_id_for_mod_root(&path);
    Ok(mod_versioning::ModRoot {
        path,
        source: if workshop_id.is_some() {
            "workshop".to_string()
        } else {
            "local".to_string()
        },
        workshop_id,
        mod_id,
        name,
    })
}

fn configured_include_mods() -> Result<bool, String> {
    Ok(load_app_settings_at(&app_settings_path()?)?.version_include_mods)
}

fn mod_version_store(
    root: &mod_versioning::ModRoot,
    store_base: &Path,
) -> Result<PathBuf, String> {
    let store = mod_versioning::store_path(store_base, root);
    versioning::ensure_store_outside_root(&store, &root.path)?;
    Ok(store)
}

/// 还原模组文件后把 `jar-mod-state.json` 的启用标记对齐到磁盘实际状态，
/// 否则下次打开模组页时 `apply_jar_mod_file_state` 会把 `mod.info` 又改回去。
fn reconcile_jar_state_for_root(root: &Path) -> Result<(), String> {
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    if state.mods.is_empty() {
        return Ok(());
    }
    let root_key = version_path_key(root);
    let (mod_id, _, _, _) = lua_mod_metadata(root);
    let disabled_on_disk = !root.join("mod.info").is_file();
    let mut changed = false;
    for entry in state.mods.iter_mut() {
        let same_dir = version_path_key(Path::new(&entry.mod_dir)) == root_key;
        let same_mod = !mod_id.is_empty() && entry.mod_id.eq_ignore_ascii_case(&mod_id);
        if (same_dir || same_mod) && entry.disabled != disabled_on_disk {
            entry.disabled = disabled_on_disk;
            changed = true;
        }
    }
    if changed {
        write_jar_mod_state(&state_path, &state)?;
    }
    Ok(())
}

/// 模组步骤的进度单位：每个模组占整条进度条的 1/N，模组内部再按 0–1 插值。
/// 直接把内层的文件数/字节数透出去，会让百分比在「第几个模组」和「这个模组里的
/// 第几个文件」之间来回跳，看起来像每个文件的进度。
const MOD_PROGRESS_UNITS: f64 = 1000.0;

fn mod_progress_units(index: usize, fraction: f64, total: usize) -> (u64, u64) {
    let total_units = (total.max(1) as f64) * MOD_PROGRESS_UNITS;
    let done = ((index as f64 + fraction.clamp(0.0, 1.0)) * MOD_PROGRESS_UNITS).min(total_units);
    (done.round() as u64, total_units.round() as u64)
}

/// 开始处理第 index+1 个模组时的进度（占整条进度条的 index/total）。
fn mod_marker_event(index: usize, total: usize, name: &str) -> versioning::ProgressEvent {
    let (processed_bytes, total_bytes) = mod_progress_units(index, 0.0, total);
    versioning::ProgressEvent {
        phase: "mods".to_string(),
        processed_files: index as u64,
        total_files: total as u64,
        processed_bytes,
        total_bytes,
        current_path: Some(format!("{name}（{}/{}）", index + 1, total)),
    }
}

/// 把某个模组内部的扫描/复制进度折算成「这个模组自己占的那一小格」。
fn mod_inner_event(
    index: usize,
    total: usize,
    name: &str,
    event: versioning::ProgressEvent,
) -> versioning::ProgressEvent {
    let fraction = if event.total_bytes > 0 {
        event.processed_bytes as f64 / event.total_bytes as f64
    } else if event.total_files > 0 {
        event.processed_files as f64 / event.total_files as f64
    } else {
        0.0
    };
    let (processed_bytes, total_bytes) = mod_progress_units(index, fraction, total);
    versioning::ProgressEvent {
        phase: "mods".to_string(),
        processed_files: index as u64,
        total_files: total as u64,
        processed_bytes,
        total_bytes,
        current_path: Some(format!("{name}（{}/{}）", index + 1, total)),
    }
}

/// 给一组模组各记录一个版本（没有历史时建基准，否则记增量），返回引用和失败数。
/// 进度按模组等分；调用方负责把引用写进游戏版本或整合包。
fn capture_mod_versions(
    roots: &[mod_versioning::ModRoot],
    label: &str,
    parallel: bool,
    progress: &mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>,
) -> Result<(Vec<versioning::ModVersionRef>, usize), String> {
    let (store_base, _) = configured_version_options()?;
    let total = roots.len();
    let mut references = Vec::new();
    let mut failures = 0usize;

    for (index, root) in roots.iter().enumerate() {
        progress(mod_marker_event(index, total, &root.name))?;

        let Ok(store) = mod_version_store(root, &store_base) else {
            failures += 1;
            continue;
        };
        let existing = versioning::read_versions(&store).unwrap_or_default();
        // 两种 outcome 结构相同但类型不同，这里只关心「是否新建了版本 + 新版本 ID」。
        // 内层进度折算到当前模组占的那一小格（phase 统一写成 "mods"）。
        let outcome: Result<Option<String>, String> = {
            let mut inner = |event: versioning::ProgressEvent| {
                progress(mod_inner_event(index, total, &root.name, event))
            };
            if existing.is_empty() {
                versioning::create_full_snapshot(
                    &root.path,
                    &store,
                    None,
                    "mods",
                    Some(label),
                    parallel,
                    &mut inner,
                )
                .map(|outcome| outcome.version.map(|version| version.id))
            } else {
                versioning::capture_version(
                    &root.path,
                    &store,
                    None,
                    "mods",
                    Some(label),
                    parallel,
                    &mut inner,
                )
                .map(|outcome| outcome.version.map(|version| version.id))
            }
        };
        match outcome {
            Ok(created_version_id) => {
                // 内容没变化时 capture 不会再建版本，直接引用当前链头。
                let version_id = created_version_id.or_else(|| {
                    versioning::read_versions(&store)
                        .ok()
                        .and_then(|versions| versions.last().map(|record| record.id.clone()))
                });
                if let Some(version_id) = version_id {
                    references.push(versioning::ModVersionRef {
                        key: mod_versioning::store_key(root),
                        mod_id: root.mod_id.clone(),
                        name: root.name.clone(),
                        source: root.source.clone(),
                        workshop_id: root.workshop_id.clone(),
                        version_id,
                        root_path: root.path.to_string_lossy().into_owned(),
                    });
                }
            }
            Err(error) => {
                failures += 1;
                launcher_log::error(&format!("模组「{}」记录版本失败：{error}", root.name));
            }
        }
    }

    let (processed_bytes, total_bytes) = mod_progress_units(total, 0.0, total);
    let _ = progress(versioning::ProgressEvent {
        phase: "mods".to_string(),
        processed_files: total as u64,
        total_files: total as u64,
        processed_bytes,
        total_bytes,
        current_path: None,
    });

    Ok((references, failures))
}

/// 为当前已启用的每个模组记录一个版本（没有历史时建基准，否则记增量），
/// 然后把「模组键 → 版本 ID」回填到游戏大版本的记录里。
fn record_mod_versions_for_game_version(
    game_store: &Path,
    game_version_id: &str,
    version_label: &str,
    parallel: bool,
    progress: &mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>,
) -> Result<String, String> {
    let roots = enabled_mod_roots();
    if roots.is_empty() {
        versioning::attach_mod_versions(game_store, game_version_id, Vec::new())?;
        return Ok("没有启用中的模组".to_string());
    }

    let label = format!("随游戏版本 {version_label}");
    let (references, failures) = capture_mod_versions(&roots, &label, parallel, progress)?;
    versioning::attach_mod_versions(game_store, game_version_id, references.clone())?;

    Ok(if failures > 0 {
        format!("已同时保存 {} 个模组版本（{failures} 个失败）", references.len())
    } else {
        format!("已同时保存 {} 个模组版本", references.len())
    })
}

/// 按「模组键 → 版本 ID」的引用表还原一组模组版本，返回（成功数, 跳过数）。
/// 游戏大版本和整合包共用这一条。
fn restore_mod_refs(
    references: &[versioning::ModVersionRef],
    parallel: bool,
    progress: &mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>,
) -> Result<(usize, usize), String> {
    let (store_base, _) = configured_version_options()?;
    let roots = collect_mod_roots();
    let total = references.len();
    let mut restored = 0usize;
    let mut skipped = 0usize;

    for (index, reference) in references.iter().enumerate() {
        let _ = progress(mod_marker_event(index, total, &reference.name));
        // 优先按模组键找当前目录（Steam 库搬家后路径会变），找不到再用记录的路径。
        let root = roots
            .iter()
            .find(|root| mod_versioning::store_key(root) == reference.key)
            .cloned()
            .or_else(|| {
                let stored = PathBuf::from(&reference.root_path);
                stored.is_dir().then(|| mod_versioning::ModRoot {
                    path: stored,
                    source: reference.source.clone(),
                    workshop_id: reference.workshop_id.clone(),
                    mod_id: reference.mod_id.clone(),
                    name: reference.name.clone(),
                })
            });
        let Some(root) = root else {
            skipped += 1;
            continue;
        };
        let Ok(store) = mod_version_store(&root, &store_base) else {
            skipped += 1;
            continue;
        };
        // 内层还原进度折算到当前模组占的那一小格（phase 统一写成 "mods"）。
        let restored_result = {
            let mut inner = |event: versioning::ProgressEvent| {
                progress(mod_inner_event(index, total, &root.name, event))
            };
            versioning::restore_version(
                &root.path,
                &store,
                &reference.version_id,
                true,
                parallel,
                &mut inner,
            )
        };
        match restored_result {
            Ok(_) => {
                restored += 1;
                if let Err(error) = reconcile_jar_state_for_root(&root.path) {
                    launcher_log::error(&format!("同步「{}」的 JAR 状态失败：{error}", root.name));
                }
            }
            Err(error) => {
                skipped += 1;
                launcher_log::error(&format!("还原模组「{}」失败：{error}", root.name));
            }
        }
    }

    Ok((restored, skipped))
}

/// 还原游戏大版本后，把它当时关联的模组版本一并还原。
fn restore_associated_mod_versions(
    record: &versioning::VersionRecord,
    parallel: bool,
    progress: &mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>,
) -> Result<String, String> {
    if record.mod_versions.is_empty() {
        return Ok(String::new());
    }
    let (restored, skipped) = restore_mod_refs(&record.mod_versions, parallel, progress)?;
    Ok(if skipped > 0 {
        format!("已还原 {restored} 个模组版本（{skipped} 个找不到，已跳过）")
    } else {
        format!("已还原 {restored} 个模组版本")
    })
}

/// 一次版本操作被拆成若干「步骤」（计算文件哈希 → 复制/保存变化 → 保存模组版本…）。
/// 引擎的 phase 比步骤细，这里归并成步骤序号，前端在百分比后面显示 `2/3`。
/// 返回 0 表示这个 phase 不该推进步骤（例如 done/error）。
fn version_step_for(operation: &str, phase: &str) -> u32 {
    match operation {
        "snapshot" | "capture" => match phase {
            "preparing" | "scanning" | "hashing" => 1,
            "copying" | "delta" => 2,
            "mods" => 3,
            _ => 0,
        },
        "restore" => match phase {
            "preparing" | "scanning" | "hashing" => 1,
            // PZ Optimization 的卸载/重装算进「还原文件」这一步，不单独占一格。
            "pzopt" | "restoring" => 2,
            "cleaning" => 3,
            "mods" => 4,
            _ => 0,
        },
        "modSnapshot" => match phase {
            "preparing" | "scanning" | "hashing" => 1,
            "copying" => 2,
            _ => 0,
        },
        "modCapture" => match phase {
            "preparing" | "scanning" | "hashing" => 1,
            "delta" => 2,
            _ => 0,
        },
        "modRestore" => match phase {
            "preparing" | "scanning" | "hashing" => 1,
            "restoring" => 2,
            "cleaning" => 3,
            _ => 0,
        },
        "verify" | "modVerify" => 1,
        // 一键保存 / 还原整套模组：只有一步（进度按模组等分）。
        "modSaveAll" | "modRestorePack" => match phase {
            "preparing" | "scanning" | "hashing" | "mods" => 1,
            _ => 0,
        },
        _ => 0,
    }
}

/// 游戏大版本走 `version-progress`，单个模组走 `mod-version-progress`；
/// 模组事件额外带上 `rootKey` / `rootPath`，前端据此只更新对应那一行/对话框。
fn emit_version_progress(
    app: &tauri::AppHandle,
    event: &str,
    root: Option<(&str, &str)>,
    operation_id: u64,
    operation: &str,
    status: &str,
    phase: &str,
    processed_files: u64,
    total_files: u64,
    processed_bytes: u64,
    total_bytes: u64,
    current_path: Option<String>,
    message: Option<String>,
    error: Option<String>,
    step: u32,
    steps: u32,
) {
    let _ = app.emit(
        event,
        VersionProgressUpdate {
            operation_id,
            operation: operation.to_string(),
            status: status.to_string(),
            phase: phase.to_string(),
            processed_files,
            total_files,
            processed_bytes,
            total_bytes,
            current_path,
            message,
            error,
            root_key: root.map(|(key, _)| key.to_string()),
            root_path: root.map(|(_, path)| path.to_string()),
            step,
            steps,
        },
    );
}

/// 版本操作耗时较长（扫描、复制、哈希都可能到分钟级），放进后台线程，
/// 用 `version-progress` 事件推送进度；同一时间只允许一个版本操作。
fn spawn_version_operation<F>(
    app: tauri::AppHandle,
    operation: &str,
    steps: u32,
    runner: F,
) -> Result<u64, String>
where
    F: FnOnce(&mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>) -> Result<String, String>
        + Send
        + 'static,
{
    spawn_version_operation_with(app, "version-progress", operation, None, steps, runner)
}

fn spawn_version_operation_with<F>(
    app: tauri::AppHandle,
    event: &'static str,
    operation: &str,
    root: Option<(String, String)>,
    steps: u32,
    runner: F,
) -> Result<u64, String>
where
    F: FnOnce(&mut dyn FnMut(versioning::ProgressEvent) -> Result<(), String>) -> Result<String, String>
        + Send
        + 'static,
{
    if VERSION_OPERATION_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有版本操作正在进行，请等待完成或先取消。".to_string());
    }
    VERSION_OPERATION_CANCELLED.store(false, Ordering::SeqCst);
    let operation_id = NEXT_VERSION_OPERATION_ID.fetch_add(1, Ordering::Relaxed);
    let operation_name = operation.to_string();
    let event_name = event.to_string();
    let root_key = root.as_ref().map(|(key, _)| key.clone());
    let root_path = root.as_ref().map(|(_, path)| path.clone());

    thread::spawn(move || {
        let target = root_key.as_deref().zip(root_path.as_deref());
        launcher_log::info(&format!("版本操作开始：{operation_name}"));
        emit_version_progress(
            &app,
            &event_name,
            target,
            operation_id,
            &operation_name,
            "started",
            "preparing",
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            if steps > 0 { 1 } else { 0 },
            steps,
        );

        let mut last_emit: Option<Instant> = None;
        let mut last_phase = String::from("preparing");
        let mut last_processed = 0u64;
        let mut last_total = 0u64;
        let mut last_processed_bytes = 0u64;
        let mut last_total_bytes = 0u64;
        let mut last_path: Option<String> = None;
        let mut last_step = 1u32;

        let mut progress = |event: versioning::ProgressEvent| -> Result<(), String> {
            if VERSION_OPERATION_CANCELLED.load(Ordering::SeqCst) {
                return Err("操作已取消".to_string());
            }
            last_processed = event.processed_files;
            last_total = event.total_files;
            last_processed_bytes = event.processed_bytes;
            last_total_bytes = event.total_bytes;
            last_path = event.current_path.clone();
            let step = version_step_for(&operation_name, &event.phase);
            if step > 0 {
                last_step = step.min(steps.max(1));
            }

            let finished = event.total_files > 0 && event.processed_files >= event.total_files;
            let should_emit = event.phase != last_phase
                || last_emit.is_none_or(|last| last.elapsed() >= Duration::from_millis(120))
                || finished;
            if should_emit {
                last_emit = Some(Instant::now());
                last_phase = event.phase.clone();
                emit_version_progress(
                    &app,
                    &event_name,
                    target,
                    operation_id,
                    &operation_name,
                    "progress",
                    &event.phase,
                    event.processed_files,
                    event.total_files,
                    event.processed_bytes,
                    event.total_bytes,
                    event.current_path,
                    None,
                    None,
                    last_step,
                    steps,
                );
            }
            Ok(())
        };

        let result = runner(&mut progress);
        drop(progress);
        // 先释放“正在进行”标记再推最终事件：前端收到 completed 后会立刻刷新状态，
        // 如果这里还没清标记，刷新会读回 operationRunning=true，把按钮永久卡在忙碌态。
        VERSION_OPERATION_ACTIVE.store(false, Ordering::SeqCst);

        match &result {
            Ok(message) => {
                launcher_log::info(&format!("版本操作完成：{operation_name}：{message}"))
            }
            Err(error) => {
                launcher_log::error(&format!("版本操作失败：{operation_name}：{error}"))
            }
        }

        match result {
            Ok(message) => emit_version_progress(
                &app,
                &event_name,
                target,
                operation_id,
                &operation_name,
                "completed",
                "done",
                last_processed,
                last_total,
                last_processed_bytes,
                last_total_bytes,
                last_path,
                Some(message),
                None,
                steps,
                steps,
            ),
            Err(error) => emit_version_progress(
                &app,
                &event_name,
                target,
                operation_id,
                &operation_name,
                "error",
                "error",
                last_processed,
                last_total,
                last_processed_bytes,
                last_total_bytes,
                last_path,
                None,
                Some(error),
                last_step,
                steps,
            ),
        }
    });

    Ok(operation_id)
}

#[tauri::command(async)]
fn get_version_status() -> Result<versioning::VersionStatus, String> {
    let store_path = configured_version_store_path()?;
    let game_path = detected_game_path();
    let steam_build_id =
        game_path.as_deref().and_then(|path| steam_manifest_value_for_game(path, "buildid"));
    let branch = game_path
        .as_deref()
        .and_then(|path| steam_manifest_value_for_game(path, "betakey"))
        .unwrap_or_else(|| "stable".to_string());
    versioning::get_status(
        game_path.as_deref(),
        &store_path,
        steam_build_id.as_deref(),
        &branch,
        VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst),
    )
}

#[tauri::command(async)]
fn get_version_store_path() -> Result<String, String> {
    Ok(configured_version_store_path()?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command(async)]
fn save_version_system_drive_notice_dismissed(dismissed: bool) -> Result<(), String> {
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.version_system_drive_notice_dismissed = dismissed;
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command(async)]
fn save_version_store_path(path: String) -> Result<(), String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        return Err("请等当前操作完成后再改备份位置。".to_string());
    }
    let trimmed = path.trim();
    let game_path = detected_game_path();
    let next = if trimmed.is_empty() {
        None
    } else {
        let candidate = PathBuf::from(trimmed);
        if !candidate.is_absolute() {
            return Err("备份位置要填完整路径（例如 D:\\PZ备份）。".to_string());
        }
        if let Some(game) = game_path.as_deref() {
            versioning::ensure_store_outside_root(&candidate, game)?;
        }
        fs::create_dir_all(&candidate).map_err(|error| {
            format!("创建备份文件夹失败（{}）：{error}", candidate.display())
        })?;
        Some(candidate)
    };

    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.version_store_path = next.map(|value| value.to_string_lossy().into_owned());
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command(async)]
fn save_version_parallel(enabled: bool) -> Result<(), String> {
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.version_parallel = enabled;
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command(async)]
fn start_version_snapshot(
    app: tauri::AppHandle,
    label: Option<String>,
    include_mods: Option<bool>,
) -> Result<u64, String> {
    ensure_game_closed()?;
    let context = version_context()?;
    let include_mods = match include_mods {
        Some(value) => value,
        None => configured_include_mods()?,
    };
    // 步骤：1 计算哈希 → 2 复制文件 →（有启用模组时）3 保存模组版本。
    let steps = if include_mods && !enabled_mod_roots().is_empty() {
        3
    } else {
        2
    };
    spawn_version_operation(app, "snapshot", steps, move |progress| {
        let outcome = versioning::create_full_snapshot(
            &context.game_path,
            &context.store_path,
            context.steam_build_id.as_deref(),
            &context.branch,
            label.as_deref(),
            context.parallel,
            &mut *progress,
        )?;
        let mut message = outcome.message;
        if include_mods {
            if let Some(version) = outcome.version.as_ref() {
                let summary = record_mod_versions_for_game_version(
                    &context.store_path,
                    &version.id,
                    &version.label,
                    context.parallel,
                    &mut *progress,
                )?;
                message.push_str(&format!("；{summary}"));
            }
        }
        Ok(message)
    })
}

#[tauri::command(async)]
fn start_version_capture(
    app: tauri::AppHandle,
    label: Option<String>,
    include_mods: Option<bool>,
) -> Result<u64, String> {
    ensure_game_closed()?;
    let context = version_context()?;
    let include_mods = match include_mods {
        Some(value) => value,
        None => configured_include_mods()?,
    };
    // 步骤：1 计算哈希 → 2 保存变化 →（有启用模组时）3 保存模组版本。
    let steps = if include_mods && !enabled_mod_roots().is_empty() {
        3
    } else {
        2
    };
    spawn_version_operation(app, "capture", steps, move |progress| {
        let outcome = versioning::capture_version(
            &context.game_path,
            &context.store_path,
            context.steam_build_id.as_deref(),
            &context.branch,
            label.as_deref(),
            context.parallel,
            &mut *progress,
        )?;
        let mut message = outcome.message;
        if include_mods {
            if let Some(version) = outcome.version.as_ref() {
                let summary = record_mod_versions_for_game_version(
                    &context.store_path,
                    &version.id,
                    &version.label,
                    context.parallel,
                    &mut *progress,
                )?;
                message.push_str(&format!("；{summary}"));
            }
        }
        Ok(message)
    })
}

fn pz_optimization_installed_for_game(game_path: &Path) -> bool {
    let libraries = all_libraries_from_roots(&steam_roots());
    pz_optimization_status(Some(game_path), &libraries).installed
}

fn version_pzopt_progress(label: &str) -> versioning::ProgressEvent {
    versioning::ProgressEvent {
        phase: "pzopt".to_string(),
        processed_files: 0,
        total_files: 0,
        processed_bytes: 0,
        total_bytes: 0,
        current_path: Some(label.to_string()),
    }
}

#[tauri::command(async)]
fn start_version_restore(
    app: tauri::AppHandle,
    version_id: String,
    force: bool,
    restore_mods: Option<bool>,
) -> Result<u64, String> {
    ensure_game_closed()?;
    let context = version_context()?;
    let restore_mods = restore_mods.unwrap_or(true);
    // 步骤：1 检查当前文件 → 2 还原文件 → 3 清理多余文件；大版本带模组时 +1。
    let has_mod_versions = versioning::read_versions(&context.store_path)
        .ok()
        .and_then(|versions| {
            versions
                .into_iter()
                .find(|record| record.id == version_id)
        })
        .is_some_and(|record| !record.mod_versions.is_empty());
    let steps = if restore_mods && has_mod_versions { 4 } else { 3 };
    spawn_version_operation(app, "restore", steps, move |progress| {
        // PZ Optimization 会替换游戏类文件。还原前先让它把自己的改动撤回原版，
        // 还原成功后再按“还原后的游戏版本”重新安装；否则切换版本后模组状态会残留/失效。
        let pz_was_installed = pz_optimization_installed_for_game(&context.game_path);
        if pz_was_installed {
            // 这一步已经进入收尾阶段，不再响应取消：PowerShell 安装器无法安全中断。
            let _ = progress(version_pzopt_progress("还原前卸载 PZ Optimization"));
            uninstall_pz_optimization().map_err(|error| {
                format!("还原前卸载 PZ Optimization 失败，已中止还原：{error}")
            })?;
        }

        let restore_result = versioning::restore_version(
            &context.game_path,
            &context.store_path,
            &version_id,
            force,
            context.parallel,
            &mut *progress,
        );

        let outcome = match restore_result {
            Ok(outcome) => outcome,
            Err(error) => {
                if pz_was_installed {
                    return Err(format!(
                        "{error}\n\n提示：PZ Optimization 已在还原前卸载，还原失败后不会自动重装；可以到「支持库」手动安装。"
                    ));
                }
                return Err(error);
            }
        };

        let mut message = outcome.message;
        if pz_was_installed {
            let _ = progress(version_pzopt_progress("还原后重新安装 PZ Optimization"));
            match install_pz_optimization_impl(true) {
                Ok(result) => {
                    if result.already_installed {
                        message.push_str("\nPZ Optimization 保持已安装状态。");
                    } else {
                        let revision = result
                            .installed_revision
                            .map(|value| format!("（版本 {value}）"))
                            .unwrap_or_default();
                        message.push_str(&format!("\nPZ Optimization 已重新安装{revision}。"));
                    }
                }
                Err(error) => {
                    message.push_str(&format!(
                        "\nPZ Optimization 未能自动重装：{error}\n可以到「支持库」手动安装。"
                    ));
                }
            }
        }

        // 游戏大版本里记录了当时的模组版本时，一并还原（模组已卸载的跳过）。
        if restore_mods {
            let record = versioning::read_versions(&context.store_path)
                .ok()
                .and_then(|versions| {
                    versions
                        .into_iter()
                        .find(|record| record.id == version_id)
                });
            if let Some(record) = record {
                match restore_associated_mod_versions(&record, context.parallel, &mut *progress) {
                    Ok(summary) if !summary.is_empty() => {
                        message.push_str(&format!("\n{summary}。"));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        message.push_str(&format!("\n关联的模组版本未能还原：{error}"));
                    }
                }
            }
        }

        Ok(message)
    })
}

#[tauri::command(async)]
fn start_version_verify(app: tauri::AppHandle) -> Result<u64, String> {
    let context = version_context()?;
    let verify_app = app.clone();
    spawn_version_operation(app, "verify", 1, move |progress| {
        let outcome = versioning::verify_current_version(
            &context.game_path,
            &context.store_path,
            context.parallel,
            progress,
        )?;
        // 额外的结构化结果：前端用它把「疑似」升级成「已校验」。
        let _ = verify_app.emit("version-verified", outcome.clone());
        Ok(outcome.message)
    })
}

#[tauri::command]
fn cancel_version_operation() -> Result<(), String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        VERSION_OPERATION_CANCELLED.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command(async)]
fn delete_version(version_id: String) -> Result<String, String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        return Err("请等待当前版本操作完成后再删除版本。".to_string());
    }
    let store_path = configured_version_store_path()?;
    versioning::delete_version(&store_path, &version_id)
}

#[tauri::command(async)]
fn save_version_include_mods(enabled: bool) -> Result<(), String> {
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.version_include_mods = enabled;
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command(async)]
fn list_mod_version_targets() -> Result<Vec<mod_versioning::ModVersionTarget>, String> {
    let store_base = configured_version_store_path()?;
    Ok(mod_versioning::list_targets(&store_base, &collect_mod_roots()))
}

#[tauri::command(async)]
fn get_mod_version_status(root_path: String) -> Result<mod_versioning::ModVersionStatus, String> {
    let root = resolve_mod_root(&root_path)?;
    let store_base = configured_version_store_path()?;
    mod_versioning::status(
        &store_base,
        &root,
        VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst),
    )
}

fn mod_operation_root(root: &mod_versioning::ModRoot) -> Option<(String, String)> {
    Some((
        mod_versioning::store_key(root),
        root.path.to_string_lossy().into_owned(),
    ))
}

#[tauri::command(async)]
fn start_mod_version_snapshot(
    app: tauri::AppHandle,
    root_path: String,
    label: Option<String>,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let (store_base, parallel) = configured_version_options()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modSnapshot",
        target,
        2,
        move |progress| {
            let outcome = versioning::create_full_snapshot(
                &root.path,
                &store,
                None,
                "mods",
                label.as_deref(),
                parallel,
                progress,
            )?;
            Ok(outcome.message)
        },
    )
}

#[tauri::command(async)]
fn start_mod_version_capture(
    app: tauri::AppHandle,
    root_path: String,
    label: Option<String>,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let (store_base, parallel) = configured_version_options()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modCapture",
        target,
        2,
        move |progress| {
            let outcome = versioning::capture_version(
                &root.path,
                &store,
                None,
                "mods",
                label.as_deref(),
                parallel,
                progress,
            )?;
            Ok(outcome.message)
        },
    )
}

#[tauri::command(async)]
fn start_mod_version_restore(
    app: tauri::AppHandle,
    root_path: String,
    version_id: String,
    force: bool,
) -> Result<u64, String> {
    // 还原会直接改写模组目录，游戏运行时文件可能被占用。
    ensure_game_closed()?;
    let root = resolve_mod_root(&root_path)?;
    let (store_base, parallel) = configured_version_options()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modRestore",
        target,
        3,
        move |progress| {
            let outcome = versioning::restore_version(
                &root.path,
                &store,
                &version_id,
                force,
                parallel,
                progress,
            )?;
            let message = outcome.message;
            if let Err(error) = reconcile_jar_state_for_root(&root.path) {
                launcher_log::error(&format!("同步「{}」的 JAR 状态失败：{error}", root.name));
            }
            Ok(message)
        },
    )
}

#[tauri::command(async)]
fn start_mod_version_verify(
    app: tauri::AppHandle,
    root_path: String,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let (store_base, parallel) = configured_version_options()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    let verify_app = app.clone();
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modVerify",
        target,
        1,
        move |progress| {
            let outcome =
                versioning::verify_current_version(&root.path, &store, parallel, progress)?;
            let _ = verify_app.emit("mod-version-verified", &outcome);
            Ok(outcome.message)
        },
    )
}

#[tauri::command(async)]
fn delete_mod_version(root_path: String, version_id: String) -> Result<String, String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        return Err("请等待当前版本操作完成后再删除版本。".to_string());
    }
    let root = resolve_mod_root(&root_path)?;
    let store_base = configured_version_store_path()?;
    let store = mod_version_store(&root, &store_base)?;
    versioning::delete_version(&store, &version_id)
}

#[tauri::command(async)]
fn list_mod_version_packs() -> Result<Vec<mod_versioning::ModPackRecord>, String> {
    let store_base = configured_version_store_path()?;
    mod_versioning::read_packs(&store_base)
}

#[tauri::command(async)]
fn delete_mod_version_pack(pack_id: String) -> Result<String, String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        return Err("请等当前版本操作完成后再删除整合包。".to_string());
    }
    let store_base = configured_version_store_path()?;
    let mut packs = mod_versioning::read_packs(&store_base)?;
    let label = packs
        .iter()
        .find(|pack| pack.id == pack_id)
        .map(|pack| pack.label.clone());
    let before = packs.len();
    packs.retain(|pack| pack.id != pack_id);
    if packs.len() == before {
        return Err("找不到这个整合包。".to_string());
    }
    mod_versioning::write_packs(&store_base, &packs)?;
    Ok(format!("已删除整合包「{}」。", label.unwrap_or_default()))
}

/// 一键把当前所有模组各存一个版本，并记成一条整合包。
#[tauri::command(async)]
fn start_mod_version_save_all(
    app: tauri::AppHandle,
    label: Option<String>,
) -> Result<u64, String> {
    let (store_base, parallel) = configured_version_options()?;
    let roots = collect_mod_roots();
    if roots.is_empty() {
        return Err("没有可保存的模组。".to_string());
    }
    let packs = mod_versioning::read_packs(&store_base)?;
    let pack_label = label
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("整合包 {}", packs.len() + 1));
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modSaveAll",
        None,
        1,
        move |progress| {
            let (references, failures) =
                capture_mod_versions(&roots, &pack_label, parallel, progress)?;
            if references.is_empty() {
                return Err("没有成功保存任何模组版本。".to_string());
            }
            let mut packs = mod_versioning::read_packs(&store_base)?;
            let record = mod_versioning::ModPackRecord {
                id: format!("pack-{created_at}-{}", packs.len() + 1),
                label: pack_label.clone(),
                created_at,
                mods: references.clone(),
            };
            packs.push(record);
            mod_versioning::write_packs(&store_base, &packs)?;
            Ok(if failures > 0 {
                format!(
                    "已保存整合包「{pack_label}」：{} 个模组（{failures} 个失败）",
                    references.len()
                )
            } else {
                format!("已保存整合包「{pack_label}」：{} 个模组", references.len())
            })
        },
    )
}

/// 还原一条整合包：把它记录的所有模组版本一起还原。
#[tauri::command(async)]
fn start_mod_version_restore_pack(
    app: tauri::AppHandle,
    pack_id: String,
) -> Result<u64, String> {
    // 还原会直接改写模组目录，游戏运行时文件可能被占用。
    ensure_game_closed()?;
    let (store_base, parallel) = configured_version_options()?;
    let pack = mod_versioning::read_packs(&store_base)?
        .into_iter()
        .find(|pack| pack.id == pack_id)
        .ok_or_else(|| "找不到这个整合包。".to_string())?;
    if pack.mods.is_empty() {
        return Err("这个整合包里没有模组。".to_string());
    }
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modRestorePack",
        None,
        1,
        move |progress| {
            let (restored, skipped) = restore_mod_refs(&pack.mods, parallel, progress)?;
            Ok(if skipped > 0 {
                format!(
                    "已还原整合包「{}」：{restored} 个模组（{skipped} 个找不到，已跳过）",
                    pack.label
                )
            } else {
                format!("已还原整合包「{}」：{restored} 个模组", pack.label)
            })
        },
    )
}

#[tauri::command(async)]
fn read_launcher_log(limit: Option<u64>) -> Result<launcher_log::LauncherLogTail, String> {
    launcher_log::read_tail(limit)
}

#[tauri::command(async)]
fn clear_launcher_log() -> Result<(), String> {
    launcher_log::clear()
}

#[tauri::command(async)]
fn open_launcher_log_folder() -> Result<(), String> {
    launcher_log::open_folder()
}

#[tauri::command]
fn log_frontend_message(level: String, message: String) {
    launcher_log::frontend(&level, &message);
}


#[tauri::command]
fn save_shader_download_settings(
    url: String,
    directory: String,
    threads: usize,
) -> Result<(), String> {
    if !(1..=16).contains(&threads) {
        return Err("下载线程数必须在 1 到 16 之间".to_string());
    }
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.shader_download_url = if url.trim().is_empty() {
        None
    } else {
        Some(url.trim().to_string())
    };
    settings.shader_download_directory = if directory.trim().is_empty() {
        None
    } else {
        Some(directory.trim().to_string())
    };
    settings.shader_download_threads = Some(threads);
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
async fn download_shader_file(
    app: tauri::AppHandle,
    download_id: Option<String>,
    url: String,
    file_name: String,
    directory: String,
    threads: usize,
) -> Result<String, String> {
    // 前端任务中心会传入自己的任务 ID，用于把 shader-download-progress 精确对回任务；
    // 旧的「自定义下载」入口不传时仍由后端生成。
    let download_id = download_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            format!(
                "shader-{}",
                NEXT_WORKSHOP_BACKGROUND_SCAN_ID.fetch_add(1, Ordering::Relaxed)
            )
        });
    let directory = if directory.trim().is_empty() {
        shader_download_directory()?.to_string_lossy().into_owned()
    } else {
        directory
    };
    downloader::download_file(app, download_id, url, file_name, directory, threads).await
}

#[tauri::command]
fn save_steam_relay_settings(mode: String, base: String) -> Result<(), String> {
    let mode = steam_relay::normalize_relay_mode(&mode)
        .ok_or_else(|| format!("不支持的 Steam 网络加速模式: {mode}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_relay_mode = Some(mode);
    settings.steam_relay_base = Some(steam_relay::normalize_relay_base(&base));
    save_app_settings_at(&path, &settings)
}

/// 设置页「本地代理」：空串 = 清空（退回系统代理）。填了但不合法时明确报错，
/// 免得用户以为设上了其实没生效。
#[tauri::command]
fn save_steam_proxy(proxy: String) -> Result<(), String> {
    let normalized = if proxy.trim().is_empty() {
        None
    } else {
        Some(steam_relay::normalize_steam_proxy(&proxy).ok_or_else(|| {
            format!("代理地址无效: {proxy}（示例：http://127.0.0.1:7897 或 socks5://127.0.0.1:1080）")
        })?)
    };
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_proxy = normalized;
    save_app_settings_at(&path, &settings)
}

/// 设置页「自动检测本地代理」：探常见 HTTP/SOCKS 端口，返回真能访问 Steam 的。
#[tauri::command]
async fn detect_steam_proxy() -> Vec<steam_relay::ProxyProbe> {
    steam_relay::detect_local_proxies().await
}

/// 设置 › 关于 › 反馈信息：上传最近一次启动的日志 + 用户描述。
#[tauri::command]
async fn submit_feedback(description: String) -> Result<feedback::FeedbackUploadResult, String> {
    feedback::submit_feedback(description).await
}

/// 设置页「Steam 网络加速 › 测试连接」：探一下中转的 /healthz，报耗时。
/// 探测失败也返回 Ok（结果里的 ok=false），让前端展示原因而不是当命令错误。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SteamRelayPingResult {
    ok: bool,
    latency_ms: u64,
    error: Option<String>,
}

#[tauri::command]
async fn ping_steam_relay(base: String) -> Result<SteamRelayPingResult, String> {
    let base = steam_relay::normalize_relay_base(&base);
    let started = std::time::Instant::now();
    // 与真实中转请求一致：绕过系统代理（残留死代理不该影响中转探测）。
    let result = steam_relay::steam_client(true)
        .get(format!("{base}/healthz"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(response) if response.status().is_success() => Ok(SteamRelayPingResult {
            ok: true,
            latency_ms,
            error: None,
        }),
        Ok(response) => Ok(SteamRelayPingResult {
            ok: false,
            latency_ms,
            error: Some(format!("中转服务返回 {}", response.status())),
        }),
        Err(error) => Ok(SteamRelayPingResult {
            ok: false,
            latency_ms,
            error: Some(error.to_string()),
        }),
    }
}


#[cfg(windows)]
fn create_fast_launch_backup() -> Result<Option<PathBuf>, String> {
    let game_path = project_zomboid_install_path()
        .ok_or_else(|| "找不到 Project Zomboid 安装目录，无法备份极速启动配置".to_string())?;
    let backup_dir = fast_launch_backup_directory()?;
    create_fast_launch_backup_at(&game_path, &backup_dir)?;
    Ok(Some(backup_dir))
}

#[cfg(not(windows))]
fn create_fast_launch_backup() -> Result<Option<PathBuf>, String> {
    Ok(None)
}


#[cfg(windows)]
fn restore_fast_launch_backup() -> Result<bool, String> {
    let Some(game_path) = project_zomboid_install_path() else {
        return Ok(false);
    };
    restore_fast_launch_backup_at(&game_path, &fast_launch_backup_directory()?)
}

#[cfg(not(windows))]
fn restore_fast_launch_backup() -> Result<bool, String> {
    Ok(false)
}


/// 保存「静默内存清理」开关与阈值。
///
/// 阈值在这里就收敛到 `memclean` 的可用区间，并**立刻**同步给后台轮询线程：
/// 用户改完设置不需要重启启动器，下一次轮询就按新阈值判断。
#[tauri::command]
fn save_silent_memory_cleanup(enabled: bool, threshold_percent: u8) -> Result<(), String> {
    let threshold = memclean::normalize_threshold(threshold_percent);
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.silent_memory_cleanup = enabled;
    settings.silent_memory_cleanup_threshold = threshold;
    save_app_settings_at(&path, &settings)?;
    memclean::set_silent_cleanup(enabled, threshold);
    Ok(())
}

/// 保存「启动时清理内存」开关；真正的清理发生在 [`launch_project_zomboid`] 里。
#[tauri::command]
fn save_clean_memory_on_launch(enabled: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.clean_memory_on_launch = enabled;
    save_app_settings_at(&path, &settings)
}

/// 当前系统内存占用，给设置界面显示（也用来判断阈值合不合理）。
#[tauri::command]
fn get_memory_status() -> Result<memclean::MemoryStatus, String> {
    memclean::status().ok_or_else(|| "无法读取系统内存状态".to_string())
}

/// 立即清理一次（设置界面的「立即清理」按钮）。
#[tauri::command(async)]
fn clean_memory_now() -> Result<memclean::CleanupReport, String> {
    Ok(memclean::clean_memory())
}

fn current_steam_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in steam_roots() {
        let profile = steam_profile_from_root(&root);
        paths.extend(steam_config_paths(&root, profile.as_ref()));
    }
    paths.sort();
    paths.dedup();
    paths
}

fn patch_steam_launch_options_at_paths(
    config_paths: &[PathBuf],
    memory_mb: Option<u64>,
    extra_args: &[String],
) -> Result<bool, String> {
    let mut changed_files = Vec::new();
    for path in config_paths {
        let content =
            fs::read_to_string(path).map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (patched, changed) =
            patch_steam_launch_options_content_with_memory(&content, memory_mb, extra_args)?;
        if changed {
            changed_files.push((path.clone(), patched.into_bytes()));
        }
    }
    if changed_files.is_empty() {
        return Ok(false);
    }
    apply_file_transaction(changed_files)?;
    Ok(true)
}


#[tauri::command]
fn read_jar_mod_poster(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !is_supported_image(&path) || !is_regular_file(&path) {
        return Err("JAR 模组海报不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取 JAR 模组海报失败: {error}"))
}


#[tauri::command]
fn workshop_options() -> workshop::WorkshopOptions {
    workshop::workshop_options()
}

#[tauri::command]
async fn translate_developer_news_batch(
    jobs: Vec<news::TranslationJob>,
    language: String,
) -> Result<Vec<news_translation::TranslationResult>, String> {
    news_translation::translate_batch(jobs, language).await
}
#[tauri::command]
async fn browse_workshop(
    filters: workshop::WorkshopFilters,
) -> Result<workshop::WorkshopResponse, String> {
    workshop::fetch_workshop(filters).await
}

#[tauri::command]
async fn fetch_workshop_item_detail(
    id: String,
    language: String,
) -> Result<workshop::WorkshopItemDetail, String> {
    workshop::fetch_workshop_item_detail(id, language).await
}

#[tauri::command]
fn resource_options() -> resource::ResourceOptions {
    resource::resource_options()
}

#[tauri::command]
async fn fetch_shader_resources(
    query: resource::ResourceSearchQuery,
) -> Result<resource::ResourceSearchResponse, String> {
    resource::fetch_resources(query).await
}

#[tauri::command]
async fn fetch_shader_resource_files(
    resource_id: String,
    download_source: String,
) -> Result<Vec<resource::ResourceVersionPack>, String> {
    resource::fetch_resource_files(resource_id, download_source).await
}

#[tauri::command]
async fn fetch_minecraft_versions() -> Result<Vec<resource::MinecraftVersionOption>, String> {
    resource::fetch_minecraft_versions().await
}


/// 找到游戏安装目录，按「模组加载器方案」改写启动文件（内存必写，agent 视方案而定）。
///
/// 返回是否真的改写了文件。找不到游戏目录时返回 `Ok(false)`（例如未安装）。
#[cfg(windows)]
fn apply_launch_loader(
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    let roots = steam_roots();
    let libraries = all_libraries_from_roots(&roots);
    let Some(game_path) = libraries
        .iter()
        .find_map(|library| game_path_from_library(library))
    else {
        return Ok(false);
    };
    apply_launch_loader_at_paths(
        Path::new(&game_path),
        memory_mb,
        policy_agent,
        fast_launch_agent,
    )
}

/// 无条件按 ZombieBuddy 方案改写（显式动作与历史调用点）。
#[cfg(windows)]
fn force_zombie_buddy_policy(memory_mb: Option<u64>) -> Result<bool, String> {
    apply_launch_loader(memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

#[cfg(not(windows))]
fn apply_launch_loader(
    _memory_mb: Option<u64>,
    _policy_agent: Option<&str>,
    _fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
fn force_zombie_buddy_policy(memory_mb: Option<u64>) -> Result<bool, String> {
    Ok(false)
}

/// 把 `ProjectZomboid64.json` / `ProjectZomboid64.bat` 里的 ZB agent 参数统一成
/// `-agentlib:zbNative=policy=allow-all`，让 ZombieBuddy 跳过模组审批确认界面。
/// 按「模组加载器方案」在启动前改写游戏自带的 `ProjectZomboid64.json` / `.bat`：
/// 内存上限始终写（和加载器无关，否则 JSON 里旧的 -Xmx 会覆盖启动器设置），
/// 加载器的 agent 参数只在方案启用了加载器时归一化（`policy_agent`），
/// 极速启动的 `-agentpath:<DLL>` 只在 `fast_launch_agent` 给了路径时写、否则一律摘掉。
fn apply_launch_loader_at_paths(
    game_path: &Path,
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    let json_path = game_path.join("ProjectZomboid64.json");
    let batch_path = game_path.join("ProjectZomboid64.bat");

    // `.bat` 的 `_JAVA_OPTIONS` 按空白切分，路径带空格会被拆坏，所以只有无空格路径才写进 .bat。
    let batch_agent = fast_launch_agent_for_batch(fast_launch_agent);
    if fast_launch_agent.is_some() && batch_agent.is_none() {
        // 只记日志：JSON 那条已经足够让 Steam 启动路径生效。
        eprintln!("极速启动：DLL 路径含空格，跳过 .bat 的 _JAVA_OPTIONS 改写");
    }

    let mut files = Vec::new();
    if json_path.is_file() {
        let content = fs::read_to_string(&json_path)
            .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
        let (patched, changed) =
            patch_json_launcher_with_agent(&content, memory_mb, policy_agent, fast_launch_agent)?;
        if changed {
            files.push((json_path.clone(), patched.into_bytes()));
        }
    }
    if batch_path.is_file() {
        let content = fs::read_to_string(&batch_path)
            .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
        let (patched, changed) =
            patch_batch_with_agent(&content, memory_mb, policy_agent, batch_agent);
        if changed {
            files.push((batch_path.clone(), patched.into_bytes()));
        }
    }
    if files.is_empty() {
        return Ok(false);
    }

    let backup_dir = game_path
        .join(".pzmanager-backups")
        .join(current_unix_timestamp()?);
    for (path, _) in &files {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("无效备份文件路径: {}", path.display()))?;
        backup_file(path, &backup_dir, name)?;
    }
    apply_file_transaction(files)?;
    Ok(true)
}

/// 显式动作（例如配置 Viewpoint 3D）用的入口：无条件按 ZombieBuddy 方案改写。
fn force_zombie_buddy_policy_at_paths(
    game_path: &Path,
    memory_mb: Option<u64>,
) -> Result<bool, String> {
    apply_launch_loader_at_paths(game_path, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}


/// 系统物理内存总量（MB）。
///
/// 读取内存状态、清理工作集都归 `memclean`；这里只做一次转交，
/// 免得项目里出现两套 `GlobalMemoryStatusEx` 声明（字段一改就两边不同步）。
fn system_memory_mb() -> Result<u64, String> {
    memclean::status()
        .map(|status| status.total_mb)
        .ok_or_else(|| "无法读取系统物理内存".to_string())
}

#[tauri::command]
fn get_system_memory_mb() -> Result<u64, String> {
    system_memory_mb()
}

/// 把界面给的覆盖层 DLL 路径解析成绝对路径，并**确认文件真的存在**。
///
/// 不存在就返回 `None`（= 不写 `-agentpath`）：`-agentpath` 指向的文件找不到会让
/// JVM 直接启动失败，而我们的硬约束是"任何一步不成立就回退原生扫描"，绝不能把游戏弄起不来。
fn resolve_fast_launch_agent_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    crate::overlay::imp::normalize_dll_path(trimmed)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command(async)]
fn launch_project_zomboid(
    memory_mb: u64,
    launch_args: Vec<String>,
    fast_launch_dll_path: Option<String>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (memory_mb, launch_args, fast_launch_dll_path);
        return Err("启动 Project Zomboid 只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let total_memory_mb = system_memory_mb()?;
        validate_memory_limit_mb(memory_mb, total_memory_mb)?;
        let steam_configs = current_steam_config_paths();
        if steam_configs.is_empty() {
            return Err("找不到 Steam localconfig.vdf".to_string());
        }
        // 读一次设置：「模组加载器方案」决定 agent 处理，`fastLaunch` 决定要不要写极速启动的 agent。
        let settings = load_app_settings_at(&app_settings_path()?).ok();
        let scheme = settings
            .as_ref()
            .and_then(|settings| settings.mod_loader_scheme.clone())
            .and_then(|value| normalize_mod_loader_scheme(&value))
            .unwrap_or_else(|| DEFAULT_MOD_LOADER_SCHEME.to_string());
        let policy_agent = mod_loader_policy_agent(&scheme);

        // 极速启动（`-agentpath`）的三个前提：设置开关打开、方案是 ZombieBuddy、DLL 真的存在。
        // 任一不满足都传 None —— 那会**摘掉**已经写在启动文件里的 `-agentpath:`，
        // 保证"关掉开关后下一次启动一定回到 ZombieBuddy 原生扫描"。
        let fast_launch_enabled = settings
            .as_ref()
            .is_some_and(|settings| settings.fast_launch);
        let fast_launch_agent = if fast_launch_enabled && policy_agent.is_some() {
            fast_launch_dll_path
                .as_deref()
                .and_then(resolve_fast_launch_agent_path)
        } else {
            None
        };
        match fast_launch_agent.as_deref() {
            Some(path) => eprintln!("极速启动：本次启动会带 -agentpath:{path}"),
            None if fast_launch_enabled && policy_agent.is_some() => eprintln!(
                "极速启动：开关已开但拿不到可用的覆盖层 DLL 路径，本次不注入（走 ZombieBuddy 原生扫描）"
            ),
            None => {}
        }

        // 方案不使用加载器时，先把默认启动参数里的加载器 agent 摘掉，别背着方案偷偷挂上。
        let mut effective_launch_args = launch_args;
        if policy_agent.is_none() {
            strip_loader_agent_tokens(&mut effective_launch_args);
        }
        launcher_log::info(&format!(
            "启动游戏：内存 {memory_mb} MB，加载器方案 {scheme}，极速启动 {}，启动参数 {}",
            if fast_launch_agent.is_some() { "开" } else { "关" },
            effective_launch_args.join(" ")
        ));
        patch_steam_launch_options_at_paths(
            &steam_configs,
            Some(memory_mb),
            &effective_launch_args,
        )?;

        // 再改写游戏自带的启动文件：内存上限始终写（与加载器无关，否则 JSON 里旧的 -Xmx
        // 会覆盖启动器设置）；只有方案启用了加载器时才把它的 agent 归一化成 policy=allow-all
        // （策略只在 agent 第一次加载时锁定，而 JSON/BAT 往往先于 Steam 参数被 JVM 读到）。
        apply_launch_loader(Some(memory_mb), policy_agent, fast_launch_agent.as_deref())?;
        launcher_log::info(&format!(
            "启动配置已写入（Steam 配置 {} 个）。",
            steam_configs.len()
        ));

        // 「启动时清理内存」：拉起游戏之前先把各进程的工作集还给系统。
        // 此刻游戏还没起来，所以这次清理不会被 memclean 的保护名单挡掉任何东西。
        // 清理失败/读不到内存状态都不影响启动——这是纯优化，不能变成启动的前置条件。
        if settings
            .as_ref()
            .is_some_and(|settings| settings.clean_memory_on_launch)
        {
            let report = memclean::clean_memory();
            launcher_log::info(&format!(
                "启动前清理内存：释放 {} MB（清理 {} 个进程，跳过 {} 个，耗时 {} ms）",
                report.freed_mb, report.trimmed, report.skipped, report.elapsed_ms
            ));
            eprintln!(
                "启动前清理内存：释放 {} MB（清理 {} 个进程，跳过 {} 个，耗时 {} ms）",
                report.freed_mb, report.trimmed, report.skipped, report.elapsed_ms
            );
        }

        let launch_result = Command::new("cmd")
            .args([
                "/C",
                "start",
                "",
                &format!("steam://rungameid/{PROJECT_ZOMBOID_APP_ID}"),
            ])
            .spawn();
        match launch_result {
            Ok(_) => {
                launcher_log::info("已发送 Steam 启动请求。");
                Ok(())
            }
            Err(error) => {
                let message = format!("启动 Project Zomboid 失败: {error}");
                launcher_log::error(&message);
                Err(message)
            }
        }
    }
}

/// WebView2 默认把用户数据目录放在 `%LOCALAPPDATA%\<identifier>`。
/// 不再探测默认位置是否可写：普通文件写入能成功，并不代表 WebView2 的浏览器进程
/// 能真正使用该目录，探测结果无法作为依据。开发期直接使用固定的、已验证可启动的目录。
#[cfg(target_os = "windows")]
fn ensure_webview2_data_folder() {
    // 外部已指定则尊重
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some() {
        return;
    }

    // 不再探测默认位置:默认位置"可写"不代表 WebView2 能用
    let dir = if cfg!(debug_assertions) {
        // 开发期:放在项目外,避免被 Vite 监视
        std::path::PathBuf::from(r"F:\webview2-data\pzmanager")
    } else {
        // 发布版:仍然用默认位置,不改变用户机器上的行为
        return;
    };

    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("无法创建 WebView2 数据目录 {dir:?}: {e}"));

    // 启动阶段单线程;如果 edition 是 2024,要包进 unsafe { }
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &dir);
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    launcher_log::start_session();
    let default_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        launcher_log::error(&format!("未捕获异常：{info}"));
        default_panic_hook(info);
    }));

    // 必须在 tauri::Builder 构建之前设置，WebView2 才会读取它。
    #[cfg(target_os = "windows")]
    ensure_webview2_data_folder();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // 静默内存清理：把持久化的开关/阈值交给后台轮询线程。
            // 读配置失败就按"关闭"处理——启动流程绝不能被清理功能拖住。
            let loaded_settings = app_settings_path()
                .and_then(|path| load_app_settings_at(&path))
                .ok();
            let (enabled, threshold) = loaded_settings
                .as_ref()
                .map(|settings| {
                    (
                        settings.silent_memory_cleanup,
                        settings.silent_memory_cleanup_threshold,
                    )
                })
                .unwrap_or((false, memclean::DEFAULT_THRESHOLD_PERCENT));
            memclean::set_silent_cleanup(enabled, threshold);
            memclean::start_silent_loop(app.handle().clone());

            // 恢复上一次记住的窗口尺寸（拖动窗口边缘调整后会自动记住）。
            // 默认 840×560 由 tauri.conf.json 提供，这里只在有有效记录时覆盖。
            if let Some(settings) = loaded_settings.as_ref() {
                let remembered_size = match (settings.window_width, settings.window_height) {
                    (Some(width), Some(height)) => {
                        match (normalize_window_width(width), normalize_window_height(height)) {
                            (Some(width), Some(height)) => Some((width, height)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((width, height)) = remembered_size {
                    use tauri::Manager;
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_size(tauri::LogicalSize::new(width, height));
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_project_zomboid,
            read_launcher_log,
            clear_launcher_log,
            open_launcher_log_folder,
            log_frontend_message,
            get_system_memory_mb,
            get_app_settings,
            save_memory_limit,
            save_developer_mode,
            save_steam_account_hidden,
            save_translation_language,
            save_font_contrast,
            save_blur_strength,
            save_font_scale,
            save_font_family,
            save_overlay_background_color,
            save_window_size,
            restore_default_window_size,
            save_mod_loader_scheme,
            save_fast_launch,
            save_silent_memory_cleanup,
            save_clean_memory_on_launch,
            save_steam_relay_settings,
            save_steam_proxy,
            detect_steam_proxy,
            get_version_status,
            get_version_store_path,
            save_version_system_drive_notice_dismissed,
            save_version_store_path,
            save_version_parallel,
            start_version_snapshot,
            start_version_capture,
            start_version_restore,
            start_version_verify,
            cancel_version_operation,
            delete_version,
            save_version_include_mods,
            list_mod_version_targets,
            get_mod_version_status,
            start_mod_version_snapshot,
            start_mod_version_capture,
            start_mod_version_restore,
            start_mod_version_verify,
            delete_mod_version,
            list_mod_version_packs,
            delete_mod_version_pack,
            start_mod_version_save_all,
            start_mod_version_restore_pack,
            submit_feedback,
            ping_steam_relay,
            get_memory_status,
            clean_memory_now,
            fast_launch_cache_status,
            startup_timing,
            set_background_path,
            set_wallpaper_effect,
            list_custom_backgrounds,
            scan_workshop_backgrounds,
            start_workshop_background_scan,
            import_custom_background,
            read_background,
            read_jar_mod_poster,
            read_lua_mod_poster,
            start_zombie_buddy_jar_scan,
            list_lua_mods,
            set_lua_mod_enabled,
            set_lua_mods_enabled,
            save_lua_mod_order,
            set_zombie_buddy_mod_decision,
            set_zombie_buddy_mods_decisions,
            apply_jar_mod_file_state,
            list_mod_presets,
            save_mod_preset,
            apply_mod_preset,
            delete_mod_preset,
            install_zombie_buddy,
            install_pz_optimization,
            uninstall_pz_optimization,
            configure_viewpoint_three_d,
            launch_project_zomboid,
            overlay::inject_overlay,
            overlay_session::start_overlay_session,
            overlay_session::check_game_process,
            overlay_session::stop_game_process,
            list_viewpoint_shots,
            read_viewpoint_shot,
            list_viewpoint_shaderpacks,
            import_viewpoint_shaderpack,
            delete_viewpoint_shaderpack,
            workshop_options,
            browse_workshop,
            fetch_workshop_item_detail,
            resource_options,
            fetch_shader_resources,
            fetch_shader_resource_files,
            save_shader_download_settings,
            download_shader_file,
            fetch_minecraft_versions,
            fetch_developer_news,
            translate_developer_news_batch,
            check_launcher_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        apply_file_transaction, apply_jar_mod_state_to_roots, apply_launch_loader_at_paths,
        build_fast_launch_cache, class_annotations, compose_title_background_at,
        configure_viewpoint_three_d_at_paths, create_fast_launch_backup_at, delete_shaderpack_at,
        discover_mod_info_files, fast_launch_cache_matches, fast_launch_cache_mods_at,
        force_zombie_buddy_policy_at_paths, game_path_from_library, import_shaderpack_to,
        install_locations_from_libraries, install_zombie_buddy_at_paths,
        is_builtin_background_path, jar_scan_candidates, jar_scan_inputs,
        library_paths_from_config, list_viewpoint_shaderpacks_at, list_viewpoint_shots_at,
        load_app_settings_at, merge_lua_mod_entry, mod_inner_event, mod_jar_path,
        mod_loader_policy_agent, mod_marker_event, mod_poster_path, mod_progress_units,
        normalize_blur_strength, normalize_font_contrast, normalize_font_family,
        normalize_font_scale, normalize_mod_loader_scheme, normalize_translation_language,
        normalize_window_height, normalize_window_width, order_mod_ids_by_requires,
        normalize_overlay_background_color,
        parse_default_mod_ids, parse_java_mod_info, parse_lua_mod_info, parse_lua_sorting_rules,
        patch_batch_content, patch_batch_content_with_memory, patch_batch_with_agent,
        patch_json_launcher_content, patch_json_launcher_content_with_memory,
        patch_json_launcher_with_agent, patch_steam_launch_options_content,
        patch_steam_launch_options_content_with_memory, pick_lua_mod_id, plan_lua_mod_enable,
        pz_optimization_status, read_default_mod_ids, read_fast_launch_cache_at,
        read_jar_mod_state, read_viewpoint_shot_at, restore_fast_launch_backup_at,
        save_app_settings_at, scan_jar_mods_at_with_callback, set_jar_mod_info_enabled,
        should_write_fast_launch_cache, steam_config_paths, steam_executable_from_roots,
        steam_profile_from_loginusers, steam_user_data_id, strip_loader_agent_tokens,
        validate_memory_limit_mb, vdf_value, version_step_for, viewpoint_three_d_status,
        workshop_path_from_library,
        write_default_mod_ids, write_fast_launch_cache_at, write_jar_mod_state,
        zombie_buddy_source_from_libraries, zombie_buddy_status, AppSettings, FastLaunchCache,
        JarModStateEntry, JarModStateFile, JavaPatchMethod, LuaModEntry, ScannedJarMod,
        SteamProfile, FAST_LAUNCH_BACKUP_MANIFEST, FAST_LAUNCH_BACKUP_VERSION,
        FAST_LAUNCH_CACHE_FILE, FAST_LAUNCH_CACHE_FORMAT, PROJECT_ZOMBOID_APP_ID,
        VIEWPOINT_MOD_ID, ZOMBIE_BUDDY_AGENT, ZOMBIE_BUDDY_MOD_ID,
        ZOMBIE_BUDDY_POLICY_AGENT,
    };
    use image::{GenericImageView, RgbaImage};
    use std::{fs, io::Write, path::Path};
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn parses_java_mod_info_fields_case_insensitively() {
        let mod_info = parse_java_mod_info(
            "name=Wind Sway\nid=WindSway\nJavaJarFile=media/java/client/windsway.jar\nJavaPkgName=pzmod.windsway\n",
        )
        .unwrap();

        assert_eq!(mod_info.name, "Wind Sway");
        assert_eq!(mod_info.id, "WindSway");
        assert_eq!(mod_info.jar_file, "media/java/client/windsway.jar");
        assert_eq!(mod_info.package_name, "pzmod.windsway");
    }

    #[test]
    fn ignores_mod_info_without_a_jar_or_package() {
        assert!(parse_java_mod_info("name=Lua only mod\nid=lua_mod\n").is_none());
        assert!(parse_java_mod_info("javaJarFile=mod.jar\n").is_none());
    }

    #[test]
    fn detects_pz_optimization_workshop_and_installed_versions() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        let classes = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("3805285544")
            .join("mods")
            .join("PZ_Optimization")
            .join("42")
            .join("pzopt-classes");
        fs::create_dir_all(classes.join("pzopt")).unwrap();
        fs::write(classes.join("pzopt-files.txt"), "pzopt/Test.class\n").unwrap();
        fs::write(classes.join("pzopt").join("Test.class"), b"class").unwrap();
        fs::write(
            classes.join("pzopt").join("build-info.properties"),
            "revision=abc1234567\nbuilt=42\n",
        )
        .unwrap();
        fs::write(classes.parent().unwrap().join("install.ps1"), "# fixture\n").unwrap();

        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        let libraries = [library];
        let available = pz_optimization_status(Some(&game), &libraries);
        assert!(available.available);
        assert!(!available.installed);
        assert_eq!(available.available_revision.as_deref(), Some("abc1234567"));

        fs::write(game.join("pzopt-installed.txt"), "# installed\n").unwrap();
        fs::create_dir_all(game.join("pzopt")).unwrap();
        fs::write(
            game.join("pzopt").join("build-info.properties"),
            "revision=abc1234567\n",
        )
        .unwrap();
        let installed = pz_optimization_status(Some(&game), &libraries);
        assert!(installed.installed);
        assert_eq!(installed.installed_revision.as_deref(), Some("abc1234567"));
    }

    #[test]
    fn offers_force_install_when_workshop_classes_match_no_game_revision() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        let version_root = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("3805285544")
            .join("mods")
            .join("PZ_Optimization")
            .join("42");
        let classes = version_root.join("pzopt-classes");
        fs::create_dir_all(classes.join("pzopt")).unwrap();
        fs::write(classes.join("pzopt-files.txt"), "pzopt/Test.class\n").unwrap();
        fs::write(classes.join("pzopt").join("Test.class"), b"class").unwrap();
        fs::write(
            classes.join("pzopt").join("build-info.properties"),
            "revision=4a0e9546ec\nbuilt=42\n",
        )
        .unwrap();
        fs::write(version_root.join("install.ps1"), "# fixture\n").unwrap();

        // 模拟 b42.22：游戏 revision 与 Workshop 里的构建不一致。
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        let jar = fs::File::create(game.join("projectzomboid.jar")).unwrap();
        let mut zip = ZipWriter::new(jar);
        zip.start_file("zombie/GitVersion.class", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"revision e013dbb529").unwrap();
        zip.finish().unwrap();

        let libraries = [library];
        let status = pz_optimization_status(Some(&game), &libraries);
        assert!(!status.available);
        assert!(status.force_install_available);
        assert!(status.installer_path.is_some());
        assert_eq!(status.source_path, None);
    }

    #[test]
    fn persists_background_and_memory_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let settings = AppSettings {
            background_path: Some("C:\\background.png".to_string()),
            shader_download_url: None,
            shader_download_directory: None,
            shader_download_threads: None,
            memory_limit_mb: Some(8192),
            translation_language: Some("ja".to_string()),
            font_contrast: Some(45),
            blur_strength: Some(120),
            font_family: Some("\"Microsoft YaHei UI\", sans-serif".to_string()),
            font_scale: Some(110),
            window_width: Some(970),
            window_height: Some(600),
            overlay_background_color: Some("#1E90FF".to_string()),
            mod_loader_scheme: Some("none".to_string()),
            fast_launch: false,
            developer_mode: true,
            steam_account_hidden: true,
            silent_memory_cleanup: true,
            silent_memory_cleanup_threshold: 85,
            clean_memory_on_launch: true,
            wallpaper_effect: None,
            steam_relay_mode: Some("relay".to_string()),
            steam_relay_base: Some("https://mirror.example/steam".to_string()),
            steam_proxy: Some("socks5://127.0.0.1:1080".to_string()),
            version_store_path: None,
            version_parallel: true,
            version_include_mods: true,
            version_system_drive_notice_dismissed: false,
        };

        save_app_settings_at(&path, &settings).unwrap();

        assert_eq!(load_app_settings_at(&path).unwrap(), settings);
    }

    #[test]
    fn mod_step_progress_is_weighted_per_mod() {
        // 第 2 个模组（index 1）、共 3 个：起点是整条进度的 1/3。
        let marker = mod_marker_event(1, 3, "Beta");
        assert_eq!(marker.phase, "mods");
        assert_eq!((marker.processed_bytes, marker.total_bytes), (1000, 3000));
        assert_eq!(marker.current_path.as_deref(), Some("Beta（2/3）"));

        // 模组内部 50% → 整条进度的 1.5/3，而不是 50%。
        let inner = mod_inner_event(
            1,
            3,
            "Beta",
            crate::versioning::ProgressEvent {
                phase: "copying".to_string(),
                processed_files: 5,
                total_files: 10,
                processed_bytes: 50,
                total_bytes: 100,
                current_path: Some("file.bin".to_string()),
            },
        );
        assert_eq!(inner.phase, "mods");
        assert_eq!((inner.processed_bytes, inner.total_bytes), (1500, 3000));
        assert_eq!(inner.current_path.as_deref(), Some("Beta（2/3）"));

        // 内层进度被夹在 0–1，不会跳回上一个模组、也不会越过下一个。
        let overshoot = mod_inner_event(
            1,
            3,
            "Beta",
            crate::versioning::ProgressEvent {
                phase: "copying".to_string(),
                processed_files: 99,
                total_files: 10,
                processed_bytes: 999,
                total_bytes: 100,
                current_path: None,
            },
        );
        assert_eq!(overshoot.processed_bytes, 2000);
        assert_eq!(mod_progress_units(3, 0.0, 3), (3000, 3000));
    }

    #[test]
    fn maps_version_phases_to_operation_steps() {
        // 游戏大版本：计算哈希 → 复制/保存变化 → 保存模组版本
        assert_eq!(version_step_for("snapshot", "preparing"), 1);
        assert_eq!(version_step_for("snapshot", "hashing"), 1);
        assert_eq!(version_step_for("snapshot", "copying"), 2);
        assert_eq!(version_step_for("snapshot", "mods"), 3);
        assert_eq!(version_step_for("capture", "delta"), 2);
        // 还原：检查 → 还原（含 PZ Optimization）→ 清理 → 还原模组版本
        assert_eq!(version_step_for("restore", "hashing"), 1);
        assert_eq!(version_step_for("restore", "pzopt"), 2);
        assert_eq!(version_step_for("restore", "restoring"), 2);
        assert_eq!(version_step_for("restore", "cleaning"), 3);
        assert_eq!(version_step_for("restore", "mods"), 4);
        // 单个模组
        assert_eq!(version_step_for("modSnapshot", "copying"), 2);
        assert_eq!(version_step_for("modCapture", "delta"), 2);
        assert_eq!(version_step_for("modRestore", "cleaning"), 3);
        assert_eq!(version_step_for("verify", "hashing"), 1);
        // 结束态不推进步骤
        assert_eq!(version_step_for("capture", "done"), 0);
        assert_eq!(version_step_for("capture", "error"), 0);
    }

    #[test]
    fn loads_legacy_settings_without_developer_mode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        std::fs::write(&path, r#"{"backgroundPath":null,"memoryLimitMb":8192}"#).unwrap();

        let settings = load_app_settings_at(&path).unwrap();

        assert!(!settings.developer_mode);
        assert!(!settings.steam_account_hidden);
        assert_eq!(settings.memory_limit_mb, Some(8192));
        assert_eq!(settings.translation_language, None);
        assert_eq!(settings.font_contrast, None);
        assert_eq!(settings.blur_strength, None);
        assert_eq!(settings.mod_loader_scheme, None);
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.font_scale, None);
        assert_eq!(settings.window_width, None);
        assert_eq!(settings.window_height, None);
        assert_eq!(settings.overlay_background_color, None);
    }

    /// 覆盖层背景色：接受 #RGB / #RRGGBB，统一成大写 #RRGGBB；坏值按"没设置过"处理。
    #[test]
    fn normalizes_overlay_background_colors() {
        assert_eq!(
            normalize_overlay_background_color("#1e90ff"),
            Some("#1E90FF".to_string())
        );
        assert_eq!(
            normalize_overlay_background_color("ff0"),
            Some("#FFFF00".to_string())
        );
        assert_eq!(
            normalize_overlay_background_color("  #AABBCC  "),
            Some("#AABBCC".to_string())
        );
        // 坏值不能变成黑色：返回 None，覆盖层继续用默认纯红。
        assert_eq!(normalize_overlay_background_color("#GGGGGG"), None);
        assert_eq!(normalize_overlay_background_color("#12345"), None);
        assert_eq!(normalize_overlay_background_color(""), None);
        assert_eq!(
            normalize_overlay_background_color("red"),
            None,
            "颜色名不支持，只接受十六进制"
        );
    }

    /// 界面字号/字体与窗口尺寸的归一化：坏值要么夹到范围内，要么按"没设置过"处理。
    #[test]
    fn clamps_font_scale_and_validates_window_size() {
        use super::{FONT_SCALE_MAX, FONT_SCALE_MIN, WINDOW_WIDTH_MIN};

        assert_eq!(normalize_font_scale(0), FONT_SCALE_MIN);
        assert_eq!(normalize_font_scale(100), 100);
        assert_eq!(normalize_font_scale(255), FONT_SCALE_MAX);
        // 空字体 = 默认字体栈；前后空白要裁掉。
        assert_eq!(normalize_font_family("   "), None);
        assert_eq!(
            normalize_font_family("  Consolas, monospace "),
            Some("Consolas, monospace".to_string())
        );
        // 窗口尺寸低于下限 / 超过防御上限都不落盘。
        assert_eq!(normalize_window_width(WINDOW_WIDTH_MIN - 1), None);
        assert_eq!(normalize_window_width(WINDOW_WIDTH_MIN), Some(WINDOW_WIDTH_MIN));
        assert_eq!(normalize_window_height(560), Some(560));
        assert_eq!(normalize_window_height(559), None);
        assert_eq!(normalize_window_width(10001), None);
    }

    fn scanned_jar(id: &str, decision: Option<bool>, disabled: bool) -> ScannedJarMod {
        ScannedJarMod {
            id: id.to_string(),
            info_id: id.to_string(),
            mod_info_path: format!("H:/mods/{id}/mod.info"),
            mod_root: format!("H:/mods/{id}"),
            disabled,
            workshop_id: None,
            jar_path: format!("H:/mods/{id}/{id}.jar"),
            poster_path: None,
            package_name: id.to_lowercase(),
            jar_hash: format!("hash-{id}"),
            // 真实扫描产出的是**内部名**（斜杠分隔）——测试也照这个来，
            // 否则"缓存里是内部名、注入器却当二进制名用"这类 bug 测不出来。
            patch_classes: vec![format!("{}/Patch_A", id.to_lowercase())],
            patch_methods: Vec::new(),
            lua_classes: vec![format!("{}/Lua_A", id.to_lowercase())],
            decision,
        }
    }

    #[test]
    fn builds_fast_launch_cache_from_approved_and_enabled_mods_only() {
        let scanned = vec![
            scanned_jar("Viewpoint", Some(true), false),
            scanned_jar("Pending", None, false),
            scanned_jar("Rejected", Some(false), false),
            scanned_jar("DisabledApproved", Some(true), true),
        ];
        let cache = build_fast_launch_cache(&scanned, 42);
        assert_eq!(cache.format, FAST_LAUNCH_CACHE_FORMAT);
        assert_eq!(cache.generated_at, 42);
        // 未批准 / 已拒绝 / 已禁用的都不能进缓存：缓存和启用列表不一致会丢补丁。
        assert_eq!(cache.mods.len(), 1);
        assert_eq!(cache.mods[0].mod_name, "Viewpoint");
        // 扫描给的是内部名（viewpoint/Patch_A），缓存里必须落成二进制名（注入器要用 forName）。
        assert_eq!(
            cache.mods[0].patch_classes,
            vec!["viewpoint.Patch_A".to_string()]
        );
        assert_eq!(
            cache.mods[0].lua_classes,
            vec!["viewpoint.Lua_A".to_string()]
        );

        // 指纹只跟「启用列表 + jar 内容」有关：换个生成时间不影响。
        assert_eq!(
            build_fast_launch_cache(&scanned, 99).mod_set_hash,
            cache.mod_set_hash
        );
        // jar 内容变了（hash 变）指纹必须跟着变，注入器才能发现缓存过期。
        let mut changed = scanned.clone();
        changed[0].jar_hash = "hash-changed".to_string();
        assert_ne!(
            build_fast_launch_cache(&changed, 42).mod_set_hash,
            cache.mod_set_hash
        );
    }

    #[test]
    fn fast_launch_cache_consistency_check() {
        let temp = tempfile::tempdir().unwrap();
        let approved = vec![
            scanned_jar("Viewpoint", Some(true), false),
            scanned_jar("Rejected", Some(false), false),
        ];
        let cache = build_fast_launch_cache(&approved, 11);
        let path = write_fast_launch_cache_at(temp.path(), &cache).unwrap();

        // 读回来的缓存必须依然判定为"一致"。
        let restored = read_fast_launch_cache_at(&path).unwrap();
        assert!(fast_launch_cache_matches(&restored, &approved));

        // 启用列表变了（多批准一个）→ 指纹变 → 必须判定不一致，注入器要回退原生扫描。
        let mut changed = approved.clone();
        changed[1].decision = Some(true);
        assert!(!fast_launch_cache_matches(&restored, &changed));

        // jar 内容变了（hash 变）也要判定不一致。
        let mut rehashed = approved.clone();
        rehashed[0].jar_hash = "hash-other".to_string();
        assert!(!fast_launch_cache_matches(&restored, &rehashed));
    }

    #[test]
    fn writes_fast_launch_cache_file() {
        let temp = tempfile::tempdir().unwrap();
        let scanned = vec![scanned_jar("Viewpoint", Some(true), false)];
        let cache = build_fast_launch_cache(&scanned, 7);

        let path = write_fast_launch_cache_at(temp.path(), &cache).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            FAST_LAUNCH_CACHE_FILE
        );

        // 落盘内容必须能原样读回：注入器就是读这个文件拿模组清单和指纹。
        let raw = fs::read_to_string(&path).unwrap();
        let restored: FastLaunchCache = serde_json::from_str(&raw).unwrap();
        assert_eq!(restored, cache);
        assert_eq!(restored.mods.len(), 1);
        assert_eq!(restored.mods[0].mod_name, "Viewpoint");
    }

    #[test]
    fn normalizes_mod_loader_scheme_setting() {
        assert_eq!(
            normalize_mod_loader_scheme("zombieBuddy").as_deref(),
            Some("zombieBuddy")
        );
        assert_eq!(
            normalize_mod_loader_scheme(" ZOMBIEBUDDY ").as_deref(),
            Some("zombieBuddy")
        );
        assert_eq!(normalize_mod_loader_scheme("none").as_deref(), Some("none"));
        assert_eq!(normalize_mod_loader_scheme("vfe"), None);
        assert_eq!(
            mod_loader_policy_agent("zombieBuddy"),
            Some(ZOMBIE_BUDDY_POLICY_AGENT)
        );
        assert_eq!(mod_loader_policy_agent("none"), None);
    }

    #[test]
    fn strips_loader_agent_from_launch_args_without_loader() {
        // 前端默认启动参数带 -agentlib:zbNative；方案不使用加载器时要摘掉，别偷偷挂上。
        let mut args = vec![ZOMBIE_BUDDY_AGENT.to_string(), "--".to_string()];
        assert!(strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["--".to_string()]);

        // 带策略的写法、以及夹杂空白的写法都算加载器 agent。
        let mut args = vec![
            " -agentlib:zbNative=policy=deny-new ".to_string(),
            "-Xmx4096m".to_string(),
        ];
        assert!(strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["-Xmx4096m".to_string()]);

        // 没有加载器参数时原样返回。
        let mut args = vec!["--".to_string()];
        assert!(!strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["--".to_string()]);
    }

    #[test]
    fn skips_loader_agent_when_scheme_has_no_loader() {
        // 方案选了"不使用加载器"：只改内存，绝不能往启动文件里塞 agent。
        let json = r#"{"vmArgs":["-Xmx3072m","-Dfoo=bar"]}"#;
        let (patched_json, changed) =
            patch_json_launcher_with_agent(json, Some(8192), None, None).unwrap();
        assert!(changed);
        assert!(!patched_json.contains("zbNative"));
        assert!(patched_json.contains("-Xmx8192m"));

        let batch = "SET _JAVA_OPTIONS=-Xmx3072m\r\n";
        let (patched_batch, changed) = patch_batch_with_agent(batch, Some(8192), None, None);
        assert!(changed);
        assert!(!patched_batch.contains("zbNative"));
        assert!(patched_batch.contains("-Xmx8192m"));

        // 启用加载器时照旧写入并归一化 agent。
        let (with_agent, _) =
            patch_json_launcher_with_agent(json, Some(8192), Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
                .unwrap();
        assert!(with_agent.contains(ZOMBIE_BUDDY_POLICY_AGENT));
    }

    #[test]
    fn clamps_font_contrast_setting() {
        assert_eq!(normalize_font_contrast(0), 0);
        assert_eq!(normalize_font_contrast(45), 45);
        assert_eq!(normalize_font_contrast(100), 100);
        assert_eq!(normalize_font_contrast(255), 100);
    }

    #[test]
    fn clamps_blur_strength_setting() {
        assert_eq!(normalize_blur_strength(0), 0);
        assert_eq!(normalize_blur_strength(120), 120);
        assert_eq!(normalize_blur_strength(200), 200);
        assert_eq!(normalize_blur_strength(255), 200);
    }

    #[test]
    fn recognizes_builtin_background_paths() {
        assert!(is_builtin_background_path("builtin:default"));
        assert!(is_builtin_background_path(" builtin:default "));
        assert!(!is_builtin_background_path(r"C:\Users\me\Pictures\a.png"));
        assert!(!is_builtin_background_path(""));
    }

    #[test]
    fn normalizes_translation_language_setting() {
        assert_eq!(
            normalize_translation_language("zh-CN").as_deref(),
            Some("zh-CN")
        );
        assert_eq!(
            normalize_translation_language("zh-cn").as_deref(),
            Some("zh-CN")
        );
        assert_eq!(
            normalize_translation_language(" zh-tw ").as_deref(),
            Some("zh-TW")
        );
        assert_eq!(normalize_translation_language("RU").as_deref(), Some("ru"));
        // 设置页没提供的语言和空值不能被悄悄当成 English，交给默认值处理。
        assert_eq!(normalize_translation_language("de"), None);
        assert_eq!(normalize_translation_language(""), None);
    }

    #[test]
    fn composes_four_title_images_in_quadrants() {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("title.png"),
            temp.path().join("title2.png"),
            temp.path().join("title3.png"),
            temp.path().join("title4.png"),
        ];
        for (index, path) in paths.iter().enumerate() {
            let image = RgbaImage::from_pixel(2, 3, image::Rgba([index as u8, 0, 0, 255]));
            image.save(path).unwrap();
        }
        let output = temp.path().join("joined.png");

        compose_title_background_at([&paths[0], &paths[1], &paths[2], &paths[3]], &output).unwrap();

        let joined = image::open(output).unwrap();
        assert_eq!(joined.dimensions(), (4, 6));
        assert_eq!(joined.get_pixel(0, 0)[0], 0);
        assert_eq!(joined.get_pixel(2, 0)[0], 1);
        assert_eq!(joined.get_pixel(0, 3)[0], 2);
        assert_eq!(joined.get_pixel(2, 3)[0], 3);
    }

    #[test]
    fn resolves_common_mod_info_jar_from_a_version_directory() {
        let temp = tempfile::tempdir().unwrap();
        let mod_root = temp.path().join("mods").join("Viewpoint");
        let common = mod_root.join("common");
        let versioned_jar = mod_root.join("42").join("media/java/client/Viewpoint.jar");
        fs::create_dir_all(&common).unwrap();
        fs::create_dir_all(versioned_jar.parent().unwrap()).unwrap();
        fs::write(&versioned_jar, b"jar").unwrap();
        let info = parse_java_mod_info(
            "name=Viewpoint\nid=Viewpoint\njavaJarFile=media/java/client/Viewpoint.jar\njavaPkgName=viewpoint\n",
        )
        .unwrap();
        let info_path = common.join("mod.info");

        assert_eq!(mod_jar_path(&info_path, &info), Some(versioned_jar));
    }

    #[test]
    fn resolves_mod_info_poster_from_its_own_directory() {
        let temp = tempfile::tempdir().unwrap();
        let info_path = temp.path().join("42").join("mod.info");
        let poster_path = info_path.parent().unwrap().join("preview.png");
        fs::create_dir_all(info_path.parent().unwrap()).unwrap();
        fs::write(&poster_path, b"image").unwrap();
        let info = parse_java_mod_info(
            "name=Example Mod\nid=Example\njavaJarFile=libs/example.jar\njavaPkgName=example\nposter=preview.png\n",
        )
        .unwrap();

        assert_eq!(info.poster.as_deref(), Some("preview.png"));
        assert_eq!(mod_poster_path(&info_path, &info), Some(poster_path));
    }

    #[test]
    fn excludes_zombie_buddy_loader_from_jar_mod_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let loader_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy")
            .join("41");
        let regular_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456789")
            .join("mods")
            .join("Example")
            .join("42");

        for (root, jar_name) in [
            (&loader_root, "ZombieBuddy.jar"),
            (&regular_root, "Example.jar"),
        ] {
            fs::create_dir_all(root.join("libs")).unwrap();
            fs::write(
                root.join("mod.info"),
                format!(
                    "name={jar_name}\nid={jar_name}\njavaJarFile=libs/{jar_name}\njavaPkgName=example\n"
                ),
            )
            .unwrap();
            fs::write(root.join("libs").join(jar_name), b"jar").unwrap();
        }

        let candidates = jar_scan_candidates(&[temp.path().to_path_buf()]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, Some(123456789));
    }

    /// 极速启动缓存必须同时含 ZombieBuddy 加载器本体：它自己的补丁类在**子包**
    /// `me.zed_0xff.zombie_buddy.patches` 里，而 mod.info 声明的包是父包。
    /// 漏掉它的后果就是游戏里弹"ZombieBuddy 未安装"（强制模式下每个包都查不到）。
    #[test]
    fn includes_zombie_buddy_loader_and_subpackages_in_fast_launch_cache() {
        let temp = tempfile::tempdir().unwrap();
        let loader_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy")
            .join("42");
        let regular_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456789")
            .join("mods")
            .join("Example")
            .join("42");

        for (root, jar_name, package_name, sub) in [
            (
                &loader_root,
                "ZombieBuddy.jar",
                "me.zed_0xff.zombie_buddy",
                "patches/Patch_Core",
            ),
            (
                &regular_root,
                "Example.jar",
                "example.patch",
                "Patch_Example",
            ),
        ] {
            fs::create_dir_all(root.join("libs")).unwrap();
            fs::write(
                root.join("mod.info"),
                format!(
                    "name={jar_name}
id={jar_name}
javaJarFile=libs/{jar_name}
javaPkgName={package_name}
"
                ),
            )
            .unwrap();
            let prefix = package_name.replace('.', "/");
            let file = fs::File::create(root.join("libs").join(jar_name)).unwrap();
            let mut archive = ZipWriter::new(file);
            archive
                .start_file(
                    format!("{prefix}/{sub}.class"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
            archive
                .write_all(&annotated_class_bytes(
                    &format!("{prefix}/{sub}"),
                    "Lme/zed_0xff/zombie_buddy/Patch;",
                ))
                .unwrap();
            archive.finish().unwrap();
        }

        // 界面上的「JAR 模组」列表仍然排除加载器本体。
        let candidates = jar_scan_candidates(&[temp.path().to_path_buf()]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, Some(123456789));

        // 缓存清单：普通模组 + 加载器本体，且子包里的类也要收到、落成二进制名。
        let mods = fast_launch_cache_mods_at(&[temp.path().to_path_buf()]);
        assert_eq!(mods.len(), 2);
        let loader = mods
            .iter()
            .find(|entry| entry.package_name == "me.zed_0xff.zombie_buddy")
            .expect("缓存必须包含 ZombieBuddy 加载器本体");
        assert_eq!(
            loader.patch_classes,
            vec!["me.zed_0xff.zombie_buddy.patches.Patch_Core"]
        );

        let cache = build_fast_launch_cache(&mods, 1);
        assert!(cache.mods.iter().any(|entry| entry
            .patch_classes
            .iter()
            .any(|class| class == "me.zed_0xff.zombie_buddy.patches.Patch_Core")));
    }

    #[test]
    fn detects_zombie_buddy_patch_annotation_in_classfile() {
        let class_bytes =
            annotated_class_bytes("viewpoint/Patch_Foo", "Lme/zed_0xff/zombie_buddy/Patch;");

        let annotations = class_annotations(&class_bytes).unwrap();

        assert_eq!(annotations.class_name, "viewpoint.Patch_Foo");
        assert!(annotations.patch);
        assert!(!annotations.lua_class);
    }

    #[test]
    fn detects_concrete_patch_method_annotations() {
        let overwrite = class_annotations(&annotated_patch_method_class_bytes(
            "example/patch/Patch_Example",
            "apply",
            "Lorg/spongepowered/asm/mixin/Overwrite;",
        ))
        .unwrap();
        let inject = class_annotations(&annotated_patch_method_class_bytes(
            "example/patch/Patch_Example",
            "enter",
            "Lme/zed_0xff/zombie_buddy/Patch$OnEnter;",
        ))
        .unwrap();

        assert_eq!(overwrite.patch_methods.len(), 1);
        assert_eq!(overwrite.patch_methods[0].annotation, "Overwrite");
        assert_eq!(overwrite.patch_methods[0].method_name, "apply");
        assert_eq!(inject.patch_methods.len(), 1);
        assert_eq!(inject.patch_methods[0].annotation, "Inject");
        assert_eq!(inject.patch_methods[0].method_name, "enter");
    }

    #[test]
    fn scans_annotated_jar_mod() {
        let temp = tempfile::tempdir().unwrap();
        let mod_directory = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456")
            .join("mods")
            .join("Example");
        let jar_path = mod_directory.join("42").join("libs").join("example.jar");
        fs::create_dir_all(jar_path.parent().unwrap()).unwrap();
        fs::write(
            mod_directory.join("42").join("mod.info"),
            "name=Example Mod\nid=Example\njavaJarFile=libs/example.jar\njavaPkgName=example.patch\nposter=preview.png\n",
        )
        .unwrap();
        fs::write(mod_directory.join("42").join("preview.png"), b"image").unwrap();
        let file = fs::File::create(&jar_path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file(
                "example/patch/Patch_Example.class",
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(&annotated_class_bytes(
                "example/patch/Patch_Example",
                "Lme/zed_0xff/zombie_buddy/Patch;",
            ))
            .unwrap();
        archive
            .start_file(
                "example/patch/MethodPatch.class",
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(&annotated_patch_method_class_bytes(
                "example/patch/MethodPatch",
                "apply",
                "Lorg/spongepowered/asm/mixin/Overwrite;",
            ))
            .unwrap();
        archive.finish().unwrap();
        let mut progress = Vec::new();
        let mods =
            scan_jar_mods_at_with_callback(&[temp.path().to_path_buf()], |scanned, total, item| {
                progress.push((scanned, total, item.is_some()))
            });

        assert_eq!(mods.len(), 1);
        assert_eq!(progress, vec![(1, 1, true)]);
        assert_eq!(mods[0].id, "Example Mod");
        assert_eq!(mods[0].workshop_id, Some(123456));
        assert_eq!(
            mods[0].poster_path,
            Some(
                mod_directory
                    .join("42")
                    .join("preview.png")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        assert_eq!(mods[0].patch_classes, vec!["example.patch.Patch_Example"]);
        assert_eq!(
            mods[0].patch_methods,
            vec![JavaPatchMethod {
                annotation: "Overwrite".to_string(),
                method_name: "apply".to_string(),
            }]
        );
        assert!(mods[0].lua_classes.is_empty());
        assert_eq!(mods[0].decision, Some(true));
    }

    fn annotated_class_bytes(class_name: &str, annotation: &str) -> Vec<u8> {
        fn utf8(bytes: &mut Vec<u8>, value: &str) {
            bytes.push(1);
            bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&52_u16.to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        utf8(&mut bytes, class_name);
        bytes.push(7);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        utf8(&mut bytes, "java/lang/Object");
        bytes.push(7);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        utf8(&mut bytes, "RuntimeVisibleAnnotations");
        utf8(&mut bytes, annotation);
        bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&4_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&5_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes
    }

    fn annotated_patch_method_class_bytes(
        class_name: &str,
        method_name: &str,
        method_annotation: &str,
    ) -> Vec<u8> {
        fn utf8(bytes: &mut Vec<u8>, value: &str) {
            bytes.push(1);
            bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&52_u16.to_be_bytes());
        bytes.extend_from_slice(&9_u16.to_be_bytes());
        utf8(&mut bytes, class_name);
        bytes.push(7);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        utf8(&mut bytes, "java/lang/Object");
        bytes.push(7);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        utf8(&mut bytes, "RuntimeVisibleAnnotations");
        utf8(&mut bytes, method_name);
        utf8(&mut bytes, "()V");
        utf8(&mut bytes, method_annotation);
        bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&4_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&0x0009_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u16.to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&5_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes
    }

    #[test]
    fn lists_only_supported_screenshot_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("first.PNG"), b"png").unwrap();
        fs::write(temp.path().join("notes.txt"), b"text").unwrap();
        fs::create_dir(temp.path().join("nested.jpg")).unwrap();

        let shots = list_viewpoint_shots_at(temp.path()).unwrap();

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].name, "first.PNG");
    }

    /// 诊断（默认忽略）：用启动器自己的 class 注解解析器扫描本机 workshop，
    /// 打印每个 Java 模组 jar 声明的注入方法(advice)数，并汇总未禁用模组的总数。
    ///
    /// 这个总数就是启动界面进度条的分母——运行期 `[ZB] patching … with N advice(s)` 的 ΣN
    /// 与之高度吻合（实测 614 vs 611）。
    ///
    /// 运行：`cargo test --lib stats_workshop_patch_methods -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stats_workshop_patch_methods() {
        let roots = jar_scan_inputs();
        if roots.is_empty() {
            println!("没有找到 workshop 目录，跳过。");
            return;
        }
        let jars: Vec<String> = scan_jar_mods_at_with_callback(&roots, |_, _, _| {})
            .into_iter()
            .map(|module| module.jar_path)
            .collect();

        let mut total_advice = 0;
        let mut total_patch_classes = 0;
        for path in &jars {
            let Ok(file) = fs::File::open(path) else {
                continue;
            };
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                continue;
            };
            let mut advice = 0;
            let mut patch_classes = 0;
            for index in 0..zip.len() {
                let Ok(mut entry) = zip.by_index(index) else {
                    continue;
                };
                if !entry.name().ends_with(".class") {
                    continue;
                }
                let mut bytes = Vec::new();
                if std::io::Read::read_to_end(&mut entry, &mut bytes).is_err() {
                    continue;
                }
                if let Some(annotations) = class_annotations(&bytes) {
                    if annotations.patch {
                        patch_classes += 1;
                    }
                    advice += annotations.patch_methods.len();
                }
            }
            println!(
                "{advice:>4} advice {patch_classes:>3} @Patch class  {}",
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            total_advice += advice;
            total_patch_classes += patch_classes;
        }
        println!(
            "TOTAL advice={total_advice}  @Patch classes={total_patch_classes}  jars={}",
            jars.len()
        );
    }

    #[test]
    fn finds_steam_executable_in_registered_roots() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path().join("Steam");
        fs::create_dir_all(&steam_root).unwrap();
        fs::write(steam_root.join("steam.exe"), b"steam").unwrap();

        assert_eq!(
            steam_executable_from_roots(std::slice::from_ref(&steam_root)),
            Some(steam_root.join("steam.exe"))
        );
    }

    #[test]
    fn reads_screenshot_bytes_without_allowing_directory_traversal() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("shot.png"), b"image-data").unwrap();
        fs::write(
            temp.path().parent().unwrap().join("outside.png"),
            b"outside",
        )
        .unwrap();

        assert_eq!(
            read_viewpoint_shot_at(temp.path(), "shot.png").unwrap(),
            b"image-data"
        );
        assert!(read_viewpoint_shot_at(temp.path(), "../outside.png").is_err());
    }

    #[test]
    fn imports_zip_without_overwriting_existing_shaderpacks() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("lighting.ZIP");
        let target = temp.path().join("shaderpacks");
        fs::write(&source, b"new-pack").unwrap();
        fs::create_dir_all(&target).unwrap();

        import_shaderpack_to(&source, &target).unwrap();

        assert_eq!(fs::read(target.join("lighting.ZIP")).unwrap(), b"new-pack");
        fs::write(&source, b"replacement").unwrap();
        assert!(import_shaderpack_to(&source, &target).is_err());
        assert_eq!(fs::read(target.join("lighting.ZIP")).unwrap(), b"new-pack");
        assert_eq!(list_viewpoint_shaderpacks_at(&target).unwrap().len(), 1);
    }

    #[test]
    fn rejects_non_zip_shaderpack_imports() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("readme.txt");
        fs::write(&source, b"not a zip").unwrap();

        assert!(import_shaderpack_to(&source, &temp.path().join("shaderpacks")).is_err());
    }

    #[test]
    fn deletes_only_shaderpacks_inside_the_managed_directory() {
        let temp = tempfile::tempdir().unwrap();
        let pack = temp.path().join("shaderpacks").join("pack.zip");
        fs::create_dir_all(pack.parent().unwrap()).unwrap();
        fs::write(&pack, b"zip").unwrap();

        delete_shaderpack_at(pack.parent().unwrap(), "pack.zip").unwrap();

        assert!(!pack.exists());
        assert!(delete_shaderpack_at(pack.parent().unwrap(), "../outside.zip").is_err());
        assert!(delete_shaderpack_at(pack.parent().unwrap(), "missing.zip").is_err());
    }

    #[test]
    fn parses_escaped_windows_paths() {
        let content = r#""path" "D:\\SteamLibrary"
"label" ""
"contentid" "123""#;

        assert_eq!(
            vdf_value(content, "path"),
            Some(r#"D:\SteamLibrary"#.to_string())
        );
    }

    #[test]
    fn finds_game_directory_from_library_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let manifest_dir = library.join("steamapps");
        let game_dir = library
            .join("steamapps")
            .join("common")
            .join("ProjectZomboid");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            manifest_dir.join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        assert_eq!(
            game_path_from_library(&library),
            Some(game_dir.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn returns_none_when_game_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let manifest_dir = library.join("steamapps");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        assert_eq!(game_path_from_library(&library), None);
    }

    #[test]
    fn reads_library_paths_from_config() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path().join("Steam");
        let second_library = temp.path().join("SecondaryLibrary");
        let config_dir = steam_root.join("steamapps");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&second_library).unwrap();
        let escaped_path = second_library.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            config_dir.join("libraryfolders.vdf"),
            format!(
                r#""libraryfolders"
{{
    "1"
    {{
        "path" "{escaped_path}"
    }}
}}"#
            ),
        )
        .unwrap();

        let paths = library_paths_from_config(&steam_root);

        assert!(paths.contains(&steam_root));
        assert!(paths.contains(&second_library));
    }

    #[test]
    fn finds_workshop_directory_for_project_zomboid() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop_path = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("108600");
        fs::create_dir_all(&workshop_path).unwrap();

        assert_eq!(
            workshop_path_from_library(&library),
            Some(workshop_path.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn returns_none_when_workshop_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        fs::create_dir_all(library.join("steamapps")).unwrap();

        assert_eq!(workshop_path_from_library(&library), None);
    }

    #[test]
    fn emits_each_workshop_background_as_it_is_found() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join("workshop/content/108600/123/mods/TestMod/backgrounds");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("background.png"), b"image").unwrap();

        let mut emitted = Vec::new();
        let backgrounds = super::workshop_backgrounds_at_with_callback(
            &[temp.path().join("workshop/content/108600")],
            |entry| emitted.push(entry.id.clone()),
        );

        assert_eq!(backgrounds.len(), 1);
        assert_eq!(emitted, vec![backgrounds[0].id.clone()]);
    }

    #[test]
    fn reads_most_recent_steam_profile() {
        let content = r#""users"
{
    "76561198000000001"
    {
        "AccountName" "older_account"
        "PersonaName" "Older Profile"
        "MostRecent" "0"
    }
    "76561198000000002"
    {
        "AccountName" "current_account"
        "PersonaName" "Current Profile"
        "MostRecent" "1"
    }
}"#;

        assert_eq!(
            steam_profile_from_loginusers(content),
            Some(SteamProfile {
                steam_id: "76561198000000002".to_string(),
                account_name: "current_account".to_string(),
                persona_name: "Current Profile".to_string(),
            })
        );
    }

    #[test]
    fn patches_json_launcher_with_allow_all_policy() {
        let content = r#"{
    "mainClass": "zombie.game",
    "vmArgs": ["-Xmx4G"]
}"#;

        let (patched, changed) = patch_json_launcher_content(content).unwrap();

        assert!(changed);
        assert!(patched.contains(r#""-agentlib:zbNative=policy=allow-all""#));
        assert!(patched.contains(r#""-Xmx4G""#));

        let (patched_again, changed_again) = patch_json_launcher_content(&patched).unwrap();
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn patches_batch_java_options_with_allow_all_policy() {
        let content = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        let (patched, changed) = patch_batch_content(content);

        assert!(changed);
        assert!(patched.contains("SET _JAVA_OPTIONS=-agentlib:zbNative=policy=allow-all -Xmx4G"));
        assert!(patched.contains("\r\n"));

        let (patched_again, changed_again) = patch_batch_content(&patched);
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn replaces_json_memory_limit_without_removing_zombie_buddy_agent() {
        let content = r#"{
    "vmArgs": ["-agentlib:zbNative", "-Xmx4G", "-Dexample=true"]
}"#;

        let (patched, changed) =
            patch_json_launcher_content_with_memory(content, Some(8192)).unwrap();

        assert!(changed);
        assert!(patched.contains(r#""-agentlib:zbNative=policy=allow-all""#));
        assert!(!patched.contains(r#""-agentlib:zbNative""#));
        assert!(patched.contains(r#""-Xmx8192m""#));
        assert!(!patched.contains(r#""-Xmx4G""#));
        assert!(patched.contains(r#""-Dexample=true""#));
    }

    #[test]
    fn replaces_batch_memory_limit_without_removing_zombie_buddy_agent() {
        let content =
            "SET _JAVA_OPTIONS=-agentlib:zbNative -Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        let (patched, changed) = patch_batch_content_with_memory(content, Some(16384));

        assert!(changed);
        assert!(
            patched.contains("SET _JAVA_OPTIONS=-agentlib:zbNative=policy=allow-all -Xmx16384m")
        );
        assert!(!patched.contains("-Xmx4G"));
        assert!(patched.contains("\r\n"));
    }

    #[test]
    fn forces_zombie_buddy_allow_all_policy_on_launch_files() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();

        // 模拟"默认 prompt"的游戏配置：JSON 与 BAT 都是不带 policy 的 agent。
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative","-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\nstart ProjectZomboid64.exe\r\n",
        )
        .unwrap();

        // 同时带上内存上限：JSON/BAT 里的 -Xmx 也要被改成启动器设置的值。
        assert!(force_zombie_buddy_policy_at_paths(&game, Some(12288)).unwrap());

        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        assert!(json.contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(!json.contains(r#""-agentlib:zbNative""#));
        assert!(
            json.contains(r#""-Xmx12288m""#),
            "JSON 里的 -Xmx 应被改写为启动器设置: {json}"
        );
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(batch.contains(&format!("SET _JAVA_OPTIONS={ZOMBIE_BUDDY_POLICY_AGENT}")));
        assert!(
            batch.contains("-Xmx12288m"),
            "BAT 里的 -Xmx 应被改写: {batch}"
        );
        assert!(game.join(".pzmanager-backups").is_dir());

        // 幂等：第二次不应再改动。
        assert!(!force_zombie_buddy_policy_at_paths(&game, Some(12288)).unwrap());
    }

    /// 从 `ProjectZomboid64.json` 文本里取出 `vmArgs`（JSON 里反斜杠是转义的，所以必须解析后再比）。
    fn json_vm_args(json: &str) -> Vec<String> {
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        parsed["vmArgs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn writes_and_strips_fast_launch_agent_token() {
        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        let expected = format!("-agentpath:{dll}");

        // 写入：`-agentpath:<DLL>` 进 vmArgs，且不动 ZombieBuddy agent 的归一化。
        let json = r#"{"vmArgs":["-Xmx4G","-agentlib:zbNative"]}"#;
        let (patched, changed) =
            patch_json_launcher_with_agent(json, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), Some(dll))
                .unwrap();
        assert!(changed);
        assert!(patched.contains(ZOMBIE_BUDDY_POLICY_AGENT));
        // **必须在最前**：JVMTI agent 按命令行顺序加载，我们要赶在 ZombieBuddy 自己的
        // `-agentlib:zbNative` 之前打开类加载 hook（否则 PatchEngine 早就定义完了）。
        assert_eq!(
            json_vm_args(&patched).first().map(String::as_str),
            Some(expected.as_str()),
            "`-agentpath:` 必须是 vmArgs[0]: {patched}"
        );

        // 幂等：同样的输入再来一次不该有变化。
        let (again, changed) = patch_json_launcher_with_agent(
            &patched,
            None,
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll),
        )
        .unwrap();
        assert!(!changed, "重复写入必须是幂等的: {again}");
        assert_eq!(again.matches("-agentpath:").count(), 1);

        // 重复条目会被收敛成一条（否则 DLL 会被加载两次、Agent_OnLoad 走两遍）。
        let duplicated = r#"{"vmArgs":["-Xmx4G","-agentpath:F:\\a.dll","-agentpath:F:\\a.dll"]}"#;
        let (deduped, changed) =
            patch_json_launcher_with_agent(duplicated, None, None, Some(r"F:\a.dll")).unwrap();
        assert!(changed);
        assert_eq!(deduped.matches("-agentpath:").count(), 1);

        // 关掉开关（None）= 必须把已有的 `-agentpath:` 全部摘掉：
        // 指向不存在的 DLL 会让 JVM 直接启动失败，所以"关开关"必须真的清干净。
        let (cleaned, changed) =
            patch_json_launcher_with_agent(&patched, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
                .unwrap();
        assert!(changed);
        assert!(
            !cleaned.contains("-agentpath"),
            "关掉开关后不能残留: {cleaned}"
        );

        // 路径变化时改写同一条，不新增。
        let (moved, changed) =
            patch_json_launcher_with_agent(&patched, None, None, Some(r"F:\other\pz_overlay.dll"))
                .unwrap();
        assert!(changed);
        assert_eq!(moved.matches("-agentpath:").count(), 1);
        assert!(json_vm_args(&moved).contains(&r"-agentpath:F:\other\pz_overlay.dll".to_string()));

        // 已经干净、且不需要写时不应报"改过"。
        let (_, changed) = patch_json_launcher_with_agent(json, None, None, None).unwrap();
        assert!(!changed);
    }

    #[test]
    fn skips_fast_launch_agent_in_batch_when_path_has_spaces() {
        let batch = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        // 无空格路径：写进 .bat 的 _JAVA_OPTIONS。
        let (patched, changed) = patch_batch_with_agent(
            batch,
            None,
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(r"F:\a\pz_overlay.dll"),
        );
        assert!(changed);
        assert!(patched.contains("-agentpath:F:\\a\\pz_overlay.dll"));

        // 含空格路径：不能写进 .bat（`_JAVA_OPTIONS` 按空白切分会被拆坏）。
        let (untouched, changed) =
            patch_batch_with_agent(batch, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), None);
        assert!(changed, "归一化 ZB agent 仍应生效");
        assert!(!untouched.contains("-agentpath"));

        // 已有 `-agentpath:` 时传 None 必须摘掉。
        let with_agent = "SET _JAVA_OPTIONS=-agentpath:F:\\a.dll -Xmx4G\r\n";
        let (stripped, changed) = patch_batch_with_agent(with_agent, None, None, None);
        assert!(changed);
        assert!(!stripped.contains("-agentpath"));
    }

    #[test]
    fn applies_fast_launch_agent_to_launch_files_with_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n",
        )
        .unwrap();

        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll)
        )
        .unwrap());
        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        assert!(
            json_vm_args(&json).contains(&format!("-agentpath:{dll}")),
            "{json}"
        );
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(batch.contains(&format!("-agentpath:{dll}")), "{batch}");
        assert!(game.join(".pzmanager-backups").is_dir());

        // 关掉极速启动（None）：两个文件里的 `-agentpath:` 都必须消失。
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            None
        )
        .unwrap());
        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(!json.contains("-agentpath"), "{json}");
        assert!(!batch.contains("-agentpath"), "{batch}");
    }

    #[test]
    fn restores_launcher_files_from_fast_launch_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        let json_original = r#"{"vmArgs":["-Xmx4G"]}"#;
        let bat_original = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";
        fs::write(game.join("ProjectZomboid64.json"), json_original).unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), bat_original).unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        let manifest = create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        assert_eq!(manifest.version, FAST_LAUNCH_BACKUP_VERSION);
        assert!(backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST).is_file());

        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll)
        )
        .unwrap());
        assert!(fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains("-agentpath"));

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap(),
            json_original
        );
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap(),
            bat_original
        );
        assert!(game.join(".pzmanager-backups").is_dir());
    }

    #[test]
    fn fast_launch_backup_removes_files_created_after_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("ProjectZomboid64.json"), r#"{"vmArgs":[]}"#).unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\n",
        )
        .unwrap();

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert!(!game.join("ProjectZomboid64.bat").is_file());
    }

    #[test]
    fn fast_launch_backup_strips_stale_agent_from_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentpath:F:\\old.dll","-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentpath:F:\\old.dll -Xmx4G\r\n",
        )
        .unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        assert!(
            !fs::read_to_string(backup_dir.join("ProjectZomboid64.json"))
                .unwrap()
                .contains("-agentpath")
        );
        assert!(!fs::read_to_string(backup_dir.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains("-agentpath"));

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert!(!fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains("-agentpath"));
        assert!(!fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains("-agentpath"));
    }

    #[test]
    fn keeps_fast_launch_cache_unless_explicitly_refreshed() {
        // 显式点「刷新 JAR 列表」→ 一定重建。
        assert!(should_write_fast_launch_cache(true, true));
        assert!(should_write_fast_launch_cache(true, false));
        // 自动扫描：已有缓存一律保持不动（缓存持久化），没有缓存才补一份。
        assert!(!should_write_fast_launch_cache(false, true));
        assert!(should_write_fast_launch_cache(false, false));
    }

    #[test]
    fn rejects_memory_limit_above_physical_memory() {
        assert!(validate_memory_limit_mb(16384, 8192).is_err());
        assert!(validate_memory_limit_mb(8192, 16384).is_ok());
    }

    #[test]
    fn patches_steam_launch_options_for_project_zomboid() {
        let content = r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                    "108600"
                    {
                        "LaunchOptions" "-novid"
                    }
                }
            }
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content(content).unwrap();

        assert!(changed);
        assert!(
            patched.contains(r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""#)
        );

        let (patched_again, changed_again) = patch_steam_launch_options_content(&patched).unwrap();
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn adds_required_separator_after_an_existing_agent() {
        let content = r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative -novid"
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content(content).unwrap();

        assert!(changed);
        assert!(
            patched.contains(r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""#)
        );
    }

    #[test]
    fn writes_memory_and_extra_arguments_to_steam_launch_options() {
        let content = r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-novid"
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content_with_memory(
            content,
            Some(8192),
            &["-debug".to_string()],
        )
        .unwrap();

        assert!(changed);
        assert!(patched.contains(
            r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -Xmx8192m -- -novid -debug""#
        ));

        let (patched_again, changed_again) =
            patch_steam_launch_options_content_with_memory(&patched, Some(16384), &[]).unwrap();
        assert!(changed_again);
        assert!(patched_again
            .contains(r#""-agentlib:zbNative=policy=allow-all -Xmx16384m -- -novid -debug""#));
        assert!(!patched_again.contains("-Xmx8192m"));
    }

    #[test]
    fn installs_both_targets_and_creates_a_backup() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp
            .path()
            .join("workshop")
            .join("mods")
            .join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp
            .path()
            .join("userdata")
            .join("123")
            .join("config")
            .join("localconfig.vdf");
        fs::create_dir_all(source.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(steam_config.parent().unwrap()).unwrap();

        fs::write(source.join("libs").join("zbNative.dll"), b"dll").unwrap();
        fs::write(source.join("libs").join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                }
            }
        }
    }
}"#,
        )
        .unwrap();

        let result =
            install_zombie_buddy_at_paths(&game, &source, &[steam_config.clone()]).unwrap();

        assert!(result.changed);
        assert!(game.join("zbNative.dll").is_file());
        assert!(game.join("ZombieBuddy.jar").is_file());
        assert!(fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(fs::read_to_string(&steam_config)
            .unwrap()
            .contains("-agentlib:zbNative=policy=allow-all --"));
        let backup_path = result.backup_path.unwrap();
        assert_eq!(
            fs::read_to_string(Path::new(&backup_path).join("ProjectZomboid64.json")).unwrap(),
            r#"{"vmArgs":["-Xmx4G"]}"#
        );
        assert_eq!(
            fs::read_to_string(Path::new(&backup_path).join("steam-localconfig-0.vdf")).unwrap(),
            r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                }
            }
        }
    }
}"#
        );
    }

    #[test]
    fn finds_game_and_workshop_source_in_separate_libraries() {
        let temp = tempfile::tempdir().unwrap();
        let game_library = temp.path().join("GameLibrary");
        let incomplete_library = temp.path().join("IncompleteWorkshopLibrary");
        let workshop_library = temp.path().join("WorkshopLibrary");
        let game = game_library
            .join("steamapps")
            .join("common")
            .join("ProjectZomboid");
        let workshop = workshop_library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("108600")
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy");
        fs::create_dir_all(
            incomplete_library
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(PROJECT_ZOMBOID_APP_ID)
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs"),
        )
        .unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(workshop.join("libs")).unwrap();
        fs::write(workshop.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(workshop.join("libs/ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game_library
                .join("steamapps")
                .join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        let source_libraries = [incomplete_library.clone(), workshop_library.clone()];
        assert_eq!(
            zombie_buddy_source_from_libraries(&source_libraries),
            Some(workshop.clone())
        );

        let locations =
            install_locations_from_libraries(&[game_library, incomplete_library, workshop_library])
                .unwrap();

        assert_eq!(locations.0, game.to_string_lossy().into_owned());
        assert_eq!(locations.1, workshop);
    }

    #[test]
    fn detects_existing_install_without_workshop_source() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("zbNative.dll"), b"dll").unwrap();
        fs::write(game.join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative --"
        }
    }
}"#,
        )
        .unwrap();

        let status = zombie_buddy_status(Some(&game), None, &[steam_config]);

        assert!(status.installed);
        assert!(!status.available);
        assert!(status.missing_components.is_empty());
    }

    #[test]
    fn selects_only_the_most_recent_steam_user_config() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path();
        let profile = SteamProfile {
            steam_id: "76561197960389184".to_string(),
            account_name: "current_account".to_string(),
            persona_name: "Current Profile".to_string(),
        };
        let current_config = steam_root
            .join("userdata")
            .join("123456")
            .join("config")
            .join("localconfig.vdf");
        let other_config = steam_root
            .join("userdata")
            .join("654321")
            .join("config")
            .join("localconfig.vdf");
        fs::create_dir_all(current_config.parent().unwrap()).unwrap();
        fs::create_dir_all(other_config.parent().unwrap()).unwrap();
        fs::write(&current_config, "").unwrap();
        fs::write(&other_config, "").unwrap();

        assert_eq!(
            steam_config_paths(steam_root, Some(&profile)),
            vec![current_config]
        );
        assert!(steam_config_paths(steam_root, None).is_empty());
    }

    #[test]
    fn rolls_back_earlier_writes_when_a_later_write_fails() {
        let temp = tempfile::tempdir().unwrap();
        let existing_file = temp.path().join("existing.txt");
        let missing_parent_file = temp.path().join("missing").join("later.txt");
        fs::write(&existing_file, b"original").unwrap();

        let result = apply_file_transaction(vec![
            (existing_file.clone(), b"updated".to_vec()),
            (missing_parent_file.clone(), b"later".to_vec()),
        ]);

        assert!(result.is_err());
        assert_eq!(fs::read(existing_file).unwrap(), b"original");
        assert!(!missing_parent_file.exists());
    }

    #[test]
    fn converts_steam_id_64_to_userdata_account_id() {
        let profile = SteamProfile {
            steam_id: "76561197960389184".to_string(),
            account_name: "test".to_string(),
            persona_name: "Test".to_string(),
        };

        assert_eq!(steam_user_data_id(&profile), Some("123456".to_string()));
    }

    #[test]
    fn invalid_steam_config_does_not_partially_install_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        fs::create_dir_all(source.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(source.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(source.join("libs/ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), "start game\r\n").unwrap();
        fs::write(&steam_config, "not a valid Steam VDF").unwrap();

        let result = install_zombie_buddy_at_paths(&game, &source, &[steam_config]);

        assert!(result.is_err());
        assert!(!game.join("zbNative.dll").exists());
        assert!(!game.join("ZombieBuddy.jar").exists());
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap(),
            r#"{"vmArgs":["-Xmx4G"]}"#
        );
    }

    #[test]
    fn installed_loader_is_reported_as_already_installed_without_source_files() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        let missing_source = temp.path().join("unsubscribed-workshop-mod");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("zbNative.dll"), b"dll").unwrap();
        fs::write(game.join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative --"
        }
    }
}"#,
        )
        .unwrap();

        let result =
            install_zombie_buddy_at_paths(&game, &missing_source, &[steam_config]).unwrap();

        assert!(result.already_installed);
        assert!(!result.changed);
        assert!(result.backup_path.is_none());
    }

    #[test]
    fn detects_viewpoint_mod_subscription() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID);

        fs::create_dir_all(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint")
                .join("mod.info"),
            "name=Project Viewpoint\n",
        )
        .unwrap();
        fs::create_dir_all(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs")
                .join("zbNative.dll"),
            b"dll",
        )
        .unwrap();
        fs::write(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs")
                .join("ZombieBuddy.jar"),
            b"jar",
        )
        .unwrap();

        let status = viewpoint_three_d_status(None, &[library]);

        assert!(status.ready);
        assert!(status.mods.iter().all(|item| item.subscribed));
        assert!(status.missing_components.is_empty());
    }

    #[test]
    fn configures_viewpoint_files_and_copies_loader_jar() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID);
        let zombie_buddy = workshop
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");

        fs::create_dir_all(zombie_buddy.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint")
                .join("mod.info"),
            "name=Project Viewpoint\n",
        )
        .unwrap();
        fs::write(zombie_buddy.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(zombie_buddy.join("libs/ZombieBuddy.jar"), b"loader").unwrap();
        fs::write(zombie_buddy.join("libs/extra-native.dll"), b"extra").unwrap();
        fs::write(game.join("ProjectZomboid64.json"), r#"{"vmArgs":[]}"#).unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), "start game\r\n").unwrap();

        let result = configure_viewpoint_three_d_at_paths(&game, &[library], &[]).unwrap();

        assert!(result.changed);
        assert_eq!(fs::read(game.join("zbNative.dll")).unwrap(), b"dll");
        assert_eq!(fs::read(game.join("extra-native.dll")).unwrap(), b"extra");
        // 不再用 Temporary Fix 覆盖：游戏目录的 jar 直接来自 ZombieBuddy 工坊自带的 libs。
        assert_eq!(fs::read(game.join("ZombieBuddy.jar")).unwrap(), b"loader");
        assert!(fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(result.backup_path.is_some());
    }

    #[test]
    fn rewrites_default_mod_block_and_keeps_other_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.txt");
        fs::write(
            &path,
            "-- keep this comment\r\nmods\r\n{\r\n    mod = \"Existing\",\r\n}\r\nother = true\r\n",
        )
        .unwrap();

        write_default_mod_ids(&path, &["Existing".to_string(), "NewMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content,
            "-- keep this comment\r\nmods\r\n{\r\n    mod = Existing,\r\n    mod = NewMod,\r\n}\r\nother = true\r\n"
        );
        // 关键回归：写回的 ID 不能带引号（PZ 会把引号当成 ID 的一部分而识别不到模组）。
        assert!(!content.contains("mod = \""));
        assert_eq!(parse_default_mod_ids(&content), ["Existing", "NewMod"]);
    }

    #[test]
    fn creates_default_mod_list_with_version_header() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.txt");

        write_default_mod_ids(&path, &["SomeMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("VERSION = 1,"));
        assert!(content.contains("mod = SomeMod,"));
        assert_eq!(parse_default_mod_ids(&content), ["SomeMod"]);
    }

    #[test]
    fn reads_empty_default_list_when_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.txt");

        assert!(read_default_mod_ids(&path).unwrap().is_empty());
    }

    #[test]
    fn creates_default_mod_block_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mods").join("default.txt");

        write_default_mod_ids(&path, &["FreshMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(parse_default_mod_ids(&content), ["FreshMod"]);
        assert!(content.contains("mod = FreshMod,"));
        assert!(!content.contains("mod = \""));
    }

    #[test]
    fn parses_required_mod_ids_from_mod_info() {
        let info = parse_lua_mod_info(
            "id=Addon\nname=Addon\nrequire=Core, \"Extras\"\nloadAfter=Framework\nloadBefore=Overhaul\nloadFirst=on\nloadLast=off\n",
        );

        assert_eq!(info.requires, ["Core", "Extras"]);
        assert_eq!(info.load_after, ["Framework"]);
        assert_eq!(info.load_before, ["Overhaul"]);
        assert!(info.load_first);
        assert!(!info.load_last);
    }

    #[test]
    fn parses_lua_sorting_rules() {
        let rules = parse_lua_sorting_rules(
            "[ZombieBuddy]\nloadBefore=ModLoadOrderSorter_b42\nloadFirst=on\n[Addon]\nloadAfter=Core\nloadLast=on\n",
        );

        assert_eq!(rules["zombiebuddy"].load_before, ["ModLoadOrderSorter_b42"]);
        assert!(rules["zombiebuddy"].load_first);
        assert_eq!(rules["addon"].load_after, ["Core"]);
        assert!(rules["addon"].load_last);
    }

    #[test]
    fn applies_build_42_and_mlos_load_rules() {
        let mut zombie_buddy = lua_mod("ZombieBuddy", "ZombieBuddy");
        zombie_buddy.load_first = true;
        zombie_buddy.load_before = vec!["ModLoadOrderSorter_b42".to_string()];
        let mut sorter = lua_mod("ModLoadOrderSorter_b42", "Sorter");
        sorter.load_after = vec!["ZombieBuddy".to_string()];
        let mut manager = lua_mod("ModManager", "Mod Manager");
        manager.requires = vec!["ModLoadOrderSorter_b42".to_string()];
        let entries = vec![lua_mod("Other", "Other"), manager, sorter, zombie_buddy];
        let ids = vec![
            "Other".to_string(),
            "ModManager".to_string(),
            "ModLoadOrderSorter_b42".to_string(),
            "ZombieBuddy".to_string(),
        ];

        assert_eq!(
            order_mod_ids_by_requires(&entries, &ids),
            [
                "ZombieBuddy",
                "ModLoadOrderSorter_b42",
                "ModManager",
                "Other"
            ]
        );
    }

    #[test]
    fn deduplicates_mod_ids_and_prefers_the_b42_entry() {
        let mut legacy = lua_mod("ModManager", "Mod Manager");
        legacy.path = "old/ModManager".to_string();
        legacy.has_b42 = false;
        legacy.requires = vec!["LegacyDependency".to_string()];
        let mut modern = lua_mod("modmanager", "[B42] Mod Manager");
        modern.path = "new/[B42] Mod Manager".to_string();
        modern.requires = vec!["ModLoadOrderSorter_b42".to_string()];

        merge_lua_mod_entry(&mut legacy, modern);

        assert_eq!(legacy.name, "[B42] Mod Manager");
        assert_eq!(legacy.path, "new/[B42] Mod Manager");
        assert!(legacy.has_b42);
        assert_eq!(legacy.requires, ["ModLoadOrderSorter_b42"]);
    }

    #[test]
    fn orders_prerequisites_before_dependents() {
        let core = lua_mod("Core", "Core");
        let mut addon = lua_mod("Addon", "Addon");
        addon.requires = vec!["Core".to_string()];
        let entries = vec![core, addon];

        let ordered = order_mod_ids_by_requires(
            &entries,
            &["Addon".to_string(), "Core".to_string(), "Other".to_string()],
        );

        assert_eq!(ordered, ["Core", "Addon", "Other"]);
    }

    #[test]
    fn plans_recursive_prerequisites_and_reports_missing() {
        let mut core = lua_mod("Core", "Core");
        core.requires = vec!["Base".to_string()];
        let mut addon = lua_mod("Addon", "Addon");
        addon.requires = vec!["Core".to_string()];
        let entries = vec![core, addon];

        let mut ids = vec!["Existing".to_string()];
        let mut added = Vec::new();
        let mut missing = Vec::new();
        let mut active = std::collections::HashSet::new();
        plan_lua_mod_enable(
            &entries,
            "Addon",
            &mut ids,
            &mut added,
            &mut missing,
            &mut active,
        );

        assert_eq!(added, ["Core", "Addon"]);
        assert_eq!(missing, ["Base"]);
        assert_eq!(
            order_mod_ids_by_requires(&entries, &ids),
            ["Existing", "Core", "Addon"]
        );
    }

    fn lua_mod(id: &str, name: &str) -> LuaModEntry {
        LuaModEntry {
            id: id.to_string(),
            name: name.to_string(),
            path: format!("mods/{name}"),
            source: "workshop".to_string(),
            workshop_id: Some("1".to_string()),
            enabled: false,
            load_order: None,
            requires: Vec::new(),
            load_after: Vec::new(),
            load_before: Vec::new(),
            load_first: false,
            load_last: false,
            has_common: false,
            has_b42: true,
            is_loader: false,
            poster_path: None,
        }
    }

    #[test]
    fn matches_lua_mod_by_info_id_before_name() {
        let entries = vec![lua_mod("BritaWeapons", "Brita's Weapon Pack")];

        assert_eq!(
            pick_lua_mod_id(&entries, Some("britaweapons"), "ignored"),
            Some("BritaWeapons".to_string())
        );
        assert_eq!(
            pick_lua_mod_id(&entries, None, "Brita's Weapon Pack"),
            Some("BritaWeapons".to_string())
        );
        assert_eq!(pick_lua_mod_id(&entries, Some("Missing"), "Missing"), None);
    }

    #[test]
    fn keeps_jar_mod_info_id_on_scanned_entries() {
        let info = parse_java_mod_info(
            "name=Brita's Weapon Pack\nid=BritaWeapons\njavajarfile=Brita.jar\njavapkgname=brita.core\n",
        )
        .unwrap();

        assert_eq!(info.name, "Brita's Weapon Pack");
        assert_eq!(info.id, "BritaWeapons");
    }

    #[test]
    fn disabling_jar_mod_overwrites_existing_disabled_file() {
        let temp = tempfile::tempdir().unwrap();
        let enabled_path = temp.path().join("mod.info");
        let disabled_path = temp.path().join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        let result = set_jar_mod_info_enabled(enabled_path.to_str().unwrap(), false).unwrap();

        assert_eq!(result, disabled_path);
        assert!(!enabled_path.exists());
        assert_eq!(fs::read(&disabled_path).unwrap(), b"id=Authoritative\n");
    }

    #[test]
    fn enabling_jar_mod_keeps_mod_info_as_the_authoritative_file() {
        let temp = tempfile::tempdir().unwrap();
        let enabled_path = temp.path().join("mod.info");
        let disabled_path = temp.path().join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        let result = set_jar_mod_info_enabled(enabled_path.to_str().unwrap(), true).unwrap();

        assert_eq!(result, enabled_path);
        assert!(!disabled_path.exists());
        assert_eq!(fs::read(&enabled_path).unwrap(), b"id=Authoritative\n");
    }

    #[test]
    fn jar_scan_prefers_mod_info_and_falls_back_to_disabled_file() {
        let temp = tempfile::tempdir().unwrap();
        let mod_root = temp.path().join("mods").join("Example");
        fs::create_dir_all(&mod_root).unwrap();
        let enabled_path = mod_root.join("mod.info");
        let disabled_path = mod_root.join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        assert_eq!(
            discover_mod_info_files(temp.path()),
            vec![enabled_path.clone()]
        );

        fs::remove_file(&enabled_path).unwrap();
        assert_eq!(discover_mod_info_files(temp.path()), vec![disabled_path]);
    }

    #[test]
    fn reapplies_user_state_after_mod_info_is_redownloaded() {
        let temp = tempfile::tempdir().unwrap();
        let mods_root = temp.path().join("mods");
        let mod_root = mods_root.join("Example");
        fs::create_dir_all(&mod_root).unwrap();
        fs::write(
            mod_root.join("mod.info"),
            "name=Example Mod\nid=Example\njavaJarFile=Example.jar\njavaPkgName=example.patch\n",
        )
        .unwrap();
        fs::write(mod_root.join("Example.jar"), b"jar").unwrap();
        let state = JarModStateFile {
            format_version: 1,
            mods: vec![JarModStateEntry {
                mod_dir: mod_root.to_string_lossy().into_owned(),
                mod_id: "Example".to_string(),
                workshop_id: None,
                package_name: "example.patch".to_string(),
                disabled: true,
            }],
        };

        apply_jar_mod_state_to_roots(std::slice::from_ref(&mods_root), &state).unwrap();
        assert!(!mod_root.join("mod.info").exists());
        assert!(mod_root.join("mod.info.disabled").exists());

        fs::write(
            mod_root.join("mod.info"),
            "name=Example Mod Updated\nid=Example\njavaJarFile=Example.jar\njavaPkgName=example.patch\n",
        )
        .unwrap();
        apply_jar_mod_state_to_roots(std::slice::from_ref(&mods_root), &state).unwrap();

        assert!(!mod_root.join("mod.info").exists());
        assert!(fs::read_to_string(mod_root.join("mod.info.disabled"))
            .unwrap()
            .contains("Example Mod Updated"));
    }

    #[test]
    fn persists_jar_mod_state_in_user_data_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jar-mod-state.json");
        let state = JarModStateFile {
            format_version: 1,
            mods: vec![JarModStateEntry {
                mod_dir: "C:\\mods\\Example".to_string(),
                mod_id: "Example".to_string(),
                workshop_id: Some(123),
                package_name: "example.patch".to_string(),
                disabled: true,
            }],
        };

        write_jar_mod_state(&path, &state).unwrap();

        let loaded = read_jar_mod_state(&path).unwrap();
        assert_eq!(loaded.mods.len(), 1);
        assert_eq!(loaded.mods[0].mod_id, "Example");
        assert!(loaded.mods[0].disabled);
    }
}


impl Default for ModPresetFile {
    fn default() -> Self {
        Self {
            format_version: 1,
            presets: Vec::new(),
        }
    }
}


#[tauri::command]
async fn fetch_developer_news(
    count: u32,
    language: String,
    only_gid: Option<String>,
) -> Result<news::DeveloperNewsResponse, String> {
    news::fetch_developer_news(count, language, only_gid).await
}

#[tauri::command]
async fn check_launcher_update() -> Result<update::LauncherUpdateStatus, String> {
    update::fetch_launcher_update(update::UPDATE_FEED_URL).await
}
