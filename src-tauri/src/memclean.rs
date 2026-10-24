//! 静默内存清理 / 启动时清理内存（设置 › 启动）。
//!
//! 形态参考 PCL2（Plain Craft Launcher 2）的「内存优化」，可以对照它三处实现：
//!
//! * `Plain Craft Launcher 2\\Application.xaml.vb` 的 `--memory` 分支：把清理丢给一个**独立进程**跑，
//!   用 `AvailablePhysicalMemory` 的**前后差**算出"释放了多少 K"，再当成退出码回报；
//! * `Modules\\Minecraft\\ModLaunch.vb` 的 `McLaunchMemoryOptimize`：启动流程里插一段"内存优化"，
//!   作为启动的一个阶段（本项目的「启动时清理内存」对应这一条）；
//! * `Modules\\ModEvent.vb` 的 `EventType.内存优化`：可被随时触发的清理动作（本项目的
//!   「静默内存清理」由后台轮询按阈值触发这一条）。
//!
//! 清理本身**不结束任何进程**，只是把各进程的驻留页（working set）还给系统：
//! 也就是 `SetProcessWorkingSetSizeEx(h, -1, -1)`，等价于 `EmptyWorkingSet(h)`。
//! 效果量化沿用 PCL2 的口径——清理前后的**可用物理内存差**。
//!
//! 与 PCL2 的差异（有意为之）：清理时**跳过游戏进程**。把正在跑的游戏的工作集压掉，
//! 接下来每一帧都要缺页重读，玩家感受到的就是卡顿；而清理别的进程（浏览器、Steam、
//! 甚至启动器自己）对游戏只有好处。

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

/// 静默清理的默认触发阈值：系统内存占用达到 80% 就清一次。
pub const DEFAULT_THRESHOLD_PERCENT: u8 = 80;
/// 阈值下限：太低会一直触发，清理本身也要遍历进程，没意义。
pub const MIN_THRESHOLD_PERCENT: u8 = 30;
/// 阈值上限：留出余量，别等系统已经在换页了才动手。
pub const MAX_THRESHOLD_PERCENT: u8 = 95;

/// 后台轮询间隔。取 10s：足够及时，又不会让"遍历几百个进程"这件事变成常态开销。
const POLL_INTERVAL: Duration = Duration::from_secs(10);
/// 两次静默清理之间的最小间隔。清理会带来一次全系统的缺页小高峰，
/// 不能因为内存一直居高不下就每 10 秒压一遍。
const CLEANUP_COOLDOWN: Duration = Duration::from_secs(60);

/// 清理时**绝不**碰的进程（按可执行文件名、大小写不敏感匹配）。
///
/// 游戏本体是 java.exe（PZ 的 jre64），另外把常见的 PZ 启动壳也列上：
/// 压掉它们的工作集只会让游戏卡顿，而不会腾出任何有意义的可用内存。
const PROTECTED_EXE_NAMES: &[&str] = &[
    "java.exe",
    "javaw.exe",
    "projectzomboid64.exe",
    "projectzomboid32.exe",
    "projectzomboid.exe",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStatus {
    pub total_mb: u64,
    pub available_mb: u64,
    /// 已用物理内存百分比（0–100）。
    pub used_percent: u8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    /// 成功压掉工作集的进程数。
    pub trimmed: u32,
    /// 主动跳过的进程数（自己、游戏进程、权限不足的系统进程）。
    pub skipped: u32,
    /// 打开了但压不动的进程数（受保护进程常见）。
    pub failed: u32,
    /// 清理前后可用物理内存差（MB），口径与 PCL2 一致。
    pub freed_mb: u64,
    pub used_percent_before: u8,
    pub used_percent_after: u8,
    pub elapsed_ms: u64,
}

/// 把用户给的阈值收敛到可用区间；0 视为"没设置过"，回落到默认值。
pub fn normalize_threshold(value: u8) -> u8 {
    if value == 0 {
        DEFAULT_THRESHOLD_PERCENT
    } else {
        value.clamp(MIN_THRESHOLD_PERCENT, MAX_THRESHOLD_PERCENT)
    }
}

#[cfg(windows)]
mod win {
    use std::mem::size_of;

    use super::MemoryStatus;

    #[repr(C)]
    #[derive(Default)]
    pub struct MemoryStatusEx {
        pub dw_length: u32,
        pub dw_memory_load: u32,
        pub ull_total_phys: u64,
        pub ull_avail_phys: u64,
        pub ull_total_page_file: u64,
        pub ull_avail_page_file: u64,
        pub ull_total_virtual: u64,
        pub ull_avail_virtual: u64,
        pub ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    pub fn status() -> Option<MemoryStatus> {
        let mut raw = MemoryStatusEx {
            dw_length: size_of::<MemoryStatusEx>() as u32,
            ..Default::default()
        };
        let succeeded = unsafe { GlobalMemoryStatusEx(&mut raw) };
        if succeeded == 0 || raw.ull_total_phys == 0 {
            return None;
        }
        let used = raw.ull_total_phys.saturating_sub(raw.ull_avail_phys);
        Some(MemoryStatus {
            total_mb: raw.ull_total_phys / 1024 / 1024,
            available_mb: raw.ull_avail_phys / 1024 / 1024,
            // 四舍五入，避免 79.6% 显示成 79% 而阈值 80% 迟迟不触发这种"差一点"的观感。
            used_percent: ((used.saturating_mul(100) + raw.ull_total_phys / 2) / raw.ull_total_phys)
                as u8,
        })
    }
}

#[cfg(windows)]
enum TrimOutcome {
    Trimmed,
    Skipped,
    Failed,
}

/// 把单个进程的工作集还给系统。
///
/// `(SIZE_T)-1, (SIZE_T)-1` 是 Win32 约定的"尽可能多地移除工作集页"，跟
/// `EmptyWorkingSet` 等价，但不需要额外开 `Win32_System_ProcessStatus` feature。
#[cfg(windows)]
fn trim_process_working_set(pid: u32) -> TrimOutcome {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Memory::{
        SetProcessWorkingSetSizeEx, SETPROCESSWORKINGSETSIZEEX_FLAGS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };

    let access = PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION;
    let Ok(handle) = (unsafe { OpenProcess(access, false, pid) }) else {
        // 系统进程、其它用户的进程、受保护进程都会在这里被拒绝：这是常态，不算失败。
        return TrimOutcome::Skipped;
    };
    let result = unsafe {
        SetProcessWorkingSetSizeEx(
            handle,
            usize::MAX,
            usize::MAX,
            SETPROCESSWORKINGSETSIZEEX_FLAGS(0),
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    if result.is_ok() {
        TrimOutcome::Trimmed
    } else {
        TrimOutcome::Failed
    }
}

#[cfg(windows)]
fn trim_all_processes(report: &mut CleanupReport) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return;
    };

    let self_pid = std::process::id();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while has_entry {
        let pid = entry.th32ProcessID;
        let name_len = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe_name = String::from_utf16_lossy(&entry.szExeFile[..name_len]).to_ascii_lowercase();

        // pid 0 是 System Idle Process（没有地址空间），我们自己也不清：
        // 把启动器自己的界面压进页面文件，下一次交互就会卡一下。
        if pid == 0 || pid == self_pid || PROTECTED_EXE_NAMES.contains(&exe_name.as_str()) {
            report.skipped += 1;
        } else {
            match trim_process_working_set(pid) {
                TrimOutcome::Trimmed => report.trimmed += 1,
                TrimOutcome::Skipped => report.skipped += 1,
                TrimOutcome::Failed => report.failed += 1,
            }
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }

    let _ = unsafe { CloseHandle(snapshot) };
}

#[cfg(not(windows))]
fn trim_all_processes(_report: &mut CleanupReport) {}

/// 当前系统内存状态；读不到就返回 `None`（调用方一律按"这次不清理"处理）。
pub fn status() -> Option<MemoryStatus> {
    #[cfg(windows)]
    {
        win::status()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 立即清理一次：遍历进程、把工作集还给系统，并回报释放量。
///
/// 这个函数是**同步阻塞**的（典型 100–300 ms，进程越多越久），调用方要放到
/// 后台线程或异步命令里，别卡住 UI 线程。
pub fn clean_memory() -> CleanupReport {
    let started = Instant::now();
    let before = status();
    let mut report = CleanupReport {
        used_percent_before: before.map_or(0, |value| value.used_percent),
        ..Default::default()
    };

    trim_all_processes(&mut report);

    let after = status();
    report.used_percent_after = after.map_or(0, |value| value.used_percent);
    if let (Some(before), Some(after)) = (before, after) {
        // 只有在可用内存**变多**时才算释放；变小说明期间别的进程又吃走了内存，
        // 那不是本次清理的功劳，不要虚报。
        report.freed_mb = after.available_mb.saturating_sub(before.available_mb);
    }
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    report
}

// ---------------------------------------------------------------------------
// 静默清理：后台线程按阈值轮询
// ---------------------------------------------------------------------------

static SILENT_ENABLED: AtomicBool = AtomicBool::new(false);
static SILENT_THRESHOLD_PERCENT: AtomicU8 = AtomicU8::new(DEFAULT_THRESHOLD_PERCENT);
static SILENT_LOOP_STARTED: AtomicBool = AtomicBool::new(false);

/// 把设置里的开关与阈值送进后台线程（改设置后立即生效，不用重启启动器）。
pub fn set_silent_cleanup(enabled: bool, threshold_percent: u8) {
    SILENT_ENABLED.store(enabled, Ordering::Relaxed);
    SILENT_THRESHOLD_PERCENT.store(normalize_threshold(threshold_percent), Ordering::Relaxed);
}

/// 当前生效的配置 `(enabled, threshold_percent)`，给日志与调试用。
pub fn silent_cleanup_config() -> (bool, u8) {
    (
        SILENT_ENABLED.load(Ordering::Relaxed),
        SILENT_THRESHOLD_PERCENT.load(Ordering::Relaxed),
    )
}

/// 启动后台轮询线程。重复调用只会启动一次。
///
/// 触发条件：**系统整体内存占用 ≥ 用户阈值**，且距上次清理已超过 `CLEANUP_COOLDOWN`。
/// 每次清理后向前端发一个 `memory-cleaned` 事件，界面可以据此刷新占用率/给个提示。
pub fn start_silent_loop(app: tauri::AppHandle) {
    if SILENT_LOOP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::spawn(move || {
        use tauri::Emitter;

        let mut last_cleanup: Option<Instant> = None;
        loop {
            thread::sleep(POLL_INTERVAL);

            if !SILENT_ENABLED.load(Ordering::Relaxed) {
                continue;
            }
            let Some(status) = status() else {
                continue;
            };
            let (_, threshold) = silent_cleanup_config();
            if status.used_percent < threshold {
                continue;
            }
            if last_cleanup.is_some_and(|instant| instant.elapsed() < CLEANUP_COOLDOWN) {
                continue;
            }

            let report = clean_memory();
            last_cleanup = Some(Instant::now());
            eprintln!(
                "静默内存清理：占用 {}% ≥ 阈值 {}%，释放 {} MB（清理 {} 个进程，跳过 {} 个，耗时 {} ms）",
                status.used_percent,
                threshold,
                report.freed_mb,
                report.trimmed,
                report.skipped,
                report.elapsed_ms
            );
            let _ = app.emit("memory-cleaned", report);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_clamped_and_zero_falls_back_to_default() {
        assert_eq!(normalize_threshold(0), DEFAULT_THRESHOLD_PERCENT);
        assert_eq!(normalize_threshold(1), MIN_THRESHOLD_PERCENT);
        assert_eq!(normalize_threshold(100), MAX_THRESHOLD_PERCENT);
        assert_eq!(normalize_threshold(85), 85);
    }

    #[test]
    fn clean_memory_reports_a_plausible_shape() {
        let before = status();
        let report = clean_memory();
        // `cargo test -- --nocapture` 时能看到真实数字，方便判断清理是否真的有效。
        println!("清理报告: {report:?}（清理前 {before:?}）");
        // 清理本身不能报错、也不能把数字搞成"负释放"。
        assert!(report.elapsed_ms < 60_000);
        assert!(report.trimmed + report.skipped + report.failed > 0 || before.is_none());
        if let (Some(before), Some(after)) = (before, status()) {
            assert!(report.used_percent_before <= 100);
            assert!(report.used_percent_after <= 100);
            assert!(before.total_mb > 0 && after.total_mb > 0);
        }
    }

    #[test]
    fn silent_config_round_trips() {
        set_silent_cleanup(true, 75);
        assert_eq!(silent_cleanup_config(), (true, 75));
        set_silent_cleanup(false, 0);
        assert_eq!(silent_cleanup_config(), (false, DEFAULT_THRESHOLD_PERCENT));
    }
}
