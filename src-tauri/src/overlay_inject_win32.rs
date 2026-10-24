//! 默认注入后端：原生 Win32 `CreateRemoteThread` + `LoadLibraryW`。
//!
//! 与 dll-syringe 内部使用的是同一套机制，只是不依赖 nightly 专属 feature，
//! 因此可以在 stable 工具链上直接编译。

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::core::{w, Error, PCSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, OpenProcess, WaitForSingleObject, INFINITE,
    LPTHREAD_START_ROUTINE, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_WRITE,
};

/// `LoadLibraryW` 导出名（NUL 结尾，供 `GetProcAddress` 使用）。
const LOAD_LIBRARY_W: &[u8] = b"LoadLibraryW\0";

/// 注入 DLL 到指定进程，失败时返回带排查建议的中文错误信息。
pub(crate) fn inject_dll(pid: u32, dll_path: &Path) -> Result<(), String> {
    let mut wide_path: Vec<u16> = dll_path.as_os_str().encode_wide().collect();
    if wide_path.contains(&0) {
        return Err("覆盖层 DLL 路径包含非法字符（NUL）。".to_string());
    }
    wide_path.push(0);
    let path_bytes = wide_path.len() * std::mem::size_of::<u16>();

    // SAFETY: 全部是对目标进程的 Win32 调用，参数来自上面的缓冲区与当前进程 API。
    unsafe {
        let process = OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE,
            false,
            pid,
        )
        .map_err(|error| {
            format!(
                "打开游戏进程失败（PID {pid}）: {error}。{}",
                access_hint(&error)
            )
        })?;

        let result = inject_into_process(process, &wide_path, path_bytes);
        let _ = CloseHandle(process);
        result
    }
}

/// 在已打开的进程句柄上完成：分配远程内存 → 写入路径 → 远程调用 LoadLibraryW。
///
/// # Safety
/// `process` 必须是带注入权限的有效进程句柄。
unsafe fn inject_into_process(
    process: HANDLE,
    wide_path: &[u16],
    path_bytes: usize,
) -> Result<(), String> {
    unsafe {
        let remote_buffer = VirtualAllocEx(
            process,
            None,
            path_bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote_buffer.is_null() {
            return Err(format!(
                "在游戏进程中分配内存失败: {}",
                Error::from_thread()
            ));
        }

        let mut written = 0usize;
        if let Err(error) = WriteProcessMemory(
            process,
            remote_buffer,
            wide_path.as_ptr() as *const c_void,
            path_bytes,
            Some(&mut written),
        ) {
            free_remote(process, remote_buffer);
            return Err(format!("写入游戏进程内存失败: {error}"));
        }
        if written != path_bytes {
            free_remote(process, remote_buffer);
            return Err(format!(
                "写入游戏进程内存不完整（{written}/{path_bytes} 字节）。"
            ));
        }

        let kernel32 = match GetModuleHandleW(w!("kernel32.dll")) {
            Ok(module) => module,
            Err(error) => {
                free_remote(process, remote_buffer);
                return Err(format!("获取 kernel32.dll 模块句柄失败: {error}"));
            }
        };

        // LoadLibraryW 的地址在本进程与目标进程里是同一个（系统 DLL 共享映像），
        // 所以可以把本进程解析到的地址直接当作远程线程入口。
        let Some(load_library) = GetProcAddress(kernel32, PCSTR::from_raw(LOAD_LIBRARY_W.as_ptr()))
        else {
            free_remote(process, remote_buffer);
            return Err("无法在 kernel32.dll 中解析 LoadLibraryW。".to_string());
        };
        let start_routine: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(*mut c_void) -> u32,
        >(load_library));

        let thread = match CreateRemoteThread(
            process,
            None,
            0,
            start_routine,
            Some(remote_buffer as *const c_void),
            0,
            None,
        ) {
            Ok(thread) => thread,
            Err(error) => {
                free_remote(process, remote_buffer);
                return Err(format!(
                    "在游戏进程中创建远程线程失败: {error}。{}",
                    access_hint(&error)
                ));
            }
        };

        let wait_result = WaitForSingleObject(thread, INFINITE);
        let mut exit_code: u32 = 0;
        let exit_code_result = GetExitCodeThread(thread, &mut exit_code);
        let _ = CloseHandle(thread);
        free_remote(process, remote_buffer);

        if wait_result != WAIT_OBJECT_0 {
            return Err("等待远程 LoadLibraryW 线程结束失败。".to_string());
        }
        exit_code_result.map_err(|error| format!("读取远程线程退出码失败: {error}"))?;

        if exit_code == 0 {
            return Err("远程 LoadLibraryW 返回空句柄：DLL 未能在游戏进程中加载。\
                 常见原因：DLL 位数与游戏不一致（本项目只支持 x64）、DLL 路径含中文或权限受限、\
                 或被杀毒软件拦截。"
                .to_string());
        }

        Ok(())
    }
}

/// 就地释放远程分配的内存；失败时忽略（进程退出会自动回收）。
unsafe fn free_remote(process: HANDLE, buffer: *mut c_void) {
    let _ = unsafe { VirtualFreeEx(process, buffer, 0, MEM_RELEASE) };
}

/// 把 Win32 错误翻译成可执行的提示。
fn access_hint(error: &Error) -> &'static str {
    match win32_code(error) {
        Some(5) => "拒绝访问：游戏可能以管理员身份运行而启动器没有，或安全软件拦截了远程线程注入。",
        Some(87) => "参数无效：请确认目标进程仍在运行。",
        _ => "请确认游戏仍在运行、位数一致（x64），并检查安全软件是否拦截注入。",
    }
}

/// 从 `windows::core::Error` 里取出 Win32 错误码（HRESULT_FROM_WIN32 形式）。
fn win32_code(error: &Error) -> Option<u32> {
    let code = error.code().0 as u32;
    (code & 0xFFFF_0000 == 0x8007_0000).then_some(code & 0xFFFF)
}
