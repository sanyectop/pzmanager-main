//! Project Zomboid 覆盖层 / 启动器共享的纯解析逻辑。
//!
//! 这里只放不依赖 Windows、Tauri 或文件系统的函数：阶段识别、advice 计数、
//! 完成标记、空闲判定、进度估算和控制台行规范化。启动器与注入 DLL 共用同一份，
//! 避免两边的完成条件/阶段名漂移。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// 主菜单/加载完成标记（游戏自身与 ZombieBuddy 的控制台输出）。
pub const COMPLETION_MARKERS: [&str; 3] = [
    "STATE: exit zombie.gameStates.TermsOfServiceState",
    "added to the main menu",
    "ZombieBuddy.getVersion()",
];

/// 允许「静默即完成」的收尾阶段。
pub const SILENCE_COMPLETION_PHASES: [&str; 2] = ["注册 Lua 接口", "初始化完成回调"];

/// 收尾阶段的 `[ZB]` 静默阈值。
pub const ZB_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
/// 其它阶段的静默兜底阈值（ZB 中途联网/ClassGraph 扫描会长时间无输出）。
///
/// 实测联网（Authors list fetch）最长静默 41 秒，60 秒留了余量；再长就会让
/// 加载界面压在已经能玩的游戏上（42.22 上实测到过 2 分钟仍未收尾）。
pub const ZB_IDLE_TIMEOUT_ANY_PHASE: Duration = Duration::from_secs(60);

/// 覆盖层左下角保留的控制台行数。
pub const MAX_CONSOLE_LINES: usize = 6;
/// 单行控制台输出的最大字符数（超出加省略号）。
pub const MAX_CONSOLE_LINE_CHARS: usize = 120;

/// 从 `[ZB]` 行推导当前加载阶段（用于界面显示）。
pub fn zb_phase(line: &str) -> &'static str {
    if line.contains("ZomboidFileSystem.loadMods") {
        "扫描模组列表"
    } else if line.contains("java mod list to load") {
        "读取 Java 模组清单"
    } else if line.contains("Java mod approvals read") {
        "读取模组审批记录"
    } else if line.contains("checking mods ban status") {
        "校验模组来源"
    } else if line.contains("ZBS verification") {
        "校验模组签名"
    } else if line.contains("Waiting for Java mods approval") {
        "等待模组审批"
    } else if line.contains("onGameInitComplete") {
        "初始化完成回调"
    } else if line.contains("Exposing class to Lua") {
        "注册 Lua 接口"
    } else if line.contains("Transformed:") {
        "转换游戏类"
    } else if line.contains("patching ") {
        "注入游戏类"
    } else {
        "加载中"
    }
}

/// ZombieBuddy 的初始化完成回调是它在退出前的最后一条里程碑。
///
/// 之后游戏还要加载纹理和主菜单，主菜单会播放 Bink 过场；覆盖层应在这里结束。
pub fn on_game_init_complete(line: &str) -> bool {
    line.contains("Running callbacks for onGameInitComplete")
}

/// 匹配主菜单/加载完成标记（返回命中的标记文本）。
pub fn completion_marker(line: &str) -> Option<&'static str> {
    COMPLETION_MARKERS
        .iter()
        .find(|marker| line.contains(**marker))
        .copied()
}

/// 根据 `[ZB]` 阶段与已输出行数估算加载进度（0.0~1.0）。
///
/// 只用于没有精确 advice 分母的兜底路径；阶段里程碑 + 行数缓慢增长。
pub fn progress_for(phase: &str, zb_lines: u64) -> f32 {
    let phase_value: f32 = match phase {
        "扫描模组列表" => 0.15,
        "读取 Java 模组清单" => 0.25,
        "读取模组审批记录" => 0.35,
        "校验模组来源" => 0.45,
        "校验模组签名" => 0.55,
        "等待模组审批" => 0.62,
        "注入游戏类" => 0.72,
        "转换游戏类" => 0.82,
        "注册 Lua 接口" => 0.88,
        "初始化完成回调" => 0.94,
        "加载中" => 0.5,
        _ => 0.08,
    };
    let creep = (zb_lines as f32 / 3000.0 * 0.6).min(0.6);
    phase_value.max(creep)
}

/// 是否可以因为 `[ZB]` 静默而判定加载完成。
///
/// 必须等游戏真正开始渲染（`overlay_started`），否则早期类加载空档会误判。
pub fn idle_completion(
    overlay_started: bool,
    phase: &str,
    last_zb_at: Option<Instant>,
    now: Instant,
) -> bool {
    if !overlay_started {
        return false;
    }
    let Some(last) = last_zb_at else {
        return false;
    };
    let idle = now.duration_since(last);
    if SILENCE_COMPLETION_PHASES.contains(&phase) {
        idle >= ZB_IDLE_TIMEOUT
    } else {
        idle >= ZB_IDLE_TIMEOUT_ANY_PHASE
    }
}

/// 解析 `[ZB] patching <目标> with N advice(s)` 里的 N。
pub fn parse_advice_count(line: &str) -> Option<u64> {
    let tail = line.split("patching ").nth(1)?.split(" with ").nth(1)?;
    let digits: String = tail
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 从 `[ZB]     <modId>   <路径>.jar` 行里取出最后的 jar 路径。
pub fn extract_jar_path(line: &str) -> Option<String> {
    line.split_whitespace()
        .rev()
        .find(|token| token.to_ascii_lowercase().ends_with(".jar"))
        .map(str::to_string)
}

/// 规范化一行游戏控制台输出：去掉 `LOG ... > ` 前缀、截断到 120 字符。
pub fn normalize_console_line(line: &str) -> Option<String> {
    let trimmed = line.trim_end_matches(['\r', '\n']).trim();
    let content = match trimmed.rfind("> ") {
        Some(index) => trimmed[index + 2..].trim(),
        None => trimmed,
    };
    if content.is_empty() {
        return None;
    }
    let mut value: String = content.chars().take(MAX_CONSOLE_LINE_CHARS).collect();
    if content.chars().count() > MAX_CONSOLE_LINE_CHARS {
        value.push('…');
    }
    Some(value)
}

/// 追加一行到界面日志队列（连续去重，最多保留 6 行）。
pub fn push_console_line(lines: &mut VecDeque<String>, line: &str) {
    let Some(value) = normalize_console_line(line) else {
        return;
    };
    if lines.back().is_some_and(|last| last == &value) {
        return;
    }
    lines.push_back(value);
    while lines.len() > MAX_CONSOLE_LINES {
        lines.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_zombie_buddy_stages() {
        assert_eq!(
            zb_phase("[ZB] ZomboidFileSystem.loadMods(102 mods) ..."),
            "扫描模组列表"
        );
        assert_eq!(zb_phase("[ZB] checking mods ban status"), "校验模组来源");
        assert_eq!(
            zb_phase("[ZB] Running callbacks for onGameInitComplete"),
            "初始化完成回调"
        );
        assert_eq!(zb_phase("[ZB] Exposing class to Lua"), "注册 Lua 接口");
        assert_eq!(zb_phase("[ZB] patching zombie.IsoChunk"), "注入游戏类");
        assert_eq!(zb_phase("[ZB] something else"), "加载中");
    }

    #[test]
    fn detects_completion_markers_and_init_complete() {
        assert!(on_game_init_complete(
            "LOG  : General      f:0> [ZB] Running callbacks for onGameInitComplete"
        ));
        assert!(!on_game_init_complete("[ZB] patching zombie.IsoChunk"));
        assert_eq!(
            completion_marker("LOG  : General      f:0> STATE: exit zombie.gameStates.TermsOfServiceState"),
            Some("STATE: exit zombie.gameStates.TermsOfServiceState")
        );
        assert_eq!(
            completion_marker("LOG  : Lua          f:0> :\t[ZombieBuddy] ZombieBuddy.getVersion() = 2.3.4"),
            Some("ZombieBuddy.getVersion()")
        );
        assert_eq!(completion_marker("[ZB] patching"), None);
    }

    #[test]
    fn parses_advice_counts_and_jar_paths() {
        assert_eq!(
            parse_advice_count("[ZB] patching zombie.core.textures.SmartTexture with 3 advice(s)"),
            Some(3)
        );
        assert_eq!(parse_advice_count("[ZB] no advice"), None);
        assert_eq!(
            extract_jar_path(
                "LOG  : General      f:0> [ZB]     viewpoint                     H:\\SteamLibrary\\steamapps\\workshop\\content\\108600\\3809306528\\mods\\Viewpoint\\42\\media\\java\\client\\Viewpoint.jar"
            ),
            Some("H:\\SteamLibrary\\steamapps\\workshop\\content\\108600\\3809306528\\mods\\Viewpoint\\42\\media\\java\\client\\Viewpoint.jar".to_string())
        );
        assert_eq!(extract_jar_path("[ZB] Java mod approvals read from ..."), None);
    }

    #[test]
    fn estimates_progress_by_phase_and_line_count() {
        assert!(progress_for("扫描模组列表", 0) >= 0.15);
        assert!(progress_for("注入游戏类", 1000) >= 0.72);
        assert!(progress_for("初始化完成回调", 0) >= 0.94);
        assert!(progress_for("未知", 10_000) <= 0.6);
    }

    #[test]
    fn idle_completion_requires_rendering_and_late_phase() {
        let now = Instant::now();
        let old = now - (ZB_IDLE_TIMEOUT + Duration::from_secs(5));
        let ancient = now - (ZB_IDLE_TIMEOUT_ANY_PHASE + Duration::from_secs(5));

        assert!(!idle_completion(false, "初始化完成回调", Some(old), now));
        assert!(!idle_completion(true, "初始化完成回调", Some(now), now));
        assert!(!idle_completion(true, "初始化完成回调", None, now));
        assert!(idle_completion(true, "初始化完成回调", Some(old), now));
        assert!(idle_completion(true, "注册 Lua 接口", Some(old), now));
        assert!(!idle_completion(true, "校验模组来源", Some(old), now));
        assert!(idle_completion(true, "校验模组来源", Some(ancient), now));
    }

    #[test]
    fn normalizes_and_keeps_only_the_latest_lines() {
        assert_eq!(
            normalize_console_line("LOG  : General      f:0> hello\r\n"),
            Some("hello".to_string())
        );
        assert_eq!(normalize_console_line("   "), None);
        let long = "x".repeat(MAX_CONSOLE_LINE_CHARS + 5);
        let normalized = normalize_console_line(&long).unwrap();
        assert_eq!(normalized.chars().count(), MAX_CONSOLE_LINE_CHARS + 1);
        assert!(normalized.ends_with('…'));

        let mut lines = VecDeque::new();
        for index in 0..10 {
            push_console_line(&mut lines, &format!("LOG  : General f:0> line {index}"));
        }
        assert_eq!(lines.len(), MAX_CONSOLE_LINES);
        assert_eq!(lines.back().unwrap(), "line 9");
        push_console_line(&mut lines, "line 9");
        assert_eq!(lines.len(), MAX_CONSOLE_LINES);
    }
}
