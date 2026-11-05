/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Box } from "@chakra-ui/react";
import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauriRuntime } from "../app/utils";
import { TASK_FLIGHT_DURATION_MS } from "./tasks";

/** 徽标落位前的宽度过渡提前量：飞行结束前 0.2s 开始变宽。 */
const TRIGGER_EXPAND_LEAD_MS = 200;

export function WindowTitlebar({
  badgeCount,
  badgeFlightId,
  blockingOverlay,
  children,
  nativeTitlebar,
  taskCount,
  taskPanelOpen,
  onToggleTasks,
}: {
  /** 任务中心徽标上的数字（进行中任务数）。 */
  badgeCount?: number;
  /** 进行中的徽标飞入动画 id；非空期间徽标以绝对定位隐藏（不占布局，
   *  按钮宽度保持不变），并在飞行结束前 0.2s 把宽度渐变到带上徽标的目标宽度。 */
  badgeFlightId?: number | null;
  /** 有强制遮罩（隐私协议）盖在标题栏上方时，关掉标题栏自身的 backdrop-filter。
   *  WebView2 里「固定层模糊 + 遮罩内嵌跨域 iframe」会在重采样时闪出白色块。 */
  blockingOverlay?: boolean;
  children: ReactNode;
  /** 原生窗口标题栏开关：true = 系统画标题栏，自绘栏两侧的 LOGO 与窗口按钮滑出隐藏。 */
  nativeTitlebar?: boolean;
  taskCount: number;
  taskPanelOpen: boolean;
  onToggleTasks: () => void;
}) {
  const [isMaximized, setIsMaximized] = useState(false);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const expandTimerRef = useRef<number | null>(null);

  // 徽标飞入期间的按钮宽度编排：
  // - 飞行全程：徽标绝对定位隐藏，不占布局，宽度保持原样；
  // - 结束前 0.2s：锁定当前宽度为过渡起点，渐变到「当前宽度 + 徽标宽 + 间距」；
  // - 落地：解除内联宽度（自然宽度恰为目标值），徽标转入文档流并弹跳出现。
  useEffect(() => {
    const button = triggerRef.current;
    if (!button) {
      return;
    }
    if (badgeFlightId == null) {
      if (expandTimerRef.current !== null) {
        window.clearTimeout(expandTimerRef.current);
        expandTimerRef.current = null;
      }
      button.style.transition = "";
      button.style.width = "";
      return;
    }
    const badge = button.querySelector<HTMLElement>(".pz-task-trigger-badge.is-reserved");
    const narrow = button.offsetWidth;
    const gap = Number.parseFloat(getComputedStyle(button).columnGap) || 0;
    const target = badge !== null ? narrow + gap + badge.offsetWidth : null;
    if (target === null) {
      return;
    }
    expandTimerRef.current = window.setTimeout(
      () => {
        expandTimerRef.current = null;
        button.style.transition = "none";
        button.style.width = `${button.offsetWidth}px`;
        // 强制一次布局，让过渡拿到明确的起点值而不是从 auto 直接跳变。
        void button.offsetWidth;
        button.style.transition = "width 0.2s cubic-bezier(0.33, 1, 0.68, 1)";
        button.style.width = `${target}px`;
      },
      Math.max(0, TASK_FLIGHT_DURATION_MS - TRIGGER_EXPAND_LEAD_MS),
    );
    return () => {
      if (expandTimerRef.current !== null) {
        window.clearTimeout(expandTimerRef.current);
        expandTimerRef.current = null;
      }
    };
  }, [badgeFlightId]);

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
  const badgeReserved = badgeFlightId != null;
  return (
    <Box
      className={`pz-window-titlebar${nativeTitlebar ? " pz-titlebar-native" : ""}${
        blockingOverlay ? " pz-titlebar-static" : ""
      }`}
    >
      {children}
      <Box aria-label="拖动窗口" className="pz-window-drag-region" data-tauri-drag-region onMouseDown={(event) => { if (currentWindow && event.button === 0) runWindowAction(() => currentWindow.startDragging()); }} onDoubleClick={(event) => { if (currentWindow && !(event.target as HTMLElement).closest("[data-titlebar-control]")) runWindowAction(() => currentWindow.toggleMaximize()); }} />
      <button
        aria-expanded={taskPanelOpen}
        aria-label={taskCount > 0 ? "任务中心，" + taskCount + " 个进行中" : "任务中心"}
        className={["pz-task-trigger", taskCount > 0 ? "is-busy" : "", taskPanelOpen ? "is-open" : ""].join(" ")}
        data-titlebar-control
        onClick={onToggleTasks}
        ref={triggerRef}
        title="任务中心"
        type="button"
      >
        <span className="pz-task-trigger-label">任务</span>
        {/* key 带上 reserved 状态：飞入落地时重挂载，弹跳动画准时重播。 */}
        {(badgeCount ?? taskCount) > 0 && (
          <span
            className={`pz-task-trigger-badge pz-badge-pop${badgeReserved ? " is-reserved" : ""}`}
            key={`${badgeCount ?? taskCount}-${badgeReserved ? "reserved" : "shown"}`}
          >
            {badgeCount ?? taskCount}
          </span>
        )}
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
