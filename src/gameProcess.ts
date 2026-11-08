export function shouldResetGameWindow(
  gameWindowReady: boolean,
  status: { running: boolean; windowReady: boolean },
): boolean {
  return gameWindowReady && (!status.running || !status.windowReady);
}

/** 点击启动后等待游戏进程出现的上限：超过就熔断本次"等待启动"，让用户能立刻重试。
 *  15 秒足够 Steam 从协议调用到拉起游戏；超过基本是 Steam 没起、被拦或没装。 */
export const LAUNCH_START_TIMEOUT_MS = 15_000;

/** 是否需要熔断本次启动等待：只有在超时前始终没见过游戏进程才算。 */
export function shouldBreakLaunchWait({
  sawRunning,
  elapsedMs,
  timeoutMs = LAUNCH_START_TIMEOUT_MS,
}: {
  sawRunning: boolean;
  elapsedMs: number;
  timeoutMs?: number;
}): boolean {
  return !sawRunning && elapsedMs >= timeoutMs;
}

/** 熔断提示文案：Steam 都没起来和"Steam 在但游戏没拉起"是两种排查方向，分开说。 */
export function launchTimeoutMessage(steamRunning: boolean): string {
  const seconds = Math.round(LAUNCH_START_TIMEOUT_MS / 1000);
  return steamRunning
    ? `启动超时：${seconds} 秒内没有拉起游戏进程，已中断本次启动等待。Steam 正在运行，可能是更新未完成或被拦截，可直接重试。`
    : `启动超时：${seconds} 秒内没有拉起 Steam，已中断本次启动等待。请确认 Steam 已安装并能正常启动，然后重试。`;
}
