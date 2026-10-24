import { invoke } from "@tauri-apps/api/core";

/**
 * 覆盖层注入（第一阶段）前端封装。
 *
 * 后端命令：`inject_overlay(dll_path)`，会在 60 秒内轮询游戏进程
 * （优先 ProjectZomboid64.exe，其次 javaw.exe），因此点击后最长可能等待一分钟。
 */

/** 开发期默认产物路径：`npm run tauri:dev` 的工作目录是 `src-tauri`。 */
export const DEV_OVERLAY_DLL_PATH =
  "../overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll";

/**
 * 请求后端把覆盖层 DLL 注入游戏进程。
 *
 * 成功返回后端给出的中文描述；失败抛出带有具体原因的 Error，
 * 直接把它显示到弹窗/Toast 即可。
 *
 * 用法（React 示例）：
 *
 * ```tsx
 * async function handleInjectOverlay() {
 *   try {
 *     const message = await injectOverlay(DEV_OVERLAY_DLL_PATH);
 *     showToast(message, "success");
 *   } catch (error: unknown) {
 *     showToast(error instanceof Error ? error.message : String(error), "error");
 *   }
 * }
 *
 * <button type="button" onClick={handleInjectOverlay}>
 *   测试覆盖层
 * </button>
 * ```
 */
export async function injectOverlay(dllPath: string): Promise<string> {
  const trimmed = dllPath.trim();
  if (!trimmed) {
    throw new Error("请先指定覆盖层 DLL 路径。");
  }

  try {
    return await invoke<string>("inject_overlay", { dllPath: trimmed });
  } catch (error: unknown) {
    // Tauri 的 Err(String) 会被原样抛出，这里保证调用方拿到 Error 实例。
    throw error instanceof Error ? error : new Error(String(error));
  }
}
