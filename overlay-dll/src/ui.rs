//! 游戏窗口**内部**的加载界面：可自定义的背景色 + 左下角最新 6 行控制台输出 + 底部居中进度条。
//!
//! 两个阶段都用同一个 GDI 渲染出来的画面（DIB）：
//! * 游戏还没开始换帧：直接 `BitBlt` 到游戏窗口 DC（窗口内容归 GDI 管）；
//! * 游戏开始换帧后：把同一张 DIB 上传成 GL 纹理，在 `wglSwapBuffers` 里画满屏四边形，
//!   这样界面始终在游戏窗口内部，不会去遮挡别的窗口。
//!
//! 状态（阶段文字 / 进度 / 控制台行）由启动器写入 `%TEMP%\pz_overlay_status.txt`。

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
    DeleteObject, DrawTextW, FillRect, FrameRect, GetDC, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, RedrawWindow, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DIB_RGB_COLORS, RDW_INVALIDATE, RDW_UPDATENOW,
    DEFAULT_CHARSET, DT_CENTER, DT_END_ELLIPSIS, DT_VCENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
    FF_DONTCARE, FW_BOLD, FW_NORMAL, HGDIOBJ, OUT_DEFAULT_PRECIS, SRCCOPY, TRANSPARENT,
    CreatePen, Polyline, PS_SOLID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClientRect, GetForegroundWindow, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::GetCurrentProcess;

/// 启动器写入的状态文件名（位于 `%TEMP%`）。
pub const STATUS_FILE: &str = "pz_overlay_status.txt";
/// 覆盖层默认背景色（纯红）——启动器没配置自定义颜色时使用。
pub const DEFAULT_BACKGROUND: (u8, u8, u8) = (255, 0, 0);
/// 界面显示的控制台行数。
const MAX_LINES: usize = 6;
/// 加载界面刷新间隔：40ms ≈ 25fps（换帧前的 GDI 重绘 / 换帧后的纹理更新都以它为准）。
const REFRESH_INTERVAL: Duration = Duration::from_millis(40);
/// 无边框且未前台时，最多用这个间隔主动触发一次窗口重绘。
const BACKGROUND_REDRAW_INTERVAL: Duration = Duration::from_millis(200);
/// DLL 可能随 JVM 极速启动提前加载，游戏窗口晚几秒出现是正常的；
/// 超过这个时长仍找不到窗口才记日志，避免把正常等待当成故障。
const WINDOW_FIND_WARN_DELAY: Duration = Duration::from_secs(15);

/// GL 纹理版本号（内容变化时自增），供 hook 判断是否需要重新上传。
pub static IMAGE_VERSION: AtomicU64 = AtomicU64::new(0);
/// 无边框窗口启动时，尝试把游戏窗口带到前台；最多尝试三次。
static WINDOW_ACTIVATION_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
/// 上一次后台重绘触发时间（毫秒，基于 UNIX_EPOCH）。
static LAST_BACKGROUND_REDRAW_MS: AtomicU64 = AtomicU64::new(0);
/// 是否已成功找到过游戏窗口。
static WINDOW_FOUND: AtomicBool = AtomicBool::new(false);
/// 「迟迟找不到游戏窗口」只记一次日志的打点。
static WINDOW_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「DIB 创建失败」只记一次日志的打点。
static DIB_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「界面绘制前置条件不满足」只记一次日志的打点。
static RENDER_PRECHECK_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「界面状态锁取不到」只记一次日志的打点（红色无字时的重要线索）。
static UI_LOCK_UNAVAILABLE_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「界面状态互斥锁中毒」只记一次日志的打点。
static MUTEX_POISON_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「客户区拿不到 / 为 0×0」只记一次日志的打点（两个失败点共用，都是画不出界面的同因）。
static CLIENT_RECT_PROBLEM_LOGGED: AtomicBool = AtomicBool::new(false);
/// 「加载界面已生成」只记一次日志的打点。
static IMAGE_READY_LOGGED: AtomicBool = AtomicBool::new(false);
/// UI 线程心跳计数（排查"界面线程是否还活着 / DIB 是否还在"）。
static HEARTBEAT_TICKS: AtomicU32 = AtomicU32::new(0);
/// 累计换帧次数（诊断用：界面没画出来时区分"图没生成"还是"游戏不再换帧"）。
static SWAP_COUNT: AtomicU64 = AtomicU64::new(0);

/// hook 每帧调用：只加一个原子计数。
pub fn note_swap() {
    SWAP_COUNT.fetch_add(1, Ordering::Relaxed);
}
/// 游戏是否已经调用过一次 `wglSwapBuffers`：换帧开始后 GL 覆盖会随每一帧出现，
/// GDI 直接贴窗口 DC 的路径就该退场——否则它只会在低帧率/呈现间隙里"闪"出界面。
pub static SWAP_SEEN: AtomicBool = AtomicBool::new(false);

/// 由 hook 线程在第一次换帧时置位。
pub fn mark_swap_seen() {
    SWAP_SEEN.store(true, Ordering::Release);
}

pub fn swap_seen() -> bool {
    SWAP_SEEN.load(Ordering::Acquire)
}

struct UiState {
    phase: String,
    progress: f32,
    lines: VecDeque<String>,
    version: u64,
    // DIB + 内存 DC
    memory_dc: isize,
    bitmap: isize,
    previous_bitmap: isize,
    bits: isize,
    width: i32,
    height: i32,
    /// 供 GL 上传用的 RGBA 数据（DIB 是 BGRA，需要换序）。
    rgba: Vec<u8>,
    running: bool,
    /// 窗口 DPI 缩放（1.0 = 96dpi）。字体与布局都按 DIP 定义再乘这个系数。
    scale: f32,
    /// 是否已经拿到"真实进度"（启动器给了 advice 分母之后才为 true）。
    has_progress: bool,
    /// 当前字体是按哪个 scale 创建的（变化时重建）。
    font_scale: f32,
    body_font: isize,
    logo_big: isize,
    logo_small: isize,
    /// 背景色（RGB）。默认纯红；启动器可通过状态文件的 `bgcolor=#RRGGBB` 覆盖。
    background: (u8, u8, u8),
}

// SAFETY: 上面保存的都是 GDI 句柄/内存地址，只在 UI 线程与渲染线程里受锁保护地使用。
unsafe impl Send for UiState {}

static UI: OnceLock<Mutex<UiState>> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);
/// DLL 加载时刻：比它更早的状态文件属于上一局，必须忽略（否则会显示上次的 89%/100%）。
static STATUS_EPOCH: OnceLock<SystemTime> = OnceLock::new();

fn state() -> &'static Mutex<UiState> {
    UI.get_or_init(|| {
        Mutex::new(UiState {
            phase: "准备中".to_string(),
            progress: 0.0,
            lines: VecDeque::new(),
            version: 0,
            memory_dc: 0,
            bitmap: 0,
            previous_bitmap: 0,
            bits: 0,
            width: 0,
            height: 0,
            rgba: Vec::new(),
            running: true,
            scale: 1.0,
            has_progress: false,
            font_scale: 0.0,
            body_font: 0,
            logo_big: 0,
            logo_small: 0,
            background: DEFAULT_BACKGROUND,
        })
    })
}

/// 启动窗口内加载界面线程（幂等）。
pub fn start(game_pid: u32) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("pz-overlay-ui".to_string())
        .spawn(move || {
            // 界面线程一旦死亡，GL 阶段就只剩背景色、没有任何界面内容，
            // 因此 panic 必须落日志，否则"红屏无字"无从排查。
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(game_pid)));
            if let Err(panic) = result {
                crate::log_ui(&format!("界面线程 panic 退出: {}", panic_message(&panic)));
            }
        });
    if spawned.is_err() {
        STARTED.store(false, Ordering::SeqCst);
        crate::log_ui("创建界面线程失败，覆盖层将只有背景色、没有任何界面内容。");
    }
}

/// 从 `catch_unwind` 的载荷里取出可读的 panic 信息。
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = panic.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text.clone()
    } else {
        "未知 panic".to_string()
    }
}

/// 结束界面（覆盖层停止时调用）。
pub fn stop() {
    if let Ok(mut ui) = state().lock() {
        ui.running = false;
    }
}

fn run(game_pid: u32) {
    let font = unsafe {
        CreateFontW(
            -16,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Consolas"),
        )
    };
    // LOGO 用的大号粗体
    let logo_big = unsafe {
        CreateFontW(
            -36,
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    };
    let logo_small = unsafe {
        CreateFontW(
            -20,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    };

    let run_start = Instant::now();
    loop {
        // 锁中毒意味着有线程在持锁时 panic 了：原实现静默退出循环，
        // 界面从此消失且无任何线索 —— 这里至少要把原因落进日志。
        let running = match state().lock() {
            Ok(ui) => ui.running,
            Err(_) => {
                if !MUTEX_POISON_LOGGED.swap(true, Ordering::Relaxed) {
                    crate::log_ui("界面状态互斥锁已中毒，界面线程退出（GL 侧此后只剩背景色）。");
                }
                break;
            }
        };
        if !running {
            break;
        }

        poll_status_file();

        // DIB 必须无条件渲染（GL 纹理依赖它）。BitBlt 也只写目标游戏窗口自己的 DC，
        // 不需要等待窗口进入前台；无窗口模式启动时否则会一直到切回焦点后才显示。
        if let Some(window) = find_game_window(game_pid) {
            if !WINDOW_FOUND.swap(true, Ordering::Relaxed) {
                let mut client = RECT::default();
                let size = if unsafe { GetClientRect(window, &mut client) }.is_ok() {
                    format!("{}x{}", client.right - client.left, client.bottom - client.top)
                } else {
                    "未知".to_string()
                };
                crate::log_ui(&format!("已找到游戏窗口（客户区 {size}），开始绘制加载界面。"));
            }
            if let Ok(mut ui) = state().lock() {
                paint_to_window(&mut ui, window, font, logo_big, logo_small);
                // 游戏开始换帧后，GL 纹理会随每一帧覆盖全屏；
                // BitBlt 只会卡在呈现间隙里闪现，此时停用它。
                if !swap_seen() {
                    blit_to_window(&ui, window);
                }
                // 埋点：界面图是否真的生成（红屏无字时这一行决定排查方向）。
                if ui.width > 0 && ui.height > 0 && !ui.rgba.is_empty() {
                    IMAGE_AVAILABLE.store(true, Ordering::Release);
                    if !IMAGE_READY_LOGGED.swap(true, Ordering::Relaxed) {
                        crate::log_ui(&format!(
                            "加载界面已生成：DIB={}x{} rgba={}B",
                            ui.width,
                            ui.height,
                            ui.rgba.len()
                        ));
                    }
                }
            } else if !UI_LOCK_UNAVAILABLE_LOGGED.swap(true, Ordering::Relaxed) {
                crate::log_ui("界面状态锁取不到（已中毒），界面无法绘制（只记一次）。");
            }
            // 心跳：约每 10 秒一次，最多 20 条，用来判断 UI 线程有没有卡死。
            let tick = HEARTBEAT_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
            if tick % 250 == 0 && tick / 250 <= 20 {
                let (width, height, rgba_len, has_dc, has_bits) = image_debug_state();
                crate::log_ui(&format!(
                    "UI 线程心跳 #{tick}：DIB={width}x{height} rgba={rgba_len}B dc={has_dc} bits={has_bits} 换帧={}",
                    SWAP_COUNT.load(Ordering::Relaxed)
                ));
            }
            ensure_window_active_or_redraw(window);
        } else if !WINDOW_FOUND.load(Ordering::Acquire) {
            let elapsed = run_start.elapsed().as_secs_f64();
            if elapsed >= WINDOW_FIND_WARN_DELAY.as_secs_f64()
                && !WINDOW_MISSING_LOGGED.swap(true, Ordering::Relaxed)
            {
                crate::log_ui(&format!(
                    "启动 {elapsed:.0}s 后仍未找到游戏进程的可见窗口，GL 覆盖层将没有界面内容（只有背景色）。"
                ));
            }
        }

        std::thread::sleep(REFRESH_INTERVAL);
    }

    if let Ok(mut ui) = state().lock() {
        release_dib(&mut ui);
    }
    let _ = unsafe { DeleteObject(HGDIOBJ(font.0)) };
    let _ = unsafe { DeleteObject(HGDIOBJ(logo_big.0)) };
    let _ = unsafe { DeleteObject(HGDIOBJ(logo_small.0)) };
}

/// 读取启动器写的状态文件并更新界面状态。
fn poll_status_file() {
    let path = std::env::temp_dir().join(STATUS_FILE);
    let epoch = *STATUS_EPOCH.get_or_init(SystemTime::now);
    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.modified().map(|time| time < epoch).unwrap_or(false) {
            return;
        }
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
        return;
    };

    let mut phase = String::new();
    let mut progress = 0.0f32;
    let mut has_progress = false;
    let mut lines: VecDeque<String> = VecDeque::new();
    // 启动器下发的自定义背景色；格式不对就保持当前颜色。
    let mut background: Option<(u8, u8, u8)> = None;
    for line in content.lines() {
        if let Some(value) = line.strip_prefix("progress=") {
            progress = value.trim().parse().unwrap_or(progress);
            has_progress = true;
        } else if let Some(value) = line.strip_prefix("memmax=") {
            // 启动器给出的 JVM 上限（来自启动器设置里的内存限制），优先于命令行解析。
            if let Ok(limit) = value.trim().parse::<u64>() {
                if limit > 0 {
                    MEMORY_LIMIT_MB.set(limit).ok();
                }
            }
        } else if let Some(value) = line.strip_prefix("bgcolor=") {
            background = parse_hex_color(value);
        } else if let Some(value) = line.strip_prefix("phase=") {
            phase = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("line=") {
            if lines.len() < MAX_LINES {
                lines.push_back(value.trim().to_string());
            }
        }
    }
    if phase.is_empty() && lines.is_empty() && background.is_none() {
        return;
    }

    if let Ok(mut ui) = state().lock() {
        let background_changed = background.is_some_and(|rgb| rgb != ui.background);
        if ui.progress != progress || ui.phase != phase || ui.lines != lines || background_changed {
            if let Some(rgb) = background {
                ui.background = rgb;
            }
            ui.progress = progress;
            ui.has_progress = has_progress;
            ui.phase = phase;
            ui.lines = lines;
            ui.version += 1;
            IMAGE_VERSION.store(ui.version, Ordering::Release);
        }
    }
}

/// 当前背景色（RGB）——GL 侧 `glClear` 要和 DIB 里的底色一致。
pub fn background_rgb() -> (u8, u8, u8) {
    state()
        .lock()
        .map(|ui| ui.background)
        .unwrap_or(DEFAULT_BACKGROUND)
}

/// 解析 `#RGB` / `#RRGGBB`（`#` 可省）。格式不对返回 None，调用方保留旧颜色。
fn parse_hex_color(value: &str) -> Option<(u8, u8, u8)> {
    let hex = value.trim().trim_start_matches('#');
    let digits: Vec<u8> = hex
        .chars()
        .map(|ch| ch.to_digit(16).map(|digit| digit as u8))
        .collect::<Option<Vec<u8>>>()?;
    match digits.len() {
        3 => Some((
            digits[0] * 17,
            digits[1] * 17,
            digits[2] * 17,
        )),
        6 => Some((
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        )),
        _ => None,
    }
}

/// GDI 的 COLORREF 是 0x00BBGGRR（注意与常见 RGB 顺序相反）。
fn colorref(rgb: (u8, u8, u8)) -> COLORREF {
    let (red, green, blue) = rgb;
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}

/// 按背景亮度选前景色：深底用白字、浅底用近黑字，避免用户选浅色后文字看不见。
/// 阈值用 Rec. 601 加权亮度（0–255）。
fn ink_for(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (red, green, blue) = rgb;
    let luminance =
        0.299 * red as f32 + 0.587 * green as f32 + 0.114 * blue as f32;
    if luminance > 150.0 {
        (24, 24, 28)
    } else {
        (255, 255, 255)
    }
}

/// 进度条底槽：在背景色上压暗，保持与背景同一色相。
fn track_for(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (red, green, blue) = rgb;
    (
        (red as f32 * 0.55) as u8,
        (green as f32 * 0.55) as u8,
        (blue as f32 * 0.55) as u8,
    )
}

/// 把界面画到游戏窗口 DC 上（游戏还没换帧时使用）。
fn paint_to_window(
    ui: &mut UiState,
    window: HWND,
    font: windows::Win32::Graphics::Gdi::HFONT,
    logo_big: windows::Win32::Graphics::Gdi::HFONT,
    logo_small: windows::Win32::Graphics::Gdi::HFONT,
) {
    let mut client = RECT::default();
    unsafe {
        if GetClientRect(window, &mut client).is_err() {
            if !CLIENT_RECT_PROBLEM_LOGGED.swap(true, Ordering::Relaxed) {
                crate::log_ui("获取游戏窗口客户区失败，加载界面无法绘制（只记一次）。");
            }
            return;
        }
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width <= 0 || height <= 0 {
        if !CLIENT_RECT_PROBLEM_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log_ui(&format!(
                "游戏窗口客户区为 {width}x{height}（最小化或辅助窗口？），加载界面暂不绘制（只记一次）。"
            ));
        }
        return;
    }
    if !ensure_dib(ui, width, height) {
        if !DIB_FAILURE_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log_ui(&format!(
                "创建界面 DIB 失败（{width}x{height}），GL 覆盖层将没有界面内容（只有背景色）。"
            ));
        }
        return;
    }
    // 字体与布局按 DIP 定义，乘上窗口 DPI 缩放（例如 150% → 1.5）。
    let scale = dpi_scale(window);
    if (scale - ui.font_scale).abs() > 0.01 {
        rebuild_fonts(ui, scale);
    }
    if !render_image(ui, font, logo_big, logo_small) {
        return;
    }

}

/// 只有"窗口在前台"时才把 DIB 贴到窗口 DC（换帧前的可见路径）。
///
/// **注意**：DIB 的渲染（`paint_to_window`）必须无条件执行——GL 纹理要靠它才有内容；
/// 只有这一步 BitBlt 才会影响别的窗口，所以要按前台判断。
fn blit_to_window(ui: &UiState, window: HWND) {
    if ui.memory_dc == 0 {
        return;
    }
    let mut client = RECT::default();
    unsafe {
        if GetClientRect(window, &mut client).is_err() {
            return;
        }
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width <= 0 || height <= 0 {
            return;
        }
        let window_dc = GetDC(Some(window));
        if window_dc.0.is_null() {
            return;
        }
        let memory_dc = windows::Win32::Graphics::Gdi::HDC(ui.memory_dc as *mut c_void);
        let _ = BitBlt(window_dc, 0, 0, width, height, Some(memory_dc), 0, 0, SRCCOPY);
        let _ = ReleaseDC(Some(window), window_dc);
    }
}

/// 无边框模式下，窗口可能已可见但尚未成为前台，Windows 会暂停部分合成/刷新。
///
/// 这里不参与“是否绘制”的判断，只做两层兜底：启动时尝试激活窗口三次；
/// 仍未前台时，每 200ms 主动请求一次重绘，等价于替用户完成一次切出再切回。
fn ensure_window_active_or_redraw(window: HWND) {
    let foreground = unsafe { GetForegroundWindow() == window };
    if foreground {
        return;
    }

    if WINDOW_ACTIVATION_ATTEMPTS.fetch_add(1, Ordering::AcqRel) < 3 {
        unsafe {
            let _ = BringWindowToTop(window);
            let _ = SetForegroundWindow(window);
        }
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or(0);
    let last = LAST_BACKGROUND_REDRAW_MS.load(Ordering::Acquire);
    if now.saturating_sub(last) < BACKGROUND_REDRAW_INTERVAL.as_millis() as u64 {
        return;
    }
    LAST_BACKGROUND_REDRAW_MS.store(now, Ordering::Release);

    unsafe {
        let _ = RedrawWindow(
            Some(window),
            None,
            None,
            RDW_INVALIDATE | RDW_UPDATENOW,
        );
    }
}

fn ensure_dib(ui: &mut UiState, width: i32, height: i32) -> bool {
    if ui.memory_dc != 0 && ui.width == width && ui.height == height {
        return true;
    }
    release_dib(ui);

    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // 自顶向下
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    unsafe {
        let memory_dc = CreateCompatibleDC(None);
        if memory_dc.0.is_null() {
            return false;
        }
        let mut bits: *mut c_void = std::ptr::null_mut();
        let Ok(bitmap) =
            CreateDIBSection(Some(memory_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(memory_dc);
            return false;
        };
        let previous = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
        ui.memory_dc = memory_dc.0 as isize;
        ui.bitmap = bitmap.0 as isize;
        ui.previous_bitmap = previous.0 as isize;
        ui.bits = bits as isize;
        ui.width = width;
        ui.height = height;
        ui.rgba = vec![0; (width as usize) * (height as usize) * 4];
    }
    true
}

fn release_dib(ui: &mut UiState) {
    unsafe {
        if ui.memory_dc != 0 {
            let memory_dc = windows::Win32::Graphics::Gdi::HDC(ui.memory_dc as *mut c_void);
            if ui.previous_bitmap != 0 {
                SelectObject(memory_dc, HGDIOBJ(ui.previous_bitmap as *mut c_void));
            }
            if ui.bitmap != 0 {
                let _ = DeleteObject(HGDIOBJ(ui.bitmap as *mut c_void));
            }
            let _ = DeleteDC(memory_dc);
        }
    }
    ui.memory_dc = 0;
    ui.bitmap = 0;
    ui.previous_bitmap = 0;
    ui.bits = 0;
    ui.width = 0;
    ui.height = 0;
}

/// 把背景色 + 控制台行 + 进度条画进 DIB，并同步 RGBA 缓冲（供 GL 上传）。
fn render_image(
    ui: &mut UiState,
    font: windows::Win32::Graphics::Gdi::HFONT,
    logo_big: windows::Win32::Graphics::Gdi::HFONT,
    logo_small: windows::Win32::Graphics::Gdi::HFONT,
) -> bool {
    if ui.memory_dc == 0 || ui.bits == 0 {
        if !RENDER_PRECHECK_LOGGED.swap(true, Ordering::Relaxed) {
            crate::log_ui(&format!(
                "界面绘制前置条件不满足：memory_dc={} bits={}（DIB 像素指针为空时界面永远画不出来）。",
                ui.memory_dc, ui.bits
            ));
        }
        return false;
    }
    let width = ui.width;
    let height = ui.height;
        // 所有间距都按 DIP 定义再乘 DPI 缩放：否则 18 DIP 的字会被固定高度的矩形裁掉。
        let scale = ui.scale;
        let px = |value: f32| (value * scale).round() as i32;
        let line_height = px(26.0); // 18 DIP 字 + 行距
        let bottom_margin = px(24.0);
        let bar_height = px(22.0);
        let bar_top = height - bottom_margin - bar_height;
        let label_height = px(30.0);
        let label_top = bar_top - label_height - px(8.0);
        let lines_bottom = label_top - px(12.0);
        let left_margin = px(24.0);
        let mem_top = px(18.0);
        let mem_height = px(18.0);
        let mem_label_top = mem_top + mem_height + px(6.0);
        let mem_label_height = px(30.0);
        let logo_title_height = px(56.0);
        let logo_center_top = height / 2 - px(70.0);
        let separator_top = logo_center_top + logo_title_height + px(6.0);
        let separator_height = px(4.0).max(2);
        let brand_top = separator_top + separator_height + px(6.0);
        let brand_height = px(36.0);
    let memory_dc = windows::Win32::Graphics::Gdi::HDC(ui.memory_dc as *mut c_void);

    unsafe {
        let background = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        // 背景由启动器设置（默认纯红）；文字与进度条颜色按背景亮度推导，
        // 用户选浅色背景时自动改用深色字，避免"白字看不见"。
        let background_rgb = ui.background;
        let ink = colorref(ink_for(background_rgb));
        let track_color = colorref(track_for(background_rgb));
        let background_brush = CreateSolidBrush(colorref(background_rgb));
        FillRect(memory_dc, &background, background_brush);
        let _ = DeleteObject(HGDIOBJ(background_brush.0));

        let _ = SetBkMode(memory_dc, TRANSPARENT);
        // 字体/字号由 rebuild_fonts 按 DPI 生成（正文 18 DIP），这里只用状态里缓存的句柄。
        let _ = (font, logo_big, logo_small);
        let previous_font = SelectObject(memory_dc, HGDIOBJ(ui.body_font as *mut c_void));

        // 顶部：JVM 内存占用条
        let (used_mb, max_mb) = jvm_memory();
        let mem_bar_width =
            (((width as f32) * 0.6).clamp(320.0, 900.0) as i32).min((width - 80).max(160));
        let mem_bar_left = (width - mem_bar_width) / 2;

        let mem_ratio = (used_mb as f32 / max_mb.max(1) as f32).clamp(0.0, 1.0);
        let mem_track = RECT {
            left: mem_bar_left,
            top: mem_top,
            right: mem_bar_left + mem_bar_width,
            bottom: mem_top + mem_height,
        };
        let mem_track_brush = CreateSolidBrush(track_color);
        FillRect(memory_dc, &mem_track, mem_track_brush);
        let _ = DeleteObject(HGDIOBJ(mem_track_brush.0));
        let mem_filled = (mem_bar_width as f32 * mem_ratio) as i32;
        if mem_filled > 0 {
            let mem_fill = RECT {
                left: mem_bar_left + 2,
                top: mem_top + 2,
                right: (mem_bar_left + mem_filled).max(mem_bar_left + 3),
                bottom: mem_top + mem_height - 2,
            };
            let mem_fill_brush = CreateSolidBrush(ink);
            FillRect(memory_dc, &mem_fill, mem_fill_brush);
            let _ = DeleteObject(HGDIOBJ(mem_fill_brush.0));
        }
        let mem_border = CreateSolidBrush(ink);
        FrameRect(memory_dc, &mem_track, mem_border);
        let _ = DeleteObject(HGDIOBJ(mem_border.0));
        let mem_label = format!("JVM Memory  {used_mb} / {max_mb} MB  {:.0}%", mem_ratio * 100.0);
        let mut mem_label_buffer: Vec<u16> = mem_label.encode_utf16().collect();
        let mut mem_label_rect = RECT {
            left: mem_bar_left,
            top: mem_label_top,
            right: mem_bar_left + mem_bar_width,
            bottom: mem_label_top + mem_label_height,
        };
        let _ = SetTextColor(memory_dc, ink);
        DrawTextW(
            memory_dc,
            &mut mem_label_buffer,
            &mut mem_label_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        // 正中心：Project Zomboid | PZManager LOGO
        let center_top = logo_center_top;
        let previous_logo = SelectObject(memory_dc, HGDIOBJ(ui.logo_big as *mut c_void));
        let mut title_buffer: Vec<u16> = "Project Zomboid".encode_utf16().collect();
        let mut title_rect = RECT {
            left: 0,
            top: center_top,
            right: width,
            bottom: center_top + logo_title_height,
        };
        let _ = SetTextColor(memory_dc, ink);
        DrawTextW(
            memory_dc,
            &mut title_buffer,
            &mut title_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let separator_width = ((mem_bar_width as f32) * 0.5) as i32;
        let separator_left = (width - separator_width) / 2;
        let separator = RECT {
            left: separator_left,
            top: separator_top,
            right: separator_left + separator_width,
            bottom: separator_top + separator_height,
        };
        let separator_brush = CreateSolidBrush(ink);
        FillRect(memory_dc, &separator, separator_brush);
        let _ = DeleteObject(HGDIOBJ(separator_brush.0));
        SelectObject(memory_dc, HGDIOBJ(ui.logo_small as *mut c_void));
        let mut brand_buffer: Vec<u16> = "PZManager".encode_utf16().collect();
        let mut brand_rect = RECT {
            left: 0,
            top: brand_top,
            right: width,
            bottom: brand_top + brand_height,
        };
        DrawTextW(
            memory_dc,
            &mut brand_buffer,
            &mut brand_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        // 加载动画：空心方框，写死的 30 帧角度表（0.5s 一周期 @60fps：前 18 帧缓动转 90°，其余保持）。
        // 它和文字/进度条走同一条路：先画进 DIB，再由 GL 上传成纹理——
        // 因此这里每渲染一帧就把版本号 +1，强制 GL 侧每帧重新上传（否则纹理不动）。
        const ANGLE_STEPS: [f32; 30] = [
            0.00, 0.06, 0.49, 1.67, 3.95, 7.72, 13.33, 21.17, 31.61, 45.00, 58.39, 68.83,
            76.67, 82.28, 86.05, 88.33, 89.51, 89.94, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
            90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
        ];
        // 相位用墙钟（保证任何帧率下都是精确的 0.5s 一周期），轨迹用上面写死的表。
        let animation_now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or(0);
        const CYCLE_MS: u64 = 500;
        // 角度只保留 0..4 个周期（正方形每 90° 视觉等价，取模可避免长时间运行后
        // f32 数值过大、写死表里的小增量被精度吃掉而"看起来不动"）。
        let cycle_index = ((animation_now_ms / CYCLE_MS) % 4) as f32;
        let step_index = (((animation_now_ms % CYCLE_MS) * 60) / 1000) as usize;
        let angle_degrees =
            90.0 * cycle_index + ANGLE_STEPS[step_index.min(ANGLE_STEPS.len() - 1)];
        let animation_size = px(22.0).max(10) as f32;
        let animation_cx = width as f32 / 2.0;
        let animation_cy = (brand_top + brand_height + px(34.0)) as f32;
        let animation_corners =
            rotated_square(animation_cx, animation_cy, animation_size, angle_degrees);
        let thickness = ((2.0 * ui.scale).round() as i32).max(1);
        let pen = CreatePen(PS_SOLID, thickness, ink);
        if !pen.0.is_null() {
            let previous_pen = SelectObject(memory_dc, HGDIOBJ(pen.0));
            let outline = [
                point_of(animation_corners[0]),
                point_of(animation_corners[1]),
                point_of(animation_corners[2]),
                point_of(animation_corners[3]),
                point_of(animation_corners[0]),
            ];
            let _ = Polyline(memory_dc, &outline);
            if !previous_pen.is_invalid() {
                SelectObject(memory_dc, previous_pen);
            }
            let _ = DeleteObject(HGDIOBJ(pen.0));
        }
        // 顶点小方块，强化"被画出来"的观感
        let dot_radius = ((animation_size * 0.12).round() as i32).max(1);
        let dot_brush = CreateSolidBrush(ink);
        for corner in animation_corners {
            let (x, y) = (corner.0.round() as i32, corner.1.round() as i32);
            let dot = RECT {
                left: x - dot_radius,
                top: y - dot_radius,
                right: x + dot_radius,
                bottom: y + dot_radius,
            };
            FillRect(memory_dc, &dot, dot_brush);
        }
        let _ = DeleteObject(HGDIOBJ(dot_brush.0));
        // 关键：每帧都让 GL 侧重新上传纹理（25fps），动画才会动。
        ui.version += 1;
        IMAGE_VERSION.store(ui.version, Ordering::Release);
        if !previous_logo.is_invalid() {
            SelectObject(memory_dc, previous_logo);
        }


        let mut y = lines_bottom - (ui.lines.len() as i32) * line_height;
        for line in ui.lines.clone() {
            let mut buffer: Vec<u16> = line.encode_utf16().collect();
            let mut rect = RECT {
                left: left_margin,
                top: y,
                right: (width - left_margin).max(48),
                bottom: y + line_height,
            };
            let _ = SetTextColor(memory_dc, ink);
            DrawTextW(
                memory_dc,
                &mut buffer,
                &mut rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            y += line_height;
        }

        let max_bar_width = (width - 80).max(160);
        let bar_width = (((width as f32) * 0.55).clamp(320.0, 900.0) as i32).min(max_bar_width);
        let bar_left = (width - bar_width) / 2;

        // 没有真实进度时只显示阶段文字，不显示百分比（避免"假进度"）。
        let label = if ui.has_progress {
            let percent = progress_percent(ui.progress);
            format!("{}  {percent}%", ui.phase)
        } else {
            ui.phase.clone()
        };
        let mut label_buffer: Vec<u16> = label.encode_utf16().collect();
        let mut label_rect = RECT {
            left: bar_left,
            top: label_top,
            right: bar_left + bar_width,
            bottom: label_top + label_height,
        };
        let _ = SetTextColor(memory_dc, ink);
        DrawTextW(
            memory_dc,
            &mut label_buffer,
            &mut label_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );

        let track = RECT {
            left: bar_left,
            top: bar_top,
            right: bar_left + bar_width,
            bottom: bar_top + bar_height,
        };
        let track_brush = CreateSolidBrush(track_color);
        FillRect(memory_dc, &track, track_brush);
        let _ = DeleteObject(HGDIOBJ(track_brush.0));

        let filled = if ui.has_progress {
            (bar_width as f32 * ui.progress.clamp(0.0, 1.0)) as i32
        } else {
            0
        };
        if filled > 0 {
            let fill = RECT {
                left: bar_left + 2,
                top: bar_top + 2,
                right: (bar_left + filled).max(bar_left + 3),
                bottom: bar_top + bar_height - 2,
            };
            let fill_brush = CreateSolidBrush(ink);
            FillRect(memory_dc, &fill, fill_brush);
            let _ = DeleteObject(HGDIOBJ(fill_brush.0));
        }
        let border_brush = CreateSolidBrush(ink);
        FrameRect(memory_dc, &track, border_brush);
        let _ = DeleteObject(HGDIOBJ(border_brush.0));

        if !previous_font.is_invalid() {
            SelectObject(memory_dc, previous_font);
        }

        // BGRA -> RGBA，供 GL 上传。
        let source = std::slice::from_raw_parts(ui.bits as *const u8, ui.rgba.len());
        for (index, chunk) in ui.rgba.chunks_exact_mut(4).enumerate() {
            let offset = index * 4;
            chunk[0] = source[offset + 2];
            chunk[1] = source[offset + 1];
            chunk[2] = source[offset];
            chunk[3] = 0xFF;
        }
        // DIB 里的 alpha 位不参与 GDI 绘制，这里统一置 1 以便 GL 直接叠加。
        let pixels = std::slice::from_raw_parts_mut(ui.bits as *mut u32, (width * height) as usize);
        for pixel in pixels.iter_mut() {
            *pixel |= 0xFF00_0000;
        }
    }
    true
}

/// 给 GL 用的界面纹理数据（RGBA，自顶向下）。
pub fn image() -> Option<(u64, i32, i32, Vec<u8>)> {
    let ui = state().lock().ok()?;
    if ui.width <= 0 || ui.height <= 0 || ui.rgba.is_empty() {
        return None;
    }
    Some((ui.version, ui.width, ui.height, ui.rgba.clone()))
}

/// 是否已经成功生成过界面图（GL 侧不拿锁就能判断，用于"界面产不出来就别挡游戏"的兜底）。
static IMAGE_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// GL 侧每帧查询：界面图是否已经就绪。
pub fn image_available() -> bool {
    IMAGE_AVAILABLE.load(Ordering::Acquire)
}

/// 诊断用：`image()` 为空时把内部状态快照写进日志（界面缺失时唯一的线索）。
pub fn image_debug_state() -> (i32, i32, usize, bool, bool) {
    match state().lock() {
        Ok(ui) => (
            ui.width,
            ui.height,
            ui.rgba.len(),
            ui.memory_dc != 0,
            ui.bits != 0,
        ),
        // 锁中毒本身就是关键线索。
        Err(_) => (-1, -1, 0, false, false),
    }
}

/// 找到目标进程的可见主窗口（**客户区最大**的那个）。
///
/// 同一进程可能有多个"可见且带标题"的顶层窗口（辅助窗口等），
/// 先枚举到的未必是真正用于渲染的那个；曾出现"找到了窗口但客户区是 0×0、
/// 界面永远画不出来"的机器案例——最小化窗口的客户区也是 0×0。
/// 因此跳过 0×0，并在全部枚举完后取客户区面积最大者。
fn find_game_window(game_pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        best_area: i64,
        window: Option<HWND>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut window_pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
        if window_pid == search.pid
            && unsafe { IsWindowVisible(hwnd).as_bool() }
            && unsafe { GetWindowTextLengthW(hwnd) } > 0
        {
            let mut client = RECT::default();
            if unsafe { GetClientRect(hwnd, &mut client) }.is_ok() {
                let area = (client.right - client.left) as i64
                    * (client.bottom - client.top) as i64;
                if area > search.best_area {
                    search.best_area = area;
                    search.window = Some(hwnd);
                }
            }
        }
        // 继续枚举完所有窗口再挑最大的，不在第一个命中处停下。
        windows::Win32::Foundation::TRUE
    }

    let mut search = Search {
        pid: game_pid,
        best_area: 0,
        window: None,
    };
    let _ = unsafe {
        EnumWindows(
            Some(callback),
            windows::Win32::Foundation::LPARAM(&mut search as *mut Search as isize),
        )
    };
    search.window
}

/// 窗口 DPI 缩放系数（1.0 = 96dpi，例如 150% → 1.5）。
fn dpi_scale(window: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(window) };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

fn progress_percent(progress: f32) -> i32 {
    (progress.clamp(0.0, 1.0) * 100.0).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::{ink_for, parse_hex_color, progress_percent, track_for};

    #[test]
    fn loading_progress_does_not_round_up_to_one_hundred() {
        assert_eq!(progress_percent(0.995), 99);
        assert_eq!(progress_percent(0.999), 99);
        assert_eq!(progress_percent(1.0), 100);
    }

    /// 启动器下发的是 CSS 颜色：接受 #RGB / #RRGGBB，宽容大小写与省略 #。
    #[test]
    fn parses_hex_background_colors() {
        assert_eq!(parse_hex_color("#FF0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("ff8800"), Some((255, 136, 0)));
        assert_eq!(parse_hex_color("  #0f0 "), Some((0, 255, 0)));
        assert_eq!(parse_hex_color("#123456"), Some((18, 52, 86)));
        // 格式不对时返回 None，覆盖层保留当前颜色（不会变成黑色）。
        assert_eq!(parse_hex_color("#GGGGGG"), None);
        assert_eq!(parse_hex_color("#12345"), None);
        assert_eq!(parse_hex_color(""), None);
    }

    /// 深色背景用白字、浅色背景用深色字；进度条底槽是同色相的压暗版本。
    #[test]
    fn picks_readable_ink_and_track_for_background() {
        assert_eq!(ink_for((255, 0, 0)), (255, 255, 255));
        assert_eq!(ink_for((0, 0, 0)), (255, 255, 255));
        assert_eq!(ink_for((255, 255, 255)), (24, 24, 28));
        assert_eq!(ink_for((255, 235, 59)), (24, 24, 28));
        assert_eq!(track_for((200, 100, 50)), (110, 55, 27));
    }
}

/// 给定中心/边长/角度（度），返回正方形四个角点（y 向下坐标系）。
fn point_of(corner: (f32, f32)) -> windows::Win32::Foundation::POINT {
    windows::Win32::Foundation::POINT {
        x: corner.0.round() as i32,
        y: corner.1.round() as i32,
    }
}

pub fn rotated_square(
    center_x: f32,
    center_y: f32,
    size: f32,
    angle_degrees: f32,
) -> [(f32, f32); 4] {
    let angle = angle_degrees.to_radians();
    let (sin, cos) = angle.sin_cos();
    let half = size / 2.0;
    let local = [
        (-half, -half),
        (half, -half),
        (half, half),
        (-half, half),
    ];
    local.map(|(x, y)| (center_x + x * cos - y * sin, center_y + x * sin + y * cos))
}

/// 按 DPI 缩放重建字体（字体大小以 DIP 定义）。
fn rebuild_fonts(ui: &mut UiState, scale: f32) {
    unsafe {
        for handle in [ui.body_font, ui.logo_big, ui.logo_small] {
            if handle != 0 {
                let _ = DeleteObject(HGDIOBJ(handle as *mut c_void));
            }
        }
        let dip = |value: f32| -((value * scale).round().max(6.0) as i32);
        ui.body_font = CreateFontW(
            dip(18.0), // 正文：18 DIP
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Consolas"),
        )
        .0 as isize;
        ui.logo_big = CreateFontW(
            dip(36.0),
            0,
            0,
            0,
            FW_BOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
        .0 as isize;
        ui.logo_small = CreateFontW(
            dip(20.0),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
        .0 as isize;
    }
    ui.scale = scale;
    ui.font_scale = scale;
}

/// 进程当前内存占用（MB）与 `-Xmx` 上限（MB）。
///
/// 说明：JVM 堆内部数值需要 JNI 才能读到，这里用进程内存计数器 + 启动参数里的 `-Xmx` 作为
/// "占用 / 上限"，由界面每 40ms 刷新一次，足以反映加载期间的内存曲线。
fn jvm_memory() -> (u64, u64) {
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let used_mb = unsafe {
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        match GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) {
            Ok(()) => (counters.WorkingSetSize as u64) / 1024 / 1024,
            Err(_) => 0,
        }
    };
    (used_mb.max(1), jvm_max_mb())
}

/// 从自身进程命令行解析 `-Xmx`（PZ 的 JSON / Steam 启动参数都会带上）。
fn jvm_max_mb() -> u64 {
    static MAX: OnceLock<u64> = OnceLock::new();
    // 启动器写进状态文件的 memmax= 优先。
    if let Some(limit) = MEMORY_LIMIT_MB.get() {
        return *limit;
    }
    *MAX.get_or_init(|| {
        for argument in std::env::args() {
            let Some(value) = argument.strip_prefix("-Xmx") else {
                continue;
            };
            let digits: String = value.chars().take_while(|character| character.is_ascii_digit()).collect();
            if let Ok(number) = digits.parse::<u64>() {
                let unit = value.chars().last().unwrap_or('m').to_ascii_lowercase();
                return match unit {
                    'g' => number * 1024,
                    'k' => (number / 1024).max(1),
                    _ => number,
                };
            }
        }
        4096
    })
}

/// 启动器提供的 JVM 上限（MB）。
static MEMORY_LIMIT_MB: OnceLock<u64> = OnceLock::new();
