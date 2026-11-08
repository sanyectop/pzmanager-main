/** `notice`：无论是否开发者模式都显示的重要提示（如背景音乐自动播放）。
 *  `success` / `info` 只在开发者模式显示，避免日常操作刷屏。 */
export type NotificationKind = "success" | "error" | "info" | "notice";

export function shouldShowNotification(
  kind: NotificationKind,
  developerMode: boolean,
): boolean {
  return developerMode || kind === "error" || kind === "notice";
}

export function conciseNotification(message: string, developerMode: boolean): string {
  if (developerMode) {
    return message;
  }

  const firstLine = message.split(/\r?\n/, 1)[0]?.trim() || message.trim();
  return firstLine.length > 140 ? `${firstLine.slice(0, 137)}...` : firstLine;
}
