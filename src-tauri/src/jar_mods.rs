//! JAR 模组：扫描、启用状态、ZombieBuddy 决策与模组预设。
//!
//! 通过 `lib.rs` 的 `pub(crate) use jar_mods::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use zip::ZipArchive;
use crate::*;
pub(crate) static NEXT_JAR_SCAN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModStateEntry {
    pub(crate) mod_dir: String,
    pub(crate) mod_id: String,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) package_name: String,
    pub(crate) disabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModStateFile {
    pub(crate) format_version: u32,
    pub(crate) mods: Vec<JarModStateEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModDecisionResult {
    pub(crate) decision: bool,
    pub(crate) lua_mod_enabled: bool,
    pub(crate) lua_mod_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModDecisionUpdate {
    pub(crate) mod_id: String,
    pub(crate) mod_info_path: String,
    pub(crate) package_name: String,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) jar_hash: String,
    pub(crate) decision: bool,
    pub(crate) lua_mod_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModBatchDecisionResult {
    pub(crate) updated: usize,
    pub(crate) lua_mods_enabled: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JavaModInfo {
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) jar_file: String,
    pub(crate) package_name: String,
    pub(crate) poster: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JavaPatchMethod {
    pub(crate) annotation: String,
    pub(crate) method_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JavaClassAnnotations {
    pub(crate) class_name: String,
    pub(crate) patch: bool,
    pub(crate) lua_class: bool,
    pub(crate) patch_methods: Vec<JavaPatchMethod>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScannedJarMod {
    pub(crate) id: String,
    /// The `id` field of the mod's `mod.info`, used to locate the matching Lua mod.
    pub(crate) info_id: String,
    pub(crate) mod_info_path: String,
    /// 整个模组目录（`mod.info` 在 `common/` 或 `42.20/` 时向上找回模组根），
    /// 供「版本记录」直接按目录做版本隔离。
    pub(crate) mod_root: String,
    pub(crate) disabled: bool,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) jar_path: String,
    pub(crate) poster_path: Option<String>,
    pub(crate) package_name: String,
    pub(crate) jar_hash: String,
    pub(crate) patch_classes: Vec<String>,
    pub(crate) patch_methods: Vec<JavaPatchMethod>,
    pub(crate) lua_classes: Vec<String>,
    pub(crate) decision: Option<bool>,
}

pub(crate) fn parse_java_mod_info(content: &str) -> Option<JavaModInfo> {
    let mut name = None;
    let mut id = None;
    let mut jar_file = None;
    let mut package_name = None;
    let mut poster = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }

        match key.trim().to_ascii_lowercase().as_str() {
            "name" if name.is_none() => name = Some(value.to_string()),
            "id" if id.is_none() => id = Some(value.to_string()),
            "javajarfile" if jar_file.is_none() => jar_file = Some(value.to_string()),
            "javapkgname" if package_name.is_none() => package_name = Some(value.to_string()),
            "poster" if poster.is_none() => poster = Some(value.to_string()),
            _ => {}
        }
    }

    Some(JavaModInfo {
        name: name?,
        id: id?,
        jar_file: jar_file?,
        package_name: package_name?,
        poster,
    })
}

pub(crate) fn is_ignored_mod_directory(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".git" | ".vscode" | "tmp" | "temp" | "node_modules"
    )
}

pub(crate) fn discover_mod_info_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut mod_info_files = Vec::new();

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_file() {
                let file_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if file_name == "mod.info" {
                    mod_info_files.push(path);
                } else if file_name == "mod.info.disabled" && !directory.join("mod.info").is_file()
                {
                    mod_info_files.push(path);
                }
            } else if file_type.is_dir()
                && !is_ignored_mod_directory(&entry.file_name().to_string_lossy())
            {
                pending.push(path);
            }
        }
    }

    mod_info_files.sort();
    mod_info_files
}

pub(crate) fn mod_jar_path(mod_info_path: &Path, mod_info: &JavaModInfo) -> Option<PathBuf> {
    let mod_directory = mod_info_path.parent()?;
    let declared_path = Path::new(&mod_info.jar_file);
    let mut candidates = vec![mod_directory.join(declared_path)];

    if let Some(mods_directory) = mod_info_path
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "mods"))
    {
        if let Ok(relative_mod_info) = mod_directory.strip_prefix(mods_directory) {
            let mod_root = mods_directory.join(relative_mod_info.components().next()?);
            candidates.push(mod_root.join(declared_path));
            if mod_directory
                .file_name()
                .is_some_and(|name| name == "common")
            {
                let versions = fs::read_dir(&mod_root).ok()?;
                candidates.extend(versions.flatten().filter_map(|entry| {
                    entry
                        .file_type()
                        .ok()
                        .filter(|file_type| file_type.is_dir())
                        .map(|_| entry.path().join(declared_path))
                }));
            }
        }
    }

    candidates.into_iter().find(|path| path.is_file())
}

pub(crate) fn mod_poster_path(mod_info_path: &Path, mod_info: &JavaModInfo) -> Option<PathBuf> {
    let poster = mod_info.poster.as_deref()?.trim();
    if poster.is_empty() {
        return None;
    }
    let relative_path = Path::new(poster);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let path = mod_info_path.parent()?.join(relative_path);
    (is_supported_image(&path) && is_regular_file(&path)).then_some(path)
}

pub(crate) fn class_annotations(class_bytes: &[u8]) -> Option<JavaClassAnnotations> {
    let (_, class_file) = java_class_rs::parse_classfile(class_bytes).ok()?;
    let class_name_index =
        match java_class_rs::get_entry(&class_file.constant_pool, class_file.this_class)? {
            java_class_rs::ConstantPoolEntry::Class(class_info) => class_info.name_index,
            _ => return None,
        };
    let class_name =
        java_class_rs::get_utf8(&class_file.constant_pool, class_name_index)?.replace('/', ".");
    let mut patch_annotation = false;
    let mut lua_annotation = false;
    let mut patch_methods = Vec::new();

    for attribute in &class_file.attributes {
        let attribute_name =
            java_class_rs::get_utf8(&class_file.constant_pool, attribute.attribute_name_index)?;
        if !matches!(
            attribute_name,
            "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
        ) {
            continue;
        }

        if let java_class_rs::ParsedAttribute::RuntimeVisibleAnnotations(annotations)
        | java_class_rs::ParsedAttribute::RuntimeInvisibleAnnotations(annotations) =
            java_class_rs::parse_specialized_attribute(attribute, &class_file.constant_pool)
        {
            for annotation in annotations.annotations {
                let Some(annotation_descriptor) =
                    java_class_rs::get_utf8(&class_file.constant_pool, annotation.type_index)
                else {
                    continue;
                };
                match annotation_descriptor {
                    "Lme/zed_0xff/zombie_buddy/Patch;" => patch_annotation = true,
                    "Lme/zed_0xff/zombie_buddy/Exposer$LuaClass;" => lua_annotation = true,
                    _ => {}
                }
            }
        }
    }

    for method in &class_file.methods {
        let method_name = java_class_rs::get_utf8(&class_file.constant_pool, method.name_index)?;
        for attribute in &method.attributes {
            let attribute_name =
                java_class_rs::get_utf8(&class_file.constant_pool, attribute.attribute_name_index)?;
            if !matches!(
                attribute_name,
                "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
            ) {
                continue;
            }

            let annotations = match java_class_rs::parse_specialized_attribute(
                attribute,
                &class_file.constant_pool,
            ) {
                java_class_rs::ParsedAttribute::RuntimeVisibleAnnotations(annotations)
                | java_class_rs::ParsedAttribute::RuntimeInvisibleAnnotations(annotations) => {
                    annotations.annotations
                }
                _ => continue,
            };
            for annotation in annotations {
                let Some(annotation_descriptor) =
                    java_class_rs::get_utf8(&class_file.constant_pool, annotation.type_index)
                else {
                    continue;
                };
                let Some(annotation_name) = patch_method_annotation_name(annotation_descriptor)
                else {
                    continue;
                };
                patch_methods.push(JavaPatchMethod {
                    annotation: annotation_name.to_string(),
                    method_name: method_name.to_string(),
                });
            }
        }
    }

    (patch_annotation || lua_annotation || !patch_methods.is_empty()).then_some(
        JavaClassAnnotations {
            class_name,
            patch: patch_annotation,
            lua_class: lua_annotation,
            patch_methods,
        },
    )
}

pub(crate) fn patch_method_annotation_name(descriptor: &str) -> Option<&'static str> {
    let name = descriptor
        .strip_prefix('L')?
        .strip_suffix(';')?
        .rsplit(|character| character == '/' || character == '$')
        .next()?;
    match name {
        "OnEnter" | "OnExit" | "Inject" => Some("Inject"),
        "Overwrite" => Some("Overwrite"),
        "Redirect" => Some("Redirect"),
        "ModifyConstant" => Some("ModifyConstant"),
        "ModifyVariable" => Some("ModifyVariable"),
        _ => None,
    }
}

pub(crate) fn scan_java_mod_jar(
    info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> Option<ScannedJarMod> {
    let jar_path = mod_jar_path(info_path, info)?;
    let mut jar = fs::File::open(&jar_path).ok()?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = jar.read(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }
        digest.update(&buffer[..bytes_read]);
    }
    let jar_hash = format!("{:x}", digest.finalize());
    let file = fs::File::open(&jar_path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let package_prefix = format!("{}/", info.package_name.replace('.', "/"));
    let mut patch_classes = Vec::new();
    let mut patch_methods = Vec::new();
    let mut lua_classes = Vec::new();

    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        let entry_name = entry.name().to_string();
        if !entry_name.starts_with(&package_prefix)
            || !entry_name.ends_with(".class")
            || entry_name.starts_with("META-INF/versions/")
        {
            continue;
        }
        let mut class_bytes = Vec::new();
        if entry.read_to_end(&mut class_bytes).is_err() {
            continue;
        }
        let Some(class_info) = class_annotations(&class_bytes) else {
            continue;
        };
        // 收本包**及其子包**的类。ClassGraph 的 `acceptPackages(pkg)` 是递归的，
        // 而 ZombieBuddy 是按 jar 声明的包名调 `collectPatches`：它自己的补丁类在
        // `me.zed_0xff.zombie_buddy.patches`，mod.info 声明的包却是父包
        // `me.zed_0xff.zombie_buddy`。只收"正好等于声明包"的类会把这部分补丁全漏掉。
        let in_declared_package = class_info
            .class_name
            .rsplit_once('.')
            .map(|(package, _)| package)
            .is_some_and(|package| {
                package == info.package_name.as_str()
                    || package.starts_with(&format!("{}.", info.package_name))
            });
        if !in_declared_package {
            continue;
        }
        if class_info.lua_class {
            lua_classes.push(class_info.class_name.clone());
        }
        if class_info.patch {
            patch_classes.push(class_info.class_name);
        }
        patch_methods.extend(class_info.patch_methods);
    }

    if patch_classes.is_empty() && patch_methods.is_empty() && lua_classes.is_empty() {
        return None;
    }

    patch_classes.sort();
    patch_classes.dedup();
    patch_methods.sort_by(|left, right| {
        left.annotation
            .cmp(&right.annotation)
            .then_with(|| left.method_name.cmp(&right.method_name))
    });
    patch_methods.dedup();
    lua_classes.sort();
    lua_classes.dedup();
    let disabled = info_path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("mod.info.disabled")
    });

    Some(ScannedJarMod {
        id: info.name.clone(),
        info_id: info.id.clone(),
        mod_info_path: info_path.to_string_lossy().into_owned(),
        mod_root: mod_root_for_info_path(info_path)
            .to_string_lossy()
            .into_owned(),
        disabled,
        workshop_id,
        jar_path: jar_path.to_string_lossy().into_owned(),
        poster_path: mod_poster_path(info_path, info)
            .map(|path| path.to_string_lossy().into_owned()),
        package_name: info.package_name.clone(),
        jar_hash,
        patch_classes,
        patch_methods,
        lua_classes,
        decision: Some(!disabled),
    })
}

pub(crate) fn jar_mod_state_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("jar-mod-state.json"))
}

pub(crate) fn read_jar_mod_state(path: &Path) -> Result<JarModStateFile, String> {
    if !path.is_file() {
        return Ok(JarModStateFile::default());
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取 JAR 模组状态失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析 JAR 模组状态失败: {error}"))
}

pub(crate) fn write_jar_mod_state(path: &Path, state: &JarModStateFile) -> Result<(), String> {
    // 注意：这里**不**再删极速启动的缓存（见 `invalidate_fast_launch_cache` 的说明）：
    // 缓存只在用户显式刷新 JAR 列表时重建，平时保持持久化。
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("写入 JAR 模组状态失败: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 JAR 模组状态目录失败: {error}"))?;
    }
    fs::write(path, format!("{content}\n"))
        .map_err(|error| format!("写入 JAR 模组状态失败: {error}"))
}

pub(crate) fn upsert_jar_mod_state_entry(state: &mut JarModStateFile, entry: JarModStateEntry) {
    state.format_version = 1;
    if let Some(existing) = state.mods.iter_mut().find(|existing| {
        existing.mod_dir.eq_ignore_ascii_case(&entry.mod_dir)
            || (existing.workshop_id.is_some()
                && existing.workshop_id == entry.workshop_id
                && existing.mod_id.eq_ignore_ascii_case(&entry.mod_id)
                && existing
                    .package_name
                    .eq_ignore_ascii_case(&entry.package_name))
    }) {
        *existing = entry;
    } else {
        state.mods.push(entry);
    }
}

pub(crate) fn jar_mod_state_matches(
    entry: &JarModStateEntry,
    mod_info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> bool {
    let mod_dir = mod_info_path.parent().unwrap_or(mod_info_path);
    entry
        .mod_dir
        .eq_ignore_ascii_case(&mod_dir.to_string_lossy())
        || (entry.mod_id.eq_ignore_ascii_case(&info.id)
            && entry.workshop_id == workshop_id
            && (entry.package_name.is_empty()
                || entry.package_name.eq_ignore_ascii_case(&info.package_name)))
}

pub(crate) fn apply_jar_mod_state_to_roots(
    roots: &[PathBuf],
    state: &JarModStateFile,
) -> Result<usize, String> {
    if state.mods.is_empty() {
        return Ok(0);
    }
    let candidates = jar_scan_candidates(roots);
    let mut changed = 0;
    for entry in &state.mods {
        let Some((info_path, _, _)) = candidates.iter().find(|(info_path, info, workshop_id)| {
            jar_mod_state_matches(entry, info_path, info, *workshop_id)
        }) else {
            continue;
        };
        let info_path_string = info_path.to_string_lossy().into_owned();
        let enabled = !entry.disabled;
        let currently_enabled = info_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("mod.info"));
        if enabled != currently_enabled {
            set_jar_mod_info_enabled(&info_path_string, enabled)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// 「启动时保留 JAR 模组启用状态」是否开启；读设置失败按老行为（把记录套回文件）处理。
pub(crate) fn jar_mod_use_disk_state_on_startup() -> bool {
    app_settings_path()
        .and_then(|path| load_app_settings_at(&path))
        .is_ok_and(|settings| settings.jar_mod_use_disk_state_on_startup)
}

/// 以磁盘当前状态为基准：把 `mod.info` / `mod.info.disabled` 的实际启用状态
/// 回写进 `jar-mod-state.json`，**不改动任何模组文件**。
///
/// 返回新增或状态发生变化的条目数。
pub(crate) fn adopt_jar_mod_state_from_roots(
    roots: &[PathBuf],
    state: &mut JarModStateFile,
) -> usize {
    let mut changed = 0;
    for (info_path, info, workshop_id) in jar_scan_candidates(roots) {
        let entry = JarModStateEntry {
            mod_dir: mod_dir_string(&info_path),
            mod_id: info.id,
            workshop_id,
            package_name: info.package_name,
            disabled: !jar_info_is_enabled(&info_path),
        };
        // 与 `upsert_jar_mod_state_entry` 用同一套匹配规则，先判断状态是否真的变了。
        let known = state.mods.iter().find(|existing| {
            existing.mod_dir.eq_ignore_ascii_case(&entry.mod_dir)
                || (existing.workshop_id.is_some()
                    && existing.workshop_id == entry.workshop_id
                    && existing.mod_id.eq_ignore_ascii_case(&entry.mod_id)
                    && existing.package_name.eq_ignore_ascii_case(&entry.package_name))
        });
        if known.is_none_or(|existing| existing.disabled != entry.disabled) {
            changed += 1;
        }
        upsert_jar_mod_state_entry(state, entry);
    }
    changed
}

/// 启动 / 自动扫描前的 JAR 模组状态处理：
/// * 默认（设置关闭）：把 `jar-mod-state.json` 的记录套回文件，工坊重下覆盖后仍能恢复用户选择；
/// * 设置开启：以磁盘当前状态为基准，只回写记录，不碰 `mod.info`。
pub(crate) fn sync_jar_mod_state_on_startup(
    roots: &[PathBuf],
    state: &mut JarModStateFile,
    state_path: &Path,
) -> Result<usize, String> {
    if !jar_mod_use_disk_state_on_startup() {
        return apply_jar_mod_state_to_roots(roots, state);
    }
    let changed = adopt_jar_mod_state_from_roots(roots, state);
    if changed > 0 {
        write_jar_mod_state(state_path, state)?;
    }
    Ok(changed)
}

pub(crate) fn jar_scan_candidates(roots: &[PathBuf]) -> Vec<(PathBuf, JavaModInfo, Option<u64>)> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        for info_path in discover_mod_info_files(root) {
            let Ok(content) = fs::read_to_string(&info_path) else {
                continue;
            };
            let Some(info) = parse_java_mod_info(&content) else {
                continue;
            };
            let Some(jar_path) = mod_jar_path(&info_path, &info) else {
                continue;
            };
            let key = (
                jar_path.to_string_lossy().to_ascii_lowercase(),
                info.package_name.clone(),
            );
            if !seen.insert(key) {
                continue;
            }
            let workshop_id = workshop_id_from_info_path(&info_path);
            // 加载器本体不进「JAR 模组」列表（它是方案本身，不是可审批的模组），
            // 但它的补丁类必须进极速启动缓存，见 `zombie_buddy_loader_cache_mod_at`。
            if workshop_id == Some(ZOMBIE_BUDDY_WORKSHOP_ID) {
                continue;
            }
            candidates.push((info_path, info, workshop_id));
        }
    }
    candidates
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JarModScanUpdate {
    pub(crate) scan_id: u64,
    pub(crate) status: String,
    pub(crate) scanned: usize,
    pub(crate) total: usize,
    pub(crate) item: Option<ScannedJarMod>,
    pub(crate) error: Option<String>,
}

pub(crate) fn jar_scan_inputs() -> Vec<PathBuf> {
    all_libraries_from_roots(&steam_roots())
        .into_iter()
        .map(|library| {
            library
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(PROJECT_ZOMBOID_APP_ID)
        })
        .filter(|path| path.is_dir())
        .chain(
            std::env::var_os("USERPROFILE")
                .map(|profile| PathBuf::from(profile).join("Zomboid").join("mods")),
        )
        .filter(|path| path.is_dir())
        .collect()
}

/// 扫描 JAR 模组（后台线程 + `jar-mod-scan-update` 事件推送）。
///
/// `refresh_cache` 决定这次扫描要不要**重建极速启动的预扫描缓存**：
/// * `true` —— 用户显式点了「刷新 JAR 列表」，缓存重建（这也是唯一会替换缓存的入口）；
/// * `false` —— 自动扫描（进「模组管理 › JAR」页面时触发）：**只有在缓存还不存在时**才写一份，
///   已有缓存一律保持不动 —— 玩家要的是"缓存稳定存在，什么时候换由我说了算"。
#[tauri::command]
pub(crate) fn start_zombie_buddy_jar_scan(
    app: tauri::AppHandle,
    refresh_cache: Option<bool>,
) -> Result<u64, String> {
    let refresh_cache = refresh_cache.unwrap_or(false);
    // 在扫描开始前就判断"要不要写缓存"，语义清楚也不会和扫描过程本身打架。
    let cache_exists = fast_launch_cache_path().is_some_and(|path| path.is_file());
    let should_write_cache = should_write_fast_launch_cache(refresh_cache, cache_exists);
    let scan_id = NEXT_JAR_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = app.emit(
                "jar-mod-scan-update",
                JarModScanUpdate {
                    scan_id,
                    status: "error".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: Some("JAR 模组扫描只支持 Windows".to_string()),
                },
            );
        }

        #[cfg(windows)]
        {
            let roots = jar_scan_inputs();
            let state_path = match jar_mod_state_path() {
                Ok(path) => path,
                Err(error) => {
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "error".to_string(),
                            scanned: 0,
                            total: 0,
                            item: None,
                            error: Some(error),
                        },
                    );
                    return;
                }
            };
            let mut state = match read_jar_mod_state(&state_path) {
                Ok(state) => state,
                Err(error) => {
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "error".to_string(),
                            scanned: 0,
                            total: 0,
                            item: None,
                            error: Some(error),
                        },
                    );
                    return;
                }
            };
            if let Err(error) = sync_jar_mod_state_on_startup(&roots, &mut state, &state_path) {
                let _ = app.emit(
                    "jar-mod-scan-update",
                    JarModScanUpdate {
                        scan_id,
                        status: "error".to_string(),
                        scanned: 0,
                        total: 0,
                        item: None,
                        error: Some(error),
                    },
                );
                return;
            }
            let mut candidate_total = 0;
            // 顺手收集扫描结果：极速启动开启时用它生成 ZombieBuddy 预扫描缓存。
            let mut collected: Vec<ScannedJarMod> = Vec::new();
            let _ = scan_jar_mods_at_with_callbacks(
                &roots,
                |total| {
                    candidate_total = total;
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "started".to_string(),
                            scanned: 0,
                            total,
                            item: None,
                            error: None,
                        },
                    );
                },
                |scanned, total, item| {
                    if let Some(item) = item {
                        collected.push(item.clone());
                    }
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "item".to_string(),
                            scanned,
                            total,
                            item: item.cloned(),
                            error: None,
                        },
                    );
                },
            );
            // 极速启动（实验性）：**只有**用户显式刷新 JAR 列表（`refresh_cache`）或缓存还不存在时
            // 才写预扫描缓存；其它扫描一律保持已有缓存不动（缓存持久化，见 `start_zombie_buddy_jar_scan`）。
            // 写失败只意味着"极速启动不会生效"，启动流程本身不受影响，所以这里不打断扫描。
            if should_write_cache && fast_launch_enabled() {
                let generated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_secs())
                    .unwrap_or(0);
                // 加载器本体（ZombieBuddy）也要进缓存，否则强制模式下 ZB 自己的补丁会被丢掉。
                let mut cache_mods = collected;
                if let Some(loader) = zombie_buddy_loader_cache_mod_at(&roots) {
                    cache_mods.push(loader);
                }
                let cache = build_fast_launch_cache(&cache_mods, generated_at);
                if let Some(path) = fast_launch_cache_path() {
                    if let Some(directory) = path.parent() {
                        match write_fast_launch_cache_at(directory, &cache) {
                            Ok(written) => eprintln!(
                                "极速启动：已重建预扫描缓存（{} 个模组）→ {}",
                                cache.mods.len(),
                                written.display()
                            ),
                            Err(error) => eprintln!("极速启动：写预扫描缓存失败: {error}"),
                        }
                    }
                }
            } else if fast_launch_enabled() {
                eprintln!("极速启动：本次是自动扫描且缓存已存在 —— 保持缓存不动（只有「刷新 JAR 列表」会重建）");
            }
            let _ = app.emit(
                "jar-mod-scan-update",
                JarModScanUpdate {
                    scan_id,
                    status: "completed".to_string(),
                    scanned: candidate_total,
                    total: candidate_total,
                    item: None,
                    error: None,
                },
            );
        }
    });
    Ok(scan_id)
}

pub(crate) fn set_jar_mod_info_enabled(mod_info_path: &str, enabled: bool) -> Result<PathBuf, String> {
    // 注意：启用/禁用**不再**删极速启动的缓存（缓存持久化，只在显式刷新 JAR 列表时重建）。
    // 代价是启用列表与缓存可能不一致；注入器侧的桥接类按包名查缓存，
    // 新启用的模组查不到就会回退原生扫描，所以不会因此丢补丁。
    let path = PathBuf::from(mod_info_path);
    let parent = path
        .parent()
        .ok_or_else(|| "JAR 模组 mod.info 路径无效".to_string())?;
    let enabled_path = parent.join("mod.info");
    let disabled_path = parent.join("mod.info.disabled");

    if enabled {
        if enabled_path.is_file() {
            if disabled_path.is_file() {
                fs::remove_file(&disabled_path)
                    .map_err(|error| format!("清理 {} 失败: {error}", disabled_path.display()))?;
            }
        } else if disabled_path.is_file() {
            fs::rename(&disabled_path, &enabled_path)
                .map_err(|error| format!("恢复 {} 失败: {error}", disabled_path.display()))?;
        } else {
            return Err(format!("找不到 {}", enabled_path.display()));
        }
        Ok(enabled_path)
    } else {
        if enabled_path.is_file() {
            if disabled_path.is_file() {
                fs::remove_file(&disabled_path)
                    .map_err(|error| format!("覆盖 {} 失败: {error}", disabled_path.display()))?;
            }
            fs::rename(&enabled_path, &disabled_path)
                .map_err(|error| format!("禁用 {} 失败: {error}", enabled_path.display()))?;
        } else if !disabled_path.is_file() {
            return Err(format!("找不到 {}", enabled_path.display()));
        }
        Ok(disabled_path)
    }
}

#[tauri::command(async)]
pub(crate) fn set_zombie_buddy_mod_decision(
    mod_id: String,
    mod_info_path: String,
    workshop_id: Option<u64>,
    jar_hash: String,
    decision: bool,
    lua_mod_id: Option<String>,
) -> Result<JarModDecisionResult, String> {
    if mod_id.trim().is_empty() || jar_hash.trim().is_empty() {
        return Err("缺少 JAR 模组审批信息".to_string());
    }
    set_jar_mod_info_enabled(&mod_info_path, decision)?;
    let mod_dir = Path::new(&mod_info_path)
        .parent()
        .ok_or_else(|| "JAR 模组路径无效".to_string())?
        .to_string_lossy()
        .into_owned();
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    upsert_jar_mod_state_entry(
        &mut state,
        JarModStateEntry {
            mod_dir,
            mod_id: mod_id.clone(),
            workshop_id,
            package_name: String::new(),
            disabled: !decision,
        },
    );
    write_jar_mod_state(&state_path, &state)?;

    let mut result = JarModDecisionResult {
        decision,
        lua_mod_enabled: false,
        lua_mod_id: None,
    };
    if !decision {
        return Ok(result);
    }

    let Some(target) = find_lua_mod_id(lua_mod_id.as_deref(), &mod_id) else {
        return Ok(result);
    };
    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let mut ids = read_default_mod_ids(&default_path)?;
    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut active = HashSet::new();
    plan_lua_mod_enable(
        &entries,
        &target,
        &mut ids,
        &mut added,
        &mut missing_requires,
        &mut active,
    );
    let ordered = order_mod_ids_by_requires(&entries, &ids);
    if default_path.is_file() {
        let backup_dir = user_zomboid_directory()?
            .join(".pzmanager")
            .join("default-backups")
            .join(current_unix_timestamp()?);
        backup_file(&default_path, &backup_dir, "default.txt")?;
    }
    write_default_mod_ids(&default_path, &ordered)?;
    result.lua_mod_enabled = added.iter().any(|id| id.eq_ignore_ascii_case(&target));
    result.lua_mod_id = Some(target);
    Ok(result)
}

#[tauri::command(async)]
pub(crate) fn set_zombie_buddy_mods_decisions(
    updates: Vec<JarModDecisionUpdate>,
) -> Result<JarModBatchDecisionResult, String> {
    let updates = updates
        .into_iter()
        .filter(|update| !update.mod_id.trim().is_empty() && !update.jar_hash.trim().is_empty())
        .collect::<Vec<_>>();
    if updates.is_empty() {
        return Err("没有选中 JAR 模组".to_string());
    }

    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    for update in &updates {
        set_jar_mod_info_enabled(&update.mod_info_path, update.decision)?;
        let mod_dir = Path::new(&update.mod_info_path)
            .parent()
            .ok_or_else(|| "JAR 模组路径无效".to_string())?
            .to_string_lossy()
            .into_owned();
        upsert_jar_mod_state_entry(
            &mut state,
            JarModStateEntry {
                mod_dir,
                mod_id: update.mod_id.clone(),
                workshop_id: update.workshop_id,
                package_name: update.package_name.clone(),
                disabled: !update.decision,
            },
        );
    }
    write_jar_mod_state(&state_path, &state)?;

    let mut lua_targets = Vec::new();
    if updates.iter().any(|update| update.decision) {
        let entries = lua_mod_entries()?;
        for update in updates.iter().filter(|update| update.decision) {
            if let Some(target) =
                pick_lua_mod_id(&entries, update.lua_mod_id.as_deref(), &update.mod_id)
            {
                if !lua_targets
                    .iter()
                    .any(|value: &String| value.eq_ignore_ascii_case(&target))
                {
                    lua_targets.push(target);
                }
            }
        }
    }

    let lua_mods_enabled = if lua_targets.is_empty() {
        Vec::new()
    } else {
        set_lua_mods_enabled(lua_targets, true)?.changed
    };

    Ok(JarModBatchDecisionResult {
        updated: updates.len(),
        lua_mods_enabled,
    })
}

#[tauri::command(async)]
pub(crate) fn apply_jar_mod_file_state() -> Result<usize, String> {
    let roots = jar_scan_inputs();
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    sync_jar_mod_state_on_startup(&roots, &mut state, &state_path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModPresetJarMod {
    pub(crate) mod_dir: String,
    pub(crate) mod_id: String,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) package_name: String,
    pub(crate) enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModPreset {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) lua_mod_ids: Vec<String>,
    pub(crate) jar_mods: Vec<ModPresetJarMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModPresetFile {
    pub(crate) format_version: u32,
    pub(crate) presets: Vec<ModPreset>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModPresetApplyResult {
    pub(crate) lua_enabled: usize,
    pub(crate) lua_skipped: Vec<String>,
    pub(crate) jar_changed: usize,
}

pub(crate) fn mod_presets_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("mod-presets.json"))
}

pub(crate) fn read_mod_presets(path: &Path) -> Result<ModPresetFile, String> {
    if !path.is_file() {
        return Ok(ModPresetFile::default());
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取模组预设失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析模组预设失败: {error}"))
}

pub(crate) fn write_mod_presets(path: &Path, file: &ModPresetFile) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(file).map_err(|error| format!("写入模组预设失败: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建模组预设目录失败: {error}"))?;
    }
    fs::write(path, format!("{content}\n")).map_err(|error| format!("写入模组预设失败: {error}"))
}

pub(crate) fn mod_dir_string(info_path: &Path) -> String {
    info_path
        .parent()
        .unwrap_or(info_path)
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn jar_info_is_enabled(info_path: &Path) -> bool {
    info_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("mod.info"))
}

pub(crate) fn preset_jar_matches(
    saved: &ModPresetJarMod,
    info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> bool {
    saved
        .mod_dir
        .eq_ignore_ascii_case(&mod_dir_string(info_path))
        || (saved.mod_id.eq_ignore_ascii_case(&info.id)
            && saved.workshop_id == workshop_id
            && saved.package_name.eq_ignore_ascii_case(&info.package_name))
}

/// 把当前 `default.txt` 的启用顺序（保留文件里的原始顺序）和所有 JAR 模组的启用状态存为预设。
#[tauri::command(async)]
pub(crate) fn save_mod_preset(name: String) -> Result<ModPreset, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("预设名称不能为空".to_string());
    }
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let lua_mod_ids = read_default_mod_ids(&default_path)?;
    let jar_mods: Vec<ModPresetJarMod> = jar_scan_candidates(&jar_scan_inputs())
        .into_iter()
        .map(|(info_path, info, workshop_id)| ModPresetJarMod {
            mod_dir: mod_dir_string(&info_path),
            mod_id: info.id,
            workshop_id,
            package_name: info.package_name,
            enabled: jar_info_is_enabled(&info_path),
        })
        .collect();

    let path = mod_presets_path()?;
    let mut file = read_mod_presets(&path)?;
    file.presets.retain(|preset| preset.name != name);
    let preset = ModPreset {
        id: current_unix_timestamp()?,
        name,
        lua_mod_ids,
        jar_mods,
    };
    file.presets.push(preset.clone());
    write_mod_presets(&path, &file)?;
    Ok(preset)
}

/// 一键切换到预设：Lua 按预设顺序重写 `default.txt`；JAR 模组中不在预设里的全部停用。
#[tauri::command(async)]
pub(crate) fn apply_mod_preset(id: String) -> Result<ModPresetApplyResult, String> {
    let file = read_mod_presets(&mod_presets_path()?)?;
    let preset = file
        .presets
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| "找不到该模组预设".to_string())?;

    let entries = lua_mod_entries()?;
    let mut lua_ids = Vec::new();
    let mut lua_skipped = Vec::new();
    for saved in &preset.lua_mod_ids {
        match entries
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(saved))
        {
            Some(entry) if entry.has_b42 => lua_ids.push(entry.id.clone()),
            _ => lua_skipped.push(saved.clone()),
        }
    }
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    if default_path.is_file() {
        let backup_dir = pzmanager_directory()?
            .join("default-backups")
            .join(current_unix_timestamp()?);
        backup_file(&default_path, &backup_dir, "default.txt")?;
    }
    write_default_mod_ids(&default_path, &lua_ids)?;

    let roots = jar_scan_inputs();
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    for (info_path, info, workshop_id) in jar_scan_candidates(&roots) {
        let enabled = preset
            .jar_mods
            .iter()
            .find(|saved| preset_jar_matches(saved, &info_path, &info, workshop_id))
            .is_some_and(|saved| saved.enabled);
        upsert_jar_mod_state_entry(
            &mut state,
            JarModStateEntry {
                mod_dir: mod_dir_string(&info_path),
                mod_id: info.id,
                workshop_id,
                package_name: info.package_name,
                disabled: !enabled,
            },
        );
    }
    write_jar_mod_state(&state_path, &state)?;
    let jar_changed = apply_jar_mod_state_to_roots(&roots, &state)?;

    Ok(ModPresetApplyResult {
        lua_enabled: lua_ids.len(),
        lua_skipped,
        jar_changed,
    })
}

#[tauri::command(async)]
pub(crate) fn list_mod_presets() -> Result<Vec<ModPreset>, String> {
    Ok(read_mod_presets(&mod_presets_path()?)?.presets)
}

#[tauri::command(async)]
pub(crate) fn delete_mod_preset(id: String) -> Result<(), String> {
    let path = mod_presets_path()?;
    let mut file = read_mod_presets(&path)?;
    let before = file.presets.len();
    file.presets.retain(|preset| preset.id != id);
    if file.presets.len() == before {
        return Err("找不到该模组预设".to_string());
    }
    write_mod_presets(&path, &file)
}
