//! 极速启动：预扫描缓存与启动配置备份。
//!
//! 通过 `lib.rs` 的 `pub(crate) use fast_launch::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use crate::*;
/// 删掉极速启动的预扫描缓存。
///
/// ⚠️ **策略已改（2026-10-15）：缓存持久化**。决策/启停变更**不再**自动删缓存
/// （以前挂在 `write_jar_mod_state` / `set_jar_mod_info_enabled` 上），只有用户显式点
/// 「刷新 JAR 列表」（`start_zombie_buddy_jar_scan { refresh_cache: true }`）才重建。
///
/// 为什么改：缓存被自动删掉的那一局玩家完全没加速，而这件事在游戏里**看不出来**；
/// 玩家要的是"缓存稳定存在，什么时候换由我说了算"。代价是"改了模组但没刷新"时缓存可能过期，
/// 这由设置页的状态行（`fast_launch_cache_status().matches == false`）明确提示，而不是悄悄删掉。
///
/// 保留本函数供测试与将来的「清空缓存」入口使用。
#[allow(dead_code)]
pub(crate) fn invalidate_fast_launch_cache() {
    if let Ok(directory) = pzmanager_directory() {
        let _ = fs::remove_file(directory.join(FAST_LAUNCH_CACHE_FILE));
    }
}

/// 极速启动的预扫描缓存：启动器把「已批准且启用」的 JAR 模组及其注解类清单落盘，
/// 注入器（overlay-dll）在 ZombieBuddy 扫描前把它喂给 Loader，短路 ClassGraph 扫描。
/// 只收录已批准且启用的模组是硬要求：缓存与启用列表不一致会让补丁丢失。
pub(crate) const FAST_LAUNCH_CACHE_FORMAT: u32 = 1;

pub(crate) const FAST_LAUNCH_CACHE_FILE: &str = "zb-prescan-cache.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchCache {
    pub(crate) format: u32,
    pub(crate) generated_at: u64,
    /// 一致性指纹：注入器拿它和当前启用列表比对，对不上就放弃缓存、回退原生扫描。
    pub(crate) mod_set_hash: String,
    pub(crate) mods: Vec<FastLaunchCacheMod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchCacheMod {
    pub(crate) mod_name: String,
    pub(crate) jar_path: String,
    pub(crate) base_package: String,
    pub(crate) jar_hash: String,
    pub(crate) patch_classes: Vec<String>,
    pub(crate) lua_classes: Vec<String>,
}

/// FNV-1a：只给缓存指纹用，不为这点需求引入哈希依赖。
pub(crate) fn fast_launch_fingerprint(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 把扫描产出的**内部名**（`a/b/C`）转成**二进制名**（`a.b.C`）。
///
/// 缓存里的类名是给注入器直接喂 `Class.forName` 用的，必须是二进制名。
/// 扫描（`class_annotations`）给的是内部名 —— 早期版本直接把内部名写进缓存，
/// 结果注入器侧每个模组都抛 `ClassNotFoundException` 然后回退原生扫描（**一点没加速**），
/// 日志里只看到一串 CNFE。注入器侧现在也会兜一层转换，两边都改才算稳。
pub(crate) fn binary_class_name(internal_name: &str) -> String {
    if internal_name.contains('/') {
        internal_name.replace('/', ".")
    } else {
        internal_name.to_string()
    }
}

/// 由扫描结果构造缓存：过滤掉未批准/已禁用的模组，按 jar 路径排序后算指纹。
pub(crate) fn build_fast_launch_cache(mods: &[ScannedJarMod], generated_at: u64) -> FastLaunchCache {
    let mut entries: Vec<FastLaunchCacheMod> = mods
        .iter()
        .filter(|entry| !entry.disabled && entry.decision == Some(true))
        .map(|entry| FastLaunchCacheMod {
            mod_name: entry.info_id.clone(),
            jar_path: entry.jar_path.clone(),
            base_package: entry.package_name.clone(),
            jar_hash: entry.jar_hash.clone(),
            patch_classes: entry
                .patch_classes
                .iter()
                .map(|name| binary_class_name(name))
                .collect(),
            lua_classes: entry
                .lua_classes
                .iter()
                .map(|name| binary_class_name(name))
                .collect(),
        })
        .collect();
    entries.sort_by(|left, right| left.jar_path.cmp(&right.jar_path));
    let fingerprint = entries
        .iter()
        .map(|entry| format!("{}#{}", entry.jar_path, entry.jar_hash))
        .collect::<Vec<_>>()
        .join("|");
    FastLaunchCache {
        format: FAST_LAUNCH_CACHE_FORMAT,
        generated_at,
        mod_set_hash: fast_launch_fingerprint(&fingerprint),
        mods: entries,
    }
}

/// 极速启动缓存专用：ZombieBuddy 加载器本体的扫描结果。
///
/// 它不在「JAR 模组」列表里（`jar_scan_candidates` 刻意跳过），但 ZombieBuddy 自己会调
/// `collectPatches("me.zed_0xff.zombie_buddy.patches")` —— 缓存里没有它，强制模式下
/// ZB 自己的补丁就会被丢掉（实测表现：进游戏弹"ZombieBuddy 未安装"）。
pub(crate) fn zombie_buddy_loader_cache_mod_at(roots: &[PathBuf]) -> Option<ScannedJarMod> {
    for root in roots {
        for info_path in discover_mod_info_files(root) {
            let Ok(content) = fs::read_to_string(&info_path) else {
                continue;
            };
            let Some(info) = parse_java_mod_info(&content) else {
                continue;
            };
            if workshop_id_from_info_path(&info_path) != Some(ZOMBIE_BUDDY_WORKSHOP_ID) {
                continue;
            }
            if let Some(mod_info) =
                scan_java_mod_jar(&info_path, &info, Some(ZOMBIE_BUDDY_WORKSHOP_ID))
            {
                return Some(mod_info);
            }
        }
    }
    None
}

/// 构造预扫描缓存、校验指纹时使用的**完整**模组清单：JAR 模组 + ZombieBuddy 加载器本体。
///
/// 写入（「刷新 JAR 列表」）与校验（设置页状态行）必须走同一个函数，
/// 否则指纹永远对不上、状态行会一直提示"建议重建"。
pub(crate) fn fast_launch_cache_mods_at(roots: &[PathBuf]) -> Vec<ScannedJarMod> {
    let mut mods = scan_jar_mods_at(roots);
    if let Some(loader) = zombie_buddy_loader_cache_mod_at(roots) {
        mods.push(loader);
    }
    mods
}

pub(crate) fn fast_launch_cache_mods() -> Vec<ScannedJarMod> {
    fast_launch_cache_mods_at(&jar_scan_inputs())
}

/// 读取预扫描缓存（注入器与设置页状态行都读它）。
pub(crate) fn read_fast_launch_cache_at(path: &Path) -> Result<FastLaunchCache, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取预扫描缓存失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析预扫描缓存失败: {error}"))
}

/// 缓存和「当前已批准且启用」的列表是否一致：不一致时注入器必须放弃缓存、走原生扫描。
pub(crate) fn fast_launch_cache_matches(cache: &FastLaunchCache, mods: &[ScannedJarMod]) -> bool {
    cache.format == FAST_LAUNCH_CACHE_FORMAT
        && cache.mod_set_hash == build_fast_launch_cache(mods, cache.generated_at).mod_set_hash
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchCacheStatus {
    pub(crate) exists: bool,
    pub(crate) generated_at: u64,
    pub(crate) mod_count: usize,
    pub(crate) mod_set_hash: String,
    /// 与当前扫描结果是否一致（注入器据此判断能不能用缓存）。
    pub(crate) matches: bool,
}

/// 设置 › 启动 的状态行用：报告缓存是否存在、何时生成、多少个模组、是否仍然一致。
#[tauri::command]
pub(crate) fn fast_launch_cache_status() -> Result<FastLaunchCacheStatus, String> {
    let Some(path) = fast_launch_cache_path() else {
        return Err("拿不到 .pzmanager 目录".to_string());
    };
    if !path.is_file() {
        return Ok(FastLaunchCacheStatus {
            exists: false,
            generated_at: 0,
            mod_count: 0,
            mod_set_hash: String::new(),
            matches: false,
        });
    }
    let cache = read_fast_launch_cache_at(&path)?;
    let mods = fast_launch_cache_mods();
    Ok(FastLaunchCacheStatus {
        exists: true,
        generated_at: cache.generated_at,
        mod_count: cache.mods.len(),
        mod_set_hash: cache.mod_set_hash.clone(),
        matches: fast_launch_cache_matches(&cache, &mods),
    })
}

/// 极速启动开关（读设置；读不到按关闭处理）。
pub(crate) fn fast_launch_enabled() -> bool {
    app_settings_path()
        .ok()
        .and_then(|path| load_app_settings_at(&path).ok())
        .is_some_and(|settings| settings.fast_launch)
}

/// 预扫描缓存文件的路径：`%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`。
pub(crate) fn fast_launch_cache_path() -> Option<PathBuf> {
    pzmanager_directory()
        .ok()
        .map(|directory| directory.join(FAST_LAUNCH_CACHE_FILE))
}

/// 这次 JAR 扫描要不要**重建**预扫描缓存？
///
/// * 用户显式点「刷新 JAR 列表」（`refresh_cache = true`）→ 重建（这是唯一会替换缓存的入口）；
/// * 自动扫描（进页面时触发）→ **只在缓存还不存在时**补一份，已有缓存保持不动。
///
/// 也就是说：缓存一旦生成就持久存在，玩家不动「刷新 JAR 列表」就不会被换掉。
pub(crate) fn should_write_fast_launch_cache(refresh_cache: bool, cache_exists: bool) -> bool {
    refresh_cache || !cache_exists
}

/// 写入缓存文件（目录不存在就创建）。返回写出的路径。
pub(crate) fn write_fast_launch_cache_at(
    directory: &Path,
    cache: &FastLaunchCache,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|error| format!("创建预扫描缓存目录失败: {error}"))?;
    let path = directory.join(FAST_LAUNCH_CACHE_FILE);
    let content = serde_json::to_string_pretty(cache)
        .map_err(|error| format!("序列化预扫描缓存失败: {error}"))?;
    fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("写入预扫描缓存失败: {error}"))?;
    Ok(path)
}

pub(crate) const FAST_LAUNCH_BACKUP_DIR: &str = "fast-launch-backup";

pub(crate) const FAST_LAUNCH_BACKUP_MANIFEST: &str = "manifest.json";

pub(crate) const FAST_LAUNCH_BACKUP_VERSION: u32 = 1;

pub(crate) const FAST_LAUNCH_LAUNCHER_FILES: [&str; 2] = ["ProjectZomboid64.json", "ProjectZomboid64.bat"];

/// 极速启动备份清单：只记录文件是否原本存在，文件内容本身放在同目录下。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchBackupEntry {
    pub(crate) name: String,
    pub(crate) existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchBackupManifest {
    pub(crate) version: u32,
    pub(crate) created_at: String,
    pub(crate) game_path: String,
    pub(crate) files: Vec<FastLaunchBackupEntry>,
}

/// 启用极速启动时告诉前端：备份到了哪里；关闭时告诉前端是否真的恢复了备份。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FastLaunchSaveResult {
    pub(crate) backup_path: Option<String>,
    pub(crate) restored: bool,
    pub(crate) warning: Option<String>,
}

#[cfg(windows)]
pub(crate) fn project_zomboid_install_path() -> Option<PathBuf> {
    let roots = steam_roots();
    let libraries = all_libraries_from_roots(&roots);
    libraries
        .iter()
        .find_map(|library| game_path_from_library(library))
        .map(PathBuf::from)
}

pub(crate) fn fast_launch_backup_directory() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join(FAST_LAUNCH_BACKUP_DIR))
}

/// 备份前把文件里可能残留的极速启动 agent 摘掉：即使上一次开关没走完，
/// 备份也应该代表"没有极速启动"的干净配置。
pub(crate) fn sanitized_fast_launch_backup_content(name: &str, content: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(content) else {
        return content.to_vec();
    };
    match name {
        "ProjectZomboid64.json" => patch_json_launcher_with_agent(text, None, None, None)
            .map(|(patched, _)| patched.into_bytes())
            .unwrap_or_else(|_| content.to_vec()),
        "ProjectZomboid64.bat" => patch_batch_with_agent(text, None, None, None)
            .0
            .into_bytes(),
        _ => content.to_vec(),
    }
}

pub(crate) fn write_fast_launch_backup_at(
    game_path: &Path,
    backup_dir: &Path,
) -> Result<FastLaunchBackupManifest, String> {
    fs::create_dir_all(backup_dir).map_err(|error| format!("创建极速启动备份目录失败: {error}"))?;

    let mut entries = Vec::new();
    for name in FAST_LAUNCH_LAUNCHER_FILES {
        let source = game_path.join(name);
        let existed = source.is_file();
        if existed {
            let content = fs::read(&source)
                .map_err(|error| format!("读取 {} 失败: {error}", source.display()))?;
            let sanitized = sanitized_fast_launch_backup_content(name, &content);
            fs::write(backup_dir.join(name), sanitized)
                .map_err(|error| format!("写入极速启动备份 {} 失败: {error}", source.display()))?;
        }
        entries.push(FastLaunchBackupEntry {
            name: name.to_string(),
            existed,
        });
    }

    if entries.iter().all(|entry| !entry.existed) {
        return Err(
            "找不到 ProjectZomboid64.json 或 ProjectZomboid64.bat，无法备份极速启动配置"
                .to_string(),
        );
    }

    let manifest = FastLaunchBackupManifest {
        version: FAST_LAUNCH_BACKUP_VERSION,
        created_at: current_unix_timestamp()?,
        game_path: game_path.to_string_lossy().into_owned(),
        files: entries,
    };
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("序列化极速启动备份清单失败: {error}"))?;
    fs::write(
        backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST),
        format!("{content}\n"),
    )
    .map_err(|error| format!("写入极速启动备份清单失败: {error}"))?;
    Ok(manifest)
}

/// 先写到临时目录，成功后再替换旧备份；避免备份写到一半把上一份也弄丢。
pub(crate) fn create_fast_launch_backup_at(
    game_path: &Path,
    backup_dir: &Path,
) -> Result<FastLaunchBackupManifest, String> {
    let timestamp = current_unix_timestamp()?;
    let staging = backup_dir.with_file_name(format!("{FAST_LAUNCH_BACKUP_DIR}.tmp-{timestamp}"));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("清理临时极速启动备份失败: {error}"))?;
    }
    let manifest = match write_fast_launch_backup_at(game_path, &staging) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    if backup_dir.exists() {
        if let Err(error) = fs::remove_dir_all(backup_dir) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("清理旧极速启动备份失败: {error}"));
        }
    }
    fs::rename(&staging, backup_dir).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        format!("保存极速启动备份失败: {error}")
    })?;
    Ok(manifest)
}

pub(crate) fn restore_fast_launch_backup_at(game_path: &Path, backup_dir: &Path) -> Result<bool, String> {
    let manifest_path = backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST);
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("读取极速启动备份清单失败: {error}"))?;
    let manifest: FastLaunchBackupManifest = serde_json::from_str(&content)
        .map_err(|error| format!("解析极速启动备份清单失败: {error}"))?;
    if manifest.version != FAST_LAUNCH_BACKUP_VERSION {
        return Err(format!("不支持的极速启动备份版本: {}", manifest.version));
    }

    let backup_game_path = PathBuf::from(manifest.game_path.as_str());
    let same_game_path = match (
        fs::canonicalize(&backup_game_path),
        fs::canonicalize(game_path),
    ) {
        (Ok(backup), Ok(current)) => backup == current,
        _ => backup_game_path.as_path() == game_path,
    };
    if !same_game_path {
        return Err(format!(
            "极速启动备份属于 {}，当前游戏目录是 {}；已保留备份，不自动恢复",
            backup_game_path.display(),
            game_path.display()
        ));
    }

    let safety_dir = game_path
        .join(".pzmanager-backups")
        .join(current_unix_timestamp()?);
    let mut writes = Vec::new();
    let mut removals = Vec::new();
    for entry in &manifest.files {
        if !FAST_LAUNCH_LAUNCHER_FILES.contains(&entry.name.as_str()) {
            return Err(format!("极速启动备份包含未知文件: {}", entry.name));
        }
        let target = game_path.join(&entry.name);
        if entry.existed {
            let backup = fs::read(backup_dir.join(&entry.name))
                .map_err(|error| format!("读取极速启动备份 {} 失败: {error}", entry.name))?;
            if fs::read(&target)
                .ok()
                .as_deref()
                .is_some_and(|current| current == backup.as_slice())
            {
                continue;
            }
            if target.is_file() {
                backup_file(&target, &safety_dir, &entry.name)?;
            }
            writes.push((target, backup));
        } else if target.is_file() {
            backup_file(&target, &safety_dir, &entry.name)?;
            removals.push(target);
        }
    }

    if writes.is_empty() && removals.is_empty() {
        return Ok(true);
    }
    apply_file_transaction(writes)?;
    for path in removals {
        fs::remove_file(&path)
            .map_err(|error| format!("删除极速启动残留文件 {} 失败: {error}", path.display()))?;
    }
    Ok(true)
}

#[tauri::command]
pub(crate) fn save_fast_launch(enabled: bool) -> Result<FastLaunchSaveResult, String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    let was_enabled = settings.fast_launch;

    // 先备份再落盘：备份失败就不启用，避免出现"开关开了但没有可恢复快照"。
    let backup_path = if enabled && !was_enabled {
        create_fast_launch_backup()?
    } else {
        None
    };

    settings.fast_launch = enabled;
    save_app_settings_at(&path, &settings)?;

    if enabled {
        return Ok(FastLaunchSaveResult {
            backup_path: backup_path.map(|path| path.to_string_lossy().into_owned()),
            restored: false,
            warning: None,
        });
    }

    // 关闭时优先整体恢复启用前的启动文件；恢复不了再退回只摘 -agentpath。
    let (restored, warning) = match restore_fast_launch_backup() {
        Ok(true) => (true, None),
        Ok(false) => (
            false,
            Some(
                "未找到可用的极速启动备份（或游戏目录不可用），已尝试只摘除 -agentpath。"
                    .to_string(),
            ),
        ),
        Err(error) => (
            false,
            Some(format!(
                "恢复极速启动备份失败，已只摘除 -agentpath：{error}"
            )),
        ),
    };
    if !restored {
        // 这里要传的是 agent 字面量（-agentlib:zbNative=policy=allow-all），不是方案名。
        // 曾经直接把 normalize_mod_loader_scheme 的结果（"zombieBuddy"/"none"）当
        // policy_agent 传下去，JSON/BAT 的 vmArgs 里会被写成裸的方案名 —— 那不是
        // 合法的 JVM 参数，游戏直接起不来。必须再映射成对应的 agent 字符串。
        let policy_agent = settings
            .mod_loader_scheme
            .as_deref()
            .and_then(normalize_mod_loader_scheme)
            .as_deref()
            .and_then(mod_loader_policy_agent);
        if let Err(error) = apply_launch_loader(None, policy_agent, None) {
            eprintln!("关闭极速启动后清理 -agentpath 失败（不影响设置已保存）: {error}");
        }
    }
    Ok(FastLaunchSaveResult {
        backup_path: None,
        restored,
        warning,
    })
}
