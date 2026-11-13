//! DLL 侧的 console.txt 自读器：启动器关闭/心跳超时后的回退数据源。
//!
//! 正常情况仍由启动器写 `%TEMP%\pz_overlay_status.txt`；只有状态文件陈旧时
//! 才启用这里。复用 `overlay-core` 的阶段/完成/进度规则，避免两套解析漂移。

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use overlay_core::{
    completion_marker, on_game_init_complete, parse_advice_count, progress_for, push_console_line,
    zb_phase,
};

const READ_CHUNK: usize = 8192;
/// 单次 poll 最多读这么多，避免回退刚启动时一次性读大文件卡住 UI 线程。
const MAX_READ_PER_POLL: usize = 64 * 1024;
const NO_ZB_TIMEOUT: Duration = Duration::from_secs(180);
const SESSION_MAX_DURATION: Duration = Duration::from_secs(540);

#[derive(Debug, Clone)]
pub struct ConsolePoll {
    pub phase: String,
    pub progress: f32,
    pub has_progress: bool,
    pub lines: VecDeque<String>,
    pub complete: Option<String>,
}

pub struct ConsoleReader {
    path: PathBuf,
    offset: u64,
    pending: String,
    phase: String,
    progress: f32,
    has_progress: bool,
    lines: VecDeque<String>,
    zb_lines: u64,
    applied_advices: u64,
    last_zb_at: Option<Instant>,
    session_start: SystemTime,
    started_at: Instant,
    overlay_started: bool,
    saw_zb: bool,
    complete: Option<String>,
}

impl ConsoleReader {
    /// `session_start` 来自启动器状态文件的 `session=`；缺失时退回“2 秒前”，
    /// 这样刚被游戏重写的 console.txt 会被接受，上一局的旧文件会被等到截断后再读。
    pub fn new(path: PathBuf, session_start: Option<SystemTime>) -> Self {
        Self {
            path,
            offset: 0,
            pending: String::new(),
            phase: "等待 ZombieBuddy 输出".to_string(),
            progress: 0.0,
            has_progress: false,
            lines: VecDeque::new(),
            zb_lines: 0,
            applied_advices: 0,
            last_zb_at: None,
            session_start: session_start
                .unwrap_or_else(|| SystemTime::now() - Duration::from_secs(2)),
            started_at: Instant::now(),
            overlay_started: false,
            saw_zb: false,
            complete: None,
        }
    }

    /// 首次换帧后打开 idle 判定（与启动器一致：先看到渲染，再允许静默完成）。
    pub fn set_overlay_started(&mut self, started: bool) {
        if started && !self.overlay_started {
            self.overlay_started = true;
            self.last_zb_at = None;
        }
    }

    pub fn poll(&mut self, now: Instant) -> ConsolePoll {
        if self.complete.is_none() {
            self.read_new_bytes(now);
            self.check_timeouts(now);
        }
        self.snapshot()
    }

    fn read_new_bytes(&mut self, now: Instant) {
        let Ok(metadata) = std::fs::metadata(&self.path) else {
            return;
        };
        if !metadata
            .modified()
            .map(|mtime| mtime >= self.session_start)
            .unwrap_or(false)
        {
            // 还是上一局的 console.txt：等游戏把它重写。
            return;
        }

        let size = metadata.len();
        if size < self.offset {
            self.reset_for_truncation();
        }
        if size <= self.offset {
            return;
        }

        let Ok(mut file) = File::open(&self.path) else {
            return;
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buffer = [0u8; READ_CHUNK];
        let mut total = 0usize;
        while total < MAX_READ_PER_POLL {
            let Ok(read) = file.read(&mut buffer) else {
                break;
            };
            if read == 0 {
                break;
            }
            self.offset += read as u64;
            total += read;
            self.pending
                .push_str(&String::from_utf8_lossy(&buffer[..read]));
        }

        while let Some(index) = self.pending.find('\n') {
            let line = self.pending[..index]
                .trim_end_matches('\r')
                .to_string();
            self.pending.drain(..=index);
            self.process_line(&line, now);
            if self.complete.is_some() {
                break;
            }
        }
    }

    fn process_line(&mut self, line: &str, now: Instant) {
        push_console_line(&mut self.lines, line);

        if line.contains("[ZB]") {
            self.saw_zb = true;
            self.zb_lines += 1;
            self.phase = zb_phase(line).to_string();
            if on_game_init_complete(line) {
                self.complete = Some(
                    "ZombieBuddy 初始化完成回调结束，覆盖结束。".to_string(),
                );
                return;
            }
            if let Some(count) = parse_advice_count(line) {
                self.applied_advices = self.applied_advices.saturating_add(count);
            }
            if self.overlay_started {
                self.last_zb_at = Some(now);
            }
            let estimate = progress_for(&self.phase, self.zb_lines).min(0.97);
            self.progress = self.progress.max(estimate);
            self.has_progress = true;
        } else if let Some(marker) = completion_marker(line) {
            self.complete = Some(format!(
                "检测到主菜单标记（{marker}），覆盖结束。"
            ));
        }
    }

    fn check_timeouts(&mut self, now: Instant) {
        if self.complete.is_some() {
            return;
        }
        if self.started_at.elapsed() >= SESSION_MAX_DURATION {
            self.complete = Some(
                "覆盖层已持续 9 分钟，达到上限后自动结束。".to_string(),
            );
            return;
        }
        if !self.saw_zb && self.started_at.elapsed() >= NO_ZB_TIMEOUT {
            self.complete = Some(
                "180 秒内没有检测到 [ZB] 输出，按超时结束覆盖。".to_string(),
            );
            return;
        }
        if overlay_core::idle_completion(
            self.overlay_started,
            &self.phase,
            self.last_zb_at,
            now,
        ) {
            self.complete = Some(format!(
                "[ZB] 输出静默（阶段：{}，共 {} 条），判定 ZombieBuddy 加载完成。",
                self.phase, self.zb_lines
            ));
        }
    }

    fn reset_for_truncation(&mut self) {
        self.offset = 0;
        self.pending.clear();
        self.phase = "等待 ZombieBuddy 输出".to_string();
        self.progress = 0.0;
        self.has_progress = false;
        self.lines.clear();
        self.zb_lines = 0;
        self.applied_advices = 0;
        self.last_zb_at = None;
        self.saw_zb = false;
        self.complete = None;
    }

    fn display_phase(&self) -> String {
        if self.applied_advices > 0 {
            format!("{} · {} 条 advice", self.phase, self.applied_advices)
        } else {
            self.phase.clone()
        }
    }

    fn snapshot(&self) -> ConsolePoll {
        ConsolePoll {
            phase: self.display_phase(),
            progress: self.progress,
            has_progress: self.has_progress,
            lines: self.lines.clone(),
            complete: self.complete.clone(),
        }
    }
}

/// `%USERPROFILE%\Zomboid\console.txt`（启动器没在状态文件里给出路径时的兜底）。
pub fn default_console_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|profile| profile.join("Zomboid").join("console.txt"))
        .unwrap_or_else(|| PathBuf::from("console.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    fn write_console(path: &Path, text: &str) {
        let mut file = File::create(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    fn reader(path: &Path) -> ConsoleReader {
        ConsoleReader::new(path.to_path_buf(), Some(SystemTime::UNIX_EPOCH))
    }

    #[test]
    fn reads_zb_phase_and_advice_count() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        write_console(
            &path,
            "LOG  : General f:0> [ZB] patching zombie.IsoChunk with 4 advice(s)\n",
        );
        let mut console = reader(&path);
        console.set_overlay_started(true);
        let poll = console.poll(Instant::now());
        assert_eq!(poll.phase, "注入游戏类 · 4 条 advice");
        assert!(poll.has_progress);
        assert!(poll.progress >= 0.72);
        assert!(poll.complete.is_none());
        assert_eq!(poll.lines.len(), 1);
    }

    #[test]
    fn completes_on_init_callback_before_menu() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        write_console(
            &path,
            "LOG  : General f:0> [ZB] Running callbacks for onGameInitComplete\n",
        );
        let mut console = reader(&path);
        // 没有被拒模组：完成即收尾，不进确认等待。
        let poll = console.poll(Instant::now());
        assert_eq!(
            poll.complete.as_deref(),
            Some("ZombieBuddy 初始化完成回调结束，覆盖结束。")
        );
    }

    #[test]
    fn completes_on_main_menu_marker() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        write_console(
            &path,
            "LOG  : General f:0> STATE: exit zombie.gameStates.TermsOfServiceState\n",
        );
        let mut console = reader(&path);
        let poll = console.poll(Instant::now());
        assert!(poll.complete.as_deref().unwrap().contains("主菜单标记"));
    }

    #[test]
    fn resets_when_console_is_truncated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        let mut text = String::new();
        for index in 0..200 {
            text.push_str(&format!(
                "LOG  : General f:0> [ZB] patching zombie.Test{index} with 1 advice(s)\n"
            ));
        }
        write_console(&path, &text);
        let mut console = reader(&path);
        console.set_overlay_started(true);
        let first = console.poll(Instant::now());
        assert!(first.progress > 0.0);
        assert!(first.lines.len() <= overlay_core::MAX_CONSOLE_LINES);

        write_console(&path, "LOG  : General f:0> fresh start\n");
        let second = console.poll(Instant::now());
        assert_eq!(second.phase, "等待 ZombieBuddy 输出");
        assert_eq!(second.progress, 0.0);
        assert_eq!(second.lines.len(), 1);
    }

    #[test]
    fn idle_completion_stops_after_the_init_phase() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        write_console(&path, "");
        let mut console = reader(&path);
        console.set_overlay_started(true);
        console.phase = "初始化完成回调".to_string();
        console.last_zb_at = Some(Instant::now() - Duration::from_secs(11));
        let poll = console.poll(Instant::now());
        assert!(poll.complete.as_deref().unwrap().contains("[ZB] 输出静默"));
    }

    #[test]
    fn ignores_previous_session_console_until_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("console.txt");
        write_console(
            &path,
            "LOG  : General f:0> [ZB] Running callbacks for onGameInitComplete\n",
        );
        let mut console = reader(&path);
        // 把 session_start 设到文件 mtime 之后：模拟上一局的 console。
        console.session_start = SystemTime::now() + Duration::from_secs(60);
        let poll = console.poll(Instant::now());
        assert!(poll.complete.is_none());

        console.session_start = SystemTime::UNIX_EPOCH;
        let poll = console.poll(Instant::now());
        assert!(poll.complete.is_some());
    }
}
