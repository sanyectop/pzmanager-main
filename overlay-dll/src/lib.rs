//! Project Zomboid 覆盖层验证 DLL（第一阶段）
//!
//! 验证链路：注入 → Hook OpenGL（`wglSwapBuffers`）→ 在游戏窗口内绘制纯红 → 5 秒后自动恢复。
//!
//! 本阶段刻意保持最小实现：
//! * 不读取游戏进度，不绘制任何 UI，不依赖 ZombieBuddy 或任何模组；
//! * 只用 OpenGL 1.1 固定管线调用（`opengl32.dll` 直接导出），不引入任何 GL 绑定 crate；
//! * Hook 由 `minhook` 完成，原始函数指针保存在 `AtomicU64`；
//! * 覆盖不再按固定 5 秒关闭：由启动器通过命名事件（`Local\PZManagerOverlayStop_<pid>`）控制，
//!   只有安全上限（默认 10 分钟）兜底，避免启动器异常时一直保持红色；
//! * 游戏窗口出现但还没开始换帧时，用 GDI 直接把窗口刷成红色（GL 的 glClear 此时无从下手），
//!   首次换帧后自动切换回 OpenGL 覆盖；
//! * 首次真正换帧时会触发 `Local\PZManagerOverlayStarted_<pid>`，让启动器知道红色已经出现，
//!   避免在"游戏窗口已出现但还没开始渲染"的阶段就误判为加载完成；
//! * 任何失败只写 `%TEMP%\overlay.log` 后静默退出，绝不影响游戏本体。

#[cfg(not(windows))]
compile_error!("pz_overlay 仅支持 Windows 目标，请使用 --target x86_64-pc-windows-msvc 构建。");

mod classfile;
mod fastlaunch;
mod ui;

#[cfg(windows)]
mod overlay {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use minhook::MinHook;
    use windows::core::{PCSTR, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HMODULE, WAIT_OBJECT_0};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE,
        TH32CS_SNAPMODULE32,
    };
    use windows::Win32::System::LibraryLoader::{
        DisableThreadLibraryCalls, GetModuleHandleA, GetProcAddress,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, WaitForSingleObject, INFINITE,
    };
    /// 覆盖最长持续时间（安全上限）。启动器正常时会在此之前发出停止信号。
    const OVERLAY_MAX_DURATION: Duration = Duration::from_secs(600);
    /// 启动器用来通知「ZombieBuddy 已加载完成」的命名事件前缀，实际名字会拼上游戏进程 PID。
    const STOP_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStop_";
    /// DLL 用来通知「已经画上第一帧、红色已出现」的命名事件前缀。
    const STARTED_EVENT_PREFIX: &str = "Local\\PZManagerOverlayStarted_";
    /// 等待 `opengl32.dll` 模块 / `wglSwapBuffers` 导出的最长时间。
    const WAIT_TIMEOUT: Duration = Duration::from_secs(60);
    /// 等待轮询间隔。
    const WAIT_INTERVAL: Duration = Duration::from_millis(100);
    /// 常见驱动里 `wglGetProcAddress` 可能返回 1/2/3/-1 这类非法地址，低于该值一律视为“不可用”。
    const MIN_VALID_PROC_ADDRESS: usize = 0x1_0000;

    // OpenGL 1.1 常量（opengl32.dll 只导出 GL 1.1 入口点）。
    const GL_ALL_ATTRIB_BITS: u32 = 0x000F_FFFF;
    const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
    const GL_SCISSOR_TEST: u32 = 0x0000_0C11;
    const GL_FRAMEBUFFER_BINDING: u32 = 0x0008_CA6;
    const GL_FRAMEBUFFER: u32 = 0x0008_D40;
    const GL_TEXTURE_2D: u32 = 0x0000_0DE1;
    const GL_TEXTURE_MIN_FILTER: u32 = 0x0000_2801;
    const GL_TEXTURE_MAG_FILTER: u32 = 0x0000_2800;
    const GL_LINEAR: u32 = 0x0000_2601;
    const GL_RGBA: u32 = 0x0000_1908;
    const GL_UNSIGNED_BYTE: u32 = 0x0000_1401;
    const GL_QUADS: u32 = 0x0000_0007;
    const GL_LINE_LOOP: u32 = 0x0000_0002;
    const GL_LINES: u32 = 0x0000_0001;
    const GL_PROJECTION: u32 = 0x0000_1701;
    const GL_MODELVIEW: u32 = 0x0000_1700;
    const GL_VIEWPORT: u32 = 0x0000_0BA2;
    const GL_UNPACK_ALIGNMENT: u32 = 0x0000_0CF5;
    /// 强制换帧的最小间隔。
    const FORCED_FRAME_INTERVAL: Duration = Duration::from_millis(200);

    // 地址缓存哨兵值。
    const PTR_UNRESOLVED: u64 = 0;
    const PTR_UNAVAILABLE: u64 = u64::MAX;

    /// 本 DLL 自身的模块句柄（DllMain 里记录，初始化线程里使用）。
    static SELF_MODULE: AtomicU64 = AtomicU64::new(0);
    /// 防止同一个模块被重复 `LoadLibrary` 时重复初始化。
    static INIT_STARTED: AtomicBool = AtomicBool::new(false);
    /// 防止停止事件和安全上限线程同时执行两遍卸载流程。
    static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
    /// Hook 是否已成功启用（仅用于日志与自检）。
    static HOOK_ENABLED: AtomicBool = AtomicBool::new(false);
    /// 是否正在绘制红色覆盖。由启动器的停止信号或安全上限置为 false。
    static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);
    /// 停止事件句柄（保持打开直到进程退出）。
    static STOP_EVENT: AtomicU64 = AtomicU64::new(0);
    /// 「已开始绘制」事件句柄（保持打开直到进程退出）。
    static STARTED_EVENT: AtomicU64 = AtomicU64::new(0);
    /// 第一次进入 Hook 回调时打点，避免刷日志。
    static FIRST_CALL_LOGGED: AtomicBool = AtomicBool::new(false);
    /// 动画首次绘制时打点（用于确认动画确实被执行）。
    static ANIMATION_LOGGED: AtomicBool = AtomicBool::new(false);
    /// `wglSwapBuffers` 的原始函数指针（trampoline），由 MinHook 创建时返回。
    static ORIGINAL_SWAP_BUFFERS: AtomicU64 = AtomicU64::new(0);
    /// `glBindFramebuffer` 地址缓存（GL 3.0 函数，opengl32.dll 不导出，只能运行时解析）。
    static BIND_FRAMEBUFFER: AtomicU64 = AtomicU64::new(PTR_UNRESOLVED);
    /// 界面纹理与版本（内容变化才重新上传）。
    static UI_TEXTURE: AtomicU64 = AtomicU64::new(0);
    static UI_TEXTURE_VERSION: AtomicU64 = AtomicU64::new(u64::MAX);
    /// 上一次强制换帧的时间戳（毫秒，来自 Instant 的单调时钟）。
    static LAST_FORCED_FRAME: AtomicU64 = AtomicU64::new(0);
    /// `wglMakeCurrent` 原始函数指针。
    static ORIGINAL_MAKE_CURRENT: AtomicU64 = AtomicU64::new(0);
    /// 两个被 Hook 的目标地址（结束时要按地址摘掉 Hook）。
    static SWAP_TARGET: AtomicU64 = AtomicU64::new(0);
    static MAKE_CURRENT_TARGET: AtomicU64 = AtomicU64::new(0);
    /// 日志时间的相对基准。
    static LOG_START: OnceLock<Instant> = OnceLock::new();

    /// 原始 `wglSwapBuffers` 签名：`BOOL wglSwapBuffers(HDC)`。
    /// 这里用 `*mut c_void` 表示 `HDC`、`i32` 表示 `BOOL`，避免依赖 windows crate 里句柄类型的具体封装形式。
    type WglSwapBuffersFn = unsafe extern "system" fn(hdc: *mut c_void) -> i32;
    /// `void glBindFramebuffer(GLenum target, GLuint framebuffer)`（GL 3.0）。
    type GlBindFramebufferFn = unsafe extern "system" fn(target: u32, framebuffer: u32);
    /// `BOOL wglMakeCurrent(HDC, HGLRC)`。
    type WglMakeCurrentFn =
        unsafe extern "system" fn(hdc: *mut c_void, hglrc: *mut c_void) -> i32;

    // opengl32.dll 只导出 OpenGL 1.1 函数，因此这里声明的都是 GL 1.1 里确实存在的入口点，
    // 直接静态链接即可，不需要额外的 OpenGL 绑定 crate。
    #[link(name = "opengl32")]
    extern "system" {
        fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
        fn glClear(mask: u32);
        fn glPushAttrib(mask: u32);
        fn glPopAttrib();
        fn glDisable(cap: u32);
        fn glGetIntegerv(pname: u32, params: *mut i32);
        fn glEnable(cap: u32);
        fn glGenTextures(count: i32, textures: *mut u32);
        fn glBindTexture(target: u32, texture: u32);
        fn glTexParameteri(target: u32, pname: u32, param: i32);
        fn glTexImage2D(
            target: u32,
            level: i32,
            internal_format: i32,
            width: i32,
            height: i32,
            border: i32,
            format: u32,
            kind: u32,
            pixels: *const c_void,
        );
        fn glMatrixMode(mode: u32);
        fn glPushMatrix();
        fn glPopMatrix();
        fn glLoadIdentity();
        fn glOrtho(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64);
        fn glColor4f(red: f32, green: f32, blue: f32, alpha: f32);
        fn glBegin(mode: u32);
        fn glEnd();
        fn glTexCoord2f(u: f32, v: f32);
        fn glVertex2f(x: f32, y: f32);
        fn glPixelStorei(pname: u32, param: i32);
        fn wglGetProcAddress(lpszproc: PCSTR) -> *const c_void;
        fn wglGetCurrentDC() -> *mut c_void;
    }

    /// DllMain 在 `DLL_PROCESS_ATTACH` 时调用：只做记录和起后台线程，绝不阻塞加载器。
    pub(super) fn on_process_attach(module: HMODULE) {
        SELF_MODULE.store(module.0 as usize as u64, Ordering::Release);

        // 关闭线程通知，减少无意义的 DllMain 回调。
        let _ = unsafe { DisableThreadLibraryCalls(module) };

        // 同一个模块被重复 LoadLibrary 时，DllMain 会再次收到 PROCESS_ATTACH；
        // 这里直接跳过，避免装第二个 Hook。
        if INIT_STARTED.swap(true, Ordering::SeqCst) {
            log("检测到重复的 DLL_PROCESS_ATTACH，跳过重复初始化。");
            return;
        }

        let spawned = std::thread::Builder::new()
            .name("pz-overlay-init".to_string())
            .spawn(initialize_overlay);

        if let Err(error) = spawned {
            INIT_STARTED.store(false, Ordering::SeqCst);
            log(&format!("创建初始化线程失败: {error}"));
        }

        // 极速启动（实验性）：另起线程等缓存 + 校验，失败只写日志，
        // 与覆盖层互不影响（当前阶段不会改动游戏进程里的任何东西）。
        if let Err(error) = std::thread::Builder::new()
            .name("pz-fast-launch".to_string())
            .spawn(crate::fastlaunch::run)
        {
            log(&format!("创建极速启动线程失败: {error}"));
        }
    }

    /// 初始化线程主体：等模块 → 等导出 → 装 Hook。任何一步失败都静默退出。
    fn initialize_overlay() {
        // 同一个进程里如果已经有同名模块（例如从另一个路径再注入了一份副本），
        // 直接放弃，避免两个 MinHook 实例互相覆盖同一处补丁。
        let self_module = SELF_MODULE.load(Ordering::Acquire) as usize;
        if let Some((module_name, count)) = same_named_module_count(self_module) {
            if count > 1 {
                log(&format!(
                    "进程内已存在 {count} 份名为 {module_name} 的模块，跳过重复注入。"
                ));
                return;
            }
        }

        // 从这里开始接管覆盖：加载界面在游戏窗口内部绘制
        // （换帧前 GDI 直接画窗口 DC；换帧后由 GL 纹理绘制）。
        OVERLAY_ACTIVE.store(true, Ordering::Release);
        crate::ui::start(std::process::id());

        let Some(opengl32) = wait_for_opengl32() else {
            log("等待 opengl32.dll 加载超时，放弃安装 Hook。");
            OVERLAY_ACTIVE.store(false, Ordering::Release);
            crate::ui::stop();
            return;
        };
        let Some(target) = wait_for_swap_buffers_export(opengl32) else {
            log("等待 wglSwapBuffers 导出超时，放弃安装 Hook。");
            OVERLAY_ACTIVE.store(false, Ordering::Release);
            crate::ui::stop();
            return;
        };

        // SAFETY: target 来自 opengl32.dll 的导出表，是可执行代码地址；
        // 回调签名与 wglSwapBuffers 保持一致，且全程不 panic。
        unsafe {
            match MinHook::create_hook(target, hooked_wgl_swap_buffers as *mut c_void) {
                Ok(original) => {
                    if original.is_null() {
                        log("MinHook 返回了空的原始函数指针，放弃安装 Hook。");
                        let _ = MinHook::remove_hook(target);
                        OVERLAY_ACTIVE.store(false, Ordering::Release);
                        return;
                    }

                    // 必须在 enable 之前写好原始指针：Hook 一旦生效，回调里就要用它转发。
                    ORIGINAL_SWAP_BUFFERS.store(original as usize as u64, Ordering::Release);

                    match MinHook::enable_hook(target) {
                        Ok(()) => {
                            HOOK_ENABLED.store(true, Ordering::Release);
                            OVERLAY_ACTIVE.store(true, Ordering::Release);
                            SWAP_TARGET.store(target as usize as u64, Ordering::Release);
                            log(&format!(
                                "wglSwapBuffers Hook 已启用: target=0x{:X} original=0x{:X}",
                                target as usize, original as usize
                            ));
                            install_make_current_hook(opengl32);
                            start_stop_watcher();
                        }
                        Err(status) => {
                            ORIGINAL_SWAP_BUFFERS.store(0, Ordering::Release);
                            let _ = MinHook::remove_hook(target);
                            OVERLAY_ACTIVE.store(false, Ordering::Release);
                            log(&format!(
                                "启用 Hook 失败（{}），已回退，游戏不受影响。",
                                status
                            ));
                        }
                    }
                }
                Err(status) => {
                    OVERLAY_ACTIVE.store(false, Ordering::Release);
                    log(&format!(
                        "创建 Hook 失败（{}）。常见原因：该地址已被其它覆盖层 Hook，或目标地址不可 Hook。",
                        status
                    ));
                }
            }
        }
    }

    /// Hook 回调。运行在游戏的渲染线程上，必须零 panic、零阻塞、零分配。
    unsafe extern "system" fn hooked_wgl_swap_buffers(hdc: *mut c_void) -> i32 {
        let original = ORIGINAL_SWAP_BUFFERS.load(Ordering::Acquire);

        if !FIRST_CALL_LOGGED.swap(true, Ordering::Relaxed) {
            log("首次进入 wglSwapBuffers Hook，注入链路已打通。");
            // 通知启动器：覆盖层已经真正画上第一帧（红色已可见）。
            let started = STARTED_EVENT.load(Ordering::Acquire);
            if started != 0 {
                let _ = unsafe {
                    windows::Win32::System::Threading::SetEvent(
                        windows::Win32::Foundation::HANDLE(started as usize as *mut c_void),
                    )
                };
            }
        }

        paint_red_while_active();

        if original == 0 {
            // 理论上不会发生（enable 之前一定先写入原始地址）。
            // 真遇到就返回 TRUE，宁可少交换一帧，也不能让游戏崩。
            return 1;
        }

        // SAFETY: original 由 MinHook 提供，指向未打补丁的原始 wglSwapBuffers。
        let original_fn: WglSwapBuffersFn =
            unsafe { std::mem::transmute(original as usize as *const c_void) };
        unsafe { original_fn(hdc) }
    }

    /// 额外 Hook `wglMakeCurrent`：上下文刚绑定（还没开始换帧）时，直接替游戏画一帧并呈现，
    /// 这样窗口内的加载界面不用等游戏自己的渲染循环跑起来。
    fn install_make_current_hook(opengl32: HMODULE) {
        // SAFETY: opengl32 是已加载模块；导出名合法。
        let Some(proc) = (unsafe {
            GetProcAddress(
                opengl32,
                PCSTR::from_raw(WGL_MAKE_CURRENT.as_ptr()),
            )
        }) else {
            log("未找到 wglMakeCurrent 导出，跳过提前换帧。");
            return;
        };
        let target = proc as usize as *mut c_void;
        if (target as usize) < MIN_VALID_PROC_ADDRESS {
            return;
        }

        // SAFETY: target 来自 opengl32 导出表；回调签名一致且不 panic。
        unsafe {
            match MinHook::create_hook(target, hooked_wgl_make_current as *mut c_void) {
                Ok(original) => {
                    ORIGINAL_MAKE_CURRENT.store(original as usize as u64, Ordering::Release);
                    MAKE_CURRENT_TARGET.store(target as usize as u64, Ordering::Release);
                    if let Err(status) = MinHook::enable_hook(target) {
                        ORIGINAL_MAKE_CURRENT.store(0, Ordering::Release);
                        let _ = MinHook::remove_hook(target);
                        log(&format!("wglMakeCurrent Hook 启用失败（{status}），仅依赖正常换帧。"));
                    } else {
                        log("wglMakeCurrent Hook 已启用（用于提前换帧）。");
                    }
                }
                Err(status) => {
                    log(&format!("wglMakeCurrent Hook 创建失败（{status}）。"));
                }
            }
        }
    }

    unsafe extern "system" fn hooked_wgl_make_current(
        hdc: *mut c_void,
        hglrc: *mut c_void,
    ) -> i32 {
        let original = ORIGINAL_MAKE_CURRENT.load(Ordering::Acquire);
        if original == 0 {
            return 0;
        }
        let original_fn: WglMakeCurrentFn =
            unsafe { std::mem::transmute(original as usize as *const c_void) };
        let result = unsafe { original_fn(hdc, hglrc) };

        // 只在"上下文已经变成 current"且覆盖层生效时，限流地替游戏换一帧。
        if result != 0 && OVERLAY_ACTIVE.load(Ordering::Acquire) && can_force_frame() {
            // SAFETY: 刚刚 MakeCurrent 成功，当前线程持有 GL 上下文。
            unsafe { force_present_frame() };
        }
        result
    }

    /// 覆盖层结束后**彻底退出 DLL 代码**：停界面线程 → 摘掉全部 Hook → 卸载本 DLL。
    ///
    /// 摘掉 Hook 后游戏就完全回到原始 `wglSwapBuffers` / `wglMakeCurrent`，不再有任何我们的代码被调用；
    /// 随后从 DLL 自己的线程里 `FreeLibraryAndExitThread` 把模块卸载掉。
    ///
    /// # Safety
    /// 必须在 DLL 自己的线程上调用；调用成功后该线程不会返回（模块已卸载）。
    unsafe fn shutdown_overlay() {
        if SHUTTING_DOWN.swap(true, Ordering::AcqRel) {
            return;
        }

        OVERLAY_ACTIVE.store(false, Ordering::Release);
        crate::ui::stop();
        // 等可能正在执行的 hook 回调走完（回调本身极短）。
        std::thread::sleep(Duration::from_millis(300));

        unsafe {
            let swap_target = SWAP_TARGET.swap(0, Ordering::AcqRel) as usize as *mut c_void;
            if !swap_target.is_null() {
                let _ = MinHook::disable_hook(swap_target);
                let _ = MinHook::remove_hook(swap_target);
            }
            ORIGINAL_SWAP_BUFFERS.store(0, Ordering::Release);

            let make_current_target =
                MAKE_CURRENT_TARGET.swap(0, Ordering::AcqRel) as usize as *mut c_void;
            if !make_current_target.is_null() {
                let _ = MinHook::disable_hook(make_current_target);
                let _ = MinHook::remove_hook(make_current_target);
            }
            ORIGINAL_MAKE_CURRENT.store(0, Ordering::Release);
        }
        // 注意：这里**不**调用 FreeLibraryAndExitThread。
        //
        // 本 DLL 是 Rust cdylib，Rust std 线程在退出时仍会访问本模块内的运行时代码
        // （Thread 结构、TLS 析构等）。只等 300ms 就卸模块，一旦还有线程在里面执行，
        // 卸载后就是 use-after-free —— 实测表现为"覆盖层关闭后游戏直接消失"。
        // 因此改为：摘掉全部 Hook + 停掉自绘线程 → 游戏侧不再有任何我们的代码被调用，
        // 模块保持驻留（不占 CPU、不再绘制），等游戏进程退出时由系统回收。
        log("已摘除全部 Hook，覆盖层代码停止（DLL 保持驻留以避免卸载风险）。");
    }

    /// 限流：两次强制换帧之间至少间隔 `FORCED_FRAME_INTERVAL`。
    fn can_force_frame() -> bool {
        // 见下方 shutdown_overlay()：这里只做限流。
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_millis() as u64)
            .unwrap_or(0);
        let last = LAST_FORCED_FRAME.load(Ordering::Acquire);
        if now.saturating_sub(last) < FORCED_FRAME_INTERVAL.as_millis() as u64 {
            return false;
        }
        LAST_FORCED_FRAME.store(now, Ordering::Release);
        true
    }

    /// 用当前上下文画一帧加载界面并呈现。
    unsafe fn force_present_frame() {
        let original_swap = ORIGINAL_SWAP_BUFFERS.load(Ordering::Acquire);
        if original_swap == 0 {
            return;
        }
        let dc = unsafe { wglGetCurrentDC() };
        if dc.is_null() {
            return;
        }
        // SAFETY: 当前线程上下文有效；绘制逻辑内部自行保存/恢复状态。
        unsafe {
            paint_red_frame();
            let swap: WglSwapBuffersFn =
                std::mem::transmute(original_swap as usize as *const c_void);
            swap(dc);
        }
    }

    /// 覆盖绘制闸门：只做一次原子读，停止后完全不再碰 GL。
    fn paint_red_while_active() {
        if !OVERLAY_ACTIVE.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: 该回调由 wglSwapBuffers 触发，调用线程上一定有当前 GL 上下文。
        unsafe { paint_red_frame() };
    }

    /// 创建「已开始绘制」事件，并等待启动器的停止信号（命名事件）；同时启动安全上限计时。
    fn start_stop_watcher() {
        // 事件名带上自己的 PID，启动器按「游戏进程 PID」创建同名事件。
        let started_name = format!("{STARTED_EVENT_PREFIX}{}", std::process::id());
        let wide_started: Vec<u16> = started_name.encode_utf16().chain(std::iter::once(0)).collect();
        match unsafe {
            CreateEventW(None, true, false, PCWSTR::from_raw(wide_started.as_ptr()))
        } {
            Ok(handle) => {
                STARTED_EVENT.store(handle.0 as usize as u64, Ordering::Release);
            }
            Err(error) => {
                log(&format!("创建已开始绘制事件 {started_name} 失败: {error}"));
            }
        }

        // 事件名带上自己的 PID，启动器按「游戏进程 PID」创建同名事件。
        let name = format!("{STOP_EVENT_PREFIX}{}", std::process::id());
        let wide_name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: 创建/打开一个手动重置的命名事件；失败时只记日志，靠安全上限兜底。
        match unsafe {
            CreateEventW(
                None,
                true,
                false,
                PCWSTR::from_raw(wide_name.as_ptr()),
            )
        } {
            Ok(handle) => {
                STOP_EVENT.store(handle.0 as usize as u64, Ordering::Release);
                log(&format!("已监听停止事件 {name}"));

                // HANDLE 内部是裸指针，不是 Send；这里按数值传给线程，在线程里重建。
                let raw_handle = handle.0 as usize;
                let spawned = std::thread::Builder::new()
                    .name("pz-overlay-stop".to_string())
                    .spawn(move || {
                        let handle = windows::Win32::Foundation::HANDLE(raw_handle as *mut c_void);
                        // SAFETY: handle 在本线程存活期间一直有效（静态保存，不关闭）。
                        let result = unsafe { WaitForSingleObject(handle, INFINITE) };
                        if result == WAIT_OBJECT_0 && OVERLAY_ACTIVE.load(Ordering::Acquire) {
                            log("收到启动器停止信号，覆盖绘制结束。");
                            // 摘掉 Hook 并停掉界面线程。
                            unsafe { shutdown_overlay() };
                        }
                    });
                if let Err(error) = spawned {
                    log(&format!("创建停止监听线程失败: {error}"));
                }
            }
            Err(error) => {
                log(&format!("创建停止事件 {name} 失败: {error}，仅依赖安全上限。"));
            }
        }

        let spawned = std::thread::Builder::new()
            .name("pz-overlay-deadline".to_string())
            .spawn(|| {
                std::thread::sleep(OVERLAY_MAX_DURATION);
                if OVERLAY_ACTIVE.load(Ordering::Acquire) {
                    log("达到覆盖安全上限（10 分钟），自动停止绘制。");
                    unsafe { shutdown_overlay() };
                }
            });
        if let Err(error) = spawned {
            log(&format!("创建安全上限线程失败: {error}"));
        }
    }

    /// 用 OpenGL 原生调用把整块默认帧缓冲刷成纯红，随后恢复被改动的状态。
    unsafe fn paint_red_frame() {
        unsafe {
            // 保存全部服务端属性（含 clear color、scissor 等），最后一次性还原。
            glPushAttrib(GL_ALL_ATTRIB_BITS);
            // glClear 会被 scissor 裁剪，先关闭以保证整窗变红；该状态由 glPopAttrib 还原。
            glDisable(GL_SCISSOR_TEST);

            let bind_framebuffer = resolve_bind_framebuffer();
            let mut previous_binding: i32 = 0;
            if let Some(bind_framebuffer) = bind_framebuffer {
                // 若游戏此刻正渲染到 FBO，先切回默认帧缓冲，让红色真正出现在窗口上。
                glGetIntegerv(GL_FRAMEBUFFER_BINDING, &mut previous_binding);
                if previous_binding != 0 {
                    bind_framebuffer(GL_FRAMEBUFFER, 0);
                }
            }

            glClearColor(1.0, 0.0, 0.0, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            // 在红底之上绘制加载界面。
            draw_ui_texture();

            if let Some(bind_framebuffer) = bind_framebuffer {
                if previous_binding != 0 {
                    bind_framebuffer(GL_FRAMEBUFFER, previous_binding as u32);
                }
            }

            glPopAttrib();
        }
    }

    /// 每帧绘制加载动画（空心方框，按帧计数旋转）。
    ///
    /// 独立成函数是因为：`draw_ui_texture()` 在"界面图还没准备好"时会提前返回，
    /// 之前把动画写在那里面，导致某些阶段完全不画（表现为"永远不转"）。
    fn draw_animation_overlay() {
        // 写死的每帧角度表：60 帧一个周期（≈1s），前 18 帧用三次缓动走完 90°，其余 42 帧保持。
        // 数值由 ease-in-out cubic 预先算好（0..90 度），不再做任何时间/公式推算。
        const ANGLE_STEPS: [f32; 60] = [
            0.00, 0.06, 0.49, 1.67, 3.95, 7.72, 13.33, 21.17, 31.61, 45.00, 58.39, 68.83,
            76.67, 82.28, 86.05, 88.33, 89.51, 89.94, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
            90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
            90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
            90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00, 90.00,
        ];

        let mut viewport = [0_i32; 4];
        unsafe {
            glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
        }
        let viewport_width = viewport[2] as f32;
        let viewport_height = viewport[3] as f32;
        if viewport_width <= 0.0 || viewport_height <= 0.0 {
            return;
        }
        // 直接用视口算位置与大小（不依赖 DIB / DPI 记录，先保证一定画得出来）。
        let size = (viewport_height * 0.045).clamp(16.0, 64.0);
        let center_x = viewport_width / 2.0;
        let center_y = viewport_height / 2.0 + size * 3.2;

        let frame = crate::ui::ANIMATION_FRAMES.fetch_add(1, Ordering::AcqRel);
        let angle_degrees = 90.0 * (frame / 60) as f32 + ANGLE_STEPS[(frame % 60) as usize];
        let corners = crate::ui::rotated_square(center_x, center_y, size, angle_degrees);

        if !ANIMATION_LOGGED.swap(true, Ordering::Relaxed) {
            log(&format!(
                "动画方框开始绘制: viewport={}x{} size={:.1} angle={:.1}°",
                viewport_width, viewport_height, size, angle_degrees
            ));
        }

        unsafe {
            glDisable(GL_TEXTURE_2D);
            glColor4f(1.0, 1.0, 1.0, 1.0);
            glMatrixMode(GL_PROJECTION);
            glPushMatrix();
            glLoadIdentity();
            glOrtho(0.0, viewport_width as f64, viewport_height as f64, 0.0, -1.0, 1.0);
            glMatrixMode(GL_MODELVIEW);
            glPushMatrix();
            glLoadIdentity();
            // 逐条边绘制（与参考实现一致）
            glBegin(GL_LINES);
            for index in 0..4 {
                let (x1, y1) = corners[index];
                let (x2, y2) = corners[(index + 1) % 4];
                glVertex2f(x1, y1);
                glVertex2f(x2, y2);
            }
            glEnd();
            // 顶点小方块，强化"被画出来"的观感
            let dot = (size * 0.12).max(1.5);
            glBegin(GL_QUADS);
            for (x, y) in corners {
                glVertex2f(x - dot, y - dot);
                glVertex2f(x + dot, y - dot);
                glVertex2f(x + dot, y + dot);
                glVertex2f(x - dot, y + dot);
            }
            glEnd();
            glPopMatrix();
            glMatrixMode(GL_PROJECTION);
            glPopMatrix();
            glMatrixMode(GL_MODELVIEW);
        }
    }

    /// 把 GDI 渲染好的界面（RGBA）上传成 GL 纹理并画满屏四边形。
    fn draw_ui_texture() {
        let Some((version, width, height, rgba)) = crate::ui::image() else {
            return;
        };
        if width <= 0 || height <= 0 || rgba.is_empty() {
            return;
        }
        unsafe {
            let mut viewport = [0_i32; 4];
            glGetIntegerv(GL_VIEWPORT, viewport.as_mut_ptr());
            let viewport_width = viewport[2] as f32;
            let viewport_height = viewport[3] as f32;
            if viewport_width <= 0.0 || viewport_height <= 0.0 {
                return;
            }

            let mut texture = UI_TEXTURE.load(Ordering::Acquire) as u32;
            if texture == 0 {
                glGenTextures(1, &mut texture);
                if texture == 0 {
                    return;
                }
                glBindTexture(GL_TEXTURE_2D, texture);
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as i32);
                glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as i32);
                UI_TEXTURE.store(texture as u64, Ordering::Release);
            } else {
                glBindTexture(GL_TEXTURE_2D, texture);
            }

            if UI_TEXTURE_VERSION.load(Ordering::Acquire) != version {
                glPixelStorei(GL_UNPACK_ALIGNMENT, 1);
                glTexImage2D(
                    GL_TEXTURE_2D,
                    0,
                    GL_RGBA as i32,
                    width,
                    height,
                    0,
                    GL_RGBA,
                    GL_UNSIGNED_BYTE,
                    rgba.as_ptr() as *const c_void,
                );
                UI_TEXTURE_VERSION.store(version, Ordering::Release);
            }

            glEnable(GL_TEXTURE_2D);
            glMatrixMode(GL_PROJECTION);
            glPushMatrix();
            glLoadIdentity();
            glOrtho(
                0.0,
                viewport_width as f64,
                viewport_height as f64,
                0.0,
                -1.0,
                1.0,
            );
            glMatrixMode(GL_MODELVIEW);
            glPushMatrix();
            glLoadIdentity();
            glColor4f(1.0, 1.0, 1.0, 1.0);

            glBegin(GL_QUADS);
            glTexCoord2f(0.0, 0.0);
            glVertex2f(0.0, 0.0);
            glTexCoord2f(1.0, 0.0);
            glVertex2f(viewport_width, 0.0);
            glTexCoord2f(1.0, 1.0);
            glVertex2f(viewport_width, viewport_height);
            glTexCoord2f(0.0, 1.0);
            glVertex2f(0.0, viewport_height);
            glEnd();

            // 动画方块：每帧实时绘制（不能烘进纹理，否则纹理只上传一次就不动了）。
            glPopMatrix();
            glMatrixMode(GL_PROJECTION);
            glPopMatrix();
            glMatrixMode(GL_MODELVIEW);
            glDisable(GL_TEXTURE_2D);
        }
    }

    /// 解析 GL 3.0 的 `glBindFramebuffer`，只解析一次；解析不到就永久降级（不处理 FBO 绑定）。
    fn resolve_bind_framebuffer() -> Option<GlBindFramebufferFn> {
        let cached = BIND_FRAMEBUFFER.load(Ordering::Acquire);
        if cached == PTR_UNAVAILABLE {
            return None;
        }
        if cached != PTR_UNRESOLVED {
            // SAFETY: 缓存值只可能来自下方 wglGetProcAddress 返回的合法函数地址。
            return Some(unsafe {
                std::mem::transmute::<*const c_void, GlBindFramebufferFn>(
                    cached as usize as *const c_void,
                )
            });
        }

        // 这里只在当前 GL 上下文存在的回调线程里解析，避免 wglGetProcAddress 在无上下文时返回空。
        let address = unsafe { wgl_get_proc_address(b"glBindFramebuffer\0") };
        if address.is_null() {
            BIND_FRAMEBUFFER.store(PTR_UNAVAILABLE, Ordering::Release);
            return None;
        }

        BIND_FRAMEBUFFER.store(address as usize as u64, Ordering::Release);
        // SAFETY: 地址来自 wglGetProcAddress，已按上面的规则过滤过非法小地址。
        Some(unsafe { std::mem::transmute::<*const c_void, GlBindFramebufferFn>(address) })
    }

    /// 安全版 `wglGetProcAddress`：过滤驱动返回的非法小地址。
    unsafe fn wgl_get_proc_address(name: &[u8]) -> *const c_void {
        let address = unsafe { wglGetProcAddress(PCSTR::from_raw(name.as_ptr())) };
        if address as usize >= MIN_VALID_PROC_ADDRESS {
            address
        } else {
            std::ptr::null()
        }
    }

    /// 循环等待 `opengl32.dll` 被游戏加载。
    fn wait_for_opengl32() -> Option<HMODULE> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            // SAFETY: 只是查询已加载模块，参数是合法的 NUL 结尾字符串。
            if let Ok(module) = unsafe { GetModuleHandleA(PCSTR::from_raw(OPENGL32_DLL.as_ptr())) }
            {
                if !module.is_invalid() {
                    return Some(module);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(WAIT_INTERVAL);
        }
    }

    /// 循环等待 `wglSwapBuffers` 导出可用。
    fn wait_for_swap_buffers_export(module: HMODULE) -> Option<*mut c_void> {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            // SAFETY: module 来自 GetModuleHandleA；导出名是合法的 NUL 结尾字符串。
            if let Some(proc) =
                unsafe { GetProcAddress(module, PCSTR::from_raw(WGL_SWAP_BUFFERS.as_ptr())) }
            {
                let address = proc as usize;
                if address >= MIN_VALID_PROC_ADDRESS {
                    return Some(address as *mut c_void);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(WAIT_INTERVAL);
        }
    }

    /// 统计进程内与自身同名的模块数量（含自身）。
    ///
    /// 用于识别“同一进程里注入了两份覆盖层副本”的情况：同名模块多于一份就放弃 Hook。
    fn same_named_module_count(self_module: usize) -> Option<(String, usize)> {
        // SAFETY: 0 表示当前进程；快照句柄失败时直接返回 None。
        let snapshot =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, 0) }.ok()?;

        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;

        let mut modules: Vec<(String, usize)> = Vec::new();
        // SAFETY: entry 已按 API 要求设置 dwSize。
        if unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                let name = module_base_name(&entry);
                modules.push((name, entry.hModule.0 as usize));
                // SAFETY: 同上的链表遍历，遍历结束会返回错误。
                if unsafe { Module32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };

        let self_name = modules
            .iter()
            .find(|(_, module)| *module == self_module)
            .map(|(name, _)| name.clone())?;
        let count = modules
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(&self_name))
            .count();
        Some((self_name, count))
    }

    fn module_base_name(entry: &MODULEENTRY32W) -> String {
        let name: Vec<u16> = entry
            .szModule
            .iter()
            .copied()
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&name)
    }

    /// 追加一行日志到 `%TEMP%\overlay.log`。日志失败不影响任何逻辑。
    fn log(message: &str) {
        use std::io::Write as _;

        let start = LOG_START.get_or_init(Instant::now);
        let path = std::env::temp_dir().join("overlay.log");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "[pid={} t={:.3}s] {message}",
                std::process::id(),
                start.elapsed().as_secs_f64()
            );
        }
    }

    const OPENGL32_DLL: &[u8] = b"opengl32.dll\0";
    const WGL_SWAP_BUFFERS: &[u8] = b"wglSwapBuffers\0";
    const WGL_MAKE_CURRENT: &[u8] = b"wglMakeCurrent\0";
}

/// DLL 入口。避免在加载器锁内做任何耗时操作：只记录模块句柄并起后台线程。
///
/// ABI 说明：第一个参数是 `HMODULE`（指针大小），第三个参数保留不用。
/// 这里用裸指针 + `u32` 声明，避免依赖 windows crate 对 `HINSTANCE` / `HMODULE` 封装形式的变化。
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    module: *mut std::ffi::c_void,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> i32 {
    // `DLL_PROCESS_ATTACH`
    const DLL_PROCESS_ATTACH: u32 = 1;

    if reason == DLL_PROCESS_ATTACH {
        overlay::on_process_attach(windows::Win32::Foundation::HMODULE(module));
    }

    // TRUE
    1
}
