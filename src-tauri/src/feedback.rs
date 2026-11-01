//! 「设置 › 关于 › 反馈信息」：把日志连同用户描述上传到中转服务器
//! （`steam-relay/` 的 `POST /feedback`，默认 `pzmlan.sanyec.top`）。
//!
//! 三个日志的来源与截取口径：
//! - `%USERPROFILE%\Zomboid\.pzmanager\logs\launcher.log`：启动器日志，**整份**的尾部
//!   （最多 1 MB）。用户常是重启启动器之后才反馈，只带"当前会话"会把出问题的那次丢掉。
//! - `%TEMP%\overlay.log`：注入游戏的 pz_overlay.dll **追加**写、跨启动累积，
//!   所以只取最后一个「已加载」标记之后的部分（即最近一次游戏启动）。
//! - `%USERPROFILE%\Zomboid\console.txt`：游戏每次启动重写，本身就是最近一次。
//!
//! 单文件超过 [`MAX_LOG_BYTES`] 时只传尾部并标记 `truncated`，避免一次上传几十 MB。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::steam_relay;

/// 单个日志上传上限（超出取尾部）。三个文件合计约 9MB，服务端上限 10MB。
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;
/// 启动器日志本身有 2MB 轮转上限；反馈只取最近一次启动的 1MB 尾部。
const MAX_LAUNCHER_LOG_BYTES: usize = 1024 * 1024;
/// 读文件前的硬上限：超过就只读尾部，避免把超大日志整个读进内存。
const READ_HARD_LIMIT: u64 = 64 * 1024 * 1024;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DESCRIPTION_CHARS: usize = 5000;
/// 联系方式（可选）最长字数：够填邮箱 / QQ / 群昵称，又不至于把描述撑爆。
const MAX_CONTACT_CHARS: usize = 200;
/// 本机保留的反馈编号条数上限（新的在前）。
const MAX_HISTORY_ENTRIES: usize = 50;
/// overlay.dll 每次被游戏加载都会写这行（`pz_overlay v… 已加载（pid=…）`）。
const OVERLAY_LOAD_MARKER: &str = "已加载";

#[derive(Debug, Serialize)]
struct UploadFile {
    name: &'static str,
    content: String,
    truncated: bool,
    missing: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeedbackUploadResult {
    pub(crate) feedback_id: String,
    pub(crate) uploaded: Vec<String>,
    pub(crate) skipped: Vec<String>,
}

/// 本机记住的一条提交记录（设置 › 关于 ›「已提交的反馈」列表）。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FeedbackHistoryEntry {
    pub(crate) id: String,
    /// Unix 秒；服务器回包没带时间时用本机提交时间。
    pub(crate) sent_at: u64,
    /// 描述前几十个字，方便在列表里认出是哪次反馈。
    pub(crate) summary: String,
}

#[derive(Debug, serde::Deserialize)]
struct FeedbackAck {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// 读文本：文件不存在返回 None；超大只读尾部。非 UTF-8 字节按 lossy 替换，
/// 和 `overlay_session` 读 console.txt 的做法一致。
fn read_text(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let bytes = if metadata.len() > READ_HARD_LIMIT {
        let mut file = std::fs::File::open(path).ok()?;
        let start = metadata.len() - READ_HARD_LIMIT;
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).ok()?;
        buffer
    } else {
        std::fs::read(path).ok()?
    };
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// 按字符边界安全地截取尾部；返回 (内容, 是否被截断)。
fn tail(text: String, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text, false);
    }
    let mut start = text.len() - max_bytes;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    (text[start..].to_string(), true)
}

/// overlay.log 跨启动累积：只保留最后一个「已加载」所在行起的部分。
/// 找不到标记（旧版本 DLL / 文件被清空）时按全量处理。
fn since_last_game_launch(text: &str) -> &str {
    match text.rfind(OVERLAY_LOAD_MARKER) {
        Some(index) => {
            let line_start = text[..index].rfind('\n').map_or(0, |offset| offset + 1);
            &text[line_start..]
        }
        None => text,
    }
}

fn collect_logs() -> Vec<UploadFile> {
    let mut files = Vec::new();

    match crate::launcher_log::full_log_tail(MAX_LAUNCHER_LOG_BYTES) {
        Some((content, truncated)) => files.push(UploadFile {
            name: "launcher.log",
            content,
            truncated,
            missing: false,
        }),
        None => files.push(UploadFile {
            name: "launcher.log",
            content: String::new(),
            truncated: false,
            missing: true,
        }),
    }

    let overlay_path = std::env::temp_dir().join("overlay.log");
    match read_text(&overlay_path) {
        Some(text) => {
            let sliced = since_last_game_launch(&text).to_string();
            let (content, truncated) = tail(sliced, MAX_LOG_BYTES);
            files.push(UploadFile {
                name: "overlay.log",
                content,
                truncated,
                missing: false,
            });
        }
        None => files.push(UploadFile {
            name: "overlay.log",
            content: String::new(),
            truncated: false,
            missing: true,
        }),
    }

    // console.txt 由游戏每次启动重写，本身就是最近一次启动的日志。
    let console_path = crate::user_zomboid_directory()
        .map(|directory| directory.join("console.txt"))
        .ok();
    match console_path.as_deref().and_then(read_text) {
        Some(text) => {
            let (content, truncated) = tail(text, MAX_LOG_BYTES);
            files.push(UploadFile {
                name: "console.txt",
                content,
                truncated,
                missing: false,
            });
        }
        None => files.push(UploadFile {
            name: "console.txt",
            content: String::new(),
            truncated: false,
            missing: true,
        }),
    }

    files
}

/// 描述 + 可选联系方式拼成最终上报文本（联系方式为空时原样返回描述）。
///
/// 中转站的 `POST /feedback` 只有 `description` 一个正文字段，加联系方式不需要动服务端，
/// 直接拼在描述末尾即可。
fn compose_report(description: &str, contact: &str) -> String {
    let contact = contact.trim();
    let description = description.trim();
    if contact.is_empty() {
        description.to_string()
    } else {
        format!("{description}\n\n联系方式：{contact}")
    }
}

pub(crate) async fn submit_feedback(
    description: String,
    contact: Option<String>,
) -> Result<FeedbackUploadResult, String> {
    let description = description
        .trim()
        .chars()
        .take(MAX_DESCRIPTION_CHARS)
        .collect::<String>();
    if description.is_empty() {
        return Err("请先填写错误详情，方便定位问题。".to_string());
    }
    let contact = contact
        .unwrap_or_default()
        .chars()
        .take(MAX_CONTACT_CHARS)
        .collect::<String>();
    let report = compose_report(&description, &contact);

    let logs = collect_logs();
    let uploaded: Vec<String> = logs
        .iter()
        .filter(|file| !file.missing && !file.content.is_empty())
        .map(|file| file.name.to_string())
        .collect();
    let skipped: Vec<String> = logs
        .iter()
        .filter(|file| file.missing || file.content.is_empty())
        .map(|file| file.name.to_string())
        .collect();
    if uploaded.is_empty() {
        return Err(
            "没有找到可上传的日志（launcher.log、%TEMP%\\overlay.log 与 %USERPROFILE%\\Zomboid\\console.txt 都不存在）。"
                .to_string(),
        );
    }

    let sent_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "description": report,
        "client": {
            "appVersion": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "sentAt": sent_at,
        },
        "files": logs,
    });

    let (_, base) = steam_relay::relay_config();
    // 反馈上传直达中转服务器（大陆可达），不跟随系统代理：残留死代理会让
    // 「反馈」本身也发不出去，而此时用户往往正需要它。
    let response = steam_relay::steam_client(true)
        .post(format!("{base}/feedback"))
        .json(&payload)
        .timeout(UPLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("上传反馈失败: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("读取反馈结果失败: {error}"))?;
    let ack: FeedbackAck = serde_json::from_str(&body).unwrap_or(FeedbackAck {
        ok: false,
        id: None,
        error: None,
    });
    if !status.is_success() || !ack.ok {
        let reason = ack
            .error
            .unwrap_or_else(|| format!("服务器返回 {status}"));
        return Err(match reason.as_str() {
            "rate_limited" => "提交太频繁，请稍后再试。".to_string(),
            "body_too_large" => "日志文件过大，上传被服务器拒绝。".to_string(),
            other => format!("上传反馈失败：{other}"),
        });
    }

    let result = FeedbackUploadResult {
        feedback_id: ack.id.unwrap_or_default(),
        uploaded,
        skipped,
    };
    crate::launcher_log::info(&format!(
        "反馈已提交：{}，上传 {}",
        result.feedback_id,
        result.uploaded.join("、")
    ));
    if !result.feedback_id.is_empty() {
        remember_submitted(&result.feedback_id, &description, sent_at);
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// 本机提交记录：设置 › 关于 ›「已提交的反馈」
// ---------------------------------------------------------------------------

fn history_path() -> Result<std::path::PathBuf, String> {
    Ok(crate::pzmanager_directory()?.join("feedback-history.json"))
}

/// 读本机记录；文件不存在 / 损坏都按"空"处理——这只是个方便回查的列表，不能影响主流程。
pub(crate) fn read_history() -> Vec<FeedbackHistoryEntry> {
    let Ok(path) = history_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<FeedbackHistoryEntry>>(&text).unwrap_or_default()
}

/// 记一条提交（新的在最前，最多 [`MAX_HISTORY_ENTRIES`] 条）。写失败只记日志，不影响反馈本身。
fn remember_submitted(id: &str, description: &str, sent_at: u64) {
    let Ok(path) = history_path() else {
        return;
    };
    let mut entries = read_history();
    entries.retain(|entry| entry.id != id);
    entries.insert(
        0,
        FeedbackHistoryEntry {
            id: id.to_string(),
            sent_at,
            summary: summarize(description),
        },
    );
    entries.truncate(MAX_HISTORY_ENTRIES);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&entries) {
        Ok(text) => {
            if std::fs::write(&path, text).is_err() {
                crate::launcher_log::warn("反馈编号记录写入失败（不影响反馈上传）。");
            }
        }
        Err(error) => crate::launcher_log::warn(&format!("反馈编号记录序列化失败：{error}")),
    }
}

/// 描述摘要：取第一行、压缩空白，最多 40 个字。
fn summarize(description: &str) -> String {
    let first_line = description.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let mut summary: String = first_line.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.chars().count() > 40 {
        summary = summary.chars().take(40).collect::<String>() + "…";
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::{compose_report, since_last_game_launch, summarize, tail};

    #[test]
    fn appends_optional_contact_to_the_description() {
        assert_eq!(compose_report("点了启动没反应", ""), "点了启动没反应");
        assert_eq!(compose_report("点了启动没反应", "   "), "点了启动没反应");
        assert_eq!(
            compose_report("  点了启动没反应  ", "  1234@qq.com  "),
            "点了启动没反应\n\n联系方式：1234@qq.com"
        );
    }

    #[test]
    fn summarizes_first_line_for_the_history_list() {
        assert_eq!(summarize("第一行  有 空格\n第二行"), "第一行 有 空格");
        assert_eq!(summarize("\n\n  \n只有一行"), "只有一行");
        assert_eq!(summarize(""), "");
        let long = summarize(&"模".repeat(50));
        assert_eq!(long.chars().count(), 41);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn keeps_only_the_last_game_launch_section() {
        let log = "[pid=111 t=0] pz_overlay v1.0 已加载（pid=111）\n\
                   [pid=111 t=1] 旧的一次启动\n\
                   [pid=222 t=0] pz_overlay v1.0 已加载（pid=222）\n\
                   [pid=222 t=2] 这次的问题\n";
        let sliced = since_last_game_launch(log);
        assert!(sliced.starts_with("[pid=222"));
        assert!(sliced.contains("这次的问题"));
        assert!(!sliced.contains("旧的一次启动"));
    }

    #[test]
    fn falls_back_to_full_text_without_marker() {
        assert_eq!(since_last_game_launch("no marker here"), "no marker here");
    }

    #[test]
    fn tail_is_char_boundary_safe() {
        // 中文每个字符 3 字节：从任意字节位置切都要落在字符边界上。
        // 4 字节只够放最后一个汉字（3 字节），切点会被推到字符边界。
        let (content, truncated) = tail("日志内容".to_string(), 4);
        assert!(truncated);
        assert!(content.chars().all(|character| character != '\u{FFFD}'));
        assert_eq!(content, "容");
    }

    #[test]
    fn tail_keeps_short_text_untouched() {
        let (content, truncated) = tail("short".to_string(), 1024);
        assert!(!truncated);
        assert_eq!(content, "short");
    }
}
