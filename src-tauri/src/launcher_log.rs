//! 启动器日志：追加写入 `<用户 Zomboid>/.pzmanager/logs/launcher.log`。
//!
//! 设计目标：
//! - 默认只记录启动器自己的关键事件（启动、检测、游戏启动、版本操作、
//!   PZ Optimization 安装/卸载、前端错误），不记录游戏内部日志。
//! - 单文件超过 2 MB 自动轮转，保留 3 份历史，避免无限增长。
//! - 每次启动写一条会话标记；提交反馈时只附带最近一次会话的部分。
//! - 日志只保存在本机，不会自动上传。

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

const LOG_DIR: &str = "logs";
const LOG_FILE: &str = "launcher.log";
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_BACKUPS: usize = 3;
const MAX_MESSAGE_CHARS: usize = 8000;
const READ_HARD_LIMIT: u64 = 16 * 1024 * 1024;
const DEFAULT_TAIL_BYTES: u64 = 256 * 1024;
/// 每次启动写一行，反馈上传时从这里切出“最近一次启动”的日志。
pub(crate) const SESSION_MARKER: &str = "=== launcher session ===";

static LOG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherLogTail {
    pub path: String,
    pub content: String,
    pub size: u64,
    pub exists: bool,
    pub truncated: bool,
}

fn log_file_path() -> Option<PathBuf> {
    crate::pzmanager_directory()
        .ok()
        .map(|directory| directory.join(LOG_DIR).join(LOG_FILE))
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn format_utc(seconds: u64) -> String {
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

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}Z")
}

fn normalize_message(message: &str) -> String {
    let cleaned = message.replace('\r', "");
    let mut chars = cleaned.chars();
    let mut truncated: String = chars.by_ref().take(MAX_MESSAGE_CHARS).collect();
    if chars.next().is_some() {
        truncated.push_str(" …(truncated)");
    }
    truncated
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| LOG_FILE.into());
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

fn rotate_if_needed_at(path: &Path, threshold: u64, backups: usize) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if !metadata.is_file() || metadata.len() < threshold {
        return;
    }
    if backups == 0 {
        let _ = fs::remove_file(path);
        return;
    }

    let _ = fs::remove_file(rotated_path(path, backups));
    for index in (1..backups).rev() {
        let from = rotated_path(path, index);
        if from.is_file() {
            let _ = fs::rename(&from, rotated_path(path, index + 1));
        }
    }
    let _ = fs::rename(path, rotated_path(path, 1));
}

fn append_at(path: &Path, level: &str, message: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let _guard = LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    rotate_if_needed_at(path, MAX_LOG_BYTES, MAX_BACKUPS);

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let timestamp = format_utc(now_seconds());
    let message = normalize_message(message);
    if message.is_empty() {
        writeln!(file, "{timestamp} [{level}]")?;
    } else {
        for line in message.lines() {
            writeln!(file, "{timestamp} [{level}] {line}")?;
        }
    }
    Ok(())
}

fn write_log(level: &str, message: &str) {
    // 测试进程与启动器共用 %USERPROFILE%\Zomboid\.pzmanager\logs\launcher.log；
    // cargo test 触发的日志（如熔断测试）会写进真实日志文件污染用户日志，
    // 所以测试构建下一律不落盘（launcher_log 自身测试只用 _at 变体 + 临时目录）。
    if cfg!(test) {
        return;
    }
    let Some(path) = log_file_path() else {
        return;
    };
    let _ = append_at(&path, level, message);
}

pub(crate) fn info(message: &str) {
    write_log("INFO", message);
}

pub(crate) fn warn(message: &str) {
    write_log("WARN", message);
}

pub(crate) fn error(message: &str) {
    write_log("ERROR", message);
}

pub(crate) fn frontend(level: &str, message: &str) {
    let level = match level.trim().to_ascii_lowercase().as_str() {
        "warn" | "warning" => "WARN",
        "error" => "ERROR",
        "debug" => "DEBUG",
        _ => "INFO",
    };
    write_log(level, &format!("[前端] {message}"));
}

/// 启动时写会话标记；反馈上传只取这个标记之后的部分。
pub(crate) fn start_session() {
    let path = log_file_path();
    let path_label = path
        .as_ref()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "不可用".to_string());
    info(SESSION_MARKER);
    info(&format!(
        "启动器 v{} · {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    info(&format!("日志文件：{path_label}"));
}

fn read_text_limited(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let bytes = if metadata.len() > READ_HARD_LIMIT {
        let mut file = fs::File::open(path).ok()?;
        file.seek(SeekFrom::Start(metadata.len() - READ_HARD_LIMIT))
            .ok()?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).ok()?;
        buffer
    } else {
        fs::read(path).ok()?
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// 取文本尾部：按字节截断并回退到字符边界（中文不会被切出半个字）。
fn tail_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_string(), true)
}

/// 给反馈上传用：整份启动器日志的尾部。
///
/// 以前只带"最近一次会话"，但用户常常是**重启启动器之后**才来反馈，
/// 那样出问题的那次会话就丢了。现在带上整份日志（单文件 2 MB 轮转，尾部 1 MB 足够），
/// 里面可能包含更早的启动器会话。
pub(crate) fn full_log_tail(max_bytes: usize) -> Option<(String, bool)> {
    let path = log_file_path()?;
    let text = read_text_limited(&path)?;
    Some(tail_bytes(&text, max_bytes))
}

fn read_tail_at(path: &Path, limit: u64) -> Result<LauncherLogTail, String> {
    let limit = limit.clamp(4096, READ_HARD_LIMIT);
    let path_label = path.to_string_lossy().into_owned();
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(LauncherLogTail {
                path: path_label,
                content: String::new(),
                size: 0,
                exists: false,
                truncated: false,
            });
        }
    };
    if !metadata.is_file() {
        return Ok(LauncherLogTail {
            path: path_label,
            content: String::new(),
            size: 0,
            exists: false,
            truncated: false,
        });
    }

    let size = metadata.len();
    let start = size.saturating_sub(limit);
    let mut file =
        fs::File::open(path).map_err(|error| format!("读取启动器日志失败：{error}"))?;
    if start > 0 {
        file.seek(SeekFrom::Start(start))
            .map_err(|error| format!("读取启动器日志失败：{error}"))?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("读取启动器日志失败：{error}"))?;
    let mut content = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        // 从中间截断时丢掉第一行残片，保证界面从完整行开始。
        if let Some(index) = content.find('\n') {
            content = content[index + 1..].to_string();
        }
    }
    Ok(LauncherLogTail {
        path: path_label,
        content,
        size,
        exists: true,
        truncated: start > 0,
    })
}

pub(crate) fn read_tail(limit: Option<u64>) -> Result<LauncherLogTail, String> {
    let path = log_file_path().ok_or_else(|| {
        "找不到用户 Zomboid 目录，无法读取启动器日志。".to_string()
    })?;
    read_tail_at(&path, limit.unwrap_or(DEFAULT_TAIL_BYTES))
}

pub(crate) fn clear() -> Result<(), String> {
    let path = log_file_path().ok_or_else(|| {
        "找不到用户 Zomboid 目录，无法清空启动器日志。".to_string()
    })?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建日志目录失败：{error}"))?;
    }
    {
        let _guard = LOG_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|error| format!("清空启动器日志失败：{error}"))?;
    }
    info("日志已清空。");
    Ok(())
}

pub(crate) fn open_folder() -> Result<(), String> {
    let path = log_file_path().ok_or_else(|| {
        "找不到用户 Zomboid 目录，无法打开日志文件夹。".to_string()
    })?;
    let directory = path
        .parent()
        .ok_or_else(|| "启动器日志路径无效。".to_string())?;
    fs::create_dir_all(directory).map_err(|error| format!("创建日志目录失败：{error}"))?;

    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(directory)
            .spawn()
            .map_err(|error| format!("打开日志文件夹失败：{error}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("打开日志文件夹只支持 Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_as_utc() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00Z");
        assert_eq!(format_utc(86_400), "1970-01-02 00:00:00Z");
    }

    #[test]
    fn reads_tail_and_reports_truncation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launcher.log");
        let mut content = String::new();
        for index in 0..2000 {
            content.push_str(&format!("line {index:04}\n"));
        }
        fs::write(&path, content).unwrap();

        let tail = read_tail_at(&path, 4096).unwrap();
        assert!(tail.exists);
        assert!(tail.truncated);
        assert!(tail.content.contains("line 1999"));
        assert!(!tail.content.contains("line 0000"));
    }

    #[test]
    fn rotates_when_threshold_exceeded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("launcher.log");
        fs::write(&path, b"0123456789abcdef").unwrap();

        rotate_if_needed_at(&path, 8, 3);
        assert!(!path.exists());
        assert!(rotated_path(&path, 1).is_file());

        fs::write(&path, b"second").unwrap();
        rotate_if_needed_at(&path, 3, 3);
        assert!(rotated_path(&path, 1).is_file());
        assert!(rotated_path(&path, 2).is_file());
    }

    #[test]
    fn tail_bytes_keeps_the_end_and_respects_char_boundaries() {
        let text = "旧会话\n=== launcher session ===\n最近会话\n工作记录\n";
        let (tail, truncated) = tail_bytes(text, 1024);
        assert!(!truncated);
        assert_eq!(tail, text);
        // 中文按 3 字节切：只留最后 4 字节时要落在字符边界上，且不带替换符。
        let (tail, truncated) = tail_bytes("日志内容", 4);
        assert!(truncated);
        assert_eq!(tail, "容");
        assert!(!tail.contains('\u{FFFD}'));
    }
}
