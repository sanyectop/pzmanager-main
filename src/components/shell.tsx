/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Box } from "@chakra-ui/react";
import { ReactNode, useEffect, useLayoutEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "../app/utils";

export function WindowTitlebar({
  children,
  taskCount,
  taskPanelOpen,
  onToggleTasks,
}: {
  children: ReactNode;
  taskCount: number;
  taskPanelOpen: boolean;
  onToggleTasks: () => void;
}) {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const currentWindow = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const syncMaximized = () => { void currentWindow.isMaximized().then(setIsMaximized).catch(() => undefined); };
    syncMaximized();
    void currentWindow.onResized(syncMaximized).then((cleanup) => {
      if (disposed) cleanup(); else unlisten = cleanup;
    });
    return () => { disposed = true; unlisten?.(); };
  }, []);

  // 浏览器预览下没有原生窗口：只保留导航，不渲染拖动区与窗口按钮。
  const currentWindow = isTauriRuntime() ? getCurrentWindow() : null;
  const runWindowAction = (action: () => Promise<void>) => { void action().catch(() => undefined); };
  return (
    <Box className="pz-window-titlebar">
      {children}
      <Box aria-label="拖动窗口" className="pz-window-drag-region" data-tauri-drag-region onMouseDown={(event) => { if (currentWindow && event.button === 0) runWindowAction(() => currentWindow.startDragging()); }} onDoubleClick={(event) => { if (currentWindow && !(event.target as HTMLElement).closest("[data-titlebar-control]")) runWindowAction(() => currentWindow.toggleMaximize()); }} />
      <button
        aria-expanded={taskPanelOpen}
        aria-label={taskCount > 0 ? "任务中心，" + taskCount + " 个进行中" : "任务中心"}
        className={["pz-task-trigger", taskCount > 0 ? "is-busy" : "", taskPanelOpen ? "is-open" : ""].join(" ")}
        data-titlebar-control
        onClick={onToggleTasks}
        title="任务中心"
        type="button"
      >
        <span className="pz-task-trigger-label">任务</span>
        {taskCount > 0 && <span className="pz-task-trigger-badge">{taskCount}</span>}
      </button>
      {currentWindow && (
        <Box className="pz-window-controls">
          <button aria-label="最小化" className="pz-window-control" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.minimize())} title="最小化" type="button"><span className="pz-window-icon pz-window-icon-minimize" /></button>
          <button aria-label={isMaximized ? "还原窗口" : "最大化"} className="pz-window-control" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.toggleMaximize())} title={isMaximized ? "还原窗口" : "最大化"} type="button"><span className={`pz-window-icon ${isMaximized ? "pz-window-icon-restore" : "pz-window-icon-maximize"}`} /></button>
          <button aria-label="关闭" className="pz-window-control pz-window-control-close" data-titlebar-control onClick={() => runWindowAction(() => currentWindow.close())} title="关闭" type="button"><span className="pz-window-icon pz-window-icon-close" /></button>
        </Box>
      )}
    </Box>
  );
}

export function useDialogAutoHeight(
  dialogRef: { current: HTMLDivElement | null },
  lastHeightRef: { current: number | null },
  deps: readonly unknown[],
) {
  useLayoutEffect(() => {
    const dialog = dialogRef.current;
    if (!dialog) {
      lastHeightRef.current = null;
      return;
    }
    // 空闲时高度是 auto，上一次的稳定高度存在 ref 里；若正在过渡则用当前实际高度接上。
    const previousHeight =
      dialog.style.height === "" ? lastHeightRef.current : dialog.getBoundingClientRect().height;
    dialog.style.transition = "none";
    dialog.style.height = "auto";
    const targetHeight = dialog.getBoundingClientRect().height;
    if (previousHeight == null || Math.abs(targetHeight - previousHeight) < 1) {
      dialog.style.transition = "";
      dialog.style.height = "";
      lastHeightRef.current = targetHeight;
      return;
    }
    dialog.style.height = `${previousHeight}px`;
    // 强制一次布局，让下面的过渡从旧高度开始而不是直接跳到新高度。
    void dialog.offsetHeight;
    dialog.style.transition = "";
    dialog.style.height = `${targetHeight}px`;
    const handleTransitionEnd = (event: TransitionEvent) => {
      // 只认自己这一层的 height 过渡（子元素的过渡事件会冒泡上来）。
      if (event.target !== dialog || event.propertyName !== "height") {
        return;
      }
      dialog.style.height = "";
      lastHeightRef.current = targetHeight;
    };
    dialog.addEventListener("transitionend", handleTransitionEnd);
    return () => dialog.removeEventListener("transitionend", handleTransitionEnd);
  }, deps);
}
