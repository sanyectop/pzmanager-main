//! 覆盖层注入（第一阶段：注入 DLL → Hook OpenGL → 纯红覆盖 → 自动恢复）。
//!
//! 本模块只把覆盖层 DLL 送进游戏进程，并返回可读的成功 / 失败信息；
//! Hook、绘制、5 秒计时与状态恢复全部发生在 DLL 内部，启动器不做任何进程内操作。
//!
//! 注入后端有两套，编译期二选一：
//! * 默认（stable 工具链）：`overlay_inject_win32.rs`，原生 `CreateRemoteThread` + `LoadLibraryW`；
//! * `--features dll-syringe-inject`（nightly 工具链）：`overlay_inject_dll_syringe.rs`，使用 dll-syringe。
//!
//! 之所以默认不用 dll-syringe：该 crate 所有已发布版本都带 `#![feature(...)]`
//! （`linked_list_cursors` / `once_cell_try` 等），在 stable 上直接编译失败。

#[cfg(not(windows))]
compile_error!("覆盖层注入只支持 Windows 目标。");

#[cfg(all(windows, not(feature = "dll-syringe-inject")))]
#[path = "overlay_inject_win32.rs"]
pub(crate) mod injector;

#[cfg(all(windows, feature = "dll-syringe-inject"))]
#[path = "overlay_inject_dll_syringe.rs"]
pub(crate) mod injector;

#[cfg(windows)]
pub(crate) mod imp {
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
        MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        TH32CS_SNAPPROCESS,
    };

    /// 目标进程名，按优先级排列：先找游戏主进程，找不到再退化为 Java 宿主进程。
    const TARGET_PROCESS_NAMES: [&str; 2] = ["ProjectZomboid64.exe", "javaw.exe"];
    /// 进程轮询总时长。
    const PROCESS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(60);
    /// 进程轮询间隔。
    const PROCESS_LOOKUP_INTERVAL: Duration = Duration::from_millis(500);
    /// 新版覆盖层 DLL（带加载界面）里必然包含的状态文件名字符串。
    ///
    /// 早期的"纯红探针"DLL 没有这个标记：它只会把游戏窗口刷红、不画任何界面，
    /// 注入它得到的就是"红屏无字"。注入前先做内容校验，给出可操作的错误。
    const UI_DLL_MARKER: &[u8] = b"pz_overlay_status.txt";

    /// 阻塞式注入流程，由命令层放到阻塞线程池里执行。
    pub(super) fn inject_overlay_blocking(dll_path: &str) -> Result<String, String> {
        let dll_path = normalize_dll_path(dll_path)?;
        ensure_overlay_dll_is_current(&dll_path)?;
        let dll_name = dll_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("无法解析 DLL 文件名：{}", dll_path.display()))?
            .to_string();

        let (pid, process_name) = wait_for_game_process(PROCESS_LOOKUP_TIMEOUT)?;

        // 目标进程已有同名模块时不再重复 LoadLibrary：
        // 会多一次引用计数，而且 DLL 端的 Hook 也只需要一份。
        // 这里枚举失败（例如 32 位进程）不作为错误处理，交给注入阶段给出更准确的报错。
        if let Some(loaded_path) = loaded_module_path(pid, &dll_name).unwrap_or(None) {
            let mut message = format!(
                "{dll_name} 已在 {process_name}（PID {pid}）中加载，本次跳过重复注入。"
            );
            // 预载的不是我们要注入的这份：多半是旧版残留（只刷红、没界面），
            // 或极速启动 -agentpath 指向了别的位置 —— 必须明确告诉用户，否则
            // "红屏无字"时用户根本想不到去看这个路径。
            if !module_path_matches(&loaded_path, &dll_path) {
                message.push_str(&format!(
                    "\n注意：进程里加载的是另一份副本（{loaded_path}），与本次的 {} 不一致；\
                     若出现红屏且没有文字，请清理旧版残留或重装 PZManager。",
                    dll_path.display()
                ));
            }
            return Ok(message);
        }

        super::injector::inject_dll(pid, &dll_path)?;

        Ok(format!(
            "已向 {process_name}（PID {pid}）注入覆盖层 DLL：{}",
            dll_path.display()
        ))
    }

    /// 校验 DLL 是否为带加载界面的新版覆盖层（旧探针 DLL 直接拒绝注入）。
    pub(crate) fn ensure_overlay_dll_is_current(path: &Path) -> Result<(), String> {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("读取覆盖层 DLL 失败（{}）: {error}", path.display()))?;
        if bytes
            .windows(UI_DLL_MARKER.len())
            .any(|window| window == UI_DLL_MARKER)
        {
            return Ok(());
        }
        Err(format!(
            "覆盖层 DLL 版本过旧（{}）：缺少新版界面标记，注入后只会显示红屏、没有任何文字。\
             请更新或重新安装 PZManager，或用界面上的「选择 DLL」指定新版 pz_overlay.dll。",
            path.display()
        ))
    }

    /// 目标进程里同名模块的完整路径（用于区分"预载的就是这份"与"别的位置的旧副本"）。
    pub(crate) fn loaded_module_path(pid: u32, dll_name: &str) -> Result<Option<String>, String> {
        // SAFETY: 只做只读枚举。
        let snapshot =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) }
                .map_err(|error| format!("枚举 PID {pid} 的模块列表失败: {error}"))?;

        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;

        let mut loaded = None;
        // SAFETY: entry 已按 API 要求设置 dwSize。
        if unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                if wide_fixed_string(&entry.szModule).eq_ignore_ascii_case(dll_name) {
                    loaded = Some(wide_fixed_string(&entry.szExePath));
                    break;
                }
                // SAFETY: 链表遍历结束会返回错误。
                if unsafe { Module32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };
        Ok(loaded)
    }

    /// 模块路径与预期路径是否指向同一文件（不区分大小写，不触盘）。
    pub(crate) fn module_path_matches(loaded: &str, expected: &Path) -> bool {
        loaded.trim().to_ascii_lowercase() == expected.to_string_lossy().trim().to_ascii_lowercase()
    }

    /// 解析相对 DLL 路径时依次尝试的基目录。
    ///
    /// 第一项是**当前工作目录**（历史行为：`tauri dev` 从项目根启动时工作目录是 `src-tauri`）。
    /// 其后是**可执行文件所在目录及其各级祖先目录**：从资源管理器双击 exe 启动时，
    /// 工作目录是 exe 所在目录（例如 `src-tauri\target\release`），
    /// 只按工作目录拼接会得到 `...\target\release\overlay-dll\...` 这种不存在的路径。
    pub(super) fn relative_bases() -> Vec<PathBuf> {
        let mut bases = Vec::new();

        if let Ok(current) = std::env::current_dir() {
            bases.push(current);
        }

        if let Ok(exe) = std::env::current_exe() {
            let mut next = exe.parent().map(Path::to_path_buf);
            // 上限只是防御性的：正常布局最深只需走到项目根（target\<profile> → src-tauri → 项目根）。
            for _ in 0..6 {
                let Some(dir) = next else { break };
                if !bases.contains(&dir) {
                    bases.push(dir.clone());
                }
                next = dir.parent().map(Path::to_path_buf);
            }
        }

        bases
    }

    /// 发布布局候选：DLL 作为 Tauri resource 打包后与 exe 同目录，
    /// 而前端传来的仍是开发期相对路径（`../overlay-dll/target/...`），
    /// 因此先尝试「exe 目录 + 文件名」，命中即可覆盖安装包场景。
    ///
    /// 只做路径拼接，不检查文件是否存在；存在性由调用方判断。
    pub(super) fn exe_dir_candidate(relative: &Path) -> Option<PathBuf> {
        let name = relative.file_name()?;
        let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
        Some(exe_dir.join(name))
    }

    /// 在候选基目录中挑出第一个真正存在的文件。
    ///
    /// 全部落空时返回尝试过的完整路径列表，供调用方拼进错误信息。
    pub(super) fn first_existing_file(
        bases: &[PathBuf],
        relative: &Path,
    ) -> Result<PathBuf, Vec<PathBuf>> {
        let mut tried = Vec::with_capacity(bases.len());
        for base in bases {
            let candidate = base.join(relative);
            if candidate.is_file() {
                return Ok(candidate);
            }
            tried.push(candidate);
        }
        Err(tried)
    }

    /// 校验并规范化 DLL 路径。
    ///
    /// 绝对路径按原样校验；相对路径先尝试发布布局（exe 目录 + 文件名），
    /// 再依次尝试 [`relative_bases`] 里的基目录，
    /// 因此无论是开发期从项目根启动（`npm run tauri:dev`）、从资源管理器双击 exe，
    /// 还是安装版（DLL 作为 resource 与 exe 同目录），都能定位到同一个 DLL。
    ///
    /// 刻意不使用 `canonicalize`：它会返回 `\\?\` 前缀的扩展长度路径，
    /// 而 `LoadLibraryW` 与「模块按路径比对」对这种前缀的处理并不一致。
    pub(crate) fn normalize_dll_path(raw: &str) -> Result<PathBuf, String> {
        let trimmed = raw.trim().trim_matches('"').trim();
        if trimmed.is_empty() {
            return Err("覆盖层 DLL 路径为空。".to_string());
        }

        let candidate = PathBuf::from(trimmed);
        let absolute = if candidate.is_absolute() {
            if !candidate.is_file() {
                return Err(format!(
                    "覆盖层 DLL 不存在或不是文件：{}",
                    candidate.display()
                ));
            }
            candidate
        } else if let Some(bundled) = exe_dir_candidate(&candidate).filter(|path| path.is_file()) {
            bundled
        } else {
            match first_existing_file(&relative_bases(), &candidate) {
                Ok(found) => found,
                Err(tried) => {
                    let list = tried
                        .iter()
                        .map(|path| format!("  {}", path.display()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    return Err(format!(
                        "覆盖层 DLL 不存在或不是文件，已尝试以下位置：\n{list}\n\
                         提示：可用界面上的「选择 DLL」按钮指定任意位置的 pz_overlay.dll。"
                    ));
                }
            }
        };

        match absolute
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some(extension) if extension.eq_ignore_ascii_case("dll") => {}
            _ => {
                return Err(format!("覆盖层文件必须是 .dll：{}", absolute.display()));
            }
        }

        Ok(absolute)
    }

    /// 60 秒内每 500ms 轮询一次目标进程，返回 (PID, 命中的进程名)。
    fn wait_for_game_process(timeout: Duration) -> Result<(u32, &'static str), String> {
        let deadline = Instant::now() + timeout;

        loop {
            if let Some(found) = find_game_process() {
                return Ok(found);
            }

            if Instant::now() >= deadline {
                return Err(format!(
                    "等待游戏进程超时（{} 秒）：未找到 {} 或 {}，请确认游戏已经启动并进入主菜单。",
                    timeout.as_secs(),
                    TARGET_PROCESS_NAMES[0],
                    TARGET_PROCESS_NAMES[1]
                ));
            }

            thread::sleep(PROCESS_LOOKUP_INTERVAL);
        }
    }

    /// 按优先级查找游戏进程。返回候选顺序中的第一个命中项。
    pub(crate) fn find_game_process() -> Option<(u32, &'static str)> {
        // SAFETY: 0 表示当前进程；快照句柄失败时直接返回 None。
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;

        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut processes: Vec<(u32, String)> = Vec::new();
        // SAFETY: entry 已按 API 要求设置 dwSize。
        if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                processes.push((entry.th32ProcessID, wide_fixed_string(&entry.szExeFile)));
                // SAFETY: 链表遍历结束会返回错误。
                if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };

        TARGET_PROCESS_NAMES.iter().find_map(|candidate| {
            processes
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(candidate))
                .map(|(pid, _)| (*pid, *candidate))
        })
    }

    /// 目标进程是否已经加载了同名模块。
    pub(crate) fn is_module_loaded(pid: u32, dll_name: &str) -> Result<bool, String> {
        // SAFETY: 只做只读枚举。
        let snapshot =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) }
                .map_err(|error| format!("枚举 PID {pid} 的模块列表失败: {error}"))?;

        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;

        let mut loaded = false;
        // SAFETY: entry 已按 API 要求设置 dwSize。
        if unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok() {
            loop {
                if wide_fixed_string(&entry.szModule).eq_ignore_ascii_case(dll_name) {
                    loaded = true;
                    break;
                }
                // SAFETY: 链表遍历结束会返回错误。
                if unsafe { Module32NextW(snapshot, &mut entry) }.is_err() {
                    break;
                }
            }
        }

        let _ = unsafe { CloseHandle(snapshot) };
        Ok(loaded)
    }

    /// 把 Win32 定长 UTF-16 字段转成字符串（按 NUL 截断）。
    fn wide_fixed_string<const N: usize>(buffer: &[u16; N]) -> String {
        let units: Vec<u16> = buffer
            .iter()
            .copied()
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }
}

/// 向游戏进程注入覆盖层 DLL。
///
/// 前端参数：`dllPath`（对应 Tauri 的 `dll_path`）。
/// 注入内含最长 60 秒的进程轮询，因此放到阻塞线程池执行，避免卡住前端。
#[tauri::command]
pub async fn inject_overlay(dll_path: String) -> Result<String, String> {
    #[cfg(windows)]
    {
        return tauri::async_runtime::spawn_blocking(move || {
            imp::inject_overlay_blocking(&dll_path)
        })
        .await
        .map_err(|error| format!("注入任务执行失败: {error}"))?;
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::imp::{exe_dir_candidate, first_existing_file, relative_bases};

    /// 与前端默认值同形的相对路径。
    fn overlay_relative() -> PathBuf {
        Path::new("overlay-dll")
            .join("target")
            .join("x86_64-pc-windows-msvc")
            .join("release")
            .join("pz_overlay.dll")
    }

    /// 第一个基目录里没有目标文件时继续往后找，并返回命中基目录下的完整路径。
    #[test]
    fn picks_the_first_base_that_actually_contains_the_dll() {
        let root = tempfile::tempdir().expect("tempdir");
        let empty_base = root.path().join("src-tauri");
        let real_base = root.path().join("project");
        fs::create_dir_all(&empty_base).unwrap();
        fs::create_dir_all(real_base.join(overlay_relative().parent().unwrap())).unwrap();
        fs::write(real_base.join(overlay_relative()), b"dll").unwrap();

        let found = first_existing_file(&[empty_base, real_base.clone()], &overlay_relative())
            .expect("应当命中第二个基目录");

        assert_eq!(found, real_base.join(overlay_relative()));
    }

    /// 全部落空时返回每一个尝试过的路径，错误信息才能把候选位置列全。
    #[test]
    fn reports_every_attempted_location_when_none_matches() {
        let root = tempfile::tempdir().expect("tempdir");
        let bases = vec![root.path().join("a"), root.path().join("b")];

        let tried =
            first_existing_file(&bases, &overlay_relative()).expect_err("不应命中任何基目录");

        assert_eq!(tried.len(), 2);
        assert_eq!(tried[0], bases[0].join(overlay_relative()));
        assert_eq!(tried[1], bases[1].join(overlay_relative()));
    }

    /// 发布布局候选 = exe 所在目录 + 相对路径的文件名（不检查存在性）。
    #[test]
    fn bundled_candidate_is_the_executable_directory_joined_with_the_file_name() {
        let exe_dir = std::env::current_exe()
            .expect("current_exe")
            .parent()
            .expect("exe parent")
            .to_path_buf();

        let candidate = exe_dir_candidate(&overlay_relative()).expect("相对路径必有文件名");

        assert_eq!(candidate, exe_dir.join("pz_overlay.dll"));
    }

    /// 基目录以当前工作目录打头，且包含可执行文件所在目录及其祖先——
    /// 从资源管理器双击 exe 启动时，正是靠后者兜底。
    #[test]
    fn starts_with_the_working_directory_and_includes_the_executable_ancestors() {
        let bases = relative_bases();

        if let Ok(current) = std::env::current_dir() {
            assert_eq!(bases.first(), Some(&current));
        }

        let exe_dir = std::env::current_exe()
            .expect("current_exe")
            .parent()
            .expect("exe parent")
            .to_path_buf();
        assert!(
            bases.contains(&exe_dir),
            "可执行文件所在目录必须在候选基目录中：{bases:?}"
        );
        assert!(
            bases.len() > 1,
            "除工作目录外还应包含 exe 祖先目录：{bases:?}"
        );
    }
}
