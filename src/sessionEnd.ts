/**
 * 游戏停止事件的展示判定与模态框文案（见 docs/exit-reason-classification.md）。
 *
 * 自包含模块：只含纯函数，供 node:test 直接测。
 */

/** 后端 `StopReason` 枚举的 serde camelCase 序列化值。 */
export type StopReasonLabel =
  | "normalExit"
  | "javaCrash"
  | "nativeCrash"
  | "externalTerminate"
  | "unknown";

/** 停止原因的用户可读标签（模态框里的「停止原因：…」行）。 */
export const stopReasonLabels: Record<StopReasonLabel, string> = {
  normalExit: "正常退出",
  javaCrash: "游戏崩溃（Java 异常）",
  nativeCrash: "游戏崩溃（原生异常）",
  externalTerminate: "被外部结束",
  unknown: "原因未知",
};

export function stopReasonLabel(value: string | null | undefined): string {
  if (value == null) {
    return "";
  }
  return value in stopReasonLabels
    ? stopReasonLabels[value as StopReasonLabel]
    : stopReasonLabels.unknown;
}

/**
 * 会话结束时是否应该弹报错模态框。
 *
 * 两种游戏停止事件都弹：
 * - `finished`：覆盖层会话期内游戏退出（后端给了停止原因）；
 * - `game-exited`：覆盖层收尾**之后**游戏才退出（会话后监视发现，见
 *   `watch_game_after_overlay`）——之前这条路径无人监视，崩溃静默无报。
 *
 * 正常退出（用户主动停止，含任务管理器对窗口的优雅关闭）不弹；覆盖层看门狗/
 * 超时收尾时游戏还在跑（stopReason 缺省）也不弹。后端 `error` 阶段（如
 * 「游戏进程在窗口出现前已退出」）由监听器单独弹启动失败模态框，不在这里重复。
 */
export function shouldReportSessionEnd(update: {
  phase: string;
  stopReason?: string | null;
}): boolean {
  return (
    (update.phase === "finished" || update.phase === "game-exited") &&
    update.stopReason != null &&
    update.stopReason !== "normalExit"
  );
}

/** 停止原因模态框的标题（会话内退出与会话后退出共用）。 */
export const GAME_STOP_DIALOG_TITLE = "游戏已停止";
/** 会话错误（error 阶段）模态框的标题。 */
export const GAME_LAUNCH_FAIL_TITLE = "游戏未能正常启动";

/** 从崩溃诊断里取出「缺失的类」——判断是否与注入项相关的依据。
 *
 *  后端文案（`diagnose_game_exit_from_console` / `exit_reason_diagnosis`）：
 *  `诊断：游戏缺少类 zombie.util.list.PZArrayList——几乎总是「游戏版本与模组不匹配」…`
 *  找不到（内存不足、笼统异常、原生崩溃）返回 null：那类问题与具体注入项无关，
 *  不该按类名去反查模组。
 */
export function extractSuspectClass(message: string): string | null {
  const match = /游戏缺少类\s+([A-Za-z0-9_.$]+)/.exec(message);
  return match ? match[1] : null;
}

/** 把会话结果消息拆成「结果」与「诊断：…」两段（后端把诊断拼在 reason 后面）。 */
function splitDiagnosis(message: string): { result: string; diagnosis: string } {
  const separator = " 诊断：";
  const index = message.indexOf(separator);
  if (index < 0) {
    return { result: message, diagnosis: "" };
  }
  return {
    result: message.slice(0, index),
    diagnosis: message.slice(index + 1),
  };
}

/** 组装停止原因模态框的正文：结果 / 停止原因与退出码 / 诊断，三段分明。 */
export function formatGameStopDialogBody(update: {
  message: string;
  stopReason?: string | null;
  exitCode?: number | null;
}): string {
  const { result, diagnosis } = splitDiagnosis(update.message);
  const meta: string[] = [];
  const label = stopReasonLabel(update.stopReason);
  if (label) {
    meta.push(`停止原因：${label}`);
  }
  if (update.exitCode != null) {
    const hex = update.exitCode.toString(16).toUpperCase().padStart(8, "0");
    meta.push(`退出码：${update.exitCode}（0x${hex}）`);
  }
  return [result, meta.join("　"), diagnosis].filter(Boolean).join("\n\n");
}
