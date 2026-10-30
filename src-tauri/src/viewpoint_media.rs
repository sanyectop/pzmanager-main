//! Viewpoint 截图与光影包管理。
//!
//! 通过 `lib.rs` 的 `pub(crate) use viewpoint_media::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};
use crate::*;
pub(crate) fn list_viewpoint_shots_at(directory: &Path) -> Result<Vec<ManagedFile>, String> {
    list_managed_files(directory, is_supported_image)
}

pub(crate) fn list_viewpoint_shaderpacks_at(directory: &Path) -> Result<Vec<ManagedFile>, String> {
    list_managed_files(directory, is_zip)
}

pub(crate) fn read_viewpoint_shot_at(directory: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = managed_file_path(directory, name, is_supported_image)?;
    if !is_regular_file(&path) {
        return Err("截图不存在或不是普通文件".to_string());
    }
    fs::read(path).map_err(|error| format!("读取截图失败: {error}"))
}

pub(crate) fn import_shaderpack_to(source: &Path, directory: &Path) -> Result<String, String> {
    if !is_zip(source) || !is_regular_file(source) {
        return Err("请选择有效的 ZIP 光影包文件".to_string());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无法读取 ZIP 文件名".to_string())?;
    fs::create_dir_all(directory).map_err(|error| format!("创建光影包目录失败: {error}"))?;
    let destination = directory.join(name);
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                format!("同名光影包已存在：{name}")
            } else {
                format!("创建光影包文件失败: {error}")
            }
        })?;
    let mut input =
        fs::File::open(source).map_err(|error| format!("读取 ZIP 文件失败: {error}"))?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        let _ = fs::remove_file(&destination);
        return Err(format!("复制光影包失败: {error}"));
    }
    Ok(name.to_string())
}

pub(crate) fn delete_shaderpack_at(directory: &Path, name: &str) -> Result<(), String> {
    let path = managed_file_path(directory, name, is_zip)?;
    if !is_regular_file(&path) {
        return Err("光影包不存在或不是普通文件".to_string());
    }
    fs::remove_file(path).map_err(|error| format!("删除光影包失败: {error}"))
}

#[tauri::command(async)]
pub(crate) fn list_viewpoint_shots() -> Result<ManagedDirectory, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shots");
    Ok(ManagedDirectory {
        files: list_viewpoint_shots_at(&directory)?,
        directory: directory.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub(crate) fn read_viewpoint_shot(name: String) -> Result<tauri::ipc::Response, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shots");
    read_viewpoint_shot_at(&directory, &name).map(tauri::ipc::Response::new)
}

#[tauri::command(async)]
pub(crate) fn list_viewpoint_shaderpacks() -> Result<ManagedDirectory, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    Ok(ManagedDirectory {
        files: list_viewpoint_shaderpacks_at(&directory)?,
        directory: directory.to_string_lossy().into_owned(),
    })
}

#[tauri::command(async)]
pub(crate) fn import_viewpoint_shaderpack(source_path: String) -> Result<String, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    import_shaderpack_to(Path::new(&source_path), &directory)
}

#[tauri::command(async)]
pub(crate) fn delete_viewpoint_shaderpack(name: String) -> Result<(), String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    delete_shaderpack_at(&directory, &name)
}
