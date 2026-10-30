//! Steam 安装、库目录、账号与 Project Zomboid 路径探测。
//!
//! 通过 `lib.rs` 的 `pub(crate) use steam::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;
use crate::*;
pub(crate) fn steam_user_data_id(profile: &SteamProfile) -> Option<String> {
    const STEAM_ID_64_BASE: u64 = 76_561_197_960_265_728;

    profile
        .steam_id
        .parse::<u64>()
        .ok()?
        .checked_sub(STEAM_ID_64_BASE)
        .filter(|account_id| *account_id <= u32::MAX as u64)
        .map(|account_id| account_id.to_string())
}

pub(crate) fn steam_config_paths(steam_root: &Path, profile: Option<&SteamProfile>) -> Vec<PathBuf> {
    let userdata = steam_root.join("userdata");

    if let Some(profile) = profile {
        let Some(account_id) = steam_user_data_id(profile) else {
            return Vec::new();
        };
        let path = userdata
            .join(account_id)
            .join("config")
            .join("localconfig.vdf");
        return path.is_file().then_some(path).into_iter().collect();
    }

    Vec::new()
}

pub(crate) fn all_libraries_from_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    for root in roots {
        for library in library_paths_from_config(root) {
            if !libraries.contains(&library) {
                libraries.push(library);
            }
        }
    }
    libraries
}

#[allow(dead_code)]
pub(crate) fn steam_executable_from_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join("steam.exe"))
        .find(|path| path.is_file())
}

pub(crate) fn game_path_from_library(library: &Path) -> Option<String> {
    let manifest_path = library
        .join("steamapps")
        .join(format!("appmanifest_{PROJECT_ZOMBOID_APP_ID}.acf"));
    let manifest = fs::read_to_string(manifest_path).ok()?;
    let install_dir = vdf_value(&manifest, "installdir")?;
    if install_dir.is_empty() {
        return None;
    }

    let game_path = library.join("steamapps").join("common").join(install_dir);
    game_path
        .is_dir()
        .then(|| game_path.to_string_lossy().into_owned())
}

pub(crate) fn workshop_path_from_library(library: &Path) -> Option<String> {
    let workshop_path = library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID);

    workshop_path
        .is_dir()
        .then(|| workshop_path.to_string_lossy().into_owned())
}

pub(crate) fn install_locations_from_libraries(libraries: &[PathBuf]) -> Option<(String, PathBuf)> {
    let game_path = libraries
        .iter()
        .find_map(|library| game_path_from_library(library))?;
    let source_path = libraries.iter().find_map(|library| {
        let source_path = zombie_buddy_source_from_library(library)?;
        zombie_buddy_source_has_files(&source_path).then_some(source_path)
    })?;

    Some((game_path, source_path))
}

pub(crate) fn library_paths_from_config(steam_root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![steam_root.to_path_buf()];
    let config_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(config) = fs::read_to_string(config_path) else {
        return paths;
    };

    for (_, path) in parse_vdf_pairs(&config)
        .into_iter()
        .filter(|(key, _)| key == "path")
    {
        let path = PathBuf::from(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    paths
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectZomboidPaths {
    pub(crate) profile: Option<SteamProfile>,
    pub(crate) install_path: Option<String>,
    pub(crate) workshop_path: Option<String>,
    pub(crate) zombie_buddy: ZombieBuddyStatus,
    pub(crate) pz_optimization: PzOptimizationStatus,
    pub(crate) viewpoint_3d: Viewpoint3dStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SteamProfile {
    pub(crate) steam_id: String,
    pub(crate) account_name: String,
    pub(crate) persona_name: String,
}

pub(crate) fn steam_profile_from_loginusers(content: &str) -> Option<SteamProfile> {
    let mut profiles = Vec::new();
    let mut current_steam_id: Option<String> = None;
    let mut account_name: Option<String> = None;
    let mut persona_name: Option<String> = None;
    let mut most_recent = false;

    for line in content.lines() {
        let tokens = parse_quoted_tokens(line);

        if current_steam_id.is_none() {
            if tokens.len() == 1
                && tokens[0].len() == 17
                && tokens[0]
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                current_steam_id = Some(tokens[0].clone());
                account_name = None;
                persona_name = None;
                most_recent = false;
            }
            continue;
        }

        if line.trim() == "}" {
            if let (Some(steam_id), Some(account_name), Some(persona_name)) = (
                current_steam_id.take(),
                account_name.take(),
                persona_name.take(),
            ) {
                profiles.push((
                    SteamProfile {
                        steam_id,
                        account_name,
                        persona_name,
                    },
                    most_recent,
                ));
            }
            most_recent = false;
            continue;
        }

        if tokens.len() < 2 {
            continue;
        }

        match tokens[0].as_str() {
            "AccountName" => account_name = Some(tokens[1].clone()),
            "PersonaName" => persona_name = Some(tokens[1].clone()),
            "MostRecent" => most_recent = tokens[1] == "1",
            _ => {}
        }
    }

    profiles
        .iter()
        .find(|(_, most_recent)| *most_recent)
        .or_else(|| profiles.first())
        .map(|(profile, _)| profile.clone())
}

pub(crate) fn steam_profile_from_root(steam_root: &Path) -> Option<SteamProfile> {
    let loginusers_path = steam_root.join("config").join("loginusers.vdf");
    let content = fs::read_to_string(loginusers_path).ok()?;
    steam_profile_from_loginusers(&content)
}

pub(crate) fn find_project_zomboid_paths() -> Result<ProjectZomboidPaths, String> {
    #[cfg(not(windows))]
    {
        return Err("Project Zomboid detection is only supported on Windows".to_string());
    }

    #[cfg(windows)]
    {
        let mut paths = ProjectZomboidPaths {
            profile: None,
            install_path: None,
            workshop_path: None,
            zombie_buddy: ZombieBuddyStatus {
                available: false,
                installed: false,
                source_path: None,
                missing_components: vec!["Project Zomboid 安装目录".to_string()],
            },
            pz_optimization: PzOptimizationStatus::default(),
            viewpoint_3d: viewpoint_three_d_status(None, &[]),
        };
        let mut selected_steam_configs = Vec::new();
        let mut zombie_buddy_source = None;
        let mut all_libraries = Vec::new();

        for steam_root in steam_roots() {
            if paths.profile.is_none() {
                paths.profile = steam_profile_from_root(&steam_root);
                if paths.profile.is_some() {
                    selected_steam_configs =
                        steam_config_paths(&steam_root, paths.profile.as_ref());
                }
            }

            let libraries = library_paths_from_config(&steam_root);
            for library in &libraries {
                if !all_libraries.contains(library) {
                    all_libraries.push(library.clone());
                }
            }
            if zombie_buddy_source
                .as_deref()
                .is_none_or(|source| !zombie_buddy_source_has_files(source))
            {
                zombie_buddy_source = zombie_buddy_source_from_libraries(&libraries);
            }

            for library in libraries {
                let library_game_path = game_path_from_library(&library);
                let library_workshop_path = workshop_path_from_library(&library);
                if paths.install_path.is_none() {
                    paths.install_path = library_game_path.clone();
                }
                if paths.workshop_path.is_none() {
                    paths.workshop_path = library_workshop_path.clone();
                }
            }
        }

        paths.zombie_buddy = zombie_buddy_status(
            paths.install_path.as_deref().map(Path::new),
            zombie_buddy_source.as_deref(),
            &selected_steam_configs,
        );
        paths.viewpoint_3d =
            viewpoint_three_d_status(paths.install_path.as_deref().map(Path::new), &all_libraries);
        paths.pz_optimization =
            pz_optimization_status(paths.install_path.as_deref().map(Path::new), &all_libraries);

        Ok(paths)
    }
}

#[tauri::command(async)]
pub(crate) fn detect_project_zomboid() -> Result<ProjectZomboidPaths, String> {
    let result = find_project_zomboid_paths();
    match &result {
        Ok(paths) => launcher_log::info(&format!(
            "检测游戏目录：{}",
            paths.install_path.as_deref().unwrap_or("未找到")
        )),
        Err(error) => launcher_log::warn(&format!("检测游戏目录失败：{error}")),
    }
    result
}
