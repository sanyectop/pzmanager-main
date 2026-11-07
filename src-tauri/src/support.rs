//! 支持库：ZombieBuddy、PZ Optimization、Viewpoint 3D 的检测、安装与卸载。
//!
//! 通过 `lib.rs` 的 `pub(crate) use support::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde::Serialize;
use crate::*;
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ZombieBuddyStatus {
    pub(crate) available: bool,
    pub(crate) installed: bool,
    pub(crate) source_path: Option<String>,
    pub(crate) missing_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ZombieBuddyInstallResult {
    pub(crate) changed: bool,
    pub(crate) already_installed: bool,
    pub(crate) backup_path: Option<String>,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PzOptimizationStatus {
    pub(crate) available: bool,
    pub(crate) installed: bool,
    pub(crate) source_path: Option<String>,
    pub(crate) installer_path: Option<String>,
    pub(crate) installed_revision: Option<String>,
    pub(crate) available_revision: Option<String>,
    pub(crate) missing_components: Vec<String>,
    /// 没有匹配当前游戏版本的 pzopt-classes，但找到了官方 install.ps1。
    /// 可以用脚本从 GitHub 下载匹配当前版本的构建。
    pub(crate) force_install_available: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PzOptimizationInstallResult {
    pub(crate) changed: bool,
    pub(crate) already_installed: bool,
    pub(crate) source_path: String,
    pub(crate) installed_revision: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopModStatus {
    pub(crate) name: String,
    pub(crate) workshop_id: String,
    pub(crate) subscribed: bool,
    pub(crate) source_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Viewpoint3dStatus {
    pub(crate) ready: bool,
    pub(crate) mods: Vec<WorkshopModStatus>,
    pub(crate) missing_components: Vec<String>,
}

pub(crate) fn zombie_buddy_source_from_library(library: &Path) -> Option<PathBuf> {
    let source_path = library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID)
        .join(ZOMBIE_BUDDY_MOD_ID)
        .join("mods")
        .join("ZombieBuddy");

    source_path.is_dir().then_some(source_path)
}

pub(crate) fn zombie_buddy_source_has_files(source_path: &Path) -> bool {
    source_path.join("libs").join("zbNative.dll").is_file()
        && source_path.join("libs").join("ZombieBuddy.jar").is_file()
}

pub(crate) fn zombie_buddy_source_from_libraries(libraries: &[PathBuf]) -> Option<PathBuf> {
    let mut first_available = None;

    for library in libraries {
        let Some(source_path) = zombie_buddy_source_from_library(library) else {
            continue;
        };
        if zombie_buddy_source_has_files(&source_path) {
            return Some(source_path);
        }
        if first_available.is_none() {
            first_available = Some(source_path);
        }
    }

    first_available
}

pub(crate) fn workshop_content_path(library: &Path, workshop_id: &str) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID)
        .join(workshop_id)
}

pub(crate) fn workshop_content_is_available(path: &Path) -> bool {
    path.is_dir()
        && fs::read_dir(path)
            .ok()
            .is_some_and(|mut entries| entries.next().is_some())
}

pub(crate) fn viewpoint_source_from_libraries(libraries: &[PathBuf]) -> Option<PathBuf> {
    libraries
        .iter()
        .map(|library| workshop_content_path(library, VIEWPOINT_MOD_ID))
        .find(|path| workshop_content_is_available(path))
}

pub(crate) fn workshop_mod_status(
    name: &str,
    workshop_id: &str,
    libraries: &[PathBuf],
    source_path: Option<PathBuf>,
) -> WorkshopModStatus {
    let content_path = libraries
        .iter()
        .map(|library| workshop_content_path(library, workshop_id))
        .find(|path| workshop_content_is_available(path));
    WorkshopModStatus {
        name: name.to_string(),
        workshop_id: workshop_id.to_string(),
        subscribed: content_path.is_some(),
        source_path: source_path
            .or(content_path)
            .map(|path| path.to_string_lossy().into_owned()),
    }
}

pub(crate) fn viewpoint_three_d_status(_game_path: Option<&Path>, libraries: &[PathBuf]) -> Viewpoint3dStatus {
    let zombie_buddy_source = zombie_buddy_source_from_libraries(libraries);
    let viewpoint_source = viewpoint_source_from_libraries(libraries);
    let mods = vec![
        workshop_mod_status(
            "Project Viewpoint",
            VIEWPOINT_MOD_ID,
            libraries,
            viewpoint_source.clone(),
        ),
        workshop_mod_status(
            "ZombieBuddy",
            ZOMBIE_BUDDY_MOD_ID,
            libraries,
            zombie_buddy_source.clone(),
        ),
    ];
    let mut missing_components = mods
        .iter()
        .filter(|item| !item.subscribed)
        .map(|item| format!("订阅并下载 {}", item.name))
        .collect::<Vec<_>>();

    if zombie_buddy_source.is_none() {
        if mods[1].subscribed {
            missing_components.push("ZombieBuddy/libs 文件".to_string());
        }
    } else if !zombie_buddy_source_has_files(zombie_buddy_source.as_deref().unwrap()) {
        missing_components.push("ZombieBuddy/libs 文件".to_string());
    }

    Viewpoint3dStatus {
        ready: missing_components.is_empty(),
        mods,
        missing_components,
    }
}

pub(crate) fn json_launcher_has_agent(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| {
                    object
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("vmArgs"))
                        .map(|(_, value)| value)
                })
                .and_then(serde_json::Value::as_array)
                .map(|args| {
                    args.iter()
                        .any(|arg| arg.as_str().is_some_and(contains_zombie_buddy_agent))
                })
        })
        .unwrap_or(false)
}

pub(crate) fn batch_has_agent(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .to_ascii_uppercase()
            .starts_with("SET _JAVA_OPTIONS=")
            && line
                .split_once('=')
                .is_some_and(|(_, value)| contains_zombie_buddy_agent(value))
    })
}

pub(crate) fn steam_config_has_both(content: &str) -> bool {
    // VDF 同名键**后者生效**（真实用户文件里出现过两条 LaunchOptions：一条是补丁写的，
    // 一条是用户自己在 Steam 里设的窗口参数）。Steam 实际采用最后一条，所以判定也看
    // 最后一条，而不是"任意一条满足即可"——否则状态会在 Steam 覆写后失真。
    let mut app_depth: Option<i32> = None;
    let mut last_launch_options: Option<String> = None;

    for line in content.lines() {
        if app_depth.is_none() && quoted_key(line, PROJECT_ZOMBOID_APP_ID) {
            app_depth = Some(brace_delta(line));
            continue;
        }

        if let Some(depth) = app_depth {
            let next_depth = depth + brace_delta(line);
            if quoted_key(line, "LaunchOptions") {
                last_launch_options = Some(
                    parse_quoted_tokens(line)
                        .get(1)
                        .cloned()
                        .unwrap_or_default(),
                );
            }

            if next_depth <= 0 {
                app_depth = None;
            } else {
                app_depth = Some(next_depth);
            }
        }
    }

    last_launch_options.is_some_and(|value| steam_launch_value_has_both(&value))
}

pub(crate) fn zombie_buddy_status(
    game_path: Option<&Path>,
    source_path: Option<&Path>,
    steam_configs: &[PathBuf],
) -> ZombieBuddyStatus {
    let mut missing_components = Vec::new();
    let available = source_path.is_some_and(zombie_buddy_source_has_files);

    if let Some(game_path) = game_path {
        if !game_path.join("zbNative.dll").is_file() {
            missing_components.push("zbNative.dll".to_string());
        }
        if !game_path.join("ZombieBuddy.jar").is_file() {
            missing_components.push("ZombieBuddy.jar".to_string());
        }

        let json_ready = fs::read_to_string(game_path.join("ProjectZomboid64.json"))
            .map(|content| json_launcher_has_agent(&content))
            .unwrap_or(false);
        if !json_ready {
            missing_components.push("ProjectZomboid64.json".to_string());
        }

        let batch_ready = fs::read_to_string(game_path.join("ProjectZomboid64.bat"))
            .map(|content| batch_has_agent(&content))
            .unwrap_or(false);
        if !batch_ready {
            missing_components.push("ProjectZomboid64.bat".to_string());
        }

        let steam_ready = !steam_configs.is_empty()
            && steam_configs.iter().all(|path| {
                fs::read_to_string(path)
                    .map(|content| steam_config_has_both(&content))
                    .unwrap_or(false)
            });
        if !steam_ready {
            missing_components.push("Steam LaunchOptions".to_string());
        }
    } else {
        missing_components.push("Project Zomboid 安装目录".to_string());
    }

    ZombieBuddyStatus {
        available,
        installed: game_path.is_some() && missing_components.is_empty(),
        source_path: source_path.map(|path| path.to_string_lossy().into_owned()),
        missing_components,
    }
}

pub(crate) fn simple_properties(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

pub(crate) fn pz_optimization_build_info(classes_path: &Path) -> Option<HashMap<String, String>> {
    let content =
        fs::read_to_string(classes_path.join("pzopt").join("build-info.properties")).ok()?;
    let properties = simple_properties(&content);
    properties.get("revision")?;
    Some(properties)
}

pub(crate) fn pz_optimization_files_complete(classes_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(classes_path.join("pzopt-files.txt")) else {
        return false;
    };
    let files = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    !files.is_empty()
        && files
            .iter()
            .all(|relative| classes_path.join(relative).is_file())
}

pub(crate) fn project_zomboid_revision(game_path: &Path) -> Option<String> {
    let file = fs::File::open(game_path.join("projectzomboid.jar")).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("zombie/GitVersion.class").ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    regex::Regex::new(r"\b[0-9a-f]{10}\b")
        .ok()?
        .find(&text)
        .map(|value| value.as_str().to_string())
}

pub(crate) fn pz_optimization_source_from_libraries(
    libraries: &[PathBuf],
    game_revision: Option<&str>,
) -> Option<(PathBuf, PathBuf, String)> {
    let mut best: Option<(PathBuf, PathBuf, String, u64)> = None;

    for library in libraries {
        let mod_root = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(PZ_OPTIMIZATION_MOD_ID)
            .join("mods")
            .join("PZ_Optimization");
        let Ok(entries) = fs::read_dir(&mod_root) else {
            continue;
        };

        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let version_root = entry.path();
            let classes_path = version_root.join("pzopt-classes");
            let installer_path = version_root.join("install.ps1");
            if !classes_path.is_dir() || !installer_path.is_file() {
                continue;
            }
            let Some(info) = pz_optimization_build_info(&classes_path) else {
                continue;
            };
            let Some(revision) = info.get("revision").cloned() else {
                continue;
            };
            if game_revision.is_some_and(|current| current != revision) {
                continue;
            }
            if !pz_optimization_files_complete(&classes_path) {
                continue;
            }
            let built = info
                .get("built")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if best
                .as_ref()
                .is_none_or(|(_, _, _, best_built)| built > *best_built)
            {
                best = Some((classes_path, installer_path, revision, built));
            }
        }
    }

    best.map(|(classes, installer, revision, _)| (classes, installer, revision))
}

pub(crate) fn pz_optimization_installer_from_libraries(libraries: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, u64)> = None;

    for library in libraries {
        let mod_root = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(PZ_OPTIMIZATION_MOD_ID)
            .join("mods")
            .join("PZ_Optimization");
        let Ok(entries) = fs::read_dir(&mod_root) else {
            continue;
        };

        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let version_root = entry.path();
            let installer = version_root.join("install.ps1");
            if !installer.is_file() {
                continue;
            }
            let built = pz_optimization_build_info(&version_root.join("pzopt-classes"))
                .and_then(|info| info.get("built").and_then(|value| value.parse::<u64>().ok()))
                .unwrap_or(0);
            if best
                .as_ref()
                .is_none_or(|(_, best_built)| built > *best_built)
            {
                best = Some((installer, built));
            }
        }
    }

    best.map(|(installer, _)| installer)
}

pub(crate) fn pz_optimization_status(game_path: Option<&Path>, libraries: &[PathBuf]) -> PzOptimizationStatus {
    let game_revision = game_path.and_then(project_zomboid_revision);
    let source = pz_optimization_source_from_libraries(libraries, game_revision.as_deref());
    let fallback_installer = pz_optimization_installer_from_libraries(libraries);
    let force_install_available = source.is_none() && fallback_installer.is_some();
    let installed_revision = game_path
        .and_then(|path| fs::read_to_string(path.join("pzopt").join("build-info.properties")).ok())
        .and_then(|content| simple_properties(&content).get("revision").cloned());
    let installed = game_path.is_some()
        && (game_path.is_some_and(|path| path.join("pzopt-installed.txt").is_file())
            || installed_revision.is_some());

    let mut missing_components = Vec::new();
    if game_path.is_none() {
        missing_components.push("Project Zomboid 安装目录".to_string());
    }
    if source.is_none() {
        missing_components.push(if game_revision.is_some() {
            "匹配当前游戏版本的 PZ_Optimization Workshop 文件".to_string()
        } else {
            "PZ_Optimization Workshop 文件".to_string()
        });
    }

    let (source_path, installer_path, available_revision) =
        source.map_or((None, fallback_installer.map(|path| path.to_string_lossy().into_owned()), None), |(classes, installer, revision)| {
            (
                Some(classes.to_string_lossy().into_owned()),
                Some(installer.to_string_lossy().into_owned()),
                Some(revision),
            )
        });

    PzOptimizationStatus {
        available: source_path.is_some(),
        installed,
        source_path,
        installer_path,
        installed_revision,
        available_revision,
        missing_components,
        force_install_available,
    }
}

pub(crate) fn install_zombie_buddy_at_paths(
    game_path: &Path,
    source_path: &Path,
    steam_configs: &[PathBuf],
) -> Result<ZombieBuddyInstallResult, String> {
    if !game_path.is_dir() {
        return Err("Project Zomboid 安装目录不存在".to_string());
    }
    if steam_configs.is_empty() {
        return Err("找不到 Steam localconfig.vdf".to_string());
    }

    let before = zombie_buddy_status(Some(game_path), Some(source_path), steam_configs);
    if before.installed {
        return Ok(ZombieBuddyInstallResult {
            changed: false,
            already_installed: true,
            backup_path: None,
        });
    }

    let source_dll = source_path.join("libs").join("zbNative.dll");
    let source_jar = source_path.join("libs").join("ZombieBuddy.jar");
    if !source_dll.is_file() || !source_jar.is_file() {
        return Err("Workshop 中缺少 ZombieBuddy 的核心文件".to_string());
    }

    let json_path = game_path.join("ProjectZomboid64.json");
    let json_content = fs::read_to_string(&json_path)
        .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
    let (json_patched, _) = patch_json_launcher_content(&json_content)?;

    let batch_path = game_path.join("ProjectZomboid64.bat");
    let batch_content = fs::read_to_string(&batch_path)
        .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
    let (batch_patched, _) = patch_batch_content(&batch_content);

    let mut patched_steam_configs = Vec::with_capacity(steam_configs.len());
    for config_path in steam_configs {
        let config_content = fs::read_to_string(config_path)
            .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (config_patched, changed) = patch_steam_launch_options_content(&config_content)?;
        patched_steam_configs.push((config_path.clone(), config_patched, changed));
    }

    let timestamp = current_unix_timestamp()?;
    let backup_dir = game_path.join(".pzmanager-backups").join(timestamp);
    backup_file(&game_path.join("zbNative.dll"), &backup_dir, "zbNative.dll")?;
    backup_file(
        &game_path.join("ZombieBuddy.jar"),
        &backup_dir,
        "ZombieBuddy.jar",
    )?;
    backup_file(
        &game_path.join("ProjectZomboid64.json"),
        &backup_dir,
        "ProjectZomboid64.json",
    )?;
    backup_file(
        &game_path.join("ProjectZomboid64.bat"),
        &backup_dir,
        "ProjectZomboid64.bat",
    )?;
    for (index, (config_path, _, changed)) in patched_steam_configs.iter().enumerate() {
        if *changed {
            backup_file(
                config_path,
                &backup_dir,
                &format!("steam-localconfig-{index}.vdf"),
            )?;
        }
    }

    let mut files = vec![
        (
            game_path.join("zbNative.dll"),
            fs::read(&source_dll).map_err(|error| format!("读取 zbNative.dll 失败: {error}"))?,
        ),
        (
            game_path.join("ZombieBuddy.jar"),
            fs::read(&source_jar).map_err(|error| format!("读取 ZombieBuddy.jar 失败: {error}"))?,
        ),
        (json_path, json_patched.into_bytes()),
        (batch_path, batch_patched.into_bytes()),
    ];
    for (config_path, config_patched, changed) in patched_steam_configs {
        if changed {
            files.push((config_path, config_patched.into_bytes()));
        }
    }

    apply_file_transaction(files)?;

    Ok(ZombieBuddyInstallResult {
        changed: true,
        already_installed: false,
        backup_path: Some(backup_dir.to_string_lossy().into_owned()),
    })
}

pub(crate) fn regular_files_in_directory(directory: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("读取 {} 失败: {error}", directory.display()))?;
    let mut files = Vec::new();

    for entry in entries {
        let path = entry
            .map_err(|error| format!("读取 {} 目录项失败: {error}", directory.display()))?
            .path();
        if !is_regular_file(&path) {
            continue;
        }
        let content =
            fs::read(&path).map_err(|error| format!("读取 {} 失败: {error}", path.display()))?;
        files.push((path, content));
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

pub(crate) fn configure_viewpoint_three_d_at_paths(
    game_path: &Path,
    libraries: &[PathBuf],
    steam_configs: &[PathBuf],
) -> Result<ZombieBuddyInstallResult, String> {
    if !game_path.is_dir() {
        return Err("Project Zomboid 安装目录不存在".to_string());
    }

    let status = viewpoint_three_d_status(Some(game_path), libraries);
    if !status.ready {
        return Err(format!(
            "请先订阅并下载：{}",
            status.missing_components.join("、")
        ));
    }

    let source_path = zombie_buddy_source_from_libraries(libraries)
        .ok_or_else(|| "找不到 ZombieBuddy Workshop 文件".to_string())?;
    let library_files = regular_files_in_directory(&source_path.join("libs"))?;
    if library_files.is_empty() {
        return Err("ZombieBuddy/libs 目录中没有可复制的文件".to_string());
    }

    let json_path = game_path.join("ProjectZomboid64.json");
    let json_content = fs::read_to_string(&json_path)
        .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
    let (json_patched, _) = patch_json_launcher_content(&json_content)?;

    let batch_path = game_path.join("ProjectZomboid64.bat");
    let batch_content = fs::read_to_string(&batch_path)
        .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
    let (batch_patched, _) = patch_batch_content(&batch_content);

    let mut patched_steam_configs = Vec::with_capacity(steam_configs.len());
    for config_path in steam_configs {
        let config_content = fs::read_to_string(config_path)
            .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (config_patched, changed) = patch_steam_launch_options_content(&config_content)?;
        patched_steam_configs.push((config_path.clone(), config_patched, changed));
    }

    let timestamp = current_unix_timestamp()?;
    let backup_dir = game_path.join(".pzmanager-backups").join(timestamp);
    let mut files = Vec::new();
    let mut target_paths = HashSet::new();

    for (source_file, content) in library_files {
        let name = source_file
            .file_name()
            .ok_or_else(|| format!("无效 Workshop 文件路径: {}", source_file.display()))?;
        let name = name.to_string_lossy().into_owned();
        let target = game_path.join(&name);
        if target_paths.insert(target.clone()) {
            files.push((target, content));
        }
    }

    files.push((json_path.clone(), json_patched.into_bytes()));
    files.push((batch_path.clone(), batch_patched.into_bytes()));

    for (config_path, config_patched, changed) in patched_steam_configs {
        if changed {
            files.push((config_path, config_patched.into_bytes()));
        }
    }

    let mut backup_targets = HashSet::new();
    for (path, _) in &files {
        if !backup_targets.insert(path.clone()) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("无效备份文件路径: {}", path.display()))?;
        backup_file(path, &backup_dir, name)?;
    }

    apply_file_transaction(files)?;

    Ok(ZombieBuddyInstallResult {
        changed: true,
        already_installed: false,
        backup_path: Some(backup_dir.to_string_lossy().into_owned()),
    })
}

#[tauri::command(async)]
pub(crate) fn install_zombie_buddy() -> Result<ZombieBuddyInstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("ZombieBuddy 安装只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = steam_roots();
        for steam_root in roots {
            let profile = steam_profile_from_root(&steam_root);
            let steam_configs = steam_config_paths(&steam_root, profile.as_ref());
            if steam_configs.is_empty() {
                continue;
            }

            let libraries = library_paths_from_config(&steam_root);
            let Some((game_path, source_path)) = install_locations_from_libraries(&libraries)
            else {
                continue;
            };

            if steam_configs.iter().try_fold(false, |needs_update, path| {
                let content = fs::read_to_string(path)
                    .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
                let (_, changed) = patch_steam_launch_options_content(&content)?;
                Ok::<_, String>(needs_update || changed)
            })? && steam_is_running()?
            {
                return Err("请先退出 Steam，再安装 ZombieBuddy".to_string());
            }

            return install_zombie_buddy_at_paths(
                Path::new(&game_path),
                &source_path,
                &steam_configs,
            );
        }

        Err("找不到 Project Zomboid 或 ZombieBuddy Workshop 文件".to_string())
    }
}

#[cfg(windows)]
pub(crate) fn run_pz_optimization_installer(
    installer_path: &Path,
    game_path: &Path,
    source_path: Option<&Path>,
) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut command = std::process::Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(installer_path)
        .arg("-Dir")
        .arg(game_path);
    if let Some(source_path) = source_path {
        command.arg("-From").arg(source_path);
    }

    let output = command
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("启动 PZ Optimization 安装器失败: {error}"))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = format!("{stdout}\n{stderr}").trim().to_string();
        return Err(if detail.is_empty() {
            format!("PZ Optimization 安装器退出码：{}", output.status)
        } else {
            format!("PZ Optimization 安装失败：{detail}")
        });
    }
    Ok(())
}

pub(crate) fn install_pz_optimization_impl(force: bool) -> Result<PzOptimizationInstallResult, String> {
    #[cfg(not(windows))]
    {
        let _ = force;
        return Err("PZ Optimization 一键安装只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let game_path = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
            .ok_or_else(|| "找不到 Project Zomboid 安装目录".to_string())?;
        let game_path = PathBuf::from(game_path);
        let status = pz_optimization_status(Some(&game_path), &libraries);

        if status.installed
            && status.installed_revision.is_some()
            && status.installed_revision == status.available_revision
        {
            return Ok(PzOptimizationInstallResult {
                changed: false,
                already_installed: true,
                source_path: status.source_path.unwrap_or_default(),
                installed_revision: status.installed_revision,
            });
        }

        if crate::overlay::imp::find_game_process().is_some() {
            return Err(
                "请先完全退出 Project Zomboid，再执行 PZ Optimization 一键安装。".to_string(),
            );
        }

        let source_path = status.source_path.as_deref().map(PathBuf::from);
        let installer_path = status
            .installer_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| "找不到 PZ Optimization 官方安装脚本 install.ps1。".to_string())?;

        if force && source_path.is_none() {
            // 脚本会优先使用自己旁边的 pzopt-classes（那是旧版本，revision 不匹配）；
            // 复制到临时目录后，它才会走“按当前游戏 revision 从 GitHub 下载匹配构建”的分支。
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or(0);
            let temp_dir = std::env::temp_dir().join(format!(
                "pzmanager-pzopt-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&temp_dir)
                .map_err(|error| format!("创建 PZ Optimization 临时目录失败：{error}"))?;
            let temp_installer = temp_dir.join("install.ps1");
            fs::copy(&installer_path, &temp_installer)
                .map_err(|error| format!("复制 PZ Optimization 安装脚本失败：{error}"))?;

            launcher_log::info(&format!(
                "强制安装 PZ Optimization：使用官方脚本 {}（从 GitHub 下载匹配当前游戏版本的构建）",
                installer_path.display()
            ));
            let result = run_pz_optimization_installer(&temp_installer, &game_path, None);
            let _ = fs::remove_dir_all(&temp_dir);
            result?;
        } else {
            let source_path = source_path.ok_or_else(|| {
                "找不到匹配当前游戏版本的 PZ_Optimization Workshop 文件，请先在 Steam 订阅并等待下载完成。"
                    .to_string()
            })?;
            launcher_log::info(&format!(
                "安装 PZ Optimization：源 {}，安装器 {}，游戏 {}",
                source_path.display(),
                installer_path.display(),
                game_path.display()
            ));
            run_pz_optimization_installer(&installer_path, &game_path, Some(&source_path))?;
        }

        let installed = pz_optimization_status(Some(&game_path), &libraries);
        if !installed.installed {
            return Err("PZ Optimization 安装器已结束，但未检测到安装清单".to_string());
        }
        launcher_log::info(&format!(
            "PZ Optimization 安装完成（revision {}）。",
            installed.installed_revision.as_deref().unwrap_or("未知")
        ));

        Ok(PzOptimizationInstallResult {
            changed: true,
            already_installed: false,
            source_path: status.source_path.unwrap_or_default(),
            installed_revision: installed.installed_revision,
        })
    }
}

#[tauri::command(async)]
pub(crate) fn install_pz_optimization(force: Option<bool>) -> Result<PzOptimizationInstallResult, String> {
    install_pz_optimization_impl(force.unwrap_or(false))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PzOptimizationUninstallResult {
    pub(crate) changed: bool,
    pub(crate) message: String,
}

/// 卸载 PZ Optimization：调用安装器的 `-Uninstall`，按安装清单还原游戏目录里的类文件与启动文件。
///
/// 安装器自带 `pzopt\uninstall\install.ps1`（安装时复制到游戏目录），优先使用它，
/// 这样即使已经在创意工坊取消订阅也能卸载。游戏运行中直接拒绝，不让安装器无限等待。
#[tauri::command(async)]
pub(crate) fn uninstall_pz_optimization() -> Result<PzOptimizationUninstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("PZ Optimization 卸载只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let game_path = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
            .ok_or_else(|| "找不到 Project Zomboid 安装目录".to_string())?;
        let game_path = PathBuf::from(game_path);
        let status = pz_optimization_status(Some(&game_path), &libraries);

        if !status.installed {
            return Ok(PzOptimizationUninstallResult {
                changed: false,
                message: "PZ Optimization 未安装，无需卸载。".to_string(),
            });
        }

        if crate::overlay::imp::find_game_process().is_some() {
            return Err("请先完全退出 Project Zomboid，再卸载 PZ Optimization。".to_string());
        }

        let game_installer = game_path
            .join("pzopt")
            .join("uninstall")
            .join("install.ps1");
        let installer_path = if game_installer.is_file() {
            game_installer
        } else {
            status
                .installer_path
                .as_deref()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    "找不到 PZ Optimization 卸载脚本（pzopt\\uninstall\\install.ps1）。".to_string()
                })?
        };
        launcher_log::info(&format!(
            "卸载 PZ Optimization：安装器 {}，游戏 {}",
            installer_path.display(),
            game_path.display()
        ));

        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&installer_path)
            .arg("-Dir")
            .arg(&game_path)
            .arg("-Uninstall")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("启动 PZ Optimization 卸载器失败: {error}"))?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = format!("{stdout}\n{stderr}").trim().to_string();
            return Err(if detail.is_empty() {
                format!("PZ Optimization 卸载器退出码：{}", output.status)
            } else {
                format!("PZ Optimization 卸载失败：{detail}")
            });
        }

        let after = pz_optimization_status(Some(&game_path), &libraries);
        if after.installed {
            return Err("卸载器已结束，但仍检测到 PZ Optimization 安装清单".to_string());
        }
        launcher_log::info("PZ Optimization 已卸载。");

        Ok(PzOptimizationUninstallResult {
            changed: true,
            message: "PZ Optimization 已卸载，下次启动为原版游戏。".to_string(),
        })
    }
}

#[tauri::command(async)]
pub(crate) fn configure_viewpoint_three_d() -> Result<ZombieBuddyInstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("Viewpoint 3D 模组配置只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let Some(game_path) = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
        else {
            return Err("找不到 Project Zomboid 安装目录".to_string());
        };

        let mut steam_configs = Vec::new();
        for root in &roots {
            let profile = steam_profile_from_root(root);
            steam_configs.extend(steam_config_paths(root, profile.as_ref()));
        }
        steam_configs.sort();
        steam_configs.dedup();

        let needs_steam_update = steam_configs.iter().try_fold(false, |needs_update, path| {
            let content = fs::read_to_string(path)
                .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
            let (_, changed) = patch_steam_launch_options_content(&content)?;
            Ok::<_, String>(needs_update || changed)
        })?;
        if needs_steam_update && steam_is_running()? {
            return Err("请先退出 Steam，再配置 Project Viewpoint 3D 模组".to_string());
        }

        configure_viewpoint_three_d_at_paths(Path::new(&game_path), &libraries, &steam_configs)
    }
}
