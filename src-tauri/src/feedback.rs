//! 「设置 › 关于 › 反馈信息」：把**最近一次启动**的日志连同用户描述上传到
//! 中转服务器（`steam-relay/` 的 `POST /feedback`，默认 `pzmlan.sanyec.top`）。
//!
//! 两个日志的来源与"最近一次启动"的界定：
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

/// 单个日志上传上限（超出取尾部）。两个文件合计约 8MB，服务端上限 10MB。
const MAX_LOG_BYTES: usize = 4 * 1024 * 1024;
/// 读文件前的硬上限：超过就只读尾部，避免把超大日志整个读进内存。
const READ_HARD_LIMIT: u64 = 64 * 1024 * 1024;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_DESCRIPTION_CHARS: usize = 5000;
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

pub(crate) async fn submit_feedback(description: String) -> Result<FeedbackUploadResult, String> {
    let description = description
        .trim()
        .chars()
        .take(MAX_DESCRIPTION_CHARS)
        .collect::<String>();
    if description.is_empty() {
        return Err("请先填写错误详情，方便定位问题。".to_string());
    }

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
            "没有找到可上传的日志（%TEMP%\\overlay.log 与 %USERPROFILE%\\Zomboid\\console.txt 都不存在）。"
                .to_string(),
        );
    }

    let sent_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let payload = serde_json::json!({
        "description": description,
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

    Ok(FeedbackUploadResult {
        feedback_id: ack.id.unwrap_or_default(),
        uploaded,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::{since_last_game_launch, tail};

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
