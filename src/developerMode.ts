export type NotificationKind = "success" | "error" | "info";

export function shouldShowNotification(
  kind: NotificationKind,
  developerMode: boolean,
): boolean {
  return developerMode || kind === "error";
}

export function conciseNotification(message: string, developerMode: boolean): string {
  if (developerMode) {
    return message;
  }

  const firstLine = message.split(/\r?\n/, 1)[0]?.trim() || message.trim();
  return firstLine.length > 140 ? `${firstLine.slice(0, 137)}...` : firstLine;
}
