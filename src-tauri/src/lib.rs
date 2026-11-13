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
mod auto_join;
pub(crate) use auto_join::*;
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
mod mod_compat;
pub(crate) use mod_compat::*;
mod offline_workshop;
mod settings;
pub(crate) use settings::*;
mod game_options;
pub(crate) use game_options::*;
mod shaderpack_config;
pub(crate) use shaderpack_config::*;
mod steam;
pub(crate) use steam::*;
mod support;
pub(crate) use support::*;
mod task_store;
pub(crate) use task_store::*;
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
    // 走进程快照：`tasklist` 在某些机器上要 60–110 秒（2026-11-08 实测），
    // 而这里每次安装/检查都会被调用，必须是有界的。
    Ok(overlay::imp::image_name_running("steam.exe"))
}

#[cfg(not(windows))]
fn steam_is_running() -> Result<bool, String> {
    Err("ZombieBuddy 安装只支持 Windows".to_string())
}

/// 启动超时的熔断判定要用：Steam 进程此刻是否在跑（只做一次 tasklist，开销很小）。
/// 返回 bool 而不是 Result：查不到就按"没在跑"处理，前端只需要提示文案的差异。
#[tauri::command]
fn steam_process_running() -> bool {
    steam_is_running().unwrap_or(false)
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
    /// 「通用 › 翻译引擎」：auto = 默认链（Google → 必应 → MyMemory）；其余为手动指定的
    /// 优先引擎（bing / google / mymemory / relay），失败后仍按默认顺序回退其余引擎。
    #[serde(default)]
    translation_engine: Option<String>,
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
    /// 「通用 › 外观 › 原生窗口标题栏」：开启后交给系统画标题栏（decorations），
    /// 作为自绘标题栏在个别设备 / WebView 上拖不动的兜底。缺失 = 关（自绘）。
    #[serde(default)]
    native_titlebar: bool,
    /// 「启动详情 › 启动参数」：用户在启动详情里编辑的 JVM 参数（逐条存放）。
    /// 缺失 / 空 = 用前端的默认列表（`-agentlib:zbNative` + `--`）。
    /// 只存用户填的内容；「方案不使用加载器」时的 agent 摘除在启动时做，不改这份记录。
    #[serde(default)]
    launch_arguments: Vec<String>,
    /// 覆盖层背景色（`#RRGGBB`）。缺失 = 默认纯红；启动器写状态文件时下发给 DLL。
    overlay_background_color: Option<String>,
    /// 「模组加载器方案」（`zombieBuddy` / `none`），决定启动前要不要挂加载器 agent。
    mod_loader_scheme: Option<String>,
    /// 「极速启动（实验性）」：把启动器预扫描出的模组/补丁清单喂给 ZombieBuddy，
    /// 短路它内部的 ClassGraph 扫描。只在方案 = zombieBuddy 时生效；任何失败都回退原生扫描。
    #[serde(default)]
    fast_launch: bool,
    /// 「启动时保留 JAR 模组启用状态」：开启后启动 / 自动扫描不再把
    /// `jar-mod-state.json` 的记录套回 `mod.info`，而是把磁盘上当前的启用状态
    /// 读作基准（只回写记录，不改动模组文件）。默认开启。
    #[serde(default = "default_jar_mod_use_disk_state_on_startup")]
    jar_mod_use_disk_state_on_startup: bool,
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
    /// 「启动游戏自动暂停音乐」：启动完成、界面输出「启动用时 XXXs」时暂停内置壁纸背景音乐。
    /// 缺失 = 开（用户点的是「添加这个功能」）。
    #[serde(default = "default_pause_music_on_launch")]
    pause_music_on_launch: bool,
    /// 「启动器接管光影管理」：开启后启动游戏时才覆写 viewpoint-live.properties，
    /// 游戏退出后还原。缺失 / false = 不接管（光影包页首次点「配置」时弹框询问后再开启）。
    #[serde(default)]
    shaderpack_override_enabled: bool,
    /// 「从游戏文件启动（离线）」：勾选后「启动游戏」不经过 Steam，
    /// 直接用游戏目录自带的 jre64 启动，并把创意工坊模组联接进 Zomboid\mods。
    #[serde(default)]
    launch_from_game_files: bool,
    /// 「激进内存清理」：常规清理之外，再把系统的已修改页 / 待机缓存还给空闲列表。
    #[serde(default)]
    aggressive_memory_clean: bool,
    /// 内置四季壁纸的手动 / 固定设置，前端 builtinWallpaper.ts 的 WallpaperEffectSettings 原样存取。
    #[serde(default)]
    wallpaper_effect: Option<serde_json::Value>,
    /// 内置壁纸背景音乐的播放状态（音量 / 是否在播 / 播放位置 / 面板是否展开），
    /// 前端 launchMusicUi.ts 的 LaunchMusicState 原样存取。
    #[serde(default)]
    launch_music: Option<serde_json::Value>,
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
    /// 隐私协议：用户最后同意的版本号。缺失 = 从未同意过（首次启动会弹同意框）。
    #[serde(default)]
    privacy_consented_version: Option<String>,
    /// 「实验性 › 自动加入服务器」：启动游戏后自动连入配置的服务器（原理见 auto_join.rs）。
    /// 关闭时保存命令会把目标与密码一起清空，不留在 settings.json 里。
    #[serde(default)]
    auto_join_enabled: bool,
    /// 目标服务器 `主机:端口`（保存命令里已归一化校验；坏值按未配置处理）。
    #[serde(default)]
    auto_join_target: Option<String>,
    /// 服务器密码（可选）；游戏收藏里保存过密码的服务器优先用收藏的。
    #[serde(default)]
    auto_join_password: Option<String>,
}

/// 「缺失 = 默认开启」的开关：版本保存多线程（version_parallel）与保存时备份模组
/// （version_include_mods）都用它；settings.rs 在没有 settings.json 时也要按这个默认值补。
pub(crate) fn default_version_parallel_enabled() -> bool {
    true
}

/// 新装 / 老设置文件缺少该字段时，默认「启动时保留 JAR 模组启用状态」。
pub(crate) fn default_jar_mod_use_disk_state_on_startup() -> bool {
    true
}

/// 新装 / 老设置文件缺少该字段时，默认「启动游戏自动暂停音乐」为开。
pub(crate) fn default_pause_music_on_launch() -> bool {
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
        let state_path = jar_mod_state_path()?;
        let mut state = read_jar_mod_state(&state_path)?;
        sync_jar_mod_state_on_startup(&roots, &mut state, &state_path)?;
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
    // 部分机器（如无 Xbox 服务的机器）上游戏进程名不是 ProjectZomboid64.exe /
    // javaw.exe（任务管理器里显示为 wsappx），tasklist 按映像名过滤会漏 ——
    // 先按窗口标题兜底识别一次。
    if overlay_session::game_window_pid().is_some() {
        return true;
    }

    // 同样走进程快照而不是 `tasklist`：后者在本机实测要 60–110 秒，
    // 会把"是否在运行"的检查（版本操作前置校验、启动熔断）拖到不可接受。
    overlay::imp::image_name_running("ProjectZomboid64.exe")
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

/// 前端在弹确认框**之前**先用它挡一手：游戏开着时还原必然失败，
/// 与其让用户读完确认文案再吃错误，不如直接提示。
#[tauri::command]
fn is_project_zomboid_running() -> Result<bool, String> {
    Ok(project_zomboid_running())
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
            for entry in items.flatten().filter(|entry| {
                // 跳过联接（symlink）：离线启动会把创意工坊模组联接进这里，
                // 不跳过的话模组页 / 版本快照会把同一个模组数成两次。
                entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            }) {
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
                // 433/1167 是整块盘掉线：后面每个模组都会以同样的错误失败，
                // 继续循环只会刷出几百条日志（用户反馈里出现过 525 条），直接中止。
                if versioning::is_device_gone_message(&error) {
                    return Err(format!(
                        "保存「{}」时磁盘已从系统断开，已停止本次保存（后面还有 {} 个模组未处理）。这不是模组文件损坏——是整块磁盘掉线；请检查硬盘的数据线/供电，重启电脑后再试。\n\n原始错误：{error}",
                        root.name,
                        total.saturating_sub(index + 1)
                    ));
                }
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
                // 还原是写操作：盘掉线后继续只会反复失败，还可能留下半还原的模组。
                if versioning::is_device_gone_message(&error) {
                    return Err(format!(
                        "还原「{}」时磁盘已从系统断开，已停止本次还原（后面还有 {} 个模组未处理）。请检查硬盘的数据线/供电，重启电脑后再试。\n\n原始错误：{error}",
                        root.name,
                        total.saturating_sub(index + 1)
                    ));
                }
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
        // 转换独立版本：重写增量包一步，按文件/字节上报。
        "rebase" | "modRebase" => 1,
        // 删除旧版本时可能先重挂后续版本（phase = rebase），再删文件（phase = deleting）。
        "delete" | "modDelete" => match phase {
            "rebase" | "deleting" => 1,
            _ => 0,
        },
        // 一键保存 / 还原整套模组：只有一步（进度按模组等分）。
        // 一键保存（全部/仅变更）/ 还原整套模组：只有一步（进度按模组等分）。
        "modSaveAll" | "modSaveChanges" | "modRestorePack" => match phase {
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

/// 重命名一个历史版本（只改 meta.json 里的备注，不动快照文件）。
#[tauri::command(async)]
fn rename_version(version_id: String, label: String) -> Result<String, String> {
    let store_path = configured_version_store_path()?;
    versioning::rename_version(&store_path, &version_id, &label)
}

/// 启动页用：上次切换/保存的版本名称。只读 meta.json，不扫描游戏目录。
#[tauri::command(async)]
fn get_active_version_label() -> Result<Option<String>, String> {
    let store_path = configured_version_store_path()?;
    versioning::active_version_label(&store_path)
}

#[tauri::command(async)]
fn get_version_store_path() -> Result<String, String> {
    Ok(configured_version_store_path()?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command(async)]
fn save_version_system_drive_notice_dismissed(dismissed: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.version_system_drive_notice_dismissed = dismissed)?;
    Ok(())
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

    update_app_settings(|settings| {
        settings.version_store_path = next.map(|value| value.to_string_lossy().into_owned());
    })?;
    Ok(())
}

#[tauri::command(async)]
fn save_version_parallel(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.version_parallel = enabled)?;
    Ok(())
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
    // 重装失败不中止还原，但要在完成后弹模态框警告：单独发一条事件给前端。
    let pzopt_warning_app = app.clone();
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
                    let _ = pzopt_warning_app.emit(
                        "version-restore-pzopt-warning",
                        serde_json::json!({ "error": error }),
                    );
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

#[tauri::command(async)]
fn start_version_rebase(app: tauri::AppHandle, version_id: String) -> Result<u64, String> {
    let store_path = configured_version_store_path()?;
    spawn_version_operation(app, "rebase", 1, move |progress| {
        versioning::rebase_version_progress(&store_path, &version_id, progress)
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
fn delete_version(app: tauri::AppHandle, version_id: String) -> Result<u64, String> {
    let store_path = configured_version_store_path()?;
    // 删除量可能上 GB：走后台操作上报进度。删除阶段吞掉取消请求——
    // meta 已先落盘，中途停止只会留下孤儿文件，删完为止反而更可控；
    // 删旧版本前的「转换为独立版本」仍允许取消。
    spawn_version_operation(app, "delete", 1, move |progress| {
        let mut guarded = |event: versioning::ProgressEvent| {
            if event.phase == "deleting" {
                let _ = progress(event);
                Ok(())
            } else {
                progress(event)
            }
        };
        versioning::delete_version_with_rebase_progress(&store_path, &version_id, &mut guarded)
    })
}

#[tauri::command(async)]
fn delete_version_through(app: tauri::AppHandle, version_id: String) -> Result<u64, String> {
    let store_path = configured_version_store_path()?;
    // 删除量可能上 GB：走后台操作上报进度。删除阶段吞掉取消请求——
    // meta 已先落盘，中途停止只会留下孤儿文件，删完为止反而更可控；
    // 重建新基准（copying）与重挂（rebase）阶段仍允许取消。
    spawn_version_operation(app, "delete", 1, move |progress| {
        let mut guarded = |event: versioning::ProgressEvent| {
            if event.phase == "deleting" {
                let _ = progress(event);
                Ok(())
            } else {
                progress(event)
            }
        };
        versioning::delete_versions_through_progress(&store_path, &version_id, &mut guarded)
    })
}

#[tauri::command(async)]
fn save_version_include_mods(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.version_include_mods = enabled)?;
    Ok(())
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
fn delete_mod_version(
    app: tauri::AppHandle,
    root_path: String,
    version_id: String,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let store_base = configured_version_store_path()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(app, "mod-version-progress", "modDelete", target, 1, move |progress| {
        let mut guarded = |event: versioning::ProgressEvent| {
            if event.phase == "deleting" {
                let _ = progress(event);
                Ok(())
            } else {
                progress(event)
            }
        };
        versioning::delete_version_with_rebase_progress(&store, &version_id, &mut guarded)
    })
}

#[tauri::command(async)]
fn delete_mod_version_through(
    app: tauri::AppHandle,
    root_path: String,
    version_id: String,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let store_base = configured_version_store_path()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(app, "mod-version-progress", "modDelete", target, 1, move |progress| {
        let mut guarded = |event: versioning::ProgressEvent| {
            if event.phase == "deleting" {
                let _ = progress(event);
                Ok(())
            } else {
                progress(event)
            }
        };
        versioning::delete_versions_through_progress(&store, &version_id, &mut guarded)
    })
}

#[tauri::command(async)]
fn start_mod_version_rebase(
    app: tauri::AppHandle,
    root_path: String,
    version_id: String,
) -> Result<u64, String> {
    let root = resolve_mod_root(&root_path)?;
    let store_base = configured_version_store_path()?;
    let store = mod_version_store(&root, &store_base)?;
    let target = mod_operation_root(&root);
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modRebase",
        target,
        1,
        move |progress| versioning::rebase_version_progress(&store, &version_id, progress),
    )
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

/// 整合包名称上限：和输入框宽度匹配，防止把列表行撑到无法阅读。
const MOD_PACK_LABEL_MAX_LEN: usize = 60;

fn normalize_mod_pack_label(label: &str) -> Result<String, String> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err("整合包名称不能为空。".to_string());
    }
    if trimmed.chars().count() > MOD_PACK_LABEL_MAX_LEN {
        return Err(format!(
            "整合包名称太长（最多 {MOD_PACK_LABEL_MAX_LEN} 个字）。"
        ));
    }
    Ok(trimmed.to_string())
}

/// 重命名整合包：只改 `modpacks.json` 里的显示名，模组版本引用不动。
#[tauri::command(async)]
fn rename_mod_version_pack(pack_id: String, label: String) -> Result<String, String> {
    if VERSION_OPERATION_ACTIVE.load(Ordering::SeqCst) {
        return Err("请等当前版本操作完成后再重命名整合包。".to_string());
    }
    let pack_label = normalize_mod_pack_label(&label)?;
    let store_base = configured_version_store_path()?;
    let mut packs = mod_versioning::read_packs(&store_base)?;
    let Some(pack) = packs.iter_mut().find(|pack| pack.id == pack_id) else {
        return Err("找不到这个整合包。".to_string());
    };
    let old_label = pack.label.clone();
    pack.label = pack_label.clone();
    mod_versioning::write_packs(&store_base, &packs)?;
    Ok(format!("整合包「{old_label}」已重命名为「{pack_label}」。"))
}

/// 模组是否有待保存的变化：没有历史（需要基准）或有待捕获变化都算。
/// 状态读不出来（目录消失等）按「没有变化」处理——这种模组在保存时会单独报错，
/// 保守地包含它只会让一键保存卡在一个必败的模组上。
fn mod_root_has_pending_changes(
    root: &mod_versioning::ModRoot,
    store_base: &Path,
) -> bool {
    let Ok(store) = mod_version_store(root, store_base) else {
        return false;
    };
    let versions = versioning::read_versions(&store).unwrap_or_default();
    if versions.is_empty() {
        return true;
    }
    match versioning::get_status(Some(&root.path), &store, None, "mods", false) {
        Ok(status) => status
            .pending_changes
            .is_some_and(|changes| changes.added + changes.removed + changes.modified > 0),
        Err(_) => false,
    }
}

/// 一键保存模组变更：只保存「有待捕获变化 / 还没有版本」的模组，
/// 无变化的模组自动跳过。保存成功后写一条「变更记录」（kind=changes 的整合包），
/// 之后可以在整合包列表里「还原整套」——只还原这次保存的那几个模组；
/// 全量记录（所有模组）是一键保存全部模组的职责。
#[tauri::command(async)]
fn start_mod_version_save_changes(app: tauri::AppHandle) -> Result<u64, String> {
    let (store_base, parallel) = configured_version_options()?;
    let roots = collect_mod_roots();
    if roots.is_empty() {
        return Err("没有可保存的模组。".to_string());
    }
    spawn_version_operation_with(
        app,
        "mod-version-progress",
        "modSaveChanges",
        None,
        1,
        move |progress| {
            // 逐个模组做元数据比对（不哈希，只看路径 + 大小），挑出有变化的；
            // 比对本身也要几秒，进度条按同样的模组等分推进。
            let total = roots.len();
            let mut pending = Vec::new();
            for (index, root) in roots.iter().enumerate() {
                progress(mod_marker_event(index, total, &root.name))?;
                if mod_root_has_pending_changes(root, &store_base) {
                    pending.push(root.clone());
                }
            }
            if pending.is_empty() {
                return Ok("所有模组都没有变化，无需保存。".to_string());
            }
            let skipped = total - pending.len();
            let (references, failures) =
                capture_mod_versions(&pending, "保存变化", parallel, progress)?;
            // 一个都没保存成功就没有可还原的东西，不写记录。
            let mut pack_note = String::new();
            if !references.is_empty() {
                let created_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                let mut packs = mod_versioning::read_packs(&store_base)?;
                // 编号只数变更记录，避免和「整合包 N」交叉出现「变更 3」的怪序。
                let index = packs
                    .iter()
                    .filter(|pack| pack.kind == mod_versioning::PACK_KIND_CHANGES)
                    .count()
                    + 1;
                let label = format!("变更 {index}");
                packs.push(mod_versioning::ModPackRecord {
                    id: format!("pack-{created_at}-{}", packs.len() + 1),
                    label: label.clone(),
                    created_at,
                    kind: mod_versioning::PACK_KIND_CHANGES.to_string(),
                    mods: references.clone(),
                });
                mod_versioning::write_packs(&store_base, &packs)?;
                pack_note = format!("，已记为变更记录「{label}」，可在下方整套还原");
            }
            let failure_note = if failures > 0 {
                format!("，{failures} 个失败")
            } else {
                String::new()
            };
            Ok(format!(
                "已保存 {} 个模组的变更（{} 个无变化跳过{failure_note}）{pack_note}。",
                references.len(),
                skipped
            ))
        },
    )
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
                kind: mod_versioning::PACK_KIND_ALL.to_string(),
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
    update_app_settings(|settings| {
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
    })?;
    Ok(())
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
    let normalized_base = steam_relay::normalize_relay_base(&base);
    update_app_settings(|settings| {
        settings.steam_relay_mode = Some(mode);
        settings.steam_relay_base = Some(normalized_base);
    })?;
    Ok(())
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
    update_app_settings(|settings| settings.steam_proxy = normalized)?;
    Ok(())
}

/// 设置页「自动检测本地代理」：探常见 HTTP/SOCKS 端口，返回真能访问 Steam 的。
#[tauri::command]
async fn detect_steam_proxy() -> Vec<steam_relay::ProxyProbe> {
    steam_relay::detect_local_proxies().await
}

/// 设置 › 关于 › 反馈信息：上传最近一次启动的日志 + 用户描述（联系方式可选，拼在描述末尾）。
#[tauri::command]
async fn submit_feedback(
    description: String,
    contact: Option<String>,
) -> Result<feedback::FeedbackUploadResult, String> {
    feedback::submit_feedback(description, contact).await
}

/// 设置 › 关于 › 反馈信息：本机提交过的反馈编号（新→旧）。
#[tauri::command]
fn list_submitted_feedback() -> Result<Vec<feedback::FeedbackHistoryEntry>, String> {
    Ok(feedback::read_history())
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
    update_app_settings(|settings| {
        settings.silent_memory_cleanup = enabled;
        settings.silent_memory_cleanup_threshold = threshold;
    })?;
    memclean::set_silent_cleanup(enabled, threshold);
    Ok(())
}

/// 保存「启动时清理内存」开关；真正的清理发生在 [`launch_project_zomboid`] 里。
#[tauri::command]
fn save_clean_memory_on_launch(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.clean_memory_on_launch = enabled)?;
    Ok(())
}

/// 保存「激进内存清理」开关；后端立刻同步给 memclean，下一次清理就按新模式跑。
#[tauri::command]
fn save_aggressive_memory_clean(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.aggressive_memory_clean = enabled)?;
    memclean::set_aggressive_cleanup(enabled);
    Ok(())
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

/// 当前启动器是否以管理员身份运行：激进清理的系统级步骤只有管理员才生效。
#[tauri::command]
fn is_launcher_elevated() -> bool {
    memclean::is_elevated()
}

/// 以管理员身份重新启动启动器；用户在 UAC 里取消时返回错误，当前实例继续运行。
#[tauri::command]
fn restart_as_admin(app: tauri::AppHandle) -> Result<(), String> {
    memclean::restart_as_admin()?;
    // 新实例已经拉起，当前实例退出，避免两个启动器同时运行。
    app.exit(0);
    Ok(())
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
    policy_agent: Option<&str>,
) -> Result<bool, String> {
    let mut changed_files = Vec::new();
    for path in config_paths {
        let content =
            fs::read_to_string(path).map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (patched, changed) = patch_steam_launch_options_content_with_memory(
            &content,
            memory_mb,
            extra_args,
            policy_agent,
        )?;
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
        // 42.22 起官方 jar 带签名：classpath 里的 `.` 会让游戏目录下遗留的散装
        // `.class` 与签名 jar 混载，JVM 抛 signer mismatch（启动弹 "JNI ERROR"）。
        // 只在「jar 已签名 + 确有散装 class」时去掉 `.`，其它版本保持原样。
        let (patched, stripped_dot) = if project_zomboid_jar_is_signed(game_path)
            && loose_class_file_count(game_path) > 0
        {
            let (patched, stripped) = strip_dot_classpath_entries(&patched)?;
            if stripped {
                launcher_log::warn(&format!(
                    "游戏目录里有散装 .class 且 projectzomboid.jar 已签名（42.22+）；\
                     已从启动 classpath 去掉 '.' 规避 JNI ERROR。\
                     建议删除 {} 等散装 class 以恢复官方目录结构。",
                    game_path.display()
                ));
            }
            (patched, stripped)
        } else {
            (patched, false)
        };
        if changed || stripped_dot {
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

/// 工坊里 ZombieBuddy 正式版的发布版本号（`mods/ZombieBuddy/java/VERSION`）。
#[cfg(windows)]
fn zombie_buddy_release_version() -> Option<String> {
    let roots = steam_roots();
    for library in all_libraries_from_roots(&roots) {
        let Some(source) = zombie_buddy_source_from_library(&library) else {
            continue;
        };
        let Ok(version) = fs::read_to_string(source.join("java").join("VERSION")) else {
            continue;
        };
        let version = version.trim();
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }
    None
}

/// 42.22+ 的游戏 jar 带签名，而工坊 ZombieBuddy 正式版官方支持范围只到 B42.21。
/// 这种「未支持组合」下，旧 Java 模组引用已移除类会在 `LuaManager.init` 阶段闪退
/// （如 DGVViewpointEyes 引用 `zombie.util.list.PZArrayList`）。每次运行只记一条，
/// 配合启动前兼容性预检定位问题模组；不阻断启动。
#[cfg(windows)]
fn log_zombie_buddy_support_notice() {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(game_path) = project_zomboid_install_path() else {
        return;
    };
    if !project_zomboid_jar_is_signed(&game_path) {
        return;
    }
    let version = zombie_buddy_release_version()
        .map(|value| format!("v{value}"))
        .unwrap_or_else(|| "未知版本".to_string());
    launcher_log::warn(&format!(
        "ZombieBuddy 版本提示：工坊正式版 {version} 官方支持 B42.12–B42.21，当前游戏为 42.22+，\
         属未支持组合。若启动闪退，请优先用启动前兼容性预检禁用引用已移除类的旧模组。"
    ));
}

/// 显式动作（例如配置 Viewpoint 3D）用的入口：无条件按 ZombieBuddy 方案改写。
#[allow(dead_code)]
fn force_zombie_buddy_policy_at_paths(
    game_path: &Path,
    memory_mb: Option<u64>,
) -> Result<bool, String> {
    apply_launch_loader_at_paths(game_path, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

/// `projectzomboid.jar` 是否带签名（42.22 官方开始签名，`META-INF/PROJECTZ.EC`）。
///
/// 只看 zip 中央目录里的签名文件，不解压，几十毫秒内完成。
fn project_zomboid_jar_is_signed(game_path: &Path) -> bool {
    let jar_path = game_path.join("projectzomboid.jar");
    let Ok(file) = fs::File::open(&jar_path) else {
        return false;
    };
    let Ok(archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let mut signed = false;
    for name in archive.file_names() {
        if name.starts_with("META-INF/")
            && (name.ends_with(".SF")
                || name.ends_with(".RSA")
                || name.ends_with(".DSA")
                || name.ends_with(".EC"))
        {
            signed = true;
            break;
        }
    }
    signed
}

/// 统计游戏目录里的散装 `.class`（旧版 pzopt / 手工替换的残留），上限 1000。
///
/// 跳过自带 JRE、资源与模组目录——它们量大且与签名 jar 无关。
fn loose_class_file_count(game_path: &Path) -> usize {
    const SKIP_DIRS: [&str; 7] = [
        "jre64",
        "media",
        "mods",
        "workshop",
        "pzopt",
        "natives",
        ".pzmanager-backups",
    ];
    let mut count = 0usize;
    let mut stack = vec![game_path.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(entry.path());
            } else if kind.is_file()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("class"))
            {
                count += 1;
                if count >= 1000 {
                    return count;
                }
            }
        }
    }
    count
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

/// 从 bat 内容里按引号感知切 token：引号内空白不算分隔符，引号本身不保留。
#[cfg(windows)]
fn tokenize_bat_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// `ProjectZomboid64.bat` 里解析出的 java 启动命令固定部分。
#[cfg(windows)]
struct BatLaunchCommand {
    /// bat 里 `-cp` 之前的固定 JVM 参数（已剔除 -Xmx / 加载器 agent，它们由启动器显式传）。
    fixed_vm_args: Vec<String>,
    classpath: String,
    main_class: String,
}

/// 从 `ProjectZomboid64.bat` 解析 java 启动命令（第一条 java 行，重试分支忽略）。
///
/// 「从游戏文件启动」不走 `ProjectZomboid64.exe`：它是原生启动器，Steam 相关
/// 初始化在没有 Steam 环境时直接弹 fatal error（2026-11-10 实测，Steam 客户端
/// 未运行即复现，游戏进程 1 秒内退出码 1）；bat 的 java 命令行不经过那一步，
/// Steam 未运行也能进游戏（游戏侧自行降级无 Steam 模式）。所以照抄 bat 的
/// 命令行 —— 固定参数 / classpath / 主类运行时从 bat 解析，pzopt 或游戏更新
/// 改了 bat 也能跟上；内存上限与加载器 agent 在 bat 里走 `_JAVA_OPTIONS`，
/// 这里改成显式 VM 参数（同一套值，见 [`build_file_java_args`]）。
#[cfg(windows)]
fn parse_bat_launch_command(bat_content: &str) -> Option<BatLaunchCommand> {
    let classpath_var = bat_content
        .lines()
        .find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("SET PZ_CLASSPATH")
                .then(|| value.trim().to_string())
        })
        // bat 没有 SET PZ_CLASSPATH 时按当前官方脚本的值兜底。
        .unwrap_or_else(|| "./;projectzomboid.jar".to_string());

    // 取第一条 java 启动行（bat 的 IF %ERRORLEVEL% 重试分支里的第二条忽略）。
    let line = bat_content.lines().find(|line| {
        let lowered = line.to_ascii_lowercase();
        lowered.contains("jre64") && lowered.contains("java.exe")
    })?;
    let tokens = tokenize_bat_line(line);
    // 形如：[java 路径, 固定参数..., "-cp", %PZ_CLASSPATH%, 主类, %1, %2]
    let cp_index = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("-cp") || token.eq_ignore_ascii_case("-classpath"))?;
    let classpath = tokens
        .get(cp_index + 1)?
        .replace("%PZ_CLASSPATH%", &classpath_var);
    let main_class = tokens.get(cp_index + 2)?.clone();
    let fixed_vm_args = tokens[1..cp_index]
        .iter()
        .filter(|token| {
            !is_memory_arg(token)
                && !is_fast_launch_agent_token(token)
                && !is_zombie_buddy_agent_token(token)
                && token.as_str() != "--"
        })
        .cloned()
        .collect();
    Some(BatLaunchCommand {
        fixed_vm_args,
        classpath,
        main_class,
    })
}

/// bat 解析失败（结构变了 / 文件缺失）时的兜底：与当前官方 bat 的第一条 java 行一致。
#[cfg(windows)]
fn default_bat_launch_command() -> BatLaunchCommand {
    BatLaunchCommand {
        fixed_vm_args: [
            "-Djava.awt.headless=true",
            "--enable-native-access=ALL-UNNAMED",
            "--add-exports=java.base/jdk.internal.misc=ALL-UNNAMED",
            "-Dzomboid.steam=1",
            "-Dzomboid.znetlog=1",
            "-XX:-CreateCoredumpOnCrash",
            "-XX:-OmitStackTraceInFastThrow",
            "-XX:+UseZGC",
            "-Djava.library.path=./win64/;./",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        classpath: "./;projectzomboid.jar".to_string(),
        main_class: "zombie.gameStates.MainScreenState".to_string(),
    }
}

/// 把用户额外启动参数拆成 JVM 参数与主类（游戏）参数：
/// `-X*` / `-XX*` / `-D`（大小写敏感，`-debug` 是游戏参数）/ `-agent*` / `--*`
/// 是 JVM 参数（必须放在 `-cp` 之前），其余跟在主类后面。agent 与 `-Xmx`
/// 由启动器显式写入，这里跳过避免重复。
#[cfg(windows)]
fn split_user_launch_args(extra_args: &[String]) -> (Vec<String>, Vec<String>) {
    let mut vm_args = Vec::new();
    let mut game_args = Vec::new();
    for token in extra_args {
        let token = token.trim();
        if token.is_empty()
            || token == "--"
            || is_zombie_buddy_agent_token(token)
            || is_fast_launch_agent_token(token)
            || is_memory_arg(token)
        {
            continue;
        }
        if token.starts_with("-X")
            || token.starts_with("-XX")
            || token.starts_with("-D")
            || token.starts_with("-agent")
            || token.starts_with("--")
        {
            vm_args.push(token.to_string());
        } else {
            game_args.push(token.to_string());
        }
    }
    (vm_args, game_args)
}

/// 组装「从游戏文件启动」的完整 java 参数（顺序即 JVM 语义）：
/// bat 固定参数 → `-Xmx`（放最后覆盖 bat 自带的值）→ 加载器 agent → 极速启动
/// agent → 用户 JVM 参数 → `-cp` → 主类 → 用户游戏参数。
#[cfg(windows)]
fn build_file_java_args(
    command: &BatLaunchCommand,
    memory_mb: u64,
    extra_args: &[String],
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Vec<String> {
    let (user_vm_args, user_game_args) = split_user_launch_args(extra_args);
    let mut args = command.fixed_vm_args.clone();
    args.push(memory_arg(memory_mb));
    if let Some(agent) = policy_agent {
        args.push(agent.to_string());
    }
    if let Some(agent) = fast_launch_agent {
        args.push(agent.to_string());
    }
    args.extend(user_vm_args);
    args.push("-cp".to_string());
    args.push(command.classpath.clone());
    args.push(command.main_class.clone());
    args.extend(user_game_args);
    args
}

/// `launch_source`：`None` / `"steam"` = 走 Steam 协议启动（默认，行为与旧版一致）；
/// `"files"` = 不经过 Steam 与 `ProjectZomboid64.exe`，按游戏目录 bat 的方式用
/// 自带 jre64 的 java 命令行直接启动。
/// 两种来源共用同一套启动配置（内存上限 / 加载器 agent / 极速启动 / 光影覆写 / 清理内存）。
#[tauri::command(async)]
fn launch_project_zomboid(
    memory_mb: u64,
    launch_args: Vec<String>,
    fast_launch_dll_path: Option<String>,
    launch_source: Option<String>,
) -> Result<String, String> {
    #[cfg(not(windows))]
    {
        let _ = (memory_mb, launch_args, fast_launch_dll_path, launch_source);
        return Err("启动 Project Zomboid 只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let from_files = matches!(launch_source.as_deref(), Some("files"));
        let total_memory_mb = system_memory_mb()?;
        validate_memory_limit_mb(memory_mb, total_memory_mb)?;
        log_zombie_buddy_support_notice();
        // 从游戏文件启动时不需要碰 Steam 配置：Steam 没装 / 没登录也要能玩。
        let steam_configs = if from_files {
            Vec::new()
        } else {
            let configs = current_steam_config_paths();
            if configs.is_empty() {
                return Err("找不到 Steam localconfig.vdf".to_string());
            }
            configs
        };
        // 读一次设置：「模组加载器方案」决定 agent 处理，`fastLaunch` 决定要不要写极速启动的 agent。
        let settings = load_app_settings_at(&app_settings_path()?).ok();
        let scheme = settings
            .as_ref()
            .and_then(|settings| settings.mod_loader_scheme.clone())
            .and_then(|value| normalize_mod_loader_scheme(&value))
            .unwrap_or_else(|| DEFAULT_MOD_LOADER_SCHEME.to_string());
        let policy_agent = mod_loader_policy_agent(&scheme);

        // 自动加入服务器（实验性）：只认「开关打开 + 地址合法」的配置，坏地址按未配置处理。
        let auto_join_target = settings
            .as_ref()
            .filter(|settings| settings.auto_join_enabled)
            .and_then(|settings| settings.auto_join_target.as_deref())
            .and_then(normalize_auto_join_target);

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
            "启动游戏：内存 {memory_mb} MB，加载器方案 {scheme}，极速启动 {}，启动参数 {}，来源 {}",
            if fast_launch_agent.is_some() { "开" } else { "关" },
            effective_launch_args.join(" "),
            if from_files { "游戏文件" } else { "Steam" }
        ));
        // 从游戏文件启动时不写 Steam LaunchOptions（没有 Steam 配置可写）。
        if !from_files {
            patch_steam_launch_options_at_paths(
                &steam_configs,
                Some(memory_mb),
                &effective_launch_args,
                policy_agent,
            )?;
        }

        // 再改写游戏自带的启动文件：内存上限始终写（与加载器无关，否则 JSON 里旧的 -Xmx
        // 会覆盖启动器设置）；只有方案启用了加载器时才把它的 agent 归一化成 policy=allow-all
        // （策略只在 agent 第一次加载时锁定，而 JSON/BAT 往往先于 Steam 参数被 JVM 读到）。
        apply_launch_loader(Some(memory_mb), policy_agent, fast_launch_agent.as_deref())?;
        launcher_log::info(&format!(
            "启动配置已写入（Steam 配置 {} 个）。",
            steam_configs.len()
        ));

        // 从游戏文件启动：解析游戏目录的 bat 拿到 java 启动命令，并确认 jre64 真的存在。
        // 缺了立刻报错，别让"游戏没起来"留到前端 15 秒熔断才暴露。
        // 注意这里不用 ProjectZomboid64.json：那是 exe 启动器读的，bat 路径有自己的
        // 固定参数（pzopt 的 AOT/GC 调优只存在于 JSON，bat 路径不生效，见 parse 文档）。
        let file_launch_java = if from_files {
            let game_path = project_zomboid_install_path()
                .ok_or_else(|| "找不到游戏安装目录，无法从游戏文件启动。请先在启动页检测到游戏。".to_string())?;
            let java = game_path.join("jre64").join("bin").join("javaw.exe");
            let java = if java.is_file() {
                java
            } else {
                let console_java = game_path.join("jre64").join("bin").join("java.exe");
                if !console_java.is_file() {
                    return Err(format!(
                        "游戏目录里找不到自带 JRE（{}），无法从游戏文件启动。",
                        console_java.display()
                    ));
                }
                console_java
            };
            let bat_content = fs::read(game_path.join("ProjectZomboid64.bat"))
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            let mut command = parse_bat_launch_command(&bat_content)
                .unwrap_or_else(default_bat_launch_command);
            // 与 JSON 同理：签名 jar + 散装 class 时 classpath 里的 `.` 会触发
            // signer mismatch（JNI ERROR），直接从命令行里去掉。
            if project_zomboid_jar_is_signed(&game_path) && loose_class_file_count(&game_path) > 0 {
                let (classpath, changed) = strip_dot_classpath_string(&command.classpath);
                if changed {
                    launcher_log::warn(&format!(
                        "游戏目录里有散装 .class 且 projectzomboid.jar 已签名（42.22+）；\
                         已从直启 classpath 去掉 '.' 规避 JNI ERROR。"
                    ));
                    command.classpath = classpath;
                }
            }
            let args = build_file_java_args(
                &command,
                memory_mb,
                &effective_launch_args,
                policy_agent,
                fast_launch_agent.as_deref(),
            );
            launcher_log::info(&format!(
                "从游戏文件启动：{} {}",
                java.display(),
                args.join(" ")
            ));
            Some((game_path, java, args))
        } else {
            None
        };

        // 光影包覆写：把启动器里配置的 minecraft.* 参数行级写进 viewpoint-live.properties。
        // 尽力而为：失败只记日志不阻断启动——游戏启动配置在上面已经写完，光影只是锦上添花。
        let shader_override_applied = apply_shaderpack_overrides_for_launch().unwrap_or_else(|error| {
            launcher_log::error(&format!("光影包覆写失败：{error}"));
            false
        });

        // 自动加入服务器（设置 › 实验性功能）：钩子装进游戏目录 + 一次性配置写进 Zomboid
        // 主目录，游戏退出后由后台线程清理（auto_join::spawn_auto_join_cleanup）。
        // 失败都只记日志不阻断启动——大不了这次不自动连入。密码绝不进日志。
        let auto_join_armed = if let Some(target) = auto_join_target.as_deref() {
            let auto_join_password = settings
                .as_ref()
                .and_then(|settings| settings.auto_join_password.as_deref());
            match project_zomboid_install_path() {
                Some(game_path) => match install_auto_join_hook_at(&game_path) {
                    Ok(true) => {
                        launcher_log::info("自动加入：钩子已写入游戏目录 media/lua/client/。")
                    }
                    Ok(false) => {}
                    Err(error) => {
                        launcher_log::warn(&format!("自动加入：{error}（本次不会自动连入）"))
                    }
                },
                None => launcher_log::warn(
                    "自动加入：找不到游戏安装目录，钩子未安装（本次不会自动连入）。",
                ),
            }
            match write_auto_join_config(target, auto_join_password) {
                Ok(()) => {
                    launcher_log::info(&format!(
                        "自动加入：本次启动将连入 {target}（密码：{}）。",
                        describe_auto_join_password(auto_join_password)
                    ));
                    true
                }
                Err(error) => {
                    launcher_log::warn(&format!("自动加入：{error}（本次不会自动连入）"));
                    false
                }
            }
        } else {
            // 未启用：顺手清理残留。钩子文件在游戏运行时先留着（游戏启动时已读进内存，
            // 也不会有配置触发它），下次启动再摘；一次性配置现在就能删。
            // 这里只能用 game_running_quick（纯进程快照）：启动路径禁止 project_zomboid_running，
            // 它枚举窗口同步取标题，无响应窗口会把启动卡住（2026-11-08 的教训）。
            if !game_running_quick() {
                if let Some(game_path) = project_zomboid_install_path() {
                    match remove_auto_join_hook_at(&game_path) {
                        Ok(true) => {
                            launcher_log::info("自动加入：功能未启用，已移除游戏目录里的钩子。")
                        }
                        _ => {}
                    }
                }
            }
            if clear_auto_join_config() {
                launcher_log::info("自动加入：功能未启用，已清理残留的一次性配置。");
            }
            false
        };
        if auto_join_armed {
            spawn_auto_join_cleanup();
        }

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

        // 离线启动的创意工坊映射：Steam 没在跑时把工坊模组联接进 Zomboid\mods，
        // 否则游戏扫不到创意工坊模组（游戏只认 Zomboid\mods，工坊靠 Steamworks 提供）。
        // Steam 启动（或 Steam 在跑时的离线启动）则拆掉旧映射，避免与 Steam 工坊扫描重复。
        // 尽力而为：失败只记日志不阻断启动——大不了游戏里看不到工坊模组。
        let offline_note = if from_files {
            match offline_workshop::sync_offline_workshop_links() {
                Ok(note) => {
                    launcher_log::info(&format!("离线工坊映射：{note}"));
                    note
                }
                Err(error) => {
                    let message = format!("创建离线工坊映射失败：{error}");
                    launcher_log::error(&message);
                    message
                }
            }
        } else {
            match offline_workshop::remove_offline_workshop_links() {
                Ok(0) => String::new(),
                Ok(count) => {
                    let note = format!("已清理 {count} 个离线工坊映射。");
                    launcher_log::info(&format!("离线工坊映射：{note}"));
                    note
                }
                Err(error) => {
                    let message = format!("清理离线工坊映射失败：{error}");
                    launcher_log::error(&message);
                    message
                }
            }
        };

        let launch_result = if let Some((game_path, java, args)) = file_launch_java.as_ref() {
            // 直接用游戏自带 jre64 的 javaw 启动 JVM：进程名是 javaw.exe，
            // 覆盖层注入 / 游戏进程检测 / 停止游戏进程都按这个名字工作。
            // javaw（而非 java）与 exe 启动的观感一致：不带一个常驻控制台窗口。
            Command::new(java)
                .current_dir(game_path)
                .args(args)
                .spawn()
        } else {
            Command::new("cmd")
                .args([
                    "/C",
                    "start",
                    "",
                    &format!("steam://rungameid/{PROJECT_ZOMBOID_APP_ID}"),
                ])
                .spawn()
        };
        match launch_result {
            Ok(mut child) => {
                // 离线映射的生命周期与本次游戏进程绑定：起清理线程等游戏退出后
                // 立刻拆除联接，不再等下一次启动/重启启动器。启动器中途关闭时
                // 线程随进程消失，由下一次启动时的清理兜底。等待失败/清理失败
                // 都只记日志——联接被占用拆不掉时 remove 会跳过并留到下次。
                if from_files {
                    let spawned = std::thread::Builder::new()
                        .name("pz-offline-cleanup".to_string())
                        .spawn(move || {
                            let exit_status = child.wait();
                            match offline_workshop::remove_offline_workshop_links() {
                                Ok(0) => {}
                                Ok(count) => launcher_log::info(&format!(
                                    "游戏退出（{}）：已清理 {count} 个离线工坊映射。",
                                    exit_status
                                        .map(|status| format!("退出码 {status}"))
                                        .unwrap_or_else(|error| format!("等待失败：{error}"))
                                )),
                                Err(error) => launcher_log::warn(&format!(
                                    "游戏退出：清理离线工坊映射失败：{error}"
                                )),
                            }
                        });
                    if let Err(error) = spawned {
                        launcher_log::warn(&format!("创建离线映射清理线程失败：{error}"));
                    }
                }
                // 返回值给前端拼成功提示：离线启动时带上工坊映射结果。
                let message = if from_files {
                    offline_note
                } else {
                    "已发送 Steam 启动请求。".to_string()
                };
                launcher_log::info(if from_files {
                    "已从游戏文件启动游戏。"
                } else {
                    "已发送 Steam 启动请求。"
                });
                // 光影覆写真正写入了才需要还原监听：等游戏退出后把 viewpoint-live.properties 整文件还原。
                if shader_override_applied {
                    overlay_session::start_shaderpack_restore_watcher();
                }
                Ok(message)
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
            memclean::set_aggressive_cleanup(
                loaded_settings
                    .as_ref()
                    .is_some_and(|settings| settings.aggressive_memory_clean),
            );
            memclean::start_silent_loop(app.handle().clone());

            // 上次离线启动留下的创意工坊映射：启动器一重启就清掉，避免 Steam 模式
            // 重复加载工坊模组、模组页出现重复条目。游戏还在跑时联接可能被占用，
            // 跳过（下次启动再清）；个别拆不掉的由 remove_offline_workshop_links 记日志。
            #[cfg(windows)]
            if !overlay::imp::image_name_running("ProjectZomboid64.exe")
                && !overlay::imp::image_name_running("javaw.exe")
            {
                match offline_workshop::remove_offline_workshop_links() {
                    Ok(0) => {}
                    Ok(count) => launcher_log::info(&format!(
                        "启动器启动：已清理上次离线启动留下的 {count} 个工坊映射。"
                    )),
                    Err(error) => launcher_log::error(&format!(
                        "启动器启动：清理离线工坊映射失败：{error}"
                    )),
                }
            }

            // 恢复上一次记住的窗口尺寸（拖动窗口边缘调整后会自动记住）。
            // 默认 840×560 由 tauri.conf.json 提供，这里只在有有效记录时覆盖。
            if let Some(settings) = loaded_settings.as_ref() {
                // 原生标题栏开关在窗口内容出现前应用，避免先画出自绘栏再跳一次。
                if settings.native_titlebar {
                    use tauri::Manager;
                    if let Some(window) = app.get_webview_window("main") {
                        if let Err(error) = window.set_decorations(true) {
                            launcher_log::error(&format!("启用原生标题栏失败：{error}"));
                        }
                    }
                }
                if let (Some(width), Some(height)) = (settings.window_width, settings.window_height) {
                    let width = clamp_window_width(width);
                    let height = clamp_window_height(height);
                    use tauri::Manager;
                    if let Some(window) = app.get_webview_window("main") {
                        // 恢复失败要留痕：静默的 set_size 失败会让「记住窗口尺寸」看起来完全失效。
                        if let Err(error) = window.set_size(tauri::LogicalSize::new(width, height)) {
                            launcher_log::error(&format!(
                                "恢复窗口尺寸失败（{width}×{height}）：{error}"
                            ));
                        } else {
                            launcher_log::info(&format!("已恢复窗口尺寸 {width}×{height}。"));
                        }
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
            load_saved_tasks,
            save_tasks,
            save_memory_limit,
            save_developer_mode,
            save_steam_account_hidden,
            save_pause_music_on_launch,
            save_launch_from_game_files,
            read_game_options,
            save_game_option,
            save_shaderpack_override_enabled,
            save_translation_language,
            save_translation_engine,
            save_font_contrast,
            save_blur_strength,
            save_font_scale,
            save_font_family,
            save_overlay_background_color,
            save_window_size,
            restore_default_window_size,
            save_native_titlebar,
            save_launch_arguments,
            save_auto_join_settings,
            save_mod_loader_scheme,
            save_jar_mod_use_disk_state_on_startup,
            save_fast_launch,
            save_silent_memory_cleanup,
            save_clean_memory_on_launch,
            save_aggressive_memory_clean,
            save_steam_relay_settings,
            save_steam_proxy,
            detect_steam_proxy,
            get_version_status,
            get_active_version_label,
            get_version_store_path,
            save_version_system_drive_notice_dismissed,
            save_version_store_path,
            save_version_parallel,
            start_version_snapshot,
            start_version_capture,
            start_version_restore,
            start_version_verify,
            start_version_rebase,
            cancel_version_operation,
            delete_version,
            delete_version_through,
            rename_version,
            save_version_include_mods,
            list_mod_version_targets,
            get_mod_version_status,
            start_mod_version_snapshot,
            start_mod_version_capture,
            start_mod_version_restore,
            start_mod_version_verify,
            start_mod_version_rebase,
            delete_mod_version,
            delete_mod_version_through,
            list_mod_version_packs,
            delete_mod_version_pack,
            rename_mod_version_pack,
            start_mod_version_save_all,
            start_mod_version_save_changes,
            start_mod_version_restore_pack,
            submit_feedback,
            list_submitted_feedback,
            ping_steam_relay,
            get_memory_status,
            clean_memory_now,
            is_launcher_elevated,
            is_project_zomboid_running,
            restart_as_admin,
            fast_launch_cache_status,
            startup_timing,
            set_background_path,
            set_wallpaper_effect,
            set_launch_music_state,
            steam_process_running,
            list_custom_backgrounds,
            scan_workshop_backgrounds,
            start_workshop_background_scan,
            import_custom_background,
            read_background,
            read_jar_mod_poster,
            read_lua_mod_poster,
            start_zombie_buddy_jar_scan,
            find_jar_mods_with_injection_item,
            check_mod_compatibility,
            disable_mods_for_launch,
            list_lua_mods,
            set_lua_mod_enabled,
            set_lua_mods_enabled,
            save_lua_mod_order,
            set_zombie_buddy_mod_decision,
            set_zombie_buddy_mods_decisions,
            apply_jar_mod_file_state,
            list_mod_presets,
            auto_sort_lua_mods,
            save_privacy_consent,
            clear_privacy_consent,
            clear_launcher_user_data,
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
            list_steam_screenshots,
            read_steam_screenshot,
            list_viewpoint_shaderpacks,
            import_viewpoint_shaderpack,
            delete_viewpoint_shaderpack,
            read_shaderpack_config,
            get_shaderpack_overrides,
            save_shaderpack_override,
            clear_shaderpack_override,
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
mod tests;


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
