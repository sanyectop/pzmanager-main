//! 文件写入、事务式替换与备份。
//!
//! 通过 `lib.rs` 的 `pub(crate) use fs_utils::*;` 保持原有调用路径。
#![allow(unused_imports)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::*;
pub(crate) fn current_unix_timestamp() -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .map_err(|error| format!("获取时间失败: {error}"))
}

pub(crate) fn write_file(path: &Path, content: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("无效文件路径: {}", path.display()))?;
    let suffix = current_unix_timestamp()?;
    let temp_path = path.with_file_name(format!(
        ".{file_name}.pzmanager-{}-{suffix}.tmp",
        std::process::id()
    ));
    let displaced_path = path.with_file_name(format!(
        ".{file_name}.pzmanager-{}-{suffix}.old",
        std::process::id()
    ));

    fs::write(&temp_path, content)
        .map_err(|error| format!("写入 {} 失败: {error}", path.display()))?;

    if path.exists() {
        fs::rename(path, &displaced_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!("暂存原文件 {} 失败: {error}", path.display())
        })?;
        if let Err(error) = fs::rename(&temp_path, path) {
            let restore_error = fs::rename(&displaced_path, path).err();
            let _ = fs::remove_file(&temp_path);
            return Err(match restore_error {
                Some(restore_error) => format!(
                    "替换 {} 失败: {error}；恢复原文件失败: {restore_error}，原文件保留在 {}",
                    path.display(),
                    displaced_path.display()
                ),
                None => format!("替换 {} 失败: {error}", path.display()),
            });
        }
        let _ = fs::remove_file(displaced_path);
    } else if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("创建 {} 失败: {error}", path.display()));
    }

    Ok(())
}

pub(crate) fn apply_file_transaction(files: Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    let mut snapshots = Vec::with_capacity(files.len());
    for (path, _) in &files {
        let existed = path
            .try_exists()
            .map_err(|error| format!("检查 {} 失败: {error}", path.display()))?;
        let original = if existed {
            Some(fs::read(path).map_err(|error| format!("读取 {} 失败: {error}", path.display()))?)
        } else {
            None
        };
        snapshots.push((path.clone(), original));
    }

    for (index, (path, content)) in files.iter().enumerate() {
        if let Err(write_error) = write_file(path, content) {
            let mut rollback_errors = Vec::new();
            for (restore_path, original) in snapshots[..index].iter().rev() {
                let result = match original {
                    Some(original) => write_file(restore_path, original),
                    None => fs::remove_file(restore_path)
                        .map_err(|error| format!("移除 {} 失败: {error}", restore_path.display())),
                };
                if let Err(error) = result {
                    rollback_errors.push(error);
                }
            }

            if rollback_errors.is_empty() {
                return Err(write_error);
            }
            return Err(format!(
                "{write_error}；回滚未完全成功: {}",
                rollback_errors.join("；")
            ));
        }
    }

    Ok(())
}

pub(crate) fn backup_file(path: &Path, backup_dir: &Path, name: &str) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }

    fs::create_dir_all(backup_dir).map_err(|error| format!("创建备份目录失败: {error}"))?;
    fs::copy(path, backup_dir.join(name))
        .map_err(|error| format!("备份 {} 失败: {error}", path.display()))?;
    Ok(())
}

/// 目录能被创建、且确实写得进文件才算可写。
/// 只判断存在性不够：受限环境下目录可能"可读不可写"。
#[cfg(target_os = "windows")]
pub(crate) fn dir_is_writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".pzmanager-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
