export function shouldResetGameWindow(
  gameWindowReady: boolean,
  status: { running: boolean; windowReady: boolean },
): boolean {
  return gameWindowReady && (!status.running || !status.windowReady);
}
