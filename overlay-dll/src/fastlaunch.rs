//! 极速启动（实验性）在游戏进程里的落地模块。
//!
//! 目标（见 docs/architecture.md §6.3）：在 ZombieBuddy 扫描前，把启动器预扫描好的
//! `zb-prescan-cache.json` 喂给它的 `PatchEngine.collectPatches`，短路其中的 ClassGraph
//! 全量扫描（`javap -c` 已确认：22 处 classgraph 引用只在该类，`Loader`/`Agent`/`Patch` 均为 0）。
//!
//! 当前进度（2026-10-14 晚，已在一次性 JVM 上跑通全链路）：
//! * **正式路线 = JVMTI agent 早期加载**（`-agentpath:<pzm 覆盖层 DLL>`）：JVM 启动时调
//!   `Agent_OnLoad` → `GetEnv(JVMTI)` → `AddCapabilities` → 注册 `VMInit` → 在 `VMInit` 回调里
//!   `DefineClass` 送进辅助类并打开 `ClassFileLoadHook` → ZombieBuddy 自己加载 `PatchEngine` 时
//!   就地改成"包装式装配"（原实现改名 `collectPatches$pzm`，调用点仍指向 `collectPatches`）；
//! * **远程注入路线**（`JNI_GetCreatedJavaVMs` + attach + retransform）保留为调试路径，
//!   仍要求 `PZM_FAST_LAUNCH_INJECT=1` 才执行；agent 一旦接管就整体让路（`AGENT_LOADED`）；
//! * 门禁与任何失败都只写日志并回退：找不到缓存、指纹对不上、环境/能力/回调任一步失败、
//!   改写失败、辅助类缺失 —— 全部等于"放行原字节" = ZombieBuddy 原生扫描；
//! * 应急关闭：设 `PZM_FAST_LAUNCH_DISABLE=1` 可以让 agent 立刻什么都不做。
//!
//! 本模块刻意只用 `std` + `jni`（注入器是 cdylib，能不加依赖就不加；serde 等留给启动器侧）。

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 缓存文件名，必须与启动器侧 `FAST_LAUNCH_CACHE_FILE` 完全一致。
const CACHE_FILE: &str = "zb-prescan-cache.json";
/// 期望的缓存格式版本，必须与启动器侧 `FAST_LAUNCH_CACHE_FORMAT` 一致。
const CACHE_FORMAT: u32 = 1;

/// 辅助类 `pzm.fastlaunch.FastLaunchBridge` 的字节码，直接嵌进 DLL（注入时用 JVMTI `DefineClass` 送进 JVM）。
///
/// 重新生成方式（改了 Java 源码后必须重跑，否则播放的是旧字节码）：
/// `javac --release 8 -d java/classes java/pzm/fastlaunch/FastLaunchBridge.java`
/// 产物随仓库提交，所以**构建时不依赖 javac**。
pub(crate) const FAST_LAUNCH_BRIDGE_CLASS: &[u8] =
    include_bytes!("../java/classes/pzm/fastlaunch/FastLaunchBridge.class");
/// 等 JVM/缓存的轮询参数（DLL 注入得很早，JVM 通常还没起来）。
const WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const WAIT_INTERVAL: Duration = Duration::from_millis(500);

/// 校验结果：能拿到这些信息就说明"缓存本身可用"，能否真注入是下一步的事。
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheCheck {
    generated_at: u64,
    mod_set_hash: String,
    mod_count: usize,
}

/// 日志：写在 `%TEMP%\fast-launch.log`，和覆盖层的 `overlay.log` 同一套路。
/// 任何写日志失败都吞掉——日志本身绝不能影响游戏。
fn log(message: &str) {
    let Ok(temp) = std::env::var("TEMP") else {
        return;
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let line = format!("[{stamp}] {message}\n");
    let path = PathBuf::from(temp).join("fast-launch.log");
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

/// 缓存文件路径：`%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`（与启动器一致）。
fn cache_path() -> Option<PathBuf> {
    let profile = std::env::var_os("USERPROFILE")?;
    Some(
        PathBuf::from(profile)
            .join("Zomboid")
            .join(".pzmanager")
            .join(CACHE_FILE),
    )
}

/// 从 JSON 文本里取一个字符串字段（不引入 serde：注入器只关心少数几个字段）。
fn json_string_field(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = content.find(&needle)? + needle.len();
    let rest = content[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// 从 JSON 文本里取一个非负整数字段。
fn json_number_field(content: &str, key: &str) -> Option<u64> {
    let needle = format!("\"{key}\"");
    let start = content.find(&needle)? + needle.len();
    let rest = content[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// 读 + 校验缓存：格式版本、指纹、模组数。任何一项不对就返回 None（调用方只会记日志）。
fn read_and_check_cache(path: &std::path::Path) -> Option<CacheCheck> {
    let content = fs::read_to_string(path).ok()?;
    let format = json_number_field(&content, "format")?;
    if format != u64::from(CACHE_FORMAT) {
        log(&format!("缓存格式版本不匹配: {format} != {CACHE_FORMAT}"));
        return None;
    }
    let mod_set_hash = json_string_field(&content, "modSetHash")?;
    if mod_set_hash.is_empty() {
        log("缓存缺少 modSetHash，放弃缓存");
        return None;
    }
    let generated_at = json_number_field(&content, "generatedAt").unwrap_or(0);
    // 模组数按 `"modName"` 出现次数统计：缓存里每个模组条目必有一个 modName。
    let mod_count = content.matches("\"modName\"").count();
    Some(CacheCheck {
        generated_at,
        mod_set_hash,
        mod_count,
    })
}

/// 取一个布尔字段（只认紧跟其后的 true/false）。
fn json_bool_field(content: &str, key: &str) -> Option<bool> {
    let needle = format!("\"{key}\"");
    let start = content.find(&needle)? + needle.len();
    let rest = content[start..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    if rest.starts_with("true") {
        Some(true)
    } else if rest.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// 开关门禁：读启动器的 `settings.json`，要求「极速启动已开启」且「模组加载器方案 = zombieBuddy」。
///
/// 前端在方案不是 ZombieBuddy 时会把开关禁用，但那只是界面约束；注入器必须自己再判一次，
/// 否则关了开关、或切到"不使用加载器"的会话也会被注入。
fn fast_launch_enabled_in_settings() -> bool {
    let Some(profile) = std::env::var_os("USERPROFILE") else {
        log("拿不到 USERPROFILE，按极速启动未开启处理");
        return false;
    };
    let path = PathBuf::from(profile)
        .join("Zomboid")
        .join(".pzmanager")
        .join("settings.json");
    let Ok(content) = fs::read_to_string(&path) else {
        log("读不到启动器设置，按极速启动未开启处理");
        return false;
    };
    let enabled = json_bool_field(&content, "fastLaunch").unwrap_or(false);
    let scheme = json_string_field(&content, "modLoaderScheme").unwrap_or_default();
    log(&format!(
        "设置检查：fastLaunch={enabled} modLoaderScheme={scheme}"
    ));
    enabled && scheme.eq_ignore_ascii_case("zombieBuddy")
}

/// 把辅助类字节送进 JVM（JNI `DefineClass`，`jni.h` 实证绝对下标 5）。
///
/// 用 `null` 作为类加载器参数，即定义到 **bootstrap** 加载器下：辅助类只引用 `java.*`，
/// bootstrap 能全部看到；而 bootstrap 是所有加载器的父，所以 ZombieBuddy 的加载器也能看见它。
/// `collect(package, loader)` 里用调用方传进来的 loader 去取 `PatchEngine`，跨加载器调用没问题。
///
/// 类已经存在时 `DefineClass` 会失败（LinkageError）——那也返回 None，调用方放弃缓存即可。
#[cfg(windows)]
#[allow(dead_code)] // 投送流程接入时调用
fn define_bridge_class(env: usize) -> Option<usize> {
    type DefineClass = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const i8,
        *mut core::ffi::c_void,
        *const u8,
        i32,
    ) -> *mut core::ffi::c_void;
    type ExceptionCheck = unsafe extern "system" fn(*mut core::ffi::c_void) -> u8;
    type ExceptionClear = unsafe extern "system" fn(*mut core::ffi::c_void);

    // 下标来自本机 jni.h（含 reserved 槽位的绝对下标，见 docs/architecture.md §6.3）。
    const DEFINE_CLASS: usize = 5;
    const EXCEPTION_CLEAR: usize = 17;
    const EXCEPTION_CHECK: usize = 228;

    let jni = env as *mut core::ffi::c_void;
    let define: DefineClass = unsafe { std::mem::transmute(jni_fn(env, DEFINE_CLASS)) };
    let exception_check: ExceptionCheck =
        unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CHECK)) };
    let exception_clear: ExceptionClear = unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CLEAR)) };

    let bytes = FAST_LAUNCH_BRIDGE_CLASS;
    let length = i32::try_from(bytes.len()).ok()?;
    let class = unsafe {
        define(
            jni,
            b"pzm/fastlaunch/FastLaunchBridge\0".as_ptr() as *const i8,
            std::ptr::null_mut(),
            bytes.as_ptr(),
            length,
        )
    };
    let pending = unsafe { exception_check(jni) } != 0;
    if pending {
        unsafe { exception_clear(jni) };
    }
    if class.is_null() || pending {
        log("DefineClass 辅助类失败（类可能已存在），放弃极速启动（走原生扫描）");
        return None;
    }
    log(&format!(
        "辅助类已定义到 JVM：pzm/fastlaunch/FastLaunchBridge（{} 字节）",
        bytes.len()
    ));
    Some(class as usize)
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn define_bridge_class(_env: usize) -> Option<usize> {
    None
}

/// 只在第一次失败时记日志：这个探测在轮询里会被反复调用，不能刷屏。
fn log_once(message: &str) {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log(message);
    }
}

/// 探测已经加载的 `jvm.dll` 并调用 `JNI_GetCreatedJavaVMs`，返回 (JavaVM 指针, 数量)。
///
/// ⚠️ 实现短路时**不要**在注入早期用 `FindClass("me/zed_0xff/zombie_buddy/PatchEngine")` 做版本探测。
/// JNI 没有"只加载不初始化"的入口：`FindClass` 会触发类初始化。若在 ZombieBuddy 自己的加载流程之前
/// 拉起 `PatchEngine`，它的 `<clinit>` 会在依赖尚未就绪时执行；一旦抛 `ExceptionInInitializerError`，
/// 该类被永久标记失败，ZombieBuddy 之后自己再用它就必然报错——那不是"回退原生扫描"，而是把加载器弄坏。
///
/// 安全替代（二选一）：① JVMTI `GetLoadedClasses` 只查**已加载**的类，等 ZombieBuddy 自己加载完
/// `PatchEngine` 再介入；② JVMTI `ClassFileLoadHook` 在它被加载的那一刻接管（时序天然正确，但要写字节码）。
/// 无论哪种都要先 `ExceptionCheck(228)`，失败就 `ExceptionClear(17)` 并整体放弃缓存。
///
/// 只做探测：不 attach、不取 JVMTI 环境、不改任何 Java 对象。任何一步拿不到就返回 None，
/// 调用方只会记日志（也就是"回退原生扫描"）。
#[cfg(windows)]
fn probe_jvm() -> Option<(usize, i32)> {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    type JniGetCreatedJavaVms =
        unsafe extern "system" fn(*mut *mut core::ffi::c_void, i32, *mut i32) -> i32;

    const JVM_DLL: &[u8] = b"jvm.dll\0";
    const GET_VMS: &[u8] = b"JNI_GetCreatedJavaVMs\0";

    let module = match unsafe { GetModuleHandleA(PCSTR::from_raw(JVM_DLL.as_ptr())) } {
        Ok(module) => module,
        Err(_) => {
            log_once("还没看到 jvm.dll：JVM 尚未加载（继续等）");
            return None;
        }
    };
    let Some(proc) = (unsafe { GetProcAddress(module, PCSTR::from_raw(GET_VMS.as_ptr())) }) else {
        log_once("jvm.dll 里找不到 JNI_GetCreatedJavaVMs：这不是标准 JVM，放弃极速启动");
        return None;
    };
    let get_vms: JniGetCreatedJavaVms = unsafe { std::mem::transmute(proc) };
    let mut vm: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut count: i32 = 0;
    let status = unsafe { get_vms(&mut vm, 1, &mut count) };
    if status != 0 {
        log_once(&format!("JNI_GetCreatedJavaVMs 返回错误码 {status}"));
        return None;
    }
    Some((vm as usize, count))
}

#[cfg(not(windows))]
fn probe_jvm() -> Option<(usize, i32)> {
    None
}

/// JVMTI 版本常量（`jvmti.h` 的 `JVMTI_VERSION_1_2`）。
#[cfg(windows)]
const JVMTI_VERSION_1_2: i32 = 0x3001_0200;
/// 更早的 JVMTI 版本，作为兜底候选。
const JVMTI_VERSION_1_1: i32 = 0x3001_0100;
const JVMTI_VERSION_1_0: i32 = 0x3001_0000;

/// 从 JVM 交给我们的环境指针里取函数表，再按槽位取函数指针。
///
/// **极速启动卡了很久的那个根因就是这里，三种指针的形态其实完全一样**：
/// `JavaVM` / `JNIEnv` / `jvmtiEnv` 在运行时都不是"指针即函数表"，而是
/// **一个结构的地址，结构的第一个字段才是函数表指针**：
///
/// ```c
/// struct JavaVM_  { const struct JNIInvokeInterface_  *functions; ... };
/// struct JNIEnv_  { const struct JNINativeInterface_  *functions; ... };
/// struct _jvmtiEnv{ const struct jvmtiInterface_1_    *functions; ... };
/// ```
///
/// （`jni.h` / `jvmti.h` 在 C++ 下把 `JavaVM`/`JNIEnv`/`jvmtiEnv` 分别 typedef 成上面这三个
/// **带 `functions` 字段的结构体**，而在 C 下 typedef 成"指向函数表的指针"——照 C 的签名去读
/// 就会少解一层。）
///
/// 所以三种环境一律是：`*(void**)base` 拿到函数表，再按槽位索引。
///
/// 实测证据：
/// * `JavaVM`：`*(vm)=0x00007ffb…e790`（jvm.dll 数据段 = 函数表）、`*(vm+8)=0`
///   —— 早期把它当函数表用，槽位 6 读到的是内存里紧邻的无关全局量并调用它，
///   于是 `GetEnv` 返回垃圾 `status=1376256 (0x150000)`、`AttachCurrentThread` 直接崩；
/// * `jvmtiEnv`：把它当函数表用，槽位 87（`GetVersionNumber`）读到的是 `_jvmtiEnv` 结构后面的
///   无关内存并调用它 —— 表现为 `Agent_OnLoad` 里直接卡死（JVM 再也起不来）。
#[cfg(windows)]
fn table_fn(base: usize, index: usize) -> *const core::ffi::c_void {
    if base == 0 {
        return std::ptr::null();
    }
    // 第一层：结构体的 `functions` 字段（才是真正的函数表）。
    let table = unsafe { *(base as *const *const usize) };
    if table.is_null() {
        return std::ptr::null();
    }
    // 第二层：按槽位取函数指针。
    unsafe { *table.add(index) as *const core::ffi::c_void }
}

/// `JavaVM` 函数表（`JNIInvokeInterface_`）的槽位下标，JNI 规范固定顺序、含 reserved 槽位。
#[cfg(windows)]
const JAVA_VM_ATTACH_CURRENT_THREAD: usize = 4;
#[cfg(windows)]
const JAVA_VM_DETACH_CURRENT_THREAD: usize = 5;
#[cfg(windows)]
const JAVA_VM_GET_ENV: usize = 6;

/// 取 `JavaVM` 函数表里的函数指针（形态说明见 `table_fn`）。
///
/// 另注：`GetEnv`/`AttachCurrentThread` 的第一个参数要传**原始的 `vm`**（即 `JavaVM *`，
/// HotSpot 自己的 C++ 实现也是 `functions->GetEnv(this, …)` 这样传的），不是 `*vm`。
#[cfg(windows)]
fn vm_fn(vm: usize, index: usize) -> *const core::ffi::c_void {
    table_fn(vm, index)
}

/// 调 `JavaVM` 槽位 6 的 `GetEnv`：`version` 传 JNI 版本号拿到 `JNIEnv`，
/// 传 `JVMTI_VERSION_*` 拿到 `jvmtiEnv`（两条支路是同一个函数，JVM 按版本号分发）。
///
/// 成功返回环境指针；任何失败只记日志并返回 `None`（调用方一律回退原生扫描）。
#[cfg(windows)]
fn java_vm_get_env(vm: usize, version: i32) -> Option<usize> {
    type GetEnv =
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut *mut core::ffi::c_void, i32) -> i32;
    let raw = vm_fn(vm, JAVA_VM_GET_ENV);
    if raw.is_null() {
        log("JavaVM 槽位 6（GetEnv）是空指针，放弃极速启动（走 ZombieBuddy 原生扫描）");
        return None;
    }
    let get_env: GetEnv = unsafe { std::mem::transmute(raw) };
    let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
    let status = unsafe { get_env(vm as *mut core::ffi::c_void, &mut env, version) };
    if status != 0 || env.is_null() {
        log(&format!(
            "GetEnv(version=0x{version:08x}) 失败：status={status} env=0x{:x}",
            env as usize
        ));
        return None;
    }
    Some(env as usize)
}

/// 调 `JavaVM` 槽位 4 的 `AttachCurrentThread`（第一个参数同样必须是原始 `vm`）。
/// 返回挂上来的 `JNIEnv`；失败返回 `None`。
#[cfg(windows)]
fn java_vm_attach_current_thread(vm: usize) -> Option<usize> {
    type AttachCurrentThread = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> i32;
    let raw = vm_fn(vm, JAVA_VM_ATTACH_CURRENT_THREAD);
    if raw.is_null() {
        log("JavaVM 槽位 4（AttachCurrentThread）是空指针");
        return None;
    }
    let attach: AttachCurrentThread = unsafe { std::mem::transmute(raw) };
    let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
    let status = unsafe { attach(vm as *mut core::ffi::c_void, &mut env, std::ptr::null_mut()) };
    if status != 0 || env.is_null() {
        log(&format!(
            "AttachCurrentThread 失败：status={status} env=0x{:x}",
            env as usize
        ));
        return None;
    }
    Some(env as usize)
}

/// 调 `JavaVM` 槽位 5 的 `DetachCurrentThread`；返回 JNI 状态码（非 0 只记日志，不影响回退）。
#[cfg(windows)]
fn java_vm_detach_current_thread(vm: usize) -> i32 {
    type DetachCurrentThread = unsafe extern "system" fn(*mut core::ffi::c_void) -> i32;
    let raw = vm_fn(vm, JAVA_VM_DETACH_CURRENT_THREAD);
    if raw.is_null() {
        return -1;
    }
    let detach: DetachCurrentThread = unsafe { std::mem::transmute(raw) };
    unsafe { detach(vm as *mut core::ffi::c_void) }
}

/// `JNI_VERSION_1_6`（`jni.h`）：`GetEnv` 取 `JNIEnv` 时用。
#[cfg(windows)]
const JNI_VERSION_1_6: i32 = 0x0001_0006;

/// 取**当前线程**的 `JNIEnv`。
///
/// 只适合线程**已经挂在 JVM 上**的场合（典型就是 `Agent_OnLoad`：它的调用线程本身就是 JVM
/// 的主线程）；远程注入那条路必须自己先 attach，不能走这里。
#[cfg(windows)]
fn java_vm_current_jni_env(vm: usize) -> Option<usize> {
    java_vm_get_env(vm, JNI_VERSION_1_6)
}

/// 按 `jvmti.h` 实证的下标取 JVMTI 函数表里的函数指针（形态与 `vm_fn`/`jni_fn` 一致，见 `table_fn`）。
#[cfg(windows)]
fn jvmti_fn(env: usize, index: usize) -> *const core::ffi::c_void {
    // 踩过的坑：`jvmti.h` 在 C 下把 `jvmtiEnv` typedef 成"指向函数表的指针"，
    // 但在 C++ 下是 `struct _jvmtiEnv { const struct jvmtiInterface_1_ *functions; … }`，
    // 而 JVM 给的是后者（`&_jvmti_external`）。照 C 的签名少解一层 → 索引落到 `_jvmtiEnv`
    // 结构后面的无关内存上，调用它 = 卡死或崩在 jvm.dll 内。
    table_fn(env, index)
}

/// JVMTI `GetVersionNumber`（jvmti.h 实证绝对下标 87，含 reserved 槽位）：确认环境真的能用，而不只是指针非空。
#[cfg(windows)]
fn probe_jvmti_version(env: usize) -> Option<i32> {
    if env == 0 {
        return None;
    }
    type GetVersionNumber = unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> i32;
    // 布局自检：`*(env)` 应当是 jvm.dll 里的函数表地址（`struct _jvmtiEnv` 的 `functions` 字段）。
    let table = unsafe { *(env as *const usize) };
    log(&format!(
        "JVMTI 布局自检：env=0x{env:x} *(env)=0x{table:x}（函数表）"
    ));
    let get_version: GetVersionNumber = unsafe { std::mem::transmute(jvmti_fn(env, 87)) };
    let mut version: i32 = 0;
    let status = unsafe { get_version(env as *mut core::ffi::c_void, &mut version) };
    if status != 0 {
        log(&format!("JVMTI GetVersionNumber 失败：status={status}"));
        return None;
    }
    Some(version)
}

#[cfg(not(windows))]
fn probe_jvmti_version(_env: usize) -> Option<i32> {
    None
}

/// 试着从 `JavaVM` 拿到 JVMTI 环境指针：拿到就说明这条注入路径可用。
/// 只取环境，不设置任何回调、不 retransform、不改字节码（那些是下一步）。
#[cfg(windows)]
fn probe_jvmti(vm: usize) -> Option<usize> {
    if vm == 0 {
        return None;
    }
    // attach 交给 jni crate：实测手写虚表会在 AttachCurrentThread 上抛 0xC0000005 崩进程，
    // 而 GetEnv（槽位 6）本身没问题，所以只把 attach 这一段换成经过验证的实现。
    let jvm = match unsafe { jni::JavaVM::from_raw(vm as *mut jni::sys::JavaVM) } {
        Ok(jvm) => jvm,
        Err(error) => {
            log(&format!("包装 JavaVM 失败: {error}"));
            return None;
        }
    };
    let guard = match jvm.attach_current_thread() {
        Ok(guard) => guard,
        Err(error) => {
            log(&format!("attach 线程失败: {error}"));
            return None;
        }
    };
    log("attach 成功（jni crate）：线程已挂到 JVM 上");

    // 诊断：用 jni crate 自己的 get_env（同样走槽位 6，但是 JNI 级）验证这个槽位到底能不能用。
    // 它能用 → 问题在 JVMTI 特有路径；它也不能用 → 问题在"取 env"这条调用本身。
    match jvm.get_env() {
        Ok(jni_env) => log(&format!(
            "诊断：jni crate 的 get_env（JNI 级）成功，env=0x{:x}",
            jni_env.get_raw() as usize
        )),
        Err(error) => log(&format!("诊断：jni crate 的 get_env（JNI 级）也失败: {error}")),
    }

    // 依次尝试多个 JVMTI 版本：不同 VM/版本对版本号的要求不完全一致，
    // 与其写死一个，不如按"从新到旧"试，第一个成功的就是要用的。
    let mut chosen: Option<usize> = None;
    for version in [JVMTI_VERSION_1_2, 0x3016_0000i32, JVMTI_VERSION_1_1, JVMTI_VERSION_1_0] {
        if let Some(env) = java_vm_get_env(vm, version) {
            log(&format!("GetEnv(JVMTI) version=0x{version:08x} 成功"));
            chosen = Some(env);
            break;
        }
    }
    let Some(env) = chosen else {
        log("所有 JVMTI 版本都拿不到环境，放弃极速启动（走 ZombieBuddy 原生扫描）");
        return None;
    };
    // guard 在这里 drop，等于显式 detach。
    drop(guard);
    Some(env)
}

#[cfg(not(windows))]
fn probe_jvmti(_vm: usize) -> Option<usize> {
    None
}

/// `JNIEnv` 函数表：JNI 规范固定顺序，`FindClass` 在下标 6。
///
/// 注意 `JNIEnv*` 是"指向函数表的指针的指针"（`const JNINativeInterface**`），
/// 所以要解一层引用才能拿到表；这是 JNI 最容易写错的地方。
#[cfg(windows)]
#[repr(C)]
struct JniEnvTable {
    reserved0: *const core::ffi::c_void,
    reserved1: *const core::ffi::c_void,
    reserved2: *const core::ffi::c_void,
    reserved3: *const core::ffi::c_void,
    get_version: *const core::ffi::c_void,
    define_class: *const core::ffi::c_void,
    find_class:
        unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void,
}

/// 按 `jni.h` 实证的下标取 JNIEnv 函数表里的函数指针（形态与 `vm_fn`/`jvmti_fn` 一致，见 `table_fn`）。
#[cfg(windows)]
fn jni_fn(env: usize, index: usize) -> *const core::ffi::c_void {
    table_fn(env, index)
}

/// JNI 的 `jvalue`（联合体，x64 上 8 字节）。这里只需要按指针传数组，
/// 用一个 8 字节的 `repr(C)` 结构体承载就够。
#[cfg(windows)]
#[repr(C)]
#[derive(Clone, Copy)]
struct JValue {
    value: i64,
}

/// 版本兼容性探测：确认 ZombieBuddy 的 `PatchEngine.collectPatches(String, ClassLoader)` 还在、签名没变。
///
/// 关键是 `Class.forName(name, false, loader)` 里的 **`initialize=false`**：只加载不初始化，
/// 所以**不会**触发 `PatchEngine` 的 `<clinit>`，正好规避 §6.3 记录的那个陷阱
/// （提前初始化 + `<clinit>` 失败 = 该类永久不可用 = 把加载器弄坏）。
///
/// 只查类与方法 ID，不调用 `collectPatches`、不改任何对象。
#[cfg(windows)]
fn probe_patch_engine_compat(env: usize) -> bool {
    // 下标全部来自本机 jni.h 的 JNINativeInterface_（见 docs/architecture.md §6.3）。
    const FIND_CLASS: usize = 6;
    const GET_STATIC_METHOD_ID: usize = 113;
    const CALL_STATIC_OBJECT_METHOD_A: usize = 116;
    const NEW_STRING_UTF: usize = 167;
    const EXCEPTION_CLEAR: usize = 17;
    const EXCEPTION_CHECK: usize = 228;

    type FindClass = unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void;
    type GetStaticMethodID = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const i8,
        *const i8,
    ) -> *mut core::ffi::c_void;
    type CallStaticObjectMethodA = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const JValue,
    ) -> *mut core::ffi::c_void;
    type NewStringUTF =
        unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void;
    type ExceptionCheck = unsafe extern "system" fn(*mut core::ffi::c_void) -> u8;
    type ExceptionClear = unsafe extern "system" fn(*mut core::ffi::c_void);

    let jni = env as *mut core::ffi::c_void;
    let find_class: FindClass = unsafe { std::mem::transmute(jni_fn(env, FIND_CLASS)) };
    let get_static_method_id: GetStaticMethodID =
        unsafe { std::mem::transmute(jni_fn(env, GET_STATIC_METHOD_ID)) };
    let call_static: CallStaticObjectMethodA =
        unsafe { std::mem::transmute(jni_fn(env, CALL_STATIC_OBJECT_METHOD_A)) };
    let new_string: NewStringUTF = unsafe { std::mem::transmute(jni_fn(env, NEW_STRING_UTF)) };
    let exception_check: ExceptionCheck =
        unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CHECK)) };
    let exception_clear: ExceptionClear = unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CLEAR)) };

    // 1) java.lang.Class.forName(String, boolean, ClassLoader)
    let class_class = unsafe { find_class(jni, b"java/lang/Class\0".as_ptr() as *const i8) };
    let loader_class =
        unsafe { find_class(jni, b"java/lang/ClassLoader\0".as_ptr() as *const i8) };
    if class_class.is_null() || loader_class.is_null() {
        log("找不到 java/lang/Class 或 java/lang/ClassLoader");
        unsafe { exception_clear(jni) };
        return false;
    }
    let for_name = unsafe {
        get_static_method_id(
            jni,
            class_class,
            b"forName\0".as_ptr() as *const i8,
            b"(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;\0".as_ptr() as *const i8,
        )
    };
    let system_loader = unsafe {
        get_static_method_id(
            jni,
            loader_class,
            b"getSystemClassLoader\0".as_ptr() as *const i8,
            b"()Ljava/lang/ClassLoader;\0".as_ptr() as *const i8,
        )
    };
    if for_name.is_null() || system_loader.is_null() {
        log("取 Class.forName / ClassLoader.getSystemClassLoader 的 MethodID 失败");
        unsafe { exception_clear(jni) };
        return false;
    }
    let loader =
        unsafe { call_static(jni, loader_class, system_loader, std::ptr::null::<JValue>()) };
    let name = unsafe {
        new_string(
            jni,
            b"me.zed_0xff.zombie_buddy.PatchEngine\0".as_ptr() as *const i8,
        )
    };
    if loader.is_null() || name.is_null() {
        log("构造系统类加载器或类名失败");
        unsafe { exception_clear(jni) };
        return false;
    }

    // 2) forName(name, /*initialize=*/false, loader)：只加载不初始化。
    let arguments = [
        JValue { value: name as i64 },
        JValue { value: 0 },
        JValue { value: loader as i64 },
    ];
    let patch_engine = unsafe { call_static(jni, class_class, for_name, arguments.as_ptr()) };
    let pending = unsafe { exception_check(jni) } != 0;
    if pending {
        unsafe { exception_clear(jni) };
    }
    if patch_engine.is_null() {
        log("ZombieBuddy 的 PatchEngine 类不可加载（未安装或改版），放弃极速启动（走原生扫描）");
        return false;
    }
    log("PatchEngine 可加载（未初始化）；开始核对 collectPatches 签名");

    // 3) 核对 collectPatches 的签名仍然一致。
    let collect_patches = unsafe {
        get_static_method_id(
            jni,
            patch_engine,
            b"collectPatches\0".as_ptr() as *const i8,
            b"(Ljava/lang/String;Ljava/lang/ClassLoader;)Ljava/util/List;\0".as_ptr() as *const i8,
        )
    };
    let pending = unsafe { exception_check(jni) } != 0;
    if pending {
        unsafe { exception_clear(jni) };
    }
    if collect_patches.is_null() || pending {
        log("collectPatches 签名不匹配（ZombieBuddy 改版），放弃极速启动（走原生扫描）");
        return false;
    }
    log("版本兼容性探测通过：PatchEngine.collectPatches(String, ClassLoader) 签名相符");
    PATCH_ENGINE_CLASS.store(patch_engine as usize, std::sync::atomic::Ordering::Release);
    true
}

#[cfg(not(windows))]
fn probe_patch_engine_compat(_env: usize) -> bool {
    false
}

/// 用**非可变参数**的 `CallStaticObjectMethodA`（jni.h 实证下标 116）调一个无害静态方法，
/// 证明"按 `jvalue` 数组调用"这条路径可用。
///
/// 为什么必须走 A 版：可变参数的 `CallStaticObjectMethod`（下标 114）在 Rust 里
/// **无法声明函数指针类型**（Rust 只允许在 extern 声明里写 `...`，不允许 `fn(...)` 类型），
/// 所以正式实现取 `Class.forName(name,false,loader)` 时也只能用 A 版。
///
/// 这个探测只碰 JDK 自带的 `java/lang/ClassLoader`，不涉及 ZombieBuddy。
#[cfg(windows)]
fn probe_static_call(env: usize) -> bool {
    type FindClass = unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void;
    type GetStaticMethodID = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const i8,
        *const i8,
    ) -> *mut core::ffi::c_void;
    type CallStaticObjectMethodA = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *const JValue,
    ) -> *mut core::ffi::c_void;
    type ExceptionCheck = unsafe extern "system" fn(*mut core::ffi::c_void) -> u8;
    type ExceptionClear = unsafe extern "system" fn(*mut core::ffi::c_void);

    // 下标来自本机 jni.h 的 JNINativeInterface_（见 docs/architecture.md §6.3）。
    const FIND_CLASS: usize = 6;
    const GET_STATIC_METHOD_ID: usize = 113;
    const CALL_STATIC_OBJECT_METHOD_A: usize = 116;
    const EXCEPTION_CLEAR: usize = 17;
    const EXCEPTION_CHECK: usize = 228;

    let jni = env as *mut core::ffi::c_void;
    let find_class: FindClass = unsafe { std::mem::transmute(jni_fn(env, FIND_CLASS)) };
    let get_static_method_id: GetStaticMethodID =
        unsafe { std::mem::transmute(jni_fn(env, GET_STATIC_METHOD_ID)) };
    let call_static: CallStaticObjectMethodA =
        unsafe { std::mem::transmute(jni_fn(env, CALL_STATIC_OBJECT_METHOD_A)) };
    let exception_check: ExceptionCheck =
        unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CHECK)) };
    let exception_clear: ExceptionClear = unsafe { std::mem::transmute(jni_fn(env, EXCEPTION_CLEAR)) };

    let class = unsafe { find_class(jni, b"java/lang/ClassLoader\0".as_ptr() as *const i8) };
    if class.is_null() {
        log("找不到 java/lang/ClassLoader");
        unsafe { exception_clear(jni) };
        return false;
    }
    let method = unsafe {
        get_static_method_id(
            jni,
            class,
            b"getSystemClassLoader\0".as_ptr() as *const i8,
            b"()Ljava/lang/ClassLoader;\0".as_ptr() as *const i8,
        )
    };
    if method.is_null() {
        log("取 ClassLoader.getSystemClassLoader() 的 MethodID 失败");
        unsafe { exception_clear(jni) };
        return false;
    }
    let loader = unsafe { call_static(jni, class, method, std::ptr::null()) };
    let pending = unsafe { exception_check(jni) } != 0;
    if pending {
        unsafe { exception_clear(jni) };
    }
    if loader.is_null() || pending {
        log("调用 getSystemClassLoader() 失败");
        return false;
    }
    // 本地引用不显式 Delete：探测结束后同一个 attach 窗口就会 detach，JVM 会回收。
    log("JNI 静态调用路径可用（CallStaticObjectMethodA/116 + jvalue 数组）");
    true
}

#[cfg(not(windows))]
fn probe_static_call(_env: usize) -> bool {
    false
}

/// 用 `FindClass` 找一个**无害**的 JDK 类，验证 JNIEnv 表布局可用。
///
/// 刻意不在这里 `FindClass("me/zed_0xff/zombie_buddy/PatchEngine")`：那会在 ZombieBuddy
/// 自己加载之前把它的类触发加载，可能影响它的初始化顺序。等接入短路逻辑时再查它，
/// 并且那一步必须配 `ExceptionCheck`。
#[cfg(windows)]
fn probe_jni_env(env: usize) -> bool {
    if env == 0 {
        return false;
    }
    const CLASS_NAME: &[u8] = b"java/lang/String\0";
    let functions: &JniEnvTable = unsafe { &**(env as *const *const JniEnvTable) };
    let found = unsafe {
        (functions.find_class)(
            env as *mut core::ffi::c_void,
            CLASS_NAME.as_ptr() as *const i8,
        )
    };
    !found.is_null()
}

#[cfg(not(windows))]
fn probe_jni_env(_env: usize) -> bool {
    false
}

/// 试着 attach 当前线程拿到 `JNIEnv`（反射注入的前置），成功后立刻 detach，
/// 不给 JVM 留一个"一直挂着"的原生线程。全过程只读，不改任何 Java 对象。
#[cfg(windows)]
fn probe_attach(vm: usize) -> bool {
    if vm == 0 {
        return false;
    }
    let Some(env) = java_vm_attach_current_thread(vm) else {
        return false;
    };
    log(&format!(
        "JNIEnv 已取得=0x{env:x}（只读探测，随后 detach）"
    ));
    let jni_ok = probe_jni_env(env);
    let detach_status = java_vm_detach_current_thread(vm);
    if detach_status != 0 {
        log(&format!("DetachCurrentThread 返回 {detach_status}（不影响回退）"));
    }
    if !jni_ok {
        log("JNIEnv 函数表探测失败（FindClass 返回空），放弃极速启动（走 ZombieBuddy 原生扫描）");
        return false;
    }
    if !probe_static_call(env) {
        log("JNI 静态调用探测失败，放弃极速启动（走 ZombieBuddy 原生扫描）");
        return false;
    }
    probe_patch_engine_compat(env)
}

#[cfg(not(windows))]
fn probe_attach(_vm: usize) -> bool {
    false
}

/// 注入线程主体：等缓存出现 → 校验 → 记录结论。
///
/// 现在到这里就结束（安全空转）。下一步在这里接 JNI：拿到 `JavaVM` 后
/// `AttachCurrentThread`，反射探测 `me.zed_0xff.zombie_buddy.PatchEngine` 是否存在
/// `collectPatches(String, ClassLoader)`，再把缓存的 `patchClasses` 交给它，
/// 全程 `ExceptionCheck` + catch-all，异常即回退。
/// JVMTI 绝对下标（`jvmti.h` 实证，见 docs/architecture.md §6.3）。
const JVMTI_SET_EVENT_NOTIFICATION_MODE: usize = 1;
const JVMTI_ALLOCATE: usize = 45;
const JVMTI_SET_EVENT_CALLBACKS: usize = 121;
const JVMTI_ADD_CAPABILITIES: usize = 141;
const JVMTI_RETRANSFORM_CLASSES: usize = 151;
/// `jvmtiEventCallbacks` 里 `ClassFileLoadHook` 的槽位（该结构共 39 槽）。
const CALLBACK_SLOT_CLASS_FILE_LOAD_HOOK: usize = 4;
/// `jvmtiEventCallbacks` 里 `VMInit` 的槽位（jvmti.h 实证：0 号槽）。
const CALLBACK_SLOT_VM_INIT: usize = 0;
/// `jvmtiEventCallbacks` 的总槽位数（jvmti.h 实证 39）。
const CALLBACK_SLOT_COUNT: usize = 39;
/// `can_retransform_classes` 的位：45 位的能力结构 = 2 个 u32，它在第二个字的 bit 5。
const CAPABILITY_RETRANSFORM_WORD: usize = 1;
const CAPABILITY_RETRANSFORM_BIT: u32 = 5;
/// `JVMTI_EVENT_CLASS_FILE_LOAD_HOOK` 与 `JVMTI_ENABLE`。
const JVMTI_EVENT_CLASS_FILE_LOAD_HOOK: i32 = 54;
/// `JVMTI_EVENT_VM_INIT`（jvmti.h 实证 = 50）。
const JVMTI_EVENT_VM_INIT: i32 = 50;
const JVMTI_ENABLE: i32 = 1;
/// 回调要处理的目标类（内部名，带 `/`）。
const PATCH_ENGINE_INTERNAL_NAME: &str = "me/zed_0xff/zombie_buddy/PatchEngine";

/// 改写好的 `PatchEngine` 字节，回调里取用（`OnceLock` 保证只写一次）。
static WRAPPED_PATCH_ENGINE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
/// 本进程是否已经通过 `-agentpath` 以 JVMTI agent 身份加载了本 DLL（`Agent_OnLoad` 里置位）。
/// 置位后远程注入路径（`run()` 的后半段）整体跳过：agent 路径时序更早、且不需要 attach。
static AGENT_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// `PatchEngine` 是否已经在**加载时**被改写（回调里置位）。
/// 用来区分"hook 没赶上那次定义"和"这个类还没被加载"——排查时可一眼看出是哪种。
static PATCH_ENGINE_HOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 类加载 hook 被调用的次数（第一次调用记一行日志，确认 hook 在这个进程里真的生效）。
static HOOK_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// 回调里要用的 JVMTI 环境（回调由 JVM 调用，拿不到外部参数，只能放静态）。
static INJECT_JVMTI_ENV: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// 兼容性探测拿到的 `PatchEngine` jclass（投送时 retransform 用）。
static PATCH_ENGINE_CLASS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `VMInit` 回调：JVM 起来后第一个"能安全用 JNI"的时机 —— 只用来**把辅助类送进 JVM**。
///
/// 类加载 hook 早在 `Agent_OnLoad` 里就打开了（见那里的注释），所以这里即使失败也**不关闭**它：
/// 包装体自带 `catch (Throwable)` 兜底，辅助类缺席时全部调用都走 ZombieBuddy 的原实现。
///
/// 为什么非要绕这一下：`Agent_OnLoad` 的调用线程在 JDK 25 上**还不是 Java 线程**
/// （实测在 onload 阶段 `GetEnv(vm, JNI_VERSION_1_6)` 返回 `JNI_EDETACHED = -2`），
/// 而定义辅助类必须要有 `JNIEnv`（JVMTI 没有 DefineClass）。
#[cfg(windows)]
unsafe extern "system" fn vm_init_hook(
    _jvmti_env: *mut core::ffi::c_void,
    jni_env: *mut core::ffi::c_void,
    _thread: *mut core::ffi::c_void,
) {
    static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    log("VMInit 回调：JVM 已就绪（本线程已是 Java 线程），开始定义辅助类");
    if jni_env.is_null() {
        log("VMInit 回调没给 JNIEnv：辅助类跳过（包装体全部回退原实现，功能不生效但不报错）");
        return;
    }
    if std::env::var("PZM_FAST_LAUNCH_SKIP_BRIDGE").as_deref() == Ok("1") {
        // 只给验证用：故意不定义辅助类，检查包装体的兜底是否真的走原实现。
        log("设置了 PZM_FAST_LAUNCH_SKIP_BRIDGE=1：故意跳过辅助类定义（验证兜底路径）");
        return;
    }
    if define_bridge_class(jni_env as usize).is_none() {
        log("VMInit：辅助类定义失败（包装体仍会回退原实现，功能不生效但不报错）");
        return;
    }
    if PATCH_ENGINE_HOOKED.load(std::sync::atomic::Ordering::Acquire) {
        log("VMInit：PatchEngine 已经在加载时改写完成，辅助类就位 → 缓存路径即刻生效");
    } else {
        log("VMInit：辅助类就位；PatchEngine 还没被定义，等类加载 hook 接管");
    }
}

#[cfg(not(windows))]
unsafe extern "system" fn vm_init_hook(
    _jvmti_env: *mut core::ffi::c_void,
    _jni_env: *mut core::ffi::c_void,
    _thread: *mut core::ffi::c_void,
) {
}

/// `ClassFileLoadHook` 回调：目标类**首次加载**时把 `collectPatches` 包一层，其余一律放行。
///
/// 放行 = 交回原字节 = 原生行为；这里任何一步失败都只是"放行"，所以失败即回退是天然成立的。
#[cfg(windows)]
unsafe extern "system" fn class_file_load_hook(
    jvmti_env: *mut core::ffi::c_void,
    _jni_env: *mut core::ffi::c_void,
    class_being_redefined: *mut core::ffi::c_void,
    _loader: *mut core::ffi::c_void,
    name: *const i8,
    _protection_domain: *mut core::ffi::c_void,
    _class_data_len: i32,
    _class_data: *const u8,
    new_class_data_len: *mut i32,
    new_class_data: *mut *mut u8,
) {
    // 注意：首次加载时 class_being_redefined 是空的——只有 retransform 才会填它。
    // 所以这里只按类名匹配，不能要求它非空。
    let _ = class_being_redefined;
    if name.is_null() || new_class_data.is_null() || new_class_data_len.is_null() {
        return;
    }
    let Ok(target) = std::ffi::CStr::from_ptr(name).to_str() else {
        return;
    };
    // 第一行日志用来确认 hook 在这个进程里真的被调用（排查"没生效"时最有用的一条证据）。
    if HOOK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
        log(&format!("ClassFileLoadHook 首次被调用（首个类={target}）"));
    }
    if target != PATCH_ENGINE_INTERNAL_NAME {
        return;
    }
    if _class_data.is_null() {
        log("ClassFileLoadHook：目标类没有原始字节（早期 hook 事件），放行");
        return;
    }
    // 就地改写：JVMTI 把"当前字节"交给我们（class_data），我们包一层后交回去——
    // 既不用自己去找 class 文件，也保证改的正是 VM 正在转换的那一份。
    let original =
        unsafe { std::slice::from_raw_parts(_class_data, _class_data_len.max(0) as usize) };
    // 幂等：已经包过一次就不再包（再包会让 `collectPatches$pzm` 撞名 → ClassFormatError）。
    if crate::classfile::declares_method(original, "collectPatches$pzm") {
        PATCH_ENGINE_HOOKED.store(true, std::sync::atomic::Ordering::Release);
        log("ClassFileLoadHook：PatchEngine 已经是包装过的形态，放行（幂等）");
        return;
    }
    let wrapped = match crate::classfile::wrap_collect_patches(
        original,
        "pzm/fastlaunch/FastLaunchBridge",
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            log(&format!(
                "ClassFileLoadHook：改写失败，放行原字节（回退原生实现）: {error}"
            ));
            return;
        }
    };

    // JVMTI 要求输出缓冲由它分配（VM 之后会用 Deallocate 释放）。
    type Allocate =
        unsafe extern "system" fn(*mut core::ffi::c_void, i64, *mut *mut u8) -> i32;
    let allocate: Allocate =
        unsafe { std::mem::transmute(jvmti_fn(jvmti_env as usize, JVMTI_ALLOCATE)) };
    let mut buffer: *mut u8 = std::ptr::null_mut();
    let status = unsafe { allocate(jvmti_env, wrapped.len() as i64, &mut buffer) };
    if status != 0 || buffer.is_null() {
        log("ClassFileLoadHook：分配输出缓冲失败，放行原字节（回退原生实现）");
        return;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(wrapped.as_ptr(), buffer, wrapped.len());
        *new_class_data = buffer;
        *new_class_data_len = wrapped.len() as i32;
    }
    PATCH_ENGINE_HOOKED.store(true, std::sync::atomic::Ordering::Release);
    log(&format!(
        "ClassFileLoadHook：已提交改写后的 PatchEngine（{} -> {} 字节，短路生效）",
        original.len(),
        wrapped.len()
    ));
}

#[cfg(not(windows))]
unsafe extern "system" fn class_file_load_hook(
    _a: *mut core::ffi::c_void,
    _b: *mut core::ffi::c_void,
    _c: *mut core::ffi::c_void,
    _d: *mut core::ffi::c_void,
    _e: *const i8,
    _f: *mut core::ffi::c_void,
    _g: i32,
    _h: *const u8,
    _i: *mut i32,
    _j: *mut *mut u8,
) {
}

/// 投送：申请能力 → 注册回调 → 打开事件 → retransform 目标类。
#[cfg(windows)]
#[allow(dead_code)] // 启动接线时调用
fn inject_patch_engine(
    jvmti: usize,
    patch_engine_class: usize,
    wrapped: Vec<u8>,
) -> Result<(), String> {
    // 1) 能力：只申请 can_retransform_classes。
    type AddCapabilities = unsafe extern "system" fn(*mut core::ffi::c_void, *const u32) -> i32;
    let add_capabilities: AddCapabilities =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_ADD_CAPABILITIES)) };
    let mut capabilities = [0u32; 2];
    capabilities[CAPABILITY_RETRANSFORM_WORD] |= 1u32 << CAPABILITY_RETRANSFORM_BIT;
    let status = unsafe { add_capabilities(jvmti as *mut core::ffi::c_void, capabilities.as_ptr()) };
    if status != 0 {
        return Err(format!("AddCapabilities 失败: {status}"));
    }

    // 2) 回调：39 槽的结构体只填 ClassFileLoadHook 那一格。
    let _ = WRAPPED_PATCH_ENGINE.set(wrapped);
    INJECT_JVMTI_ENV.store(jvmti, std::sync::atomic::Ordering::Release);
    type SetEventCallbacks = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const *const core::ffi::c_void,
        i32,
    ) -> i32;
    let set_event_callbacks: SetEventCallbacks =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_SET_EVENT_CALLBACKS)) };
    let mut callbacks = [std::ptr::null::<core::ffi::c_void>(); 39];
    callbacks[CALLBACK_SLOT_CLASS_FILE_LOAD_HOOK] = class_file_load_hook as *const core::ffi::c_void;
    let status = unsafe {
        set_event_callbacks(
            jvmti as *mut core::ffi::c_void,
            callbacks.as_ptr(),
            std::mem::size_of_val(&callbacks) as i32,
        )
    };
    if status != 0 {
        return Err(format!("SetEventCallbacks 失败: {status}"));
    }

    // 3) 打开 CLASS_FILE_LOAD_HOOK 事件。
    type SetEventNotificationMode = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        i32,
        *mut core::ffi::c_void,
    ) -> i32;
    let set_mode: SetEventNotificationMode =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_SET_EVENT_NOTIFICATION_MODE)) };
    let status = unsafe {
        set_mode(
            jvmti as *mut core::ffi::c_void,
            JVMTI_ENABLE,
            JVMTI_EVENT_CLASS_FILE_LOAD_HOOK,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(format!("SetEventNotificationMode 失败: {status}"));
    }

    // 4) 提交 retransform：回调会在这次调用里拿到目标类并要求换字节。
    type RetransformClasses = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        i32,
        *const *mut core::ffi::c_void,
    ) -> i32;
    let retransform: RetransformClasses =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_RETRANSFORM_CLASSES)) };
    let classes = [patch_engine_class as *mut core::ffi::c_void];
    let status = unsafe { retransform(jvmti as *mut core::ffi::c_void, 1, classes.as_ptr()) };
    if status != 0 {
        return Err(format!("RetransformClasses 失败: {status}"));
    }
    Ok(())
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn inject_patch_engine(
    _jvmti: usize,
    _patch_engine_class: usize,
    _wrapped: Vec<u8>,
) -> Result<(), String> {
    Err("只支持 Windows".to_string())
}

/// 投送：在 attach 出来的 JNIEnv 窗口里定义辅助类，然后 retransform 目标类。
///
/// 任何失败都返回 Err，调用方只记日志——此时还没有改变游戏进程的任何行为，
/// 所以"投送失败 = 原生扫描"是天然成立的。
#[cfg(windows)]
fn deliver_fast_launch(vm: usize, jvmti: usize) -> Result<(), String> {
    if vm == 0 || jvmti == 0 {
        return Err("缺少 JavaVM 或 JVMTI 环境".to_string());
    }
    let Some(env) = java_vm_attach_current_thread(vm) else {
        return Err("AttachCurrentThread 失败".to_string());
    };
    let outcome = (|| -> Result<(), String> {
        // 先把辅助类送进去：PatchEngine 的包装方法体会 invokestatic 调它。
        define_bridge_class(env).ok_or_else(|| "DefineClass 辅助类失败".to_string())?;
        let class = PATCH_ENGINE_CLASS.load(std::sync::atomic::Ordering::Acquire);
        if class == 0 {
            return Err("拿不到 PatchEngine 的 jclass".to_string());
        }
        inject_patch_engine(jvmti, class, Vec::new())
    })();
    let detach_status = java_vm_detach_current_thread(vm);
    if detach_status != 0 {
        log(&format!("DetachCurrentThread 返回 {detach_status}（不影响投送结果）"));
    }
    outcome
}

#[cfg(not(windows))]
fn deliver_fast_launch(_vm: usize, _jvmti: usize) -> Result<(), String> {
    Err("只支持 Windows".to_string())
}

/// JVMTI agent 入口：以 `-agentpath:<DLL>` 启动时由 JVM 自己调用。
///
/// 为什么必须走这条路：JDK 9+ 下 `GetEnv(JVMTI_VERSION)` 只对**以 agent 身份加载**的调用者
/// 返回环境，远程注入进程拿不到（实测：同槽位传 JNI 版本号成功、传 JVMTI 版本号失败）。
/// 详见 docs/fast-launch-status.md §6.0。
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Agent_OnLoad(
    vm: *mut jni::sys::JavaVM,
    options: *mut i8,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    let options_text = if options.is_null() {
        String::new()
    } else {
        unsafe { std::ffi::CStr::from_ptr(options) }
            .to_string_lossy()
            .to_string()
    };
    // 带上 pid：日志是多个进程共用一个文件的，排查时必须能区分"同一个进程被加载两次"
    // 和"两个进程各加载一次"（前者会让 `DefineClass`/包装走第二遍，必须能一眼看出来）。
    log(&format!(
        "Agent_OnLoad 被 JVM 调用（pid={} options={options_text}）",
        std::process::id()
    ));
    // 让 DllMain 里起的 `run()` 线程知道"本进程已经由 -agentpath 接管"，
    // 免得它再走一遍远程注入路径（GetCreatedJavaVMs + attach + retransform）造成重复投送。
    AGENT_LOADED.store(true, std::sync::atomic::Ordering::Release);
    // 应急关闭开关（agent 路径已经在一次性 JVM 上验证过，所以不再要求"开发期显式打开"：
    // 门禁就是设置里的「极速启动」开关 + 方案必须是 ZombieBuddy，见 `start_agent_injection`；
    // 但保留一个环境变量作为"立刻让它闭嘴"的兜底手段）。
    if std::env::var("PZM_FAST_LAUNCH_DISABLE").as_deref() == Ok("1") {
        log("设置了 PZM_FAST_LAUNCH_DISABLE=1：agent 不做任何注入（应急关闭）");
        return 0; // JNI_OK
    }
    start_agent_injection(vm as usize);
    0 // JNI_OK
}

#[cfg(not(windows))]
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn Agent_OnLoad(
    _vm: *mut core::ffi::c_void,
    _options: *mut i8,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    0
}

/// 以 agent 身份**直接**取 JVMTI 环境：agent 的调用线程本来就是 JVM 线程，不需要（也不应该）attach
/// —— 这里只调 `GetEnv`（槽位 6），不碰 attach/detach。
#[cfg(windows)]
fn jvmti_env_direct(vm: usize) -> Option<usize> {
    if vm == 0 {
        return None;
    }
    // 布局自检（保留一行，防止再犯"把 JavaVM 当函数表"的错）：
    // `vm+0x00` 必须是 jvm.dll 数据段里的函数表地址，`vm+0x08` 必须是 0。
    let head = unsafe { *(vm as *const usize) };
    let next = unsafe { *((vm + std::mem::size_of::<usize>()) as *const usize) };
    log(&format!(
        "JavaVM 布局自检：vm=0x{vm:x} *(vm)=0x{head:x}（函数表） *(vm+8)=0x{next:x}"
    ));
    for version in [JVMTI_VERSION_1_2, 0x3016_0000i32, JVMTI_VERSION_1_1, JVMTI_VERSION_1_0] {
        if let Some(env) = java_vm_get_env(vm, version) {
            log(&format!("agent GetEnv(JVMTI) version=0x{version:08x} 成功"));
            return Some(env);
        }
    }
    None
}

/// agent 侧装配：门禁 → 缓存校验 → 取 JVMTI 环境 → 申请能力 → 注册回调 → 打开事件。
///
/// 之后**不做 retransform**：靠 `ClassFileLoadHook` 在 ZombieBuddy 自己加载 `PatchEngine` 时
/// 就地替换字节码。这样既不需要 attach、也不需要提前把类拉起来（避免触发 `<clinit>`）。
#[cfg(windows)]
fn start_agent_injection(vm: usize) {
    if vm == 0 {
        log("Agent_OnLoad 没有给 JavaVM，放弃");
        return;
    }
    if !fast_launch_enabled_in_settings() {
        log("极速启动未开启或方案不是 ZombieBuddy，跳过（走 ZombieBuddy 原生扫描）");
        return;
    }
    let Some(path) = cache_path() else {
        log("拿不到缓存路径，放弃");
        return;
    };
    let Some(check) = read_and_check_cache(&path) else {
        log("预扫描缓存不可用，放弃极速启动（走 ZombieBuddy 原生扫描）");
        return;
    };
    log(&format!(
        "缓存可用：generatedAt={} mods={} modSetHash={}",
        check.generated_at, check.mod_count, check.mod_set_hash
    ));

    let Some(jvmti) = jvmti_env_direct(vm) else {
        log("以 agent 身份仍拿不到 JVMTI 环境，放弃极速启动");
        return;
    };
    if let Some(version) = probe_jvmti_version(jvmti) {
        log(&format!("JVMTI 可用：version=0x{version:x}"));
    }

    // 辅助类必须**先**定义好：被包装的 `collectPatches` 方法体会 `invokestatic` 调它，
    // 一旦 PatchEngine 先被加载（包装体一执行就要解析这个方法引用），没有它就是 NoClassDefFoundError。
    //
    // 但 onload 阶段**还拿不到 JNIEnv**：实测 `GetEnv(vm, JNI_VERSION_1_6)` 返回 `-2`（JNI_EDETACHED，
    // 说明 `Agent_OnLoad` 的调用线程这时还不是 Java 线程；旧版本把这当成"手写虚表崩了"，
    // 其实是上一处函数表解引用写错导致的误判）。所以这里只做"能力 + 回调 + 打开 VMInit"，
    // 真正的定义辅助类 + 打开类加载 hook 放到 `vm_init_hook` 里做——那时回调会直接给 JNIEnv。
    match java_vm_current_jni_env(vm) {
        Some(env) => log(&format!("Agent_OnLoad 阶段也拿到了 JNIEnv=0x{env:x}（少见，直接可用）")),
        None => log(
            "Agent_OnLoad 阶段没有 JNIEnv（onload 阶段线程尚未 attach），辅助类改到 VMInit 回调里定义",
        ),
    }

    // 能力：can_generate_all_class_hook_events(bit 26) + can_retransform_classes(bit 37)
    type AddCapabilities = unsafe extern "system" fn(*mut core::ffi::c_void, *const u32) -> i32;
    let add_capabilities: AddCapabilities =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_ADD_CAPABILITIES)) };
    let mut capabilities = [0u32; 2];
    capabilities[0] |= 1u32 << 26;
    capabilities[CAPABILITY_RETRANSFORM_WORD] |= 1u32 << CAPABILITY_RETRANSFORM_BIT;
    log("步骤：AddCapabilities（can_generate_all_class_hook_events + can_retransform_classes）");
    let status = unsafe { add_capabilities(jvmti as *mut core::ffi::c_void, capabilities.as_ptr()) };
    if status != 0 {
        log(&format!("AddCapabilities 失败: {status}，放弃极速启动"));
        return;
    }
    log("AddCapabilities 成功");

    // 回调：39 槽的结构体填 VMInit(0 号槽) + ClassFileLoadHook(4 号槽)。
    // ClassFileLoadHook 先只登记**不打开**，等 VMInit 里辅助类定义成功后再打开。
    type SetEventCallbacks = unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *const *const core::ffi::c_void,
        i32,
    ) -> i32;
    let set_event_callbacks: SetEventCallbacks =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_SET_EVENT_CALLBACKS)) };
    let mut callbacks = [std::ptr::null::<core::ffi::c_void>(); CALLBACK_SLOT_COUNT];
    callbacks[CALLBACK_SLOT_VM_INIT] = vm_init_hook as *const core::ffi::c_void;
    callbacks[CALLBACK_SLOT_CLASS_FILE_LOAD_HOOK] = class_file_load_hook as *const core::ffi::c_void;
    log("步骤：SetEventCallbacks（VMInit + ClassFileLoadHook）");
    let status = unsafe {
        set_event_callbacks(
            jvmti as *mut core::ffi::c_void,
            callbacks.as_ptr(),
            std::mem::size_of_val(&callbacks) as i32,
        )
    };
    if status != 0 {
        log(&format!("SetEventCallbacks 失败: {status}，放弃极速启动"));
        return;
    }
    log("SetEventCallbacks 成功");

    // **同时立刻打开类加载 hook**（不等 VMInit）：ZombieBuddy 自己的 agent 会在它的
    // `Agent_OnLoad` 里就把 `PatchEngine` 加载好，而 `VMInit` 在所有 agent 的 onload 之后才发。
    // 所以必须一到手就打开；此时辅助类还没送进 JVM（onload 阶段拿不到 JNIEnv），
    // 中间的 `collectPatches` 调用靠包装体自带的 `catch (Throwable)` 回退原实现。
    type SetEventNotificationMode =
        unsafe extern "system" fn(*mut core::ffi::c_void, i32, i32, *mut core::ffi::c_void) -> i32;
    let set_mode: SetEventNotificationMode =
        unsafe { std::mem::transmute(jvmti_fn(jvmti, JVMTI_SET_EVENT_NOTIFICATION_MODE)) };
    log("步骤：SetEventNotificationMode(JVMTI_ENABLE, CLASS_FILE_LOAD_HOOK)");
    let status = unsafe {
        set_mode(
            jvmti as *mut core::ffi::c_void,
            JVMTI_ENABLE,
            JVMTI_EVENT_CLASS_FILE_LOAD_HOOK,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        log(&format!(
            "SetEventNotificationMode(CLASS_FILE_LOAD_HOOK) 失败: {status}，放弃极速启动"
        ));
        return;
    }
    log("类加载 hook 已打开（PatchEngine 一被定义就会改写）");

    // VMInit 只用来"把辅助类送进 JVM"（onload 阶段没有 JNIEnv）。
    log("步骤：SetEventNotificationMode(JVMTI_ENABLE, VM_INIT)");
    let status = unsafe {
        set_mode(
            jvmti as *mut core::ffi::c_void,
            JVMTI_ENABLE,
            JVMTI_EVENT_VM_INIT,
            std::ptr::null_mut(),
        )
    };
    if status != 0 {
        log(&format!(
            "SetEventNotificationMode(VM_INIT) 失败: {status}（类加载 hook 已打开，继续）"
        ));
    }
    // 回调里还要用（回调拿不到外部参数，只能放静态）。
    INJECT_JVMTI_ENV.store(jvmti, std::sync::atomic::Ordering::Release);
    log("等 VMInit 回调：在那里把辅助类送进 JVM（之前若有 collectPatches 调用，走原实现）");
}

pub(crate) fn run() {
    // 门禁先行：开关没开、或方案不是 ZombieBuddy 就直接退出，连缓存都不看。
    if !fast_launch_enabled_in_settings() {
        log("极速启动未开启或方案不是 ZombieBuddy，跳过（走 ZombieBuddy 原生扫描）");
        return;
    }
    log("极速启动模块启动：等待预扫描缓存…");
    let Some(path) = cache_path() else {
        log("拿不到 USERPROFILE，放弃极速启动（走 ZombieBuddy 原生扫描）");
        return;
    };

    let deadline = SystemTime::now() + WAIT_TIMEOUT;
    loop {
        if path.is_file() {
            break;
        }
        if SystemTime::now() >= deadline {
            log(&format!(
                "等待缓存超时（{}），放弃极速启动（走原生扫描）",
                path.display()
            ));
            return;
        }
        std::thread::sleep(WAIT_INTERVAL);
    }

    match read_and_check_cache(&path) {
        Some(check) => log(&format!(
            "缓存可用：generatedAt={} mods={} modSetHash={}（注入逻辑尚未接入，本次不改动游戏进程）",
            check.generated_at, check.mod_count, check.mod_set_hash
        )),
        None => log("缓存校验未通过，放弃极速启动（走 ZombieBuddy 原生扫描）"),
    }

    // ⚠️ 开发开关（重要）：投送路径仍在调试。
    // 为了不让玩家在正常启动时踩到，默认**不执行**投送；设 PZM_FAST_LAUNCH_INJECT=1 才继续。
    // 门禁与缓存校验照常执行并记日志，所以设置页/日志仍然可用。
    if std::env::var("PZM_FAST_LAUNCH_INJECT").as_deref() != Ok("1") {
        log("投送路径处于调试状态（未设置 PZM_FAST_LAUNCH_INJECT=1）：只做门禁与缓存校验，不注入");
        return;
    }
    if AGENT_LOADED.load(std::sync::atomic::Ordering::Acquire) {
        log("本进程已由 -agentpath 以 JVMTI agent 身份加载本 DLL：跳过远程注入路径（避免重复投送）");
        return;
    }

    // 等 JVM 起来并拿到 JavaVM：这是下一步反射注入的前置。本轮只记录结果，不改进程状态。
    let jvm_deadline = SystemTime::now() + WAIT_TIMEOUT;
    loop {
        // agent 路径（时序更早、无需 attach）一旦接管，这里就整体让路。
        if AGENT_LOADED.load(std::sync::atomic::Ordering::Acquire) {
            log("本进程已由 -agentpath 以 JVMTI agent 身份加载本 DLL：跳过远程注入路径（避免重复投送）");
            return;
        }
        if let Some((vm, count)) = probe_jvm() {
            match probe_jvmti(vm) {
                Some(env) => {
                    log(&format!(
                        "JVM 已就绪：JavaVM=0x{vm:x} 已创建数={count}，JVMTI 环境=0x{env:x}"
                    ));
                    // 先确认 JVMTI 环境真能用（下标 85 = GetVersionNumber），再谈 JNIEnv。
                    match probe_jvmti_version(env) {
                        Some(version) => log(&format!("JVMTI 可用：version=0x{version:x}")),
                        None => {
                            log("JVMTI 版本查询失败，放弃极速启动（走 ZombieBuddy 原生扫描）");
                            return;
                        }
                    }
                    // 反射注入的前置：能不能 attach 出 JNIEnv（只探测，随即 detach）。
                    if !probe_attach(vm) {
                        log("拿不到 JNIEnv，放弃极速启动（走 ZombieBuddy 原生扫描）");
                    } else {
                        match deliver_fast_launch(vm, env) {
                            Ok(()) => log(
                                "极速启动已投送：PatchEngine 的 ClassGraph 扫描入口已被接管（失败会自动回退原生实现）",
                            ),
                            Err(error) => log(&format!(
                                "投送失败，放弃极速启动（走 ZombieBuddy 原生扫描）: {error}"
                            )),
                        }
                    }
                }
                None => log(&format!(
                    "JVM 已就绪（JavaVM=0x{vm:x}）但拿不到 JVMTI 环境，放弃极速启动（走 ZombieBuddy 原生扫描）"
                )),
            }
            return;
        }
        if SystemTime::now() >= jvm_deadline {
            log("等待 JVM 超时，放弃极速启动（走 ZombieBuddy 原生扫描）");
            return;
        }
        std::thread::sleep(WAIT_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_settings_and_cache_fields() {
        let settings =
            r#"{ "fastLaunch": true, "modLoaderScheme": "zombieBuddy", "fontContrast": 50 }"#;
        assert_eq!(json_bool_field(settings, "fastLaunch"), Some(true));
        assert_eq!(
            json_string_field(settings, "modLoaderScheme").as_deref(),
            Some("zombieBuddy")
        );
        assert_eq!(json_number_field(settings, "fontContrast"), Some(50));
        // 缺字段 / 字段类型对不上都要返回 None，绝不能默认成"已开启"。
        assert_eq!(json_bool_field(settings, "missing"), None);
        assert_eq!(json_string_field("{}", "modLoaderScheme"), None);
        assert_eq!(json_bool_field(r#"{ "fastLaunch": "yes" }"#, "fastLaunch"), None);
    }

    #[test]
    fn validates_prescan_cache() {
        let cache = r#"{
  "format": 1,
  "generatedAt": 1791908270,
  "modSetHash": "087f9589016fc4e8",
  "mods": [
    { "modName": "Viewpoint", "jarPath": "H:/a.jar", "basePackage": "viewpoint", "jarHash": "h1" },
    { "modName": "Other", "jarPath": "H:/b.jar", "basePackage": "other", "jarHash": "h2" }
  ]
}"#;
        let temp = std::env::temp_dir().join(format!("pzm-fastlaunch-{}.json", std::process::id()));
        fs::write(&temp, cache).unwrap();
        let check = read_and_check_cache(&temp).expect("缓存应当通过校验");
        let _ = fs::remove_file(&temp);

        assert_eq!(check.generated_at, 1791908270);
        assert_eq!(check.mod_set_hash, "087f9589016fc4e8");
        assert_eq!(check.mod_count, 2);
    }

    #[test]
    fn rejects_cache_with_wrong_format_or_missing_hash() {
        let temp = std::env::temp_dir().join(format!("pzm-fastlaunch-bad-{}.json", std::process::id()));

        // 格式版本不匹配 → 放弃缓存。
        fs::write(&temp, r#"{ "format": 2, "modSetHash": "abc", "mods": [] }"#).unwrap();
        assert!(read_and_check_cache(&temp).is_none());

        // 缺指纹 → 放弃缓存。
        fs::write(&temp, r#"{ "format": 1, "mods": [] }"#).unwrap();
        assert!(read_and_check_cache(&temp).is_none());

        let _ = fs::remove_file(&temp);
    }
}
