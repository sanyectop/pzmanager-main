//! 可选注入后端：dll-syringe。
//!
//! 需要 nightly 工具链（dll-syringe 所有发布版本都启用了 unstable feature），
//! 通过 `cargo +nightly build --features dll-syringe-inject` 启用。
//! 默认构建走 `overlay_inject_win32.rs`，不依赖 nightly。

use std::path::Path;

use dll_syringe::error::InjectError;
use dll_syringe::process::OwnedProcess;
use dll_syringe::Syringe;

/// 注入 DLL 到指定进程，失败时返回带排查建议的中文错误信息。
pub(crate) fn inject_dll(pid: u32, dll_path: &Path) -> Result<(), String> {
    let process = OwnedProcess::from_pid(pid)
        .map_err(|error| format!("打开游戏进程失败（PID {pid}）: {error}"))?;
    let syringe = Syringe::for_process(process);

    syringe
        .inject(dll_path)
        .map(|_| ())
        .map_err(|error| describe_injection_error(&error, pid, dll_path))
}

/// 把底层错误翻译成用户能直接照做的提示。
fn describe_injection_error(error: &InjectError, pid: u32, dll_path: &Path) -> String {
    let hint = match error {
        InjectError::ArchitectureMismatch => {
            "DLL 与游戏进程位数不一致：本项目只支持 x64，请用 --target x86_64-pc-windows-msvc 构建 DLL，并确认游戏是 64 位版本。"
        }
        InjectError::ProcessInaccessible => {
            "无法访问目标进程：游戏可能已经退出，或游戏/启动器权限级别不同（例如游戏以管理员运行而启动器没有）。"
        }
        InjectError::RemoteIo(io) if io.raw_os_error() == Some(5) => {
            "目标进程拒绝加载该 DLL（拒绝访问），常见原因是安全软件拦截注入，或 DLL 位于受保护目录。"
        }
        InjectError::RemoteIo(io) if io.raw_os_error() == Some(126) => {
            "目标进程找不到或无法加载该 DLL（ERROR_MOD_NOT_FOUND），请确认路径正确且依赖齐全。"
        }
        InjectError::RemoteIo(io) if io.raw_os_error() == Some(193) => {
            "目标进程加载 DLL 失败（不是有效的 Win32 应用），通常是位数不匹配。"
        }
        InjectError::UnsupportedTarget => {
            "目标进程不受支持：请确认注入的是 Project Zomboid 的 64 位游戏进程。"
        }
        InjectError::RemoteException(exception) => {
            return format!(
                "向进程 PID {pid} 注入 {} 时目标进程抛出异常（{exception}），已中止注入。",
                dll_path.display()
            );
        }
        _ => "请检查 DLL 是否为 x64 构建、游戏是否仍在运行、以及安全软件是否拦截了 DLL 注入。",
    };

    format!(
        "向进程 PID {pid} 注入 {} 失败: {error}。{hint}",
        dll_path.display()
    )
}
