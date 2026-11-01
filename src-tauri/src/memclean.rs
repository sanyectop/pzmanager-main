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
//!
//! 激进模式（设置 › 启动 › 激进内存清理）在常规工作集修剪之后再追加系统级步骤：
//! 清空全系统工作集（`MemoryEmptyWorkingSets`，覆盖逐进程打不开的进程）、
//! 刷新系统文件缓存（`SetSystemFileCacheSize(-1, -1, 0)`）、把已修改页刷回磁盘
//! （`MemoryFlushModifiedList`），最后把待机列表转成空闲页（`MemoryPurgeStandbyList`）。
//!
//! 「已提交」（commit charge）是各进程持有的提交配额，外部工具**无法**直接回收：
//! 把驻留页挤出工作集只会让它转成待机页，提交数值本身不会回落。清理真正能改变的是
//! 「已缓存」（待机列表 + 已修改页 + 系统文件缓存），界面文案不要暗示能清「已提交」。

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
/// 激进模式下两次静默清理之间的最小间隔。
///
/// 清待机列表 / 系统文件缓存会让磁盘缓存重新预热，按分钟级高频清反而拖慢整机；
/// 所以开了激进模式后拉长到 5 分钟，只在内存压力持续偏高时才重复清理。
const AGGRESSIVE_CLEANUP_COOLDOWN: Duration = Duration::from_secs(300);

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
    /// 这次是否跑了激进模式（系统层面的工作集 / 文件缓存 / 已修改页 / 待机列表）。
    pub aggressive: bool,
    /// 清空全系统工作集（`MemoryEmptyWorkingSets`）是否成功；
    /// 游戏运行时会主动跳过，见 [`CleanupReport::protected_running`]。
    pub emptied_working_sets: bool,
    /// 系统文件缓存（`SetSystemFileCacheSize(-1, -1, 0)`）是否成功刷新。
    pub flushed_file_cache: bool,
    /// 已修改页（Modified list）是否成功刷回磁盘。
    pub flushed_modified: bool,
    /// 待机列表（Standby list）是否成功转成空闲页。
    pub purged_standby: bool,
    /// 清理时检测到游戏（或同类保护进程）在运行，因此跳过了清空全系统工作集这一步。
    pub protected_running: bool,
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

/// 游戏（或被保护的同类进程）是否正在运行。
///
/// 用于「激进模式」的静默清理：待机列表是全系统共享的文件缓存，游戏跑着时清掉它，
/// 接下来读盘都要重新从硬盘拉，表现就是卡顿。手动按钮不受这个限制。
#[cfg(windows)]
fn protected_process_running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return false;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut running = false;
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while has_entry {
        let name_len = entry
            .szExeFile
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.szExeFile.len());
        let exe_name = String::from_utf16_lossy(&entry.szExeFile[..name_len]).to_ascii_lowercase();
        if PROTECTED_EXE_NAMES.contains(&exe_name.as_str()) {
            running = true;
            break;
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    running
}

#[cfg(not(windows))]
fn protected_process_running() -> bool {
    false
}

/// 激进模式的四步系统级清理：
///
/// 1. `NtSetSystemInformation(SystemMemoryListInformation, MemoryEmptyWorkingSets)`：
///    清空全系统工作集，覆盖逐进程 `OpenProcess` 打不开的进程。游戏在跑时跳过
///    （`protect_game`），免得把游戏的工作集压掉。
/// 2. `SetSystemFileCacheSize(-1, -1, 0)`：按官方文档的「刷新」语义，把系统文件缓存
///    的工作集交还给系统。
/// 3. `MemoryFlushModifiedList`：把已修改页写回页面文件，转成待机页。
/// 4. `MemoryPurgeStandbyList`：把待机页直接标记为空闲——任务管理器里看到的
///    「可用内存」变大主要来自这一步。
///
/// 1、3 需要 `SeProfileSingleProcessPrivilege`，2、4 需要 `SeIncreaseQuotaPrivilege`。
/// 这些特权只有管理员能拿到；拿不到时调用会返回 `STATUS_PRIVILEGE_NOT_HELD`，
/// 这里如实返回 false，由界面提示"需要管理员权限"。
#[derive(Default)]
#[cfg_attr(not(windows), allow(dead_code))]
struct SystemCleanupOutcome {
    emptied_working_sets: bool,
    flushed_file_cache: bool,
    flushed_modified: bool,
    purged_standby: bool,
}

#[cfg(windows)]
fn clean_system_memory(protect_game: bool) -> SystemCleanupOutcome {
    use std::ffi::c_void;

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_SUCCESS, HANDLE, LUID};
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // windows crate 没有暴露 NtSetSystemInformation，手动绑定（稳定的 NT API）。
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSetSystemInformation(
            system_information_class: u32,
            system_information: *mut c_void,
            system_information_length: u32,
        ) -> i32;
    }

    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 0x50;
    const MEMORY_EMPTY_WORKING_SETS: u32 = 2;
    const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;

    /// 给当前进程启用一个特权。非管理员会静默失败（返回 false）。
    fn enable_privilege(name: PCWSTR) -> bool {
        unsafe {
            let mut token = HANDLE::default();
            if OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .is_err()
            {
                return false;
            }
            let mut luid = LUID::default();
            if LookupPrivilegeValueW(None, name, &mut luid).is_err() {
                let _ = CloseHandle(token);
                return false;
            }
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            // AdjustTokenPrivileges 在"只分配了部分特权"时也返回成功，必须再看 GetLastError。
            let applied = AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None)
                .is_ok()
                && GetLastError() == ERROR_SUCCESS;
            let _ = CloseHandle(token);
            applied
        }
    }

    fn set_memory_list(command: u32) -> bool {
        let mut command = command;
        let status = unsafe {
            NtSetSystemInformation(
                SYSTEM_MEMORY_LIST_INFORMATION,
                (&mut command as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            )
        };
        // NTSTATUS：>= 0 为成功（失败是 0xC0xxxxxx，按 i32 是负数）。
        status >= 0
    }

    /// 刷新系统文件缓存的工作集。
    ///
    /// 官方文档（`SetSystemFileCacheSize`）：最小值与最大值都传 `(SIZE_T)-1`
    /// 就表示「刷新缓存」；需要 `SeIncreaseQuotaPrivilege`。
    fn flush_system_file_cache() -> bool {
        use windows::Win32::System::Memory::SetSystemFileCacheSize;

        unsafe { SetSystemFileCacheSize(usize::MAX, usize::MAX, 0).is_ok() }
    }

    enable_privilege(w!("SeProfileSingleProcessPrivilege"));
    enable_privilege(w!("SeIncreaseQuotaPrivilege"));
    SystemCleanupOutcome {
        emptied_working_sets: !protect_game && set_memory_list(MEMORY_EMPTY_WORKING_SETS),
        flushed_file_cache: flush_system_file_cache(),
        flushed_modified: set_memory_list(MEMORY_FLUSH_MODIFIED_LIST),
        purged_standby: set_memory_list(MEMORY_PURGE_STANDBY_LIST),
    }
}

#[cfg(not(windows))]
fn clean_system_memory(_protect_game: bool) -> SystemCleanupOutcome {
    SystemCleanupOutcome::default()
}

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

/// 当前启动器是否以管理员（提升后）权限运行。
///
/// 激进模式的系统级步骤全都要管理员特权；非管理员时只有逐进程工作集修剪生效，
/// 界面据此提示「以管理员身份重启」。
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use std::ffi::c_void;
    use std::mem::size_of;

    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// 以管理员身份重新启动启动器（`ShellExecuteW` 的 `runas` 动词，会弹 UAC）。
///
/// 用户取消 UAC 时返回错误；成功返回 `Ok(())`，调用方随后退出当前进程，
/// 由提升后的新实例接管。
///
/// 只在非开发环境里真正执行：`tauri dev` 的程序依赖开发服务器的 URL，拉起的第二个
/// 实例会因为 CLI 已退出、dev server 已关闭而白屏，所以开发模式下直接返回错误，
/// 让用户手动以管理员身份运行。
#[cfg(windows)]
pub fn restart_as_admin() -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if tauri::is_dev() {
        return Err("开发模式下无法自动提权重启，请手动关闭后以管理员身份运行启动器。".to_string());
    }
    let exe = std::env::current_exe().map_err(|error| format!("无法定位启动器路径：{error}"))?;
    let exe = HSTRING::from(exe.as_os_str().to_string_lossy().as_ref());
    let result = unsafe { ShellExecuteW(None, w!("runas"), &exe, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW 返回值 <= 32 表示失败，用户在 UAC 里点取消就走这里。
    if result.0 as isize <= 32 {
        return Err("没有拿到管理员授权，启动器不会重启。".to_string());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn restart_as_admin() -> Result<(), String> {
    Err("只有 Windows 支持以管理员身份重启启动器".to_string())
}

/// 立即清理一次：遍历进程、把工作集还给系统，并回报释放量。
///
/// 是否附带激进模式（系统工作集 / 文件缓存 / 已修改页 / 待机列表）由设置里的
/// 开关决定，见 [`set_aggressive_cleanup`]。
///
/// 这个函数是**同步阻塞**的（典型 100–300 ms，进程越多越久，激进模式再加几十毫秒），
/// 调用方要放到后台线程或异步命令里，别卡住 UI 线程。
pub fn clean_memory() -> CleanupReport {
    clean_memory_with(aggressive_cleanup())
}

/// 同 [`clean_memory`]，但由调用方显式决定这次要不要跑激进模式
/// （静默清理在游戏运行时要用常规模式）。
pub fn clean_memory_with(aggressive: bool) -> CleanupReport {
    let started = Instant::now();
    let before = status();
    let mut report = CleanupReport {
        used_percent_before: before.map_or(0, |value| value.used_percent),
        aggressive,
        ..Default::default()
    };

    trim_all_processes(&mut report);

    if aggressive {
        // 游戏在跑时清空全系统工作集会连游戏一起压（每一步都要缺页重读），
        // 所以只跳过这一步；手动触发的刷盘 / 清待机列表仍按用户意愿执行。
        let protect_game = protected_process_running();
        let outcome = clean_system_memory(protect_game);
        report.protected_running = protect_game;
        report.emptied_working_sets = outcome.emptied_working_sets;
        report.flushed_file_cache = outcome.flushed_file_cache;
        report.flushed_modified = outcome.flushed_modified;
        report.purged_standby = outcome.purged_standby;
    }

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
/// 激进模式开关（设置 › 启动 › 内存清理 › 激进模式）。
static AGGRESSIVE_ENABLED: AtomicBool = AtomicBool::new(false);

/// 接收「激进模式」开关（改设置后立即生效，不用重启启动器）。
pub fn set_aggressive_cleanup(enabled: bool) {
    AGGRESSIVE_ENABLED.store(enabled, Ordering::Relaxed);
}

/// 当前激进模式配置，给日志与调试用。
pub fn aggressive_cleanup() -> bool {
    AGGRESSIVE_ENABLED.load(Ordering::Relaxed)
}

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
/// 触发条件：**系统整体内存占用 ≥ 用户阈值**，且距上次清理已超过 `CLEANUP_COOLDOWN`
/// （开了激进模式时改用 `AGGRESSIVE_CLEANUP_COOLDOWN`）。
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
            // 激进模式清的是系统缓存，重复太勤会拖慢读盘，间隔拉长到 5 分钟。
            let cooldown = if aggressive_cleanup() {
                AGGRESSIVE_CLEANUP_COOLDOWN
            } else {
                CLEANUP_COOLDOWN
            };
            if last_cleanup.is_some_and(|instant| instant.elapsed() < cooldown) {
                continue;
            }

            // 游戏跑着时只做常规清理：待机列表是全系统的文件缓存，
            // 清掉之后游戏下一次读盘要重新从硬盘拉，表现就是卡顿。
            let aggressive = aggressive_cleanup() && !protected_process_running();
            let report = clean_memory_with(aggressive);
            last_cleanup = Some(Instant::now());
            eprintln!(
                "静默内存清理{}：占用 {}% ≥ 阈值 {}%，释放 {} MB（清理 {} 个进程，跳过 {} 个，耗时 {} ms）",
                if report.aggressive { "（激进模式）" } else { "" },
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

    #[test]
    fn aggressive_flag_round_trips() {
        set_aggressive_cleanup(true);
        assert!(aggressive_cleanup());
        set_aggressive_cleanup(false);
        assert!(!aggressive_cleanup());
    }

    #[test]
    fn regular_cleanup_reports_no_aggressive_steps() {
        // 常规模式不碰系统工作集 / 文件缓存 / 内存列表：结果位都必须是 false，
        // 免得界面误报"已清系统缓存"。
        let report = clean_memory_with(false);
        assert!(!report.aggressive);
        assert!(!report.emptied_working_sets);
        assert!(!report.flushed_file_cache);
        assert!(!report.flushed_modified);
        assert!(!report.purged_standby);
        assert!(!report.protected_running);
    }

    /// 真的跑一次激进清理。会清空全系统的待机列表（文件缓存），
    /// 所以默认 ignore，需要时手动 `cargo test --lib aggressive_cleanup_runs -- --ignored --nocapture`。
    #[test]
    #[ignore = "会清空本机待机列表缓存"]
    fn aggressive_cleanup_runs() {
        let report = clean_memory_with(true);
        println!("激进清理报告: {report:?}");
        assert!(report.aggressive);
        // 非管理员时系统级调用会被拒绝（返回 false），这是预期行为，不能 panic。
        println!(
            "系统级步骤：全系统工作集 {}，文件缓存刷新 {}，已修改页刷盘 {}，待机列表释放 {}（游戏运行中 {}）",
            report.emptied_working_sets,
            report.flushed_file_cache,
            report.flushed_modified,
            report.purged_standby,
            report.protected_running
        );
    }
}


