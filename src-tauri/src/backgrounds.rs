//! 背景图：自定义 / 创意工坊扫描、导入与读取。
//!
//! 通过 `lib.rs` 的 `pub(crate) use backgrounds::*;` 保持原有调用路径。

use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::Emitter;
use crate::*;
pub(crate) static NEXT_WORKSHOP_BACKGROUND_SCAN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackgroundEntry {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) workshop_id: Option<String>,
    pub(crate) mod_name: Option<String>,
}

pub(crate) fn background_entry(
    id: String,
    name: String,
    path: PathBuf,
    source: &str,
    workshop_id: Option<String>,
    mod_name: Option<String>,
) -> BackgroundEntry {
    BackgroundEntry {
        id,
        name,
        path: path.to_string_lossy().into_owned(),
        source: source.to_string(),
        workshop_id,
        mod_name,
    }
}

pub(crate) fn image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp"
            )
        })
}

pub(crate) fn background_storage_directory(kind: &str) -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("backgrounds").join(kind))
}

pub(crate) fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
    let original = Path::new(file_name);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("background");
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png");
    let mut candidate = directory.join(format!("{stem}.{extension}"));
    let mut index = 2;
    while candidate.exists() {
        candidate = directory.join(format!("{stem}-{index}.{extension}"));
        index += 1;
    }
    candidate
}

pub(crate) fn compose_title_background_at(title_paths: [&Path; 4], destination: &Path) -> Result<(), String> {
    let images = title_paths
        .into_iter()
        .map(|path| image::open(path).map_err(|error| format!("读取背景图片失败: {error}")))
        .collect::<Result<Vec<DynamicImage>, _>>()?;
    let top_width = images[0].width().max(images[2].width());
    let bottom_width = images[1].width().max(images[3].width());
    let left_height = images[0].height().max(images[1].height());
    let right_height = images[2].height().max(images[3].height());
    let mut output = RgbaImage::new(top_width + bottom_width, left_height + right_height);

    for (image, x, y) in [
        (&images[0], 0, 0),
        (&images[1], top_width, 0),
        (&images[2], 0, left_height),
        (&images[3], top_width, left_height),
    ] {
        image::imageops::overlay(&mut output, &image.to_rgba8(), x.into(), y.into());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建拼接背景目录失败: {error}"))?;
    }
    output
        .save_with_format(destination, ImageFormat::Png)
        .map_err(|error| format!("保存拼接背景失败: {error}"))
}

pub(crate) fn list_custom_backgrounds_at(directory: &Path) -> Vec<BackgroundEntry> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut backgrounds = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| image_file(&entry.path()))
        .map(|entry| {
            let path = entry.path();
            background_entry(
                format!("custom:{}", path.to_string_lossy()),
                entry.file_name().to_string_lossy().into_owned(),
                path,
                "custom",
                None,
                None,
            )
        })
        .collect::<Vec<_>>();
    backgrounds.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    backgrounds
}

pub(crate) fn workshop_background_roots() -> Vec<PathBuf> {
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
        .collect()
}

pub(crate) fn workshop_backgrounds_at_with_callback<Item>(
    roots: &[PathBuf],
    mut on_item: Item,
) -> Vec<BackgroundEntry>
where
    Item: FnMut(&BackgroundEntry),
{
    let mut entries = Vec::new();
    for root in roots {
        let Ok(workshop_items) = fs::read_dir(root) else {
            continue;
        };
        for workshop_item in workshop_items.flatten() {
            let workshop_path = workshop_item.path();
            if !workshop_item.file_type().is_ok_and(|kind| kind.is_dir())
                || workshop_item
                    .file_name()
                    .to_string_lossy()
                    .parse::<u64>()
                    .is_err()
            {
                continue;
            }
            let workshop_id = workshop_item.file_name().to_string_lossy().into_owned();
            let mod_roots = discover_mod_roots(&workshop_path);
            for mod_root in mod_roots {
                let mod_name = mod_root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "未命名模组".to_string());
                let mut pending = vec![mod_root.clone()];
                let mut title_groups: HashMap<PathBuf, [Option<PathBuf>; 4]> = HashMap::new();

                while let Some(directory) = pending.pop() {
                    let Ok(files) = fs::read_dir(&directory) else {
                        continue;
                    };
                    for entry in files.flatten() {
                        let path = entry.path();
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_dir() {
                            pending.push(path);
                            continue;
                        }
                        if !kind.is_file() || !image_file(&path) {
                            continue;
                        }

                        let lower_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                        if directory.file_name().is_some_and(|name| {
                            name.to_string_lossy().eq_ignore_ascii_case("backgrounds")
                        }) {
                            let entry = background_entry(
                                format!("workshop:{}", path.to_string_lossy()),
                                format!("{mod_name} · {}", entry.file_name().to_string_lossy()),
                                path.clone(),
                                "workshop",
                                Some(workshop_id.clone()),
                                Some(mod_name.clone()),
                            );
                            on_item(&entry);
                            entries.push(entry);
                        }

                        let title_index = match lower_name.as_str() {
                            "title.png" => Some(0),
                            "title2.png" => Some(1),
                            "title3.png" => Some(2),
                            "title4.png" => Some(3),
                            _ => None,
                        };
                        if let Some(index) = title_index {
                            title_groups.entry(directory.clone()).or_default()[index] = Some(path);
                        }
                    }
                }

                for (directory, title_paths) in title_groups {
                    if title_paths.iter().any(Option::is_none) {
                        continue;
                    }
                    let [Some(title), Some(title2), Some(title3), Some(title4)] = title_paths
                    else {
                        continue;
                    };
                    let output_name = format!(
                        "{workshop_id}_{}_{}_title.png",
                        sanitize_background_name(&mod_name),
                        format!(
                            "{:x}",
                            Sha256::digest(directory.to_string_lossy().as_bytes())
                        )
                        .chars()
                        .take(8)
                        .collect::<String>()
                    );
                    let output_path = background_storage_directory("workshop")
                        .unwrap_or_else(|_| PathBuf::from(".pzmanager/backgrounds/workshop"))
                        .join(output_name);
                    if !output_path.is_file()
                        && compose_title_background_at(
                            [&title, &title2, &title3, &title4],
                            &output_path,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let entry = background_entry(
                        format!("workshop:title:{}", directory.to_string_lossy()),
                        format!("{mod_name} · 拼接标题背景"),
                        output_path,
                        "workshop",
                        Some(workshop_id.clone()),
                        Some(mod_name.clone()),
                    );
                    on_item(&entry);
                    entries.push(entry);
                }
            }
        }
    }
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    entries
}

pub(crate) fn workshop_backgrounds_at(roots: &[PathBuf]) -> Vec<BackgroundEntry> {
    workshop_backgrounds_at_with_callback(roots, |_| {})
}

pub(crate) fn discover_mod_roots(workshop_path: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut pending = vec![workshop_path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("mods")
            {
                if let Ok(mod_entries) = fs::read_dir(&path) {
                    roots.extend(
                        mod_entries
                            .flatten()
                            .filter(|mod_entry| {
                                mod_entry.file_type().is_ok_and(|kind| kind.is_dir())
                            })
                            .map(|mod_entry| mod_entry.path()),
                    );
                }
            } else {
                pending.push(path);
            }
        }
    }
    roots.sort();
    roots
}

pub(crate) fn sanitize_background_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn background_path_allowed(path: &Path) -> bool {
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    let custom_directory = background_storage_directory("custom")
        .ok()
        .and_then(|directory| fs::canonicalize(directory).ok());
    let generated_directory = background_storage_directory("workshop")
        .ok()
        .and_then(|directory| fs::canonicalize(directory).ok());
    let in_custom = custom_directory
        .as_deref()
        .is_some_and(|directory| path.starts_with(directory));
    let in_generated = generated_directory
        .as_deref()
        .is_some_and(|directory| path.starts_with(directory));
    let in_workshop = workshop_background_roots()
        .into_iter()
        .any(|root| fs::canonicalize(root).is_ok_and(|directory| path.starts_with(directory)));
    in_custom || in_generated || in_workshop
}

/// 内置背景在 settings.json 的 `backgroundPath` 里存成 `builtin:<id>`（例如 `builtin:default`）：
/// 图片随前端一起打包（`public/backgrounds/`），不是磁盘上的用户文件，
/// 所以不走 background_path_allowed 那份目录校验，前端直接用打包好的 URL 渲染。
pub(crate) const BUILTIN_BACKGROUND_PREFIX: &str = "builtin:";

pub(crate) fn is_builtin_background_path(path: &str) -> bool {
    path.trim().starts_with(BUILTIN_BACKGROUND_PREFIX)
}

pub(crate) fn import_custom_background_at(source: &Path) -> Result<BackgroundEntry, String> {
    if !image_file(source) || !is_regular_file(source) {
        return Err("请选择有效的图片文件".to_string());
    }
    let directory = background_storage_directory("custom")?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建自定义背景目录失败: {error}"))?;
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "无法读取图片文件名".to_string())?;
    let destination = unique_destination(&directory, name);
    fs::copy(source, &destination).map_err(|error| format!("复制自定义背景失败: {error}"))?;
    Ok(background_entry(
        format!("custom:{}", destination.to_string_lossy()),
        name.to_string(),
        destination,
        "custom",
        None,
        None,
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopBackgroundScanUpdate {
    pub(crate) scan_id: u64,
    pub(crate) status: String,
    pub(crate) scanned: usize,
    pub(crate) total: usize,
    pub(crate) item: Option<BackgroundEntry>,
    pub(crate) error: Option<String>,
}

#[tauri::command]
pub(crate) fn start_workshop_background_scan(app: tauri::AppHandle) -> Result<u64, String> {
    let scan_id = NEXT_WORKSHOP_BACKGROUND_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "error".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: Some("创意工坊背景扫描只支持 Windows".to_string()),
                },
            );
        }

        #[cfg(windows)]
        {
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "started".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: None,
                },
            );
            let roots = workshop_background_roots();
            let mut scanned = 0;
            let backgrounds = workshop_backgrounds_at_with_callback(&roots, |item| {
                scanned += 1;
                let _ = app.emit(
                    "workshop-background-scan-update",
                    WorkshopBackgroundScanUpdate {
                        scan_id,
                        status: "item".to_string(),
                        scanned,
                        total: 0,
                        item: Some(item.clone()),
                        error: None,
                    },
                );
            });
            let total = backgrounds.len();
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "completed".to_string(),
                    scanned: total,
                    total,
                    item: None,
                    error: None,
                },
            );
        }
    });
    Ok(scan_id)
}

#[tauri::command]
pub(crate) fn set_background_path(path: String) -> Result<(), String> {
    // `builtin:<id>` = 内置背景（随前端打包，磁盘上没有这个文件，不做目录校验）；
    // 其余一律按自定义图片 / 创意工坊背景校验目录。
    let trimmed = path.trim();
    let stored = if is_builtin_background_path(trimmed) {
        trimmed.to_string()
    } else {
        let candidate = PathBuf::from(trimmed);
        if !background_path_allowed(&candidate) {
            return Err("背景图片路径不在允许的背景目录中".to_string());
        }
        candidate.to_string_lossy().into_owned()
    };
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.background_path = Some(stored);
    save_app_settings_at(&settings_path, &settings)
}

/// 保存内置四季壁纸的手动 / 固定设置；内容由前端校验规整后再落盘。
#[tauri::command]
pub(crate) fn set_wallpaper_effect(effect: serde_json::Value) -> Result<(), String> {
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.wallpaper_effect = Some(effect);
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command]
pub(crate) fn list_custom_backgrounds() -> Result<Vec<BackgroundEntry>, String> {
    Ok(list_custom_backgrounds_at(&background_storage_directory(
        "custom",
    )?))
}

#[tauri::command(async)]
pub(crate) fn scan_workshop_backgrounds() -> Result<Vec<BackgroundEntry>, String> {
    #[cfg(not(windows))]
    {
        return Err("创意工坊背景扫描只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        Ok(workshop_backgrounds_at(&workshop_background_roots()))
    }
}

#[tauri::command(async)]
pub(crate) fn import_custom_background(source_path: String) -> Result<BackgroundEntry, String> {
    import_custom_background_at(Path::new(&source_path))
}

#[tauri::command]
pub(crate) fn read_background(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !background_path_allowed(&path) || !image_file(&path) || !is_regular_file(&path) {
        return Err("背景图片不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取背景图片失败: {error}"))
}
