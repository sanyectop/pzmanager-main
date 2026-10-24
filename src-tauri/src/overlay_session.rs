//! 覆盖层会话：游戏窗口出现后自动注入 DLL，实时解析游戏控制台的 `[ZB]` 输出显示模组加载进度，
//! 并在 ZombieBuddy 加载完成后通知 DLL 停止覆盖。
//!
//! 与 DLL 的约定：DLL 启用 Hook 后会等待命名事件 `Local\PZManagerOverlayStop_<游戏PID>`，
//! 本模块在判定加载完成时对该事件调用 `SetEvent`。

/// 会话状态事件（前端用来显示"等待窗口 / 已注入 / 加载中 / 已完成"）。
pub const SESSION_EVENT: &str = "overlay-session";
/// ZombieBuddy 加载进度事件（来自控制台的 `[ZB]` 输出）。
pub const PROGRESS_EVENT: &str = "overlay-zb-progress";

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySessionUpdate {
    pub phase: String,
    pub message: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub zb_lines: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayZbProgress {
    pub zb_lines: u64,
    pub phase: String,
    pub last_line: String,
    pub elapsed_ms: u64,
}

/// 「检测到游戏窗口 → 移除覆盖层（发停止事件）」的启动时长记录。
///
/// 持久化在 `%USERPROFILE%\Zomboid\.pzmanager\startup-timing.json`，跨启动保留，
/// 用来和上一次启动对比（没有上一次数据时 `previous_ms` 为 `None`，前端不显示对比）。
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupTiming {
    /// 最近一次测得的时长（毫秒）。
    pub last_ms: Option<u64>,
    /// 再上一次的时长（毫秒）。
    pub previous_ms: Option<u64>,
}

/// 启动时长事件：每次移除覆盖层（发停止事件）时发一次。
pub const STARTUP_EVENT: &str = "startup-timing";

/// 启动时长记录文件。
fn startup_timing_path() -> Option<std::path::PathBuf> {
    crate::pzmanager_directory()
        .ok()
        .map(|directory| directory.join("startup-timing.json"))
}

/// 读取已保存的启动时长；文件缺失或损坏都返回默认值（不报错）。
pub fn read_startup_timing() -> StartupTiming {
    let Some(path) = startup_timing_path() else {
        return StartupTiming::default();
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// 纯计算：新时长落到 `last_ms`，原来的 `last_ms` 顺延成 `previous_ms`。
fn next_startup_timing(previous: StartupTiming, duration_ms: u64) -> StartupTiming {
    StartupTiming {
        last_ms: Some(duration_ms),
        previous_ms: previous.last_ms,
    }
}

/// 记录一次新测得的启动时长：原来的 `last_ms` 顺延成 `previous_ms`。
fn record_startup_timing(duration_ms: u64) -> StartupTiming {
    let timing = next_startup_timing(read_startup_timing(), duration_ms);
    if let Some(path) = startup_timing_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(&timing) {
            let _ = std::fs::write(&path, content);
        }
    }
    timing
}

#[cfg(test)]
mod timing_tests {
    use super::{next_startup_timing, StartupTiming};

    /// 第一次没有对比对象；第二次起，"上次"是更早的那一次。
    #[test]
    fn shifts_previous_startup_duration() {
        let first = next_startup_timing(StartupTiming::default(), 90_000);
        assert_eq!(first.last_ms, Some(90_000));
        assert_eq!(first.previous_ms, None);

        let second = next_startup_timing(first, 66_000);
        assert_eq!(second.last_ms, Some(66_000));
        assert_eq!(second.previous_ms, Some(90_000));
    }
}

#[cfg(windows)]
mod imp {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant, SystemTime};

    use tauri::{AppHandle, Emitter};
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, FALSE, HANDLE, HWND, LPARAM, TRUE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, GetExitCodeProcess, OpenProcess, SetEvent, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    };

    use super::{
        record_startup_timing, OverlaySessionUpdate, OverlayZbProgress, PROGRESS_EVENT,
        SESSION_EVENT, STARTUP_EVENT,
    };
    use crate::overlay::imp as overlay_imp;

    /// 等待游戏进程出现的最长时间。
    const PROCESS_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
    /// 等待游戏窗口出现的最长时间（游戏启动慢时可能到数分钟）。
    const WINDOW_WAIT_TIMEOUT: Duration = Duration::from_secs(300);
    /// 控制台 tail / 进程探测的轮询间隔。
    ///
    /// 必须与 DLL 侧的界面刷新（40ms ≈ 25fps）对齐：否则状态文件内容最多每 400ms 变一次，
    /// 界面即使 25fps 重绘，看到的也只是 2~5fps 的"跳变"。
    const POLL_INTERVAL: Duration = Duration::from_millis(40);
    /// 等待 console.txt 被本次启动重写的最长时间。
    const CONSOLE_FRESH_TIMEOUT: Duration = Duration::from_secs(180);
    /// `[ZB]` 输出静默多久判定加载完成 —— **只对收尾阶段有效**（见 `SILENCE_COMPLETION_PHASES`）。
    ///
    /// 只在"游戏已经开始换帧"之后才计时（见 idle_completion），因此可以收紧到 10 秒，
    /// 避免主菜单已经出现、覆盖层还多画几帧（表现为"关闭后闪一下"）。
    const ZB_IDLE_TIMEOUT: Duration = Duration::from_secs(10);
    /// 收尾阶段之外的静默，必须超过这个时长才兜底结束。
    ///
    /// 为什么不能一律用 10 秒：ZombieBuddy 会在加载**中途**联网（`Authors list fetch`，
    /// 实测静默 10~41 秒）和跑 ClassGraph 全量扫描，那段时间完全没有 `[ZB]` 输出。
    /// 用 10 秒判定会让覆盖层在游戏还没进主菜单时就被摘掉 —— 表现就是
    /// "没有加载过程，直接跳到加载完成"。
    const ZB_IDLE_TIMEOUT_ANY_PHASE: Duration = Duration::from_secs(120);
    /// 允许"静默即完成"的收尾阶段；其它阶段的静默只能等 `ZB_IDLE_TIMEOUT_ANY_PHASE`。
    const SILENCE_COMPLETION_PHASES: [&str; 2] = ["注册 Lua 接口", "初始化完成回调"];
    /// 完全没有 `[ZB]` 输出时的兜底时长（从注入时刻起算）。
    const NO_ZB_TIMEOUT: Duration = Duration::from_secs(180);
    /// 注入后等待「覆盖层画出第一帧」的最长时间（游戏渲染启动可能很慢）。
    const STARTED_WAIT_TIMEOUT: Duration = Duration::from_secs(180);
    /// 覆盖层从注入起的最长持续时间（DLL 侧另有 10 分钟安全上限）。
    const SESSION_MAX_DURATION: Duration = Duration::from_secs(540);
    /// 进度事件最小发送间隔，避免刷爆前端。
    const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(200);
    /// 覆盖层达到 100% 后继续显示的时间，避免最后一帧来不及渲染。
    const COMPLETION_HOLD: Duration = Duration::from_secs(2);
    /// 停止事件名前缀，需与 DLL 保持一致。
    const STOP_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStop_";
    /// 「已开始绘制」事件名前缀，需与 DLL 保持一致。
    const STARTED_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStarted_";
    /// 主菜单/加载完成标记（游戏自身与 ZombieBuddy 的控制台输出）。
    const COMPLETION_MARKERS: [&str; 3] = [
        "STATE: exit zombie.gameStates.TermsOfServiceState",
        "added to the main menu",
        "ZombieBuddy.getVersion()",
    ];
    /// `STILL_ACTIVE`（`GetExitCodeProcess` 的返回值）。
    const STILL_ACTIVE: u32 = 259;

    struct SessionOutcome {
        reason: String,
        zb_lines: u64,
        pid: u32,
        process_name: String,
    }

    /// 把加载界面状态写进 `%TEMP%\pz_overlay_status.txt`，由注入的 DLL 在**游戏窗口内部**绘制。
    ///
    /// 不再创建任何独立窗口，因此不存在"切焦点时反复显示/隐藏"导致的闪动。
    struct StatusFile {
        path: std::path::PathBuf,
        last_write: Instant,
        lines: std::collections::VecDeque<String>,
        phase: String,
        progress: f32,
        /// 是否已经有"真实进度"（拿到 advice 分母之后才为 true）。
        has_progress: bool,
        memory_limit_mb: Option<u64>,
    }

    impl StatusFile {
        fn new() -> Self {
            let path = std::env::temp_dir().join("pz_overlay_status.txt");
            // 清掉上一局残留，避免界面一开始就显示上次的终值（例如 89%/100%）。
            let _ = std::fs::remove_file(&path);
            // 内存上限取启动器设置里的值（游戏侧命令行看不到 JSON 里的 -Xmx）。
            let memory_limit_mb = crate::app_settings_path()
                .ok()
                .and_then(|settings| crate::load_app_settings_at(&settings).ok())
                .and_then(|settings| settings.memory_limit_mb);
            Self {
                path,
                last_write: Instant::now() - Duration::from_secs(1),
                lines: std::collections::VecDeque::new(),
                phase: "准备中".to_string(),
                progress: 0.0,
                has_progress: false,
                memory_limit_mb,
            }
        }

        /// 追加一行控制台输出（保留最新 6 行，剥掉 `LOG : General f:0>` 前缀）。
        fn push_line(&mut self, line: &str) {
            let trimmed = line.trim_end_matches(['\r', '\n']).trim();
            let content = match trimmed.rfind("> ") {
                Some(index) => trimmed[index + 2..].trim(),
                None => trimmed,
            };
            if content.is_empty() {
                return;
            }
            let mut value: String = content.chars().take(120).collect();
            if content.chars().count() > 120 {
                value.push('…');
            }
            if self.lines.back().is_some_and(|last| last == &value) {
                return;
            }
            self.lines.push_back(value);
            while self.lines.len() > 6 {
                self.lines.pop_front();
            }
            self.flush();
        }

        /// 阶段估算（只增不减），用于 DLL 还没有精确分母时的兜底。
        fn set_phase_only(&mut self, phase: &str) {
            self.phase = phase.to_string();
            self.has_progress = false;
            self.flush();
        }

        /// 真实注入进度（直接覆盖，允许回落）。
        fn set_precise_progress(&mut self, phase: &str, progress: f32) {
            self.phase = phase.to_string();
            self.progress = progress.clamp(0.0, 1.0);
            self.has_progress = true;
            self.flush();
        }

        /// 忽略 40ms 节流，确保完成状态在等待前已经落到 DLL 读取的文件中。
        fn force_flush(&mut self) {
            self.last_write = Instant::now() - Duration::from_secs(1);
            self.flush();
        }

        fn close(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }

        fn flush(&mut self) {
            // 与 DLL 侧界面刷新对齐：40ms ≈ 25fps。
            if self.last_write.elapsed() < Duration::from_millis(40) {
                return;
            }
            self.last_write = Instant::now();
            let mut text = String::new();
            if self.has_progress {
                text.push_str(&format!("progress={:.3}\n", self.progress));
            }
            text.push_str(&format!("phase={}\n", self.phase));
            if let Some(limit) = self.memory_limit_mb {
                text.push_str(&format!("memmax={limit}\n"));
            }
            for line in &self.lines {
                text.push_str("line=");
                text.push_str(line);
                text.push('\n');
            }
            let temporary = self.path.with_extension("tmp");
            if std::fs::write(&temporary, text).is_ok() {
                let _ = std::fs::rename(&temporary, &self.path);
            }
        }
    }

    /// 启动一次覆盖层会话（在独立线程中运行，命令立即返回）。
    pub(super) fn run_session(app: AppHandle, dll_path: String) {
        let started = Instant::now();
        match run_session_inner(&app, &dll_path, started) {
            Ok(outcome) => emit_session(
                &app,
                "finished",
                &outcome.reason,
                Some(outcome.pid),
                Some(&outcome.process_name),
                outcome.zb_lines,
                started.elapsed(),
            ),
            Err(message) => {
                emit_session(&app, "error", &message, None, None, 0, started.elapsed());
            }
        }
    }

    fn run_session_inner(
        app: &AppHandle,
        dll_path: &str,
        started: Instant,
    ) -> Result<SessionOutcome, String> {
        let dll_path = overlay_imp::normalize_dll_path(dll_path)?;
        let dll_name = dll_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("无法解析 DLL 文件名：{}", dll_path.display()))?
            .to_string();

        emit_session(
            app,
            "waitingProcess",
            "等待游戏进程…",
            None,
            None,
            0,
            started.elapsed(),
        );

        let deadline = Instant::now() + PROCESS_WAIT_TIMEOUT;
        let (pid, process_name) = loop {
            if let Some((candidate_pid, candidate_name)) = overlay_imp::find_game_process() {
                // 上一次异常结束（例如被强制结束、弹出崩溃通知）可能留下一个仍加载着
                // 覆盖层 DLL 的僵尸进程。它会抢占"找到游戏进程"这一步，导致新启动的游戏
                // 永远等不到注入 —— 这里先把它结束掉，再继续等真正的新进程。
                if overlay_imp::is_module_loaded(candidate_pid, &dll_name).unwrap_or(false) {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/PID", &candidate_pid.to_string()])
                        .output();
                    thread::sleep(POLL_INTERVAL);
                    continue;
                }
                break (candidate_pid, candidate_name);
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "等待游戏进程超时（{} 秒）：未找到 ProjectZomboid64.exe 或 javaw.exe。",
                    PROCESS_WAIT_TIMEOUT.as_secs()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        };
        emit_session(
            app,
            "waitingWindow",
            &format!("已找到 {process_name}（PID {pid}），等待游戏窗口出现…"),
            Some(pid),
            Some(process_name),
            0,
            started.elapsed(),
        );

        let deadline = Instant::now() + WINDOW_WAIT_TIMEOUT;
        while !has_visible_window(pid) {
            if !process_alive(pid) {
                return Err("游戏进程在窗口出现前已退出。".to_string());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "等待游戏窗口超时（{} 秒）。",
                    WINDOW_WAIT_TIMEOUT.as_secs()
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }

        // 「检测到游戏窗口」的这一刻：启动时长从这里起算，到检测到主菜单标记为止。
        let window_detected_at = Instant::now();

        // 两个事件都必须在注入前创建：DLL 加载后会立刻打开同名事件。
        let stop_event = create_named_event(pid, STOP_EVENT_PREFIX)?;
        let started_event = create_named_event(pid, STARTED_EVENT_PREFIX)?;

        emit_session(
            app,
            "injecting",
            "游戏窗口已出现，正在注入覆盖层 DLL…",
            Some(pid),
            Some(process_name),
            0,
            started.elapsed(),
        );

        let already_loaded = overlay_imp::is_module_loaded(pid, &dll_name).unwrap_or(false);
        if !already_loaded {
            crate::overlay::injector::inject_dll(pid, &dll_path)?;
        }
        let injected_at = Instant::now();

        // 加载界面：只写状态文件，由 DLL 在游戏窗口**内部**绘制（红底 + 左下角控制台 + 底部进度条）。
        let mut status = StatusFile::new();
        status.set_phase_only("等待 ZombieBuddy 输出");

        let console_path = crate::user_zomboid_directory()?.join("console.txt");
        let injection_record = format!(
            "[PZManager] 覆盖层 DLL {} 已{}（{}，PID {pid}）",
            dll_path.display(),
            if already_loaded {
                "在游戏进程中加载"
            } else {
                "注入"
            },
            process_name
        );

        emit_session(
            app,
            "loading",
            "覆盖层已开启，正在读取 [ZB] 加载进度…",
            Some(pid),
            Some(process_name),
            0,
            started.elapsed(),
        );

        let session_start = SystemTime::now() - started.elapsed();

        // 精确进度分母：由日志里 "[ZB] java mod list to load:" 的 jar 列表计算（见 track_console），
        // 运行期用 "[ZB] patching … with N advice(s)" 累加分子，两者高度吻合（实测 614 vs 611）。
        let expected_advices: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
        let outcome = track_console(
            app,
            pid,
            &console_path,
            session_start,
            started,
            injected_at,
            started_event,
            &injection_record,
            &mut status,
            &expected_advices,
        );

        if let Ok(outcome) = &outcome {
            status.set_precise_progress("加载完成", 1.0);
            status.force_flush();
            // 100% 已经写入状态文件；此期间不发送停止事件，让 DLL 继续绘制最后画面。
            thread::sleep(COMPLETION_HOLD);
            let _ = outcome;
        }

        // 「移除覆盖层」的这一刻就是启动时长的终点。
        //
        // 为什么不在主菜单标记处记：会话通常在 ≥97% advice 或 `[ZB]` 静默 10 秒时就结束了
        // （见 track_console 里的两个提前返回），主菜单标记那一步经常根本走不到。
        // 而"发停止事件"是所有成功注入路径都会经过的地方。
        let startup_ms = window_detected_at.elapsed().as_millis() as u64;
        let timing = record_startup_timing(startup_ms);
        let _ = app.emit(STARTUP_EVENT, timing);

        // 通知 DLL 停止覆盖（无论成功失败都要发信号，避免一直保持红色）。
        unsafe {
            let _ = SetEvent(stop_event);
        }
        let _ = unsafe { CloseHandle(stop_event) };
        let _ = unsafe { CloseHandle(started_event) };

        status.close();

        outcome.map(|mut value| {
            value.pid = pid;
            value.process_name = process_name.to_string();
            append_console_record(
                &console_path,
                &format!("[PZManager] 覆盖层已停止：{}", value.reason),
            );
            value
        })
    }

    /// 跟踪游戏控制台输出，解析 `[ZB]` 行，并在加载完成时返回。
    fn track_console(
        app: &AppHandle,
        pid: u32,
        console_path: &Path,
        session_start: SystemTime,
        started: Instant,
        injected_at: Instant,
        started_event: HANDLE,
        injection_record: &str,
        status: &mut StatusFile,
        expected_advices: &Arc<Mutex<Option<usize>>>,
    ) -> Result<SessionOutcome, String> {
        // 等 console.txt 被本次启动重写；迟迟没变化就退化为读取现有内容。
        let fresh_deadline = injected_at + CONSOLE_FRESH_TIMEOUT;
        loop {
            if let Ok(metadata) = std::fs::metadata(console_path) {
                if metadata
                    .modified()
                    .map(|time| time >= session_start)
                    .unwrap_or(true)
                {
                    break;
                }
            }
            if !process_alive(pid) {
                return Ok(outcome("游戏进程已退出。", 0, pid, String::new()));
            }
            if Instant::now() >= fresh_deadline {
                break;
            }
            thread::sleep(POLL_INTERVAL);
        }

        // 等文件被本次启动重写之后再写记录，否则会被游戏启动时的截断冲掉。
        append_console_record(console_path, injection_record);

        let mut file = std::fs::File::open(console_path)
            .map_err(|error| format!("打开游戏控制台输出失败: {error}"))?;
        let mut offset = 0u64;
        let mut buffer = [0u8; 8192];
        let mut pending = String::new();
        let mut zb_lines = 0u64;
        let mut applied_advices = 0u64;
        let mut last_zb_line = String::new();
        let mut phase = "等待 ZombieBuddy 输出".to_string();
        let mut last_zb_at: Option<Instant> = None;
        let mut last_emit = Instant::now();
        let mut overlay_started = false;
        // "[ZB] java mod list to load:" 之后的 jar 列表（用于计算进度分母）。
        let mut collecting_jars = false;
        let mut pending_jars: Vec<String> = Vec::new();

        loop {
            if !process_alive(pid) {
                return Ok(outcome("游戏进程已退出。", zb_lines, pid, String::new()));
            }

            // 只有游戏真正开始换帧（红色已经画出来）之后，才允许做"加载完成"判定。
            if !overlay_started && unsafe { WaitForSingleObject(started_event, 0) } == WAIT_OBJECT_0
            {
                overlay_started = true;
                last_zb_at = None;
                emit_session(
                    app,
                    "loading",
                    "覆盖层已画出第一帧（红色已可见），继续读取 [ZB] 进度…",
                    Some(pid),
                    None,
                    zb_lines,
                    started.elapsed(),
                );
            }
            if !overlay_started && injected_at.elapsed() >= STARTED_WAIT_TIMEOUT {
                return Ok(outcome(
                    &format!(
                        "注入后 {} 秒内游戏仍未开始渲染，结束覆盖。",
                        STARTED_WAIT_TIMEOUT.as_secs()
                    ),
                    zb_lines,
                    pid,
                    String::new(),
                ));
            }
            if injected_at.elapsed() >= SESSION_MAX_DURATION {
                return Ok(outcome(
                    &format!(
                        "覆盖层已持续 {} 分钟，达到上限后自动结束。",
                        SESSION_MAX_DURATION.as_secs() / 60
                    ),
                    zb_lines,
                    pid,
                    String::new(),
                ));
            }
            if zb_lines == 0 && injected_at.elapsed() >= NO_ZB_TIMEOUT {
                return Ok(outcome(
                    &format!(
                        "{} 秒内没有检测到 [ZB] 输出，按超时结束覆盖。",
                        NO_ZB_TIMEOUT.as_secs()
                    ),
                    zb_lines,
                    pid,
                    String::new(),
                ));
            }

            let size = std::fs::metadata(console_path)
                .map(|meta| meta.len())
                .unwrap_or(0);
            if size < offset {
                // 文件被截断（游戏重启），从头读。
                offset = 0;
                pending.clear();
            }

            if size > offset {
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| format!("定位控制台输出失败: {error}"))?;
                let read = file
                    .read(&mut buffer)
                    .map_err(|error| format!("读取控制台输出失败: {error}"))?;
                offset += read as u64;
                pending.push_str(&String::from_utf8_lossy(&buffer[..read]));

                while let Some(index) = pending.find('\n') {
                    let line = pending[..index].trim_end_matches('\r').to_string();
                    pending.drain(..=index);

                    // 左下角展示的控制台内容：所有输出都进队列，只保留最新 6 行。
                    status.push_line(&line);

                    // 收集本次真正要加载的 Java 模组 jar 列表，用于精确进度的分母。
                    if line.contains("java mod list to load") {
                        collecting_jars = true;
                        pending_jars.clear();
                    } else if collecting_jars {
                        if let Some(path) = extract_jar_path(&line) {
                            pending_jars.push(path);
                        } else if line.contains("[ZB]") {
                            collecting_jars = false;
                            let jars = std::mem::take(&mut pending_jars);
                            let slot = Arc::clone(expected_advices);
                            let _ = thread::Builder::new()
                                .name("pz-overlay-advice-scan".to_string())
                                .spawn(move || {
                                    if let Some(total) = expected_advice_count(&jars) {
                                        if let Ok(mut value) = slot.lock() {
                                            *value = Some(total);
                                        }
                                    }
                                });
                        }
                    }

                    if line.contains("[ZB]") {
                        zb_lines += 1;
                        last_zb_line = line.clone();
                        phase = zb_phase(&line).to_string();
                        if let Some(count) = parse_advice_count(&line) {
                            applied_advices += count;
                        }

                        let precise_total = expected_advices
                            .lock()
                            .ok()
                            .and_then(|value| *value)
                            .filter(|total| *total > 0);
                        match precise_total {
                            Some(total) => {
                                // 真实进度：已注入 advice / 解析出的总数。
                                // 直接覆盖（不取最大值），否则会被偏高的阶段估算顶住。
                                let exact =
                                    (applied_advices as f32 / total as f32).clamp(0.0, 0.97);
                                status.set_precise_progress(
                                    &format!("{phase} · 注入 {applied_advices}/{total}"),
                                    exact,
                                );
                                // 注入接近完成即可结束：主菜单马上就要出现，
                                // 否则最后一帧红色会在菜单出现后才停（表现为"闪一下"）。
                                if applied_advices.saturating_mul(100) >= (total as u64) * 97 {
                                    return Ok(outcome(
                                        &format!(
                                            "已注入 {applied_advices}/{total} 条 advice（≥97%），覆盖结束。"
                                        ),
                                        zb_lines,
                                        pid,
                                        String::new(),
                                    ));
                                }
                            }
                            // 分母还没算出来（jar 列表尚未出现在日志里）时的兜底估算。
                            // 还没有真实分母：只显示阶段，不显示百分比（避免"假进度"）
                            None => status.set_phase_only(&phase),
                        }
                        // 渲染开始前的 [ZB] 输出（类加载阶段）不参与"静默判定"，
                        // 否则会把加载中的长空档误判成已完成。
                        if overlay_started {
                            last_zb_at = Some(Instant::now());
                        }
                    } else if let Some(marker) = completion_marker(&line) {
                        return Ok(outcome(
                            &format!("检测到主菜单标记（{marker}），覆盖结束。"),
                            zb_lines,
                            pid,
                            format!(" [ZB] 共 {zb_lines} 条"),
                        ));
                    }
                }

                if zb_lines > 0 && last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
                    last_emit = Instant::now();
                    let _ = app.emit(
                        PROGRESS_EVENT,
                        OverlayZbProgress {
                            zb_lines,
                            phase: phase.clone(),
                            last_line: last_zb_line.clone(),
                            elapsed_ms: started.elapsed().as_millis() as u64,
                        },
                    );
                }
            }

            if idle_completion(overlay_started, &phase, last_zb_at, Instant::now()) {
                return Ok(outcome(
                    &format!(
                        "[ZB] 输出静默（阶段：{phase}，共 {zb_lines} 条），判定 ZombieBuddy 加载完成。"
                    ),
                    zb_lines,
                    pid,
                    String::new(),
                ));
            }

            thread::sleep(POLL_INTERVAL);
        }
    }

    fn outcome(reason: &str, zb_lines: u64, pid: u32, suffix: String) -> SessionOutcome {
        SessionOutcome {
            reason: format!("{reason}{suffix}"),
            zb_lines,
            pid,
            process_name: String::new(),
        }
    }

    /// 是否可以因为 `[ZB]` 静默而判定加载完成。
    ///
    /// 两个前提（都是踩过的坑）：
    /// 1. 必须等游戏真正开始渲染（`overlay_started`）—— 启动早期的类加载空档实测超过 33 秒，
    ///    不加这个前提会在还没画出第一帧时就把覆盖层关掉；
    /// 2. 只有在**收尾阶段**（`SILENCE_COMPLETION_PHASES`）静默 10 秒才算完成。
    ///    ZB 中途联网（`Authors list fetch`，实测静默 10~41 秒）和 ClassGraph 全量扫描期间
    ///    同样没有 `[ZB]` 输出，按 10 秒判定会在加载中途误判成"加载完成"。
    ///    其它阶段只在静默 `ZB_IDLE_TIMEOUT_ANY_PHASE` 后才兜底结束。
    fn idle_completion(
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

    /// 从 `[ZB]` 行推导当前加载阶段（用于前端显示）。
    fn zb_phase(line: &str) -> &'static str {
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

    fn completion_marker(line: &str) -> Option<&'static str> {
        COMPLETION_MARKERS
            .iter()
            .find(|marker| line.contains(**marker))
            .copied()
    }

    /// 根据 `[ZB]` 阶段与已输出行数估算加载进度（0.0~1.0）。
    ///
    /// ZombieBuddy 不提供总进度，这里用"阶段里程碑 + 行数缓慢增长"的方式估算，
    /// 并且 `OverlayUi::set_progress` 只会让进度前进，不会回退。
    fn progress_for(phase: &str, zb_lines: u64) -> f32 {
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
        // 只是分母就绪前的兜底，压低上限避免"看起来卡住"。
        let creep = (zb_lines as f32 / 3000.0 * 0.6).min(0.6);
        phase_value.max(creep)
    }

    /// 统计"本次真正要加载的 Java 模组 jar"声明的注入方法(advice)总数——进度条分母。
    ///
    /// 这里用的是运行期日志里 `[ZB] java mod list to load:` 列出的 jar（已排除被禁用的模组），
    /// 再加上 ZombieBuddy 自带的 bundled patches。与运行期 `[ZB] patching … with N advice(s)`
    /// 的 ΣN 高度吻合（实测 614 vs 611）。
    fn expected_advice_count(jars: &[String]) -> Option<usize> {
        let mut total: usize = jars
            .iter()
            .map(|path| crate::count_jar_advice_methods(Path::new(path)))
            .sum();

        let libraries = crate::all_libraries_from_roots(&crate::steam_roots());
        if let Some(source) = crate::zombie_buddy_source_from_libraries(&libraries) {
            total += crate::count_jar_advice_methods(&source.join("libs").join("ZombieBuddy.jar"));
        }

        (total > 0).then_some(total)
    }

    /// 解析 `[ZB] patching <目标> with N advice(s)` 里的 N。
    fn parse_advice_count(line: &str) -> Option<u64> {
        let tail = line.split("patching ").nth(1)?.split(" with ").nth(1)?;
        let digits: String = tail
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        digits.parse().ok()
    }

    /// 从 `[ZB]     <modId>   <路径>.jar` 行里取出最后的 jar 路径。
    fn extract_jar_path(line: &str) -> Option<String> {
        line.split_whitespace()
            .rev()
            .find(|token| token.to_ascii_lowercase().ends_with(".jar"))
            .map(str::to_string)
    }

    /// 创建/打开会话用的命名事件（手动重置，初始未触发）。
    fn create_named_event(pid: u32, prefix: &str) -> Result<HANDLE, String> {
        let name = format!("{prefix}{pid}");
        let wide_name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { CreateEventW(None, true, false, PCWSTR::from_raw(wide_name.as_ptr())) }
            .map_err(|error| format!("创建事件 {name} 失败: {error}"))
    }

    /// 游戏进程是否仍在运行。
    fn process_alive(pid: u32) -> bool {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut exit_code = 0u32;
            let ok = GetExitCodeProcess(handle, &mut exit_code).is_ok();
            let _ = CloseHandle(handle);
            ok && exit_code == STILL_ACTIVE
        }
    }

    /// 目标进程是否已经有可见的顶层窗口（游戏窗口出现）。
    pub(super) fn has_visible_window(pid: u32) -> bool {
        struct Search {
            pid: u32,
            found: bool,
        }

        unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = unsafe { &mut *(lparam.0 as *mut Search) };
            let mut window_pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
            if window_pid == search.pid
                && unsafe { IsWindowVisible(hwnd).as_bool() }
                && unsafe { GetWindowTextLengthW(hwnd) } > 0
            {
                search.found = true;
                return FALSE;
            }
            TRUE
        }

        let mut search = Search { pid, found: false };
        // SAFETY: 回调只在本次调用期间借用 search，EnumWindows 返回后不再引用。
        let _ = unsafe { EnumWindows(Some(callback), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    /// 往游戏控制台输出文件追加一条记录（失败不致命）。
    fn append_console_record(path: &Path, record: &str) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{record}");
        }
    }

    fn emit_session(
        app: &AppHandle,
        phase: &str,
        message: &str,
        pid: Option<u32>,
        process_name: Option<&str>,
        zb_lines: u64,
        elapsed: Duration,
    ) {
        let _ = app.emit(
            SESSION_EVENT,
            OverlaySessionUpdate {
                phase: phase.to_string(),
                message: message.to_string(),
                pid,
                process_name: process_name.map(str::to_string),
                zb_lines,
                elapsed_ms: elapsed.as_millis() as u64,
            },
        );
    }

    #[cfg(test)]
    mod tests {
        use super::{
            completion_marker, extract_jar_path, idle_completion, parse_advice_count, zb_phase,
            ZB_IDLE_TIMEOUT, ZB_IDLE_TIMEOUT_ANY_PHASE,
        };
        use std::time::{Duration, Instant};

        #[test]
        fn parses_advice_counts_from_console_lines() {
            // 真实日志格式（advice 数量就是精确进度的分子）。
            assert_eq!(
                parse_advice_count(
                    "LOG  : General      f:0> [ZB] patching zombie.debug.DebugLog.log with 1 advice(s)"
                ),
                Some(1)
            );
            assert_eq!(
                parse_advice_count("[ZB] patching zombie.core.X.y with 12 advice(s)"),
                Some(12)
            );
            assert_eq!(
                parse_advice_count("[ZB] Transformed: zombie.core.X (new load)"),
                None
            );
            assert_eq!(
                parse_advice_count("LOG  : General      f:0> [pzopt] something"),
                None
            );
        }

        #[test]
        #[ignore]
        fn replays_real_console_for_precise_progress() {
            let path = r"C:\Users\Administrator\Zomboid\console.txt";
            let Ok(content) = std::fs::read_to_string(path) else {
                println!("没有找到真实日志，跳过");
                return;
            };

            let mut jars: Vec<String> = Vec::new();
            let mut collecting = false;
            let mut applied: u64 = 0;
            let mut timeline: Vec<(u64, u64)> = Vec::new();
            for line in content.lines() {
                if line.contains("java mod list to load") {
                    collecting = true;
                    jars.clear();
                } else if collecting {
                    if let Some(value) = extract_jar_path(line) {
                        jars.push(value);
                    } else if line.contains("[ZB]") {
                        collecting = false;
                    }
                }
                if let Some(count) = parse_advice_count(line) {
                    applied += count;
                    timeline.push((applied, applied));
                }
            }

            let mut total: usize = jars
                .iter()
                .map(|value| crate::count_jar_advice_methods(std::path::Path::new(value)))
                .sum();
            let libraries = crate::all_libraries_from_roots(&crate::steam_roots());
            if let Some(source) = crate::zombie_buddy_source_from_libraries(&libraries) {
                total +=
                    crate::count_jar_advice_methods(&source.join("libs").join("ZombieBuddy.jar"));
            }

            println!("jars={} applied={applied} total={total}", jars.len());
            if total > 0 {
                println!(
                    "final exact = {:.3}",
                    (applied as f32 / total as f32).min(0.97)
                );
                for index in [0usize, 50, 100, 200, 300, 400, 500, 600] {
                    if let Some((value, _)) = timeline.get(index) {
                        println!(
                            "  第 {index} 次注入后: {value}/{total} = {:.3}",
                            (*value as f32 / total as f32).min(0.97)
                        );
                    }
                }
            }
        }

        #[test]
        fn extracts_jar_paths_from_mod_list_lines() {
            assert_eq!(
                extract_jar_path(
                    r"LOG  : General      f:0> [ZB]     viewpoint                     H:\SteamLibrary\steamapps\workshop\content\108600\3809306528\mods\Viewpoint\42\media\java\client\Viewpoint.jar"
                ),
                Some(r"H:\SteamLibrary\steamapps\workshop\content\108600\3809306528\mods\Viewpoint\42\media\java\client\Viewpoint.jar".to_string())
            );
            assert_eq!(
                extract_jar_path("[ZB] Java mod approvals read from ..."),
                None
            );
        }

        #[test]
        fn idle_stop_requires_rendering_and_a_late_phase() {
            let now = Instant::now();
            let old = now - (ZB_IDLE_TIMEOUT + Duration::from_secs(5));
            let ancient = now - (ZB_IDLE_TIMEOUT_ANY_PHASE + Duration::from_secs(5));

            // 这是之前的真实故障：游戏还没开始换帧，[ZB] 先静默了 30 秒 → 绝不能判定完成。
            assert!(!idle_completion(false, "初始化完成回调", Some(old), now));
            // 已经开始渲染但最近还有 [ZB] 输出 → 继续等待。
            assert!(!idle_completion(true, "初始化完成回调", Some(now), now));
            assert!(!idle_completion(true, "初始化完成回调", None, now));
            // 收尾阶段静默 10 秒 → 判定完成。
            assert!(idle_completion(true, "初始化完成回调", Some(old), now));
            assert!(idle_completion(true, "注册 Lua 接口", Some(old), now));
            // 00:31 的真实故障：ZB 在"校验模组来源"阶段联网静默 10.4 秒 →
            // 绝不能判定完成（游戏还在加载，判定完成会把覆盖层提前摘掉）。
            assert!(!idle_completion(true, "校验模组来源", Some(old), now));
            // 同一个阶段静默超过 120 秒才兜底结束。
            assert!(idle_completion(true, "校验模组来源", Some(ancient), now));
        }

        #[test]
        fn maps_zombie_buddy_console_lines_to_phases() {
            assert_eq!(
                zb_phase("LOG  : General      f:0> [ZB] ZomboidFileSystem.loadMods(102 mods) ..."),
                "扫描模组列表"
            );
            assert_eq!(
                zb_phase("LOG  : General      f:0> [ZB] checking mods ban status"),
                "校验模组来源"
            );
            assert_eq!(
                zb_phase("LOG  : General      f:0> [ZB] Running callbacks for onGameInitComplete"),
                "初始化完成回调"
            );
            assert_eq!(
                zb_phase("LOG  : General      f:0> [ZB] patching zombie.GameWindow.init with 1 advice(s)"),
                "注入游戏类"
            );
            assert_eq!(zb_phase("[ZB] 其它内容"), "加载中");
        }

        #[test]
        fn detects_main_menu_completion_markers() {
            assert_eq!(
                completion_marker(
                    "LOG  : General      f:0> STATE: exit zombie.gameStates.TermsOfServiceState"
                ),
                Some("STATE: exit zombie.gameStates.TermsOfServiceState")
            );
            assert_eq!(
                completion_marker(
                    "LOG  : Lua          f:0> :	[pzopt] overlay item: added to the main menu"
                ),
                Some("added to the main menu")
            );
            assert_eq!(
                completion_marker(
                    "LOG  : Lua          f:0> :	[ZombieBuddy] ZombieBuddy.getVersion() = 2.3.4"
                ),
                Some("ZombieBuddy.getVersion()")
            );
            assert_eq!(
                completion_marker("LOG  : General      f:0> [ZB] patching something"),
                None
            );
        }
    }
}

/// 游戏进程 / 窗口状态（启动页每 0.3s 轮询用）。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameProcessStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    /// 是否已经出现可见的游戏窗口（复用注入时同一套检测逻辑）。
    pub window_ready: bool,
}

/// 检测游戏进程是否已创建、窗口是否就绪。
#[cfg(windows)]
#[tauri::command]
pub fn check_game_process() -> GameProcessStatus {
    match crate::overlay::imp::find_game_process() {
        Some((pid, name)) => GameProcessStatus {
            running: true,
            pid: Some(pid),
            process_name: Some(name.to_string()),
            window_ready: imp::has_visible_window(pid),
        },
        None => GameProcessStatus {
            running: false,
            pid: None,
            process_name: None,
            window_ready: false,
        },
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn check_game_process() -> GameProcessStatus {
    GameProcessStatus {
        running: false,
        pid: None,
        process_name: None,
        window_ready: false,
    }
}

/// 强制结束游戏进程（启动页"停止游戏进程"按钮）。
#[cfg(windows)]
#[tauri::command]
pub fn stop_game_process() -> Result<String, String> {
    let Some((pid, name)) = crate::overlay::imp::find_game_process() else {
        return Err("没有找到正在运行的游戏进程。".to_string());
    };
    let output = std::process::Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output()
        .map_err(|error| format!("结束游戏进程失败: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("结束游戏进程失败: {detail}"));
    }
    Ok(format!("已强制结束 {name}（PID {pid}）"))
}

#[cfg(not(windows))]
#[tauri::command]
pub fn stop_game_process() -> Result<String, String> {
    Err("结束游戏进程只支持 Windows".to_string())
}

/// 游戏启动后调用：等窗口出现 → 自动注入 → 实时汇报 ZombieBuddy 加载进度 → 完成后停止覆盖。
#[tauri::command]
pub fn start_overlay_session(app: tauri::AppHandle, dll_path: String) -> Result<String, String> {
    #[cfg(windows)]
    {
        let trimmed = dll_path.trim().to_string();
        if trimmed.is_empty() {
            return Err("覆盖层 DLL 路径为空。".to_string());
        }
        let spawned = std::thread::Builder::new()
            .name("pz-overlay-session".to_string())
            .spawn(move || imp::run_session(app, trimmed));
        match spawned {
            Ok(_) => Ok("已开始监听：游戏窗口出现后会自动注入覆盖层。".to_string()),
            Err(error) => Err(format!("启动覆盖层监听线程失败: {error}")),
        }
    }

    #[cfg(not(windows))]
    {
        let _ = (app, dll_path);
        Err("覆盖层会话只支持 Windows x64".to_string())
    }
}
