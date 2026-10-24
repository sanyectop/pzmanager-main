export const defaultLaunchArguments = ["-agentlib:zbNative", "--"];

export function joinLaunchArguments(argumentsList: string[]): string {
  return argumentsList.map((argument) => argument.trim()).filter(Boolean).join(" ");
}
