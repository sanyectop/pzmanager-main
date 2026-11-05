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
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible,
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
    // 静默阈值与收尾阶段由 overlay-core 统一维护（DLL 回退自读使用同一份）。
    /// 完全没有 `[ZB]` 输出时的兜底时长（从注入时刻起算）。
    const NO_ZB_TIMEOUT: Duration = Duration::from_secs(180);
    /// 注入后等待「覆盖层画出第一帧」的最长时间（游戏渲染启动可能很慢）。
    const STARTED_WAIT_TIMEOUT: Duration = Duration::from_secs(180);
    /// 覆盖层从注入起的最长持续时间（DLL 侧另有 10 分钟安全上限）。
    const SESSION_MAX_DURATION: Duration = Duration::from_secs(540);
    /// 进度事件最小发送间隔，避免刷爆前端。
    const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(200);
    /// 覆盖层达到 100% 后继续显示的时间。
    ///
    /// 只留几帧给最后一帧上传/呈现。42.22 的主菜单在完成标记之后马上播放
    /// Bink 过场（console 里的 `binkId:`），停留 2 秒会让覆盖层压在过场上，
    /// 表现为加载完成时闪屏；这里收紧到 150ms。
    const COMPLETION_HOLD: Duration = Duration::from_millis(150);
    /// 停止事件名前缀，需与 DLL 保持一致。
    const STOP_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStop_";
    /// 「已开始绘制」事件名前缀，需与 DLL 保持一致。
    const STARTED_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStarted_";
    /// `STILL_ACTIVE`（`GetExitCodeProcess` 的返回值）。
    const STILL_ACTIVE: u32 = 259;

    // ---- 渲染看门狗 -------------------------------------------------------
    // 与 DLL 约定的渲染采样文件（位于 `%TEMP%`）。有些机器游戏根本不写
    // console.txt（实测整场只有启动器自己追加的几行），所有基于 `[ZB]` 的完成判定
    // 都会失效，覆盖层只能等 180 秒兜底——用户看到的就是"加载界面压在已经能玩的游戏上"。
    const RENDER_REPORT_FILE: &str = "pz_overlay_render.txt";
    /// 采样新鲜度：DLL 每 500ms 回写一次，2 秒没更新视为已停止。
    const RENDER_REPORT_STALE: Duration = Duration::from_secs(2);
    /// 判定「游戏已在持续渲染」的最低速率（hook 调用/秒，约等于 20fps 呈现）。
    const RENDER_READY_RATE: u64 = 40;
    /// 持续渲染多久后才允许按渲染收尾（给慢机器留出 console.txt 出现的余量）。
    const RENDER_READY_HOLD: Duration = Duration::from_secs(10);
    /// 读取渲染采样的节流间隔（文件只有几十字节，但没必要 40ms 读一次）。
    const RENDER_POLL_INTERVAL: Duration = Duration::from_millis(250);

    struct SessionOutcome {
        reason: String,
        zb_lines: u64,
        pid: u32,
        process_name: String,
    }

    /// 读取 DLL 回写的渲染速率（hook 调用/秒）。采样过期或不存在时返回 None。
    fn read_render_rate() -> Option<u64> {
        let path = std::env::temp_dir().join(RENDER_REPORT_FILE);
        let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
        if modified.elapsed().ok()? > RENDER_REPORT_STALE {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        content
            .lines()
            .find_map(|line| line.strip_prefix("rate=")?.trim().parse().ok())
    }

    /// 渲染看门狗：连续观察到足够高的换帧速率后才开始计时，任何一次掉下去都重置。
    struct RenderWatch {
        ready_since: Option<Instant>,
        last_poll: Option<Instant>,
        last_rate: Option<u64>,
    }

    impl RenderWatch {
        fn new() -> Self {
            Self {
                ready_since: None,
                last_poll: None,
                last_rate: None,
            }
        }

        /// 每 250ms 采样一次；速率达标才累计，否则清零。
        fn observe(&mut self, now: Instant) {
            if self
                .last_poll
                .is_some_and(|last| now.duration_since(last) < RENDER_POLL_INTERVAL)
            {
                return;
            }
            self.last_poll = Some(now);
            let rate = read_render_rate();
            self.last_rate = rate;
            if rate.is_some_and(|value| value >= RENDER_READY_RATE) {
                self.ready_since.get_or_insert(now);
            } else {
                self.ready_since = None;
            }
        }

        /// 是否已经持续渲染足够久（`RENDER_READY_HOLD`）。
        fn sustained(&self, now: Instant) -> bool {
            self.ready_since
                .is_some_and(|since| now.duration_since(since) >= RENDER_READY_HOLD)
        }

        fn rate(&self) -> Option<u64> {
            self.last_rate
        }
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
        /// 覆盖层背景色（`#RRGGBB`）。None = 让 DLL 用它自己的默认纯红。
        background_color: Option<String>,
        /// 游戏控制台路径：启动器关闭后 DLL 靠它自己继续读日志。
        console_path: std::path::PathBuf,
        launcher_pid: u32,
        /// 本次会话的起始时间（UNIX 毫秒），DLL 用它拒绝上一局的日志。
        session_start_ms: u64,
        /// 心跳节流：即使没有新日志/进度，也至少每秒刷新一次状态文件。
        last_heartbeat: Instant,
    }

    impl StatusFile {
        fn new(console_path: std::path::PathBuf, session_start: SystemTime) -> Self {
            let path = std::env::temp_dir().join("pz_overlay_status.txt");
            // 清掉上一局残留，避免界面一开始就显示上次的终值（例如 89%/100%）。
            let _ = std::fs::remove_file(&path);
            // 内存上限 / 覆盖层背景色都取启动器设置里的值
            // （游戏侧命令行看不到 JSON 里的 -Xmx，DLL 也读不到 settings.json）。
            let loaded_settings = crate::app_settings_path()
                .ok()
                .and_then(|settings| crate::load_app_settings_at(&settings).ok());
            let memory_limit_mb = loaded_settings.as_ref().and_then(|settings| settings.memory_limit_mb);
            let background_color = loaded_settings
                .and_then(|settings| settings.overlay_background_color)
                .as_deref()
                .and_then(crate::normalize_overlay_background_color);
            let session_start_ms = session_start
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or(0);
            Self {
                path,
                last_write: Instant::now() - Duration::from_secs(1),
                lines: std::collections::VecDeque::new(),
                phase: "准备中".to_string(),
                progress: 0.0,
                has_progress: false,
                memory_limit_mb,
                background_color,
                console_path,
                launcher_pid: std::process::id(),
                session_start_ms,
                last_heartbeat: Instant::now() - Duration::from_secs(1),
            }
        }

        /// 追加一行控制台输出（保留最新 6 行，剥掉 `LOG : General f:0>` 前缀）。
        fn push_line(&mut self, line: &str) {
            let Some(value) = overlay_core::normalize_console_line(line) else {
                return;
            };
            if self.lines.back().is_some_and(|last| last == &value) {
                return;
            }
            self.lines.push_back(value);
            while self.lines.len() > overlay_core::MAX_CONSOLE_LINES {
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

        /// 启动器存活心跳：即使没有新日志/进度，也至少每秒刷一次状态文件。
        ///
        /// DLL 用 `beat=` 判断启动器是否还活着；没有这行时，`[ZB]` 长时间静默
        /// （Authors list fetch / ClassGraph 扫描）会被误判为启动器已关闭。
        fn heartbeat(&mut self) {
            if self.last_heartbeat.elapsed() < Duration::from_secs(1) {
                return;
            }
            self.last_heartbeat = Instant::now();
            self.force_flush();
        }

        fn close(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }

        /// 当前阶段（诊断日志用）。
        fn phase(&self) -> &str {
            &self.phase
        }

        fn flush(&mut self) {
            // 与 DLL 侧界面刷新对齐：40ms ≈ 25fps。
            if self.last_write.elapsed() < Duration::from_millis(40) {
                return;
            }
            self.last_write = Instant::now();
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or(0);
            let mut text = String::new();
            text.push_str(&format!("beat={now_ms}\n"));
            text.push_str(&format!("launcherpid={}\n", self.launcher_pid));
            text.push_str(&format!("session={}\n", self.session_start_ms));
            text.push_str(&format!(
                "console={}\n",
                self.console_path.to_string_lossy()
            ));
            if self.has_progress {
                text.push_str(&format!("progress={:.3}\n", self.progress));
            }
            text.push_str(&format!("phase={}\n", self.phase));
            if let Some(limit) = self.memory_limit_mb {
                text.push_str(&format!("memmax={limit}\n"));
            }
            if let Some(color) = &self.background_color {
                text.push_str(&format!("bgcolor={color}\n"));
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
        overlay_imp::ensure_overlay_dll_is_current(&dll_path)?;
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
            if let Some((candidate_pid, candidate_name)) = find_game_candidate() {
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

        // 目标进程已有同名模块时不再重复注入（极速启动经 -agentpath 预载是正常情况）；
        // 但要区分预载的是不是我们这份：别的位置的副本往往是旧版（只刷红、没界面），
        // 这是"红屏无字"的主要成因之一，必须把实际路径告诉用户。
        let mut preloaded_from: Option<String> = None;
        match overlay_imp::loaded_module_path(pid, &dll_name) {
            Ok(Some(loaded_path)) => preloaded_from = Some(loaded_path),
            // 枚举失败按未加载处理，交给注入阶段给出更准确的报错。
            Ok(None) | Err(_) => {
                crate::overlay::injector::inject_dll(pid, &dll_path)?;
            }
        }
        let already_loaded = preloaded_from.is_some();
        let mismatch_warning = preloaded_from
            .as_ref()
            .filter(|loaded| !overlay_imp::module_path_matches(loaded, &dll_path))
            .map(|loaded| {
                format!(
                    "警告：游戏进程里已加载另一份覆盖层 DLL（{loaded}），与本程序自带的 {} 不一致；\
                     旧版只会显示红屏、没有文字，请检查极速启动残留的 -agentpath 或重装 PZManager。",
                    dll_path.display()
                )
            });
        let injected_at = Instant::now();

        let console_path = crate::user_zomboid_directory()?.join("console.txt");
        let session_start = SystemTime::now() - started.elapsed();
        // 加载界面：只写状态文件，由 DLL 在游戏窗口**内部**绘制（红底 + 左下角控制台 + 底部进度条）。
        // 状态文件里同时带 console 路径、会话起始时间和心跳，启动器关闭后 DLL 可以自己继续读日志。
        let mut status = StatusFile::new(console_path.clone(), session_start);
        status.set_phase_only("等待 ZombieBuddy 输出");
        // 控制台记录里写"实际在进程里的那份 DLL"，预载旧副本时这条记录就是关键证据。
        let injection_record = format!(
            "[PZManager] 覆盖层 DLL {} 已{}（{}，PID {pid}）",
            preloaded_from.as_deref().unwrap_or(&dll_path.display().to_string()),
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
            &match &mismatch_warning {
                Some(warning) => {
                    format!("{warning} 覆盖层已开启，正在读取 [ZB] 加载进度…")
                }
                None => "覆盖层已开启，正在读取 [ZB] 加载进度…".to_string(),
            },
            Some(pid),
            Some(process_name),
            0,
            started.elapsed(),
        );

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

        match &outcome {
            Ok(value) => crate::launcher_log::info(&format!("覆盖层会话结束：{}", value.reason)),
            Err(error) => crate::launcher_log::warn(&format!("覆盖层会话失败：{error}")),
        }

        // 游戏中途退出 = 闪退：从 console 尾部找已知崩溃签名，把可操作的诊断
        // 拼进会话结果（启动面板 / 日志 / console 记录都会带上），而不是只说
        // 「游戏进程已退出」让用户猜。
        let mut outcome = outcome.map(|mut value| {
            if value.reason.contains("游戏进程已退出") {
                if let Some(diagnosis) = diagnose_game_exit_from_console(&console_path) {
                    value.reason.push(' ');
                    value.reason.push_str(&diagnosis);
                }
            }
            value
        });

        // 会话详情：用户反馈"覆盖层不消失 / 闪屏"时，这一行直接给出断点——
        // 是没有 ZB 输出、分母没解析出来，还是静默兜底。没有它只能靠猜。
        let advices = expected_advices
            .lock()
            .ok()
            .and_then(|value| *value)
            .map(|total| total.to_string())
            .unwrap_or_else(|| "未解析".to_string());
        let render_rate = read_render_rate()
            .map(|rate| format!("{rate}/s"))
            .unwrap_or_else(|| "无采样".to_string());
        crate::launcher_log::info(&format!(
            "覆盖层会话详情：ZB 行数={}，阶段={}，advice 分母={}，渲染≈{}，耗时={:.1}s，结果={}",
            outcome.as_ref().map(|value| value.zb_lines).unwrap_or(0),
            status.phase(),
            advices,
            render_rate,
            window_detected_at.elapsed().as_secs_f64(),
            outcome
                .as_ref()
                .map(|value| value.reason.as_str())
                .unwrap_or("失败"),
        ));

        if let Ok(outcome) = &outcome {
            status.set_precise_progress("加载完成", 1.0);
            status.force_flush();
            // 主菜单标记 / ZombieBuddy 完成回调出现时不再等待：42.22 之后紧接着
            // 加载纹理并播放 Bink 主菜单过场，哪怕 40ms 也可能压到过场首帧。
            // 其它收尾只留 150ms 给最后一帧。
            let hold = if outcome.reason.contains("主菜单标记")
                || outcome.reason.contains("初始化完成回调")
            {
                Duration::ZERO
            } else {
                COMPLETION_HOLD
            };
            thread::sleep(hold);
            let _ = outcome;
        }

        // 「移除覆盖层」的这一刻就是启动时长的终点。
        //
        // 为什么不在主菜单标记处记：会话通常在 ≥97% advice 或 `[ZB]` 静默 10 秒时就结束了
        // （见 track_console 里的两个提前返回），主菜单标记那一步经常根本走不到。
        // 而"发停止事件"是所有成功注入路径都会经过的地方。
        //
        // 例外：游戏中途退出（闪退）不算一次「启动」——把"从窗口出现到崩溃"的时长
        // 记成启动用时是垃圾数据，用户看到的启动时间对比会失真。崩溃时跳过记录，
        // 前端保留上一次真实启动的数值。
        if !outcome
            .as_ref()
            .is_ok_and(|value| value.reason.contains("游戏进程已退出"))
        {
            let startup_ms = window_detected_at.elapsed().as_millis() as u64;
            let timing = record_startup_timing(startup_ms);
            let _ = app.emit(STARTUP_EVENT, timing);
        }

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
        //
        // 有些机器游戏根本不写这个文件（整场只有启动器自己追加的几行），此时旧逻辑
        // 会干等 180 秒，覆盖层一直压在已经能玩的游戏上。渲染看门狗在这里也生效：
        // 游戏已经在持续换帧、而 console.txt 仍不更新，说明读不到 [ZB] 进度，
        // 直接收尾（理由写清楚，日志里能一眼看出是"没有控制台输出"而不是卡死）。
        let fresh_deadline = injected_at + CONSOLE_FRESH_TIMEOUT;
        let mut early_watch = RenderWatch::new();
        loop {
            // 等文件期间也保持心跳：DLL 不能因为这里暂时没有新日志就误判启动器已关闭。
            status.heartbeat();
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
            early_watch.observe(Instant::now());
            if early_watch.sustained(Instant::now()) {
                return Ok(outcome(
                    &format!(
                        "游戏已持续渲染 {} 秒，但 console.txt 没有更新（读不到 [ZB] 进度），结束覆盖。",
                        RENDER_READY_HOLD.as_secs()
                    ),
                    0,
                    pid,
                    format!(
                        " 渲染≈{}/s，console.txt={}",
                        early_watch.rate().unwrap_or(0),
                        console_path.display()
                    ),
                ));
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
        // 主循环里的渲染看门狗：console.txt 有更新但始终没有 [ZB] 行时同样靠它收尾。
        let mut render_watch = RenderWatch::new();

        loop {
            status.heartbeat();
            if !process_alive(pid) {
                return Ok(outcome("游戏进程已退出。", zb_lines, pid, String::new()));
            }
            render_watch.observe(Instant::now());

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
            // 没有 [ZB] 进度可显示、但游戏已经在持续渲染：覆盖层留着只会压住游戏画面。
            if zb_lines == 0 && render_watch.sustained(Instant::now()) {
                return Ok(outcome(
                    &format!(
                        "游戏已持续渲染 {} 秒（本次没有 [ZB] 输出），结束覆盖。",
                        RENDER_READY_HOLD.as_secs()
                    ),
                    zb_lines,
                    pid,
                    format!(" 渲染≈{}/s", render_watch.rate().unwrap_or(0)),
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
                        if on_game_init_complete(&line) {
                            return Ok(outcome(
                                "ZombieBuddy 初始化完成回调结束，覆盖结束。",
                                zb_lines,
                                pid,
                                String::new(),
                            ));
                        }
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

    /// 游戏中途退出时，从 console.txt 尾部找已知崩溃签名，给用户一句可操作的诊断。
    ///
    /// 只在「游戏进程已退出」的收尾里调用：完成/超时收尾不需要。文件不存在或读不出
    /// 都返回 None（保持原 reason）。扫整个 64KB 尾部——崩溃栈（uncaughtException →
    /// NoClassDefFoundError → GameThread exited）按这个顺序落盘，签名在退出标记之前。
    fn diagnose_game_exit_from_console(console_path: &Path) -> Option<String> {
        let mut file = std::fs::File::open(console_path).ok()?;
        let length = file.metadata().ok()?.len();
        let start = length.saturating_sub(64 * 1024);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;

        for line in text.lines().rev() {
            let line = line.trim();
            let missing_class = line
                .split_once("NoClassDefFoundError")
                .or_else(|| line.split_once("ClassNotFoundException"))
                .and_then(|(_, rest)| {
                    rest.trim_start()
                        .strip_prefix(':')
                        .map(|value| value.trim().to_string())
                });
            if let Some(class) = missing_class {
                let class = class.split_whitespace().next().unwrap_or(&class);
                return Some(format!(
                    "诊断：游戏缺少类 {class}——几乎总是「游戏版本与模组不匹配」。\
Steam 刚更新过游戏或模组、或刚用「版本还原」回滚过其中一边时最容易发生；\
把游戏和模组更新/回滚到同一时期即可。"
                ));
            }
            if line.contains("OutOfMemoryError") {
                return Some(
                    "诊断：游戏内存不足（OutOfMemoryError）。请在设置里调高「游戏内存」上限后重试。"
                        .to_string(),
                );
            }
        }
        if text.contains("uncaughtException") || text.contains("Exception thrown") {
            return Some(
                "诊断：游戏在加载中抛出未处理异常后退出。常见原因是模组与当前游戏版本不匹配；\
可先禁用最近新增的模组再试。"
                    .to_string(),
            );
        }
        None
    }

    #[test]
    fn diagnoses_known_crash_signatures_from_console_tail() {
        let directory = tempfile::tempdir().unwrap();
        let console = directory.path().join("console.txt");
        std::fs::write(
            &console,
            "LOG  : early noise\r\n\
             [ZB] patching zombie.IsoChunk with 2 advice(s)\r\n\
             ERROR: General      f:0> GameWindow.uncaughtException> Exception thrown\r\n\
             \tjava.lang.NoClassDefFoundError: zombie/util/list/PZArrayList at Class.getDeclaredFields0\r\n\
             \tCaused by: java.lang.ClassNotFoundException: zombie.util.list.PZArrayList\r\n\
             LOG  : GameThread exited.\r\n\
             [PZManager] 覆盖层已停止：游戏进程已退出。\r\n",
        )
        .unwrap();

        let diagnosis = diagnose_game_exit_from_console(&console).unwrap();
        // 倒序扫描会先命中 "Caused by" 行，类名以点分形式提取。
        assert!(diagnosis.contains("zombie.util.list.PZArrayList"), "{diagnosis}");
        assert!(diagnosis.contains("游戏版本与模组不匹配"));

        // 内存不足有独立提示。
        std::fs::write(
            &console,
            "ERROR: java.lang.OutOfMemoryError: Java heap space\r\nGameThread exited.\r\n",
        )
        .unwrap();
        let diagnosis = diagnose_game_exit_from_console(&console).unwrap();
        assert!(diagnosis.contains("内存不足"));

        // 没有崩溃签名（正常完成）返回 None，不影响正常收尾。
        std::fs::write(&console, "LOG  : all good\r\n").unwrap();
        assert!(diagnose_game_exit_from_console(&console).is_none());
        // 文件不存在也返回 None。
        assert!(diagnose_game_exit_from_console(&directory.path().join("missing.txt")).is_none());
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
        overlay_core::idle_completion(overlay_started, phase, last_zb_at, now)
    }

    /// 从 `[ZB]` 行推导当前加载阶段（用于前端显示）。
    fn zb_phase(line: &str) -> &'static str {
        overlay_core::zb_phase(line)
    }

    /// ZombieBuddy 的初始化完成回调是它在退出前的最后一条里程碑。
    ///
    /// 之后游戏还要加载纹理和主菜单，主菜单会播放 Bink 过场（console 的
    /// `binkId:`）。等到主菜单标记再停，覆盖层已经压在过场上；这里在
    /// 回调出现时就结束，宁可让游戏自己的加载画面露出来。
    fn on_game_init_complete(line: &str) -> bool {
        overlay_core::on_game_init_complete(line)
    }

    fn completion_marker(line: &str) -> Option<&'static str> {
        overlay_core::completion_marker(line)
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
        overlay_core::parse_advice_count(line)
    }

    /// 从 `[ZB]     <modId>   <路径>.jar` 行里取出最后的 jar 路径。
    fn extract_jar_path(line: &str) -> Option<String> {
        overlay_core::extract_jar_path(line)
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

    /// 游戏主窗口标题（精确匹配，忽略大小写与首尾空白）。
    ///
    /// 必须是**精确**匹配而不是包含：Steam 内嵌浏览器窗口的标题同样包含
    /// "Project Zomboid"（商店页 / 创意工坊页），包含匹配会把 Steam 客户端进程
    /// 误当成游戏并往里注入。
    const GAME_WINDOW_TITLE: &str = "project zomboid";

    /// 进程名兜底识别时的展示名（会话消息里用）。
    const WINDOW_TITLE_MATCH_LABEL: &str = "按窗口标题识别的游戏进程";

    fn is_game_window_title(title: &str) -> bool {
        title.trim().eq_ignore_ascii_case(GAME_WINDOW_TITLE)
    }

    /// 枚举顶层窗口，找标题恰好是游戏名的可见窗口，返回所属进程 PID。
    ///
    /// 进程名识别（ProjectZomboid64.exe / javaw.exe）的兜底：部分机器上游戏进程
    /// 不叫这两个名字 —— 实测没有 Xbox 服务的机器上，任务管理器里游戏显示为
    /// wsappx（pz 是它的子进程），按进程名永远找不到，覆盖层 / 启动计时全部失效。
    pub(super) fn game_window_pid() -> Option<u32> {
        struct Search {
            found: Option<u32>,
        }

        unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = unsafe { &mut *(lparam.0 as *mut Search) };
            if !unsafe { IsWindowVisible(hwnd).as_bool() } {
                return TRUE;
            }
            let mut buffer = [0u16; 128];
            let length = unsafe { GetWindowTextW(hwnd, &mut buffer) } as usize;
            let title = String::from_utf16_lossy(&buffer[..length.min(buffer.len())]);
            if !is_game_window_title(&title) {
                return TRUE;
            }
            let mut window_pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
            if window_pid != 0 {
                search.found = Some(window_pid);
                return FALSE;
            }
            TRUE
        }

        let mut search = Search { found: None };
        // SAFETY: 回调只在本次调用期间借用 search，EnumWindows 返回后不再引用。
        let _ = unsafe { EnumWindows(Some(callback), LPARAM(&mut search as *mut Search as isize)) };
        search.found
    }

    /// 找正在运行的游戏：优先按进程名，找不到再按窗口标题兜底。
    /// 返回 (pid, 展示用进程名)。
    pub(super) fn find_game_candidate() -> Option<(u32, &'static str)> {
        crate::overlay::imp::find_game_process()
            .or_else(|| game_window_pid().map(|pid| (pid, WINDOW_TITLE_MATCH_LABEL)))
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
            completion_marker, extract_jar_path, idle_completion, is_game_window_title,
            on_game_init_complete, parse_advice_count, zb_phase, RenderWatch, StatusFile,
            COMPLETION_HOLD, RENDER_READY_HOLD, RENDER_READY_RATE,
        };
        use overlay_core::{ZB_IDLE_TIMEOUT, ZB_IDLE_TIMEOUT_ANY_PHASE};
        use std::time::{Duration, Instant};

        #[test]
        fn game_window_title_is_an_exact_match() {
            // 游戏自己的窗口标题。
            assert!(is_game_window_title("Project Zomboid"));
            assert!(is_game_window_title("  project zomboid  "));
            // 必须精确匹配：Steam 内嵌浏览器窗口（商店页/创意工坊页）的标题
            // 同样包含这段文字，包含匹配会把 Steam 进程误当成游戏并注入。
            assert!(!is_game_window_title("Project Zomboid on Steam"));
            assert!(!is_game_window_title("Project Zomboid - 创意工坊"));
            assert!(!is_game_window_title(""));
        }

        #[test]
        fn render_watch_requires_sustained_frames() {
            let now = Instant::now();
            let mut watch = RenderWatch::new();
            // 还没开始渲染：不能收尾。
            assert!(!watch.sustained(now));

            // 渲染刚好持续到阈值：可以收尾。
            watch.ready_since = Some(now - RENDER_READY_HOLD);
            watch.last_rate = Some(RENDER_READY_RATE + 60);
            assert!(watch.sustained(now));

            // 只持续了 1 秒：还不够（慢机器可能还没写出 console.txt）。
            watch.ready_since = Some(now - Duration::from_secs(1));
            assert!(!watch.sustained(now));

            // 中途掉速：计时清零。
            watch.ready_since = None;
            assert!(!watch.sustained(now));
            assert_eq!(watch.rate(), Some(RENDER_READY_RATE + 60));
        }

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
            // 同一个阶段静默超过 60 秒才兜底结束（联网静默实测最长 41 秒）。
            assert!(idle_completion(true, "校验模组来源", Some(ancient), now));
        }

        #[test]
        fn status_file_writes_heartbeat_console_and_session() {
            let temp = tempfile::tempdir().unwrap();
            let console = temp.path().join("console.txt");
            let status_path = temp.path().join("status.txt");
            let mut status = StatusFile {
                path: status_path.clone(),
                last_write: Instant::now() - Duration::from_secs(10),
                lines: std::collections::VecDeque::new(),
                phase: "测试".to_string(),
                progress: 0.0,
                has_progress: false,
                memory_limit_mb: Some(8192),
                background_color: Some("#FF0000".to_string()),
                console_path: console.clone(),
                launcher_pid: 1234,
                session_start_ms: 1_700_000_000_000,
                last_heartbeat: Instant::now() - Duration::from_secs(10),
            };
            status.set_phase_only("测试");
            status.force_flush();
            status.heartbeat();

            let content = std::fs::read_to_string(&status_path).unwrap();
            assert!(content.contains("beat="));
            assert!(content.contains("launcherpid=1234\n"));
            assert!(content.contains("session=1700000000000\n"));
            assert!(content.contains(&format!(
                "console={}\n",
                console.to_string_lossy()
            )));
            assert!(content.contains("phase=测试\n"));
            assert!(content.contains("bgcolor=#FF0000\n"));
        }

        #[test]
        fn completion_hold_leaves_room_before_the_menu_bink_intro() {
            // 42.22 的完成标记后紧跟 binkId/主菜单过场；停留过长会压在过场上闪屏。
            assert!(COMPLETION_HOLD <= Duration::from_millis(250));
        }

        #[test]
        fn stops_on_zombie_buddy_init_complete_before_the_menu_bink() {
            assert!(on_game_init_complete(
                "LOG  : General      f:0> [ZB] Running callbacks for onGameInitComplete"
            ));
            assert!(!on_game_init_complete(
                "LOG  : General      f:0> [ZB] patching zombie.IsoChunk"
            ));
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
    match imp::find_game_candidate() {
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

/// 按窗口标题找游戏窗口所属进程（进程名识别的兜底，见 `imp::game_window_pid`）。
/// `project_zomboid_running`（版本操作的「请先关闭游戏」判断）也用它，
/// 否则无 Xbox 服务的机器上游戏开着、版本操作却检测不到。
#[cfg(windows)]
pub fn game_window_pid() -> Option<u32> {
    imp::game_window_pid()
}

#[cfg(not(windows))]
pub fn game_window_pid() -> Option<u32> {
    None
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
    let Some((pid, name)) = imp::find_game_candidate() else {
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
