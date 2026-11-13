//! 游戏设置文件 `options.ini` 的解析与修改。
//!
//! 文件位置：`%USERPROFILE%\Zomboid\options.ini`（游戏退出时整份重写）。
//! 格式极简：每行 `key=value`，没有分节 / 注释；值里可能有逗号、分号、空格和中文，
//! 所以**不能**按 Properties 的转义规则重新序列化，只能行级替换（保留其它行原样）。
//!
//! 安全约束（与 `viewpoint-live.properties` 同一套）：
//! - 游戏运行中拒绝写入（游戏退出时会整份覆盖）；
//! - 写前备份到 `.pzmanager/game-options-backups/`（保留最近 5 份）；
//! - 行级替换 + `apply_file_transaction` 原子写 + 回读校验。
//!
//! 通过 `lib.rs` 的 `pub(crate) use game_options::*;` 导出到 crate 根。

use std::fs;
use std::path::{Path, PathBuf};
use crate::*;

/// 备份目录名（与其它启动器备份并列放在 `.pzmanager` 下）。
const GAME_OPTIONS_BACKUP_DIR: &str = "game-options-backups";
/// 备份保留份数。
const GAME_OPTIONS_BACKUP_MAX: usize = 5;

/// `options.ini` 的路径（`%USERPROFILE%\Zomboid\options.ini`）。
pub(crate) fn game_options_path() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?.join("options.ini"))
}

/// 行级文档：保留原始行与换行风格。与 shaderpack 的 PropertiesDocument 不同，
/// 这里**不做反转义**——options.ini 的值就是字面量（`zoomLevels2x=250;225;...`）。
pub(crate) struct GameOptionsDocument {
    lines: Vec<String>,
    newline: &'static str,
    trailing_newline: bool,
}

impl GameOptionsDocument {
    pub(crate) fn parse(content: &str) -> Self {
        let content = content.strip_prefix('\u{feff}').unwrap_or(content);
        let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };
        let trailing_newline = content.ends_with('\n');
        let mut lines: Vec<String> = content
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        if trailing_newline {
            lines.pop();
        }
        Self {
            lines,
            newline,
            trailing_newline,
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut out = self.lines.join(self.newline);
        if self.trailing_newline {
            out.push_str(self.newline);
        }
        out
    }

    /// 全部键值（保持文件顺序；重复键取最后一次的值，但只保留第一次出现的位置）。
    pub(crate) fn entries(&self) -> Vec<(String, String)> {
        let mut result: Vec<(String, String)> = Vec::new();
        for line in &self.lines {
            let Some((key, value)) = split_option_line(line) else {
                continue;
            };
            match result.iter_mut().find(|(existing, _)| *existing == key) {
                Some(entry) => entry.1 = value,
                None => result.push((key, value)),
            }
        }
        result
    }

    pub(crate) fn get(&self, key: &str) -> Option<String> {
        self.lines
            .iter()
            .rev()
            .filter_map(|line| split_option_line(line))
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value)
    }

    /// 修改一个键：已有则替换**最后一行**（重复键取后者生效），没有则追加到末尾。
    /// 返回是否真的改动。
    pub(crate) fn set(&mut self, key: &str, value: &str) -> Result<bool, String> {
        if key.is_empty() || key.contains('\n') || key.contains('\r') {
            return Err(format!("非法的设置项名称: {key:?}"));
        }
        if value.contains('\n') || value.contains('\r') {
            return Err(format!("设置值不能包含换行: {value:?}"));
        }
        let line = format!("{key}={value}");
        let existing = self
            .lines
            .iter()
            .rposition(|candidate| split_option_line(candidate).is_some_and(|(k, _)| k == key));
        match existing {
            Some(index) => {
                if self.lines[index] == line {
                    return Ok(false);
                }
                self.lines[index] = line;
            }
            None => self.lines.push(line),
        }
        Ok(true)
    }
}

/// 拆分一行 `key=value`：空白行 / 没有 `=` 的行返回 None；键去首尾空白，值保持原样
/// （只去掉 `=` 后的一个空格，避免把用户值里的前导空格吃掉）。
pub(crate) fn split_option_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), value.strip_prefix(' ').unwrap_or(value).to_string()))
}

/// 读整份文件；不存在时返回空文档（新建）。
fn read_document(path: &Path) -> Result<(GameOptionsDocument, bool), String> {
    match fs::read(path) {
        Ok(bytes) => Ok((GameOptionsDocument::parse(&decode_options_bytes(&bytes)), true)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok((GameOptionsDocument::parse(""), false))
        }
        Err(error) => Err(format!("读取 {} 失败: {error}", path.display())),
    }
}

/// options.ini 一般是无 BOM 的 UTF-8；个别老版本是系统代码页，读不出就按 Latin-1 兜底。
fn decode_options_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&byte| byte as char).collect(),
    }
}

/// 写前备份（整份复制），只保留最近 5 份。
fn backup_game_options(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let zomboid_dir = user_zomboid_directory()?;
    let backup_dir = zomboid_dir.join(".pzmanager").join(GAME_OPTIONS_BACKUP_DIR);
    fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("创建备份目录 {} 失败: {error}", backup_dir.display()))?;
    let stamp = current_unix_timestamp()?;
    fs::copy(path, backup_dir.join(format!("options.ini.bak.{stamp}")))
        .map_err(|error| format!("备份 {} 失败: {error}", path.display()))?;

    let Ok(entries) = fs::read_dir(&backup_dir) else {
        return Ok(());
    };
    let mut backups: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("options.ini.bak."))
        .collect();
    backups.sort();
    while backups.len() > GAME_OPTIONS_BACKUP_MAX {
        let oldest = backups.remove(0);
        let _ = fs::remove_file(backup_dir.join(oldest));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameOptionEntry {
    pub(crate) key: String,
    pub(crate) value: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GameOptionsSnapshot {
    /// 文件绝对路径（界面上显示 / 方便用户自己找）。
    pub(crate) path: String,
    /// 文件是否存在（不存在 = 游戏还没运行过或已被删除）。
    pub(crate) exists: bool,
    /// 游戏是否正在运行：运行中只读（游戏退出时会整份覆盖）。
    pub(crate) game_running: bool,
    pub(crate) entries: Vec<GameOptionEntry>,
}

#[tauri::command(async)]
pub(crate) fn read_game_options() -> Result<GameOptionsSnapshot, String> {
    let path = game_options_path()?;
    let (document, exists) = read_document(&path)?;
    Ok(GameOptionsSnapshot {
        path: path.to_string_lossy().into_owned(),
        exists,
        game_running: game_running_quick(),
        entries: document
            .entries()
            .into_iter()
            .map(|(key, value)| GameOptionEntry { key, value })
            .collect(),
    })
}

/// 修改一项：行级替换 → 备份 → 原子写 → 回读校验。游戏运行中直接拒绝。
#[tauri::command(async)]
pub(crate) fn save_game_option(key: String, value: String) -> Result<(), String> {
    if game_running_quick() {
        return Err("游戏正在运行：请先退出游戏再修改设置（游戏退出时会重写 options.ini）。".to_string());
    }
    let path = game_options_path()?;
    let (mut document, existed) = read_document(&path)?;
    if !document.set(&key, &value)? {
        return Ok(());
    }
    backup_game_options(&path)?;
    let rendered = document.render();
    apply_file_transaction(vec![(path.clone(), rendered.as_bytes().to_vec())])?;

    // 回读校验：确认写进去的值就是目标值（避免编码 / 原子替换出问题）。
    let (verify, _) = read_document(&path)?;
    let actual = verify.get(&key);
    if actual.as_deref() != Some(value.as_str()) {
        return Err(format!(
            "回读校验失败：{key} 期望 {value:?}，实际 {actual:?}（原文件备份在 .pzmanager/{GAME_OPTIONS_BACKUP_DIR}）"
        ));
    }
    launcher_log::info(&format!(
        "游戏设置：{key}={value}（{}）",
        if existed { "已更新 options.ini" } else { "已创建 options.ini" }
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::GameOptionsDocument;

    #[test]
    fn parses_options_ini_preserving_values_and_order() {
        let content = "version=8\r\n\
                       fullScreen=false\r\n\
                       badHighlightColor=1.0,0.0,0.0\r\n\
                       zoomLevels2x=250;225;200\r\n\
                       voiceRecordDeviceName=麦克风 (YUKUI-780)\r\n\
                       seenNews=\r\n";
        let doc = GameOptionsDocument::parse(content);
        let entries = doc.entries();
        let keys: Vec<&str> = entries.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "version",
                "fullScreen",
                "badHighlightColor",
                "zoomLevels2x",
                "voiceRecordDeviceName",
                "seenNews"
            ]
        );
        assert_eq!(doc.get("zoomLevels2x").as_deref(), Some("250;225;200"));
        assert_eq!(
            doc.get("voiceRecordDeviceName").as_deref(),
            Some("麦克风 (YUKUI-780)")
        );
        assert_eq!(doc.get("seenNews").as_deref(), Some(""));
        // 换行风格原样保留
        assert!(doc.render().contains("\r\n"));
    }

    #[test]
    fn sets_existing_key_in_place_and_appends_missing() {
        let mut doc = GameOptionsDocument::parse("a=1\nb=2\n");
        assert!(doc.set("b", "20").unwrap());
        assert!(doc.set("c", "3").unwrap());
        assert_eq!(doc.render(), "a=1\nb=20\nc=3\n");
        // 写相同值不产生改动
        assert!(!doc.set("b", "20").unwrap());
        // 值里允许逗号 / 分号 / 空格 / 等号
        assert!(doc.set("d", "x=1, 2;3").unwrap());
        assert_eq!(doc.get("d").as_deref(), Some("x=1, 2;3"));
        // 换行值直接拒绝
        assert!(doc.set("e", "a\nb").is_err());
    }

    #[test]
    fn duplicate_keys_use_last_value_but_keep_first_position() {
        let mut doc = GameOptionsDocument::parse("k=old\nother=1\nk=new\n");
        assert_eq!(doc.get("k").as_deref(), Some("new"));
        let entries = doc.entries();
        assert_eq!(entries[0], ("k".to_string(), "new".to_string()));
        // 覆写重复键：改最后一行，前面那行保持原样
        doc.set("k", "newest").unwrap();
        assert_eq!(doc.render(), "k=old\nother=1\nk=newest\n");
    }

    #[test]
    fn strips_bom_and_handles_missing_trailing_newline() {
        let doc = GameOptionsDocument::parse("\u{feff}version=8");
        assert_eq!(doc.get("version").as_deref(), Some("8"));
        assert_eq!(doc.render(), "version=8");
    }
}

#[cfg(test)]
mod real_world_tests {
    use super::*;

    /// 本机体检（默认忽略）：解析真实的 `%USERPROFILE%\Zomboid\options.ini`，
    /// 打印条目数并抽查几个已知键。
    #[test]
    #[ignore]
    fn real_world_options_ini_parses() {
        let Some(profile) = std::env::var_os("USERPROFILE") else {
            return;
        };
        let path = std::path::PathBuf::from(profile).join("Zomboid").join("options.ini");
        if !path.is_file() {
            return;
        }
        let text = fs::read_to_string(&path).unwrap();
        let document = GameOptionsDocument::parse(&text);
        let entries = document.entries();
        println!("options.ini 条目数: {}", entries.len());
        assert!(entries.len() > 50);
        assert!(document.get("version").is_some());
    }
}
