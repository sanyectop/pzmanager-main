/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Badge, Box, Button, HStack, Text } from "@chakra-ui/react";
import { NotificationEntry, TaskItem, ToastItem } from "../app/types";
import { formatDownloadSize, notificationKindLabel, notificationKindPalette, taskStatusLabel, taskStatusPalette } from "../app/utils";

export function ToastViewport({ toasts, onClose }: { toasts: ToastItem[]; onClose: (id: number) => void }) {
  return (
    <Box aria-live="polite" className="pz-toast-viewport">
      {toasts.map((toast) => (
        <Box
          className={`pz-toast pz-toast-${toast.kind}${toast.closing ? " pz-toast-closing" : ""}`}
          key={toast.id}
          role="status"
        >
          <Box className="pz-toast-content">
            <Box className="pz-toast-indicator" />
            <Text className="pz-toast-message">{toast.message}</Text>
            <button
              aria-label="关闭提示"
              className="pz-toast-close"
              onClick={() => onClose(toast.id)}
              type="button"
            >
              ×
            </button>
          </Box>
          <Box className="pz-toast-progress" />
        </Box>
      ))}
    </Box>
  );
}

export function NotificationConsole({
  entries,
  onClear,
}: {
  entries: NotificationEntry[];
  onClear: () => void;
}) {
  return (
    <Box className="pz-panel pz-notification-console" bg="white" borderColor="gray.200" borderRadius="md" borderWidth="1px" p="4">
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="220px">
          <Text color="gray.900" fontSize="sm" fontWeight="700">通知记录</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            本次运行产生的所有通知，包括简单模式下被隐藏的信息/成功提示。
          </Text>
        </Box>
        <HStack gap="2">
          <Text color="gray.500" fontSize="xs">{entries.length} 条</Text>
          <Button disabled={entries.length === 0} onClick={onClear} size="xs" variant="ghost">清空</Button>
        </HStack>
      </HStack>
      <Box aria-live="polite" className="pz-notification-console-list" mt="3" role="log">
        {entries.length === 0 ? (
          <Text color="gray.500" fontSize="sm" py="4" textAlign="center">本次运行还没有通知。</Text>
        ) : (
          [...entries].reverse().map((entry) => (
            <HStack align="start" className="pz-notification-row" gap="2" key={entry.id}>
              <Text className="pz-notification-time">
                {new Date(entry.createdAt).toLocaleTimeString("zh-CN", { hour12: false })}
              </Text>
              <Badge colorPalette={notificationKindPalette(entry.kind)} flexShrink="0" size="sm">
                {notificationKindLabel(entry.kind)}
              </Badge>
              {!entry.visible && (
                <Badge colorPalette="gray" flexShrink="0" size="sm" variant="subtle">简单模式隐藏</Badge>
              )}
              <Text className="pz-notification-message" color="gray.700" flex="1" fontSize="xs">
                {entry.message}
              </Text>
            </HStack>
          ))
        )}
      </Box>
    </Box>
  );
}

export function TaskCenterPanel({
  tasks,
  closingTaskIds,
  closing,
  onClose,
  onClearFinished,
  onInterruptVersion,
  onRemove,
  onRetry,
}: {
  tasks: TaskItem[];
  /** 正在播放移除动画的任务 id：动画结束前仍留在列表里。 */
  closingTaskIds: string[];
  /** 整个面板正在播放关闭动画：结束后由上层卸载。 */
  closing?: boolean;
  onClose: () => void;
  onClearFinished: () => void;
  /** 版本操作（保存/还原/整合包等）进行中的中断入口；不传则不显示按钮。 */
  onInterruptVersion?: () => void;
  onRemove: (id: string) => void;
  onRetry: (task: TaskItem) => void;
}) {
  const running = tasks.filter((task) => task.status === "running").length;
  const finished = tasks.length - running;
  const ordered = [...tasks].sort((left, right) => {
    if (left.status === "running" && right.status !== "running") return -1;
    if (right.status === "running" && left.status !== "running") return 1;
    return right.createdAt - left.createdAt;
  });

  return (
    <Box
      aria-label="任务中心"
      className={`pz-task-panel${closing ? " is-closing" : ""}`}
      role="dialog"
    >
      <HStack align="start" gap="3" justify="space-between" wrap="wrap">
        <Box flex="1" minW="180px">
          <Text color="gray.900" fontSize="md" fontWeight="700">任务中心</Text>
          <Text color="gray.500" fontSize="xs" mt="1">
            {running > 0 ? running + " 个进行中" : "没有进行中的任务"}
            {finished > 0 ? " · " + finished + " 个已结束" : ""}
          </Text>
        </Box>
        <HStack gap="1">
          <Button disabled={finished === 0} onClick={onClearFinished} size="xs" variant="ghost">清除已完成</Button>
          <Button aria-label="关闭任务中心" onClick={onClose} size="xs" variant="ghost">×</Button>
        </HStack>
      </HStack>

      <Box className="pz-task-list" mt="3">
        {ordered.length === 0 ? (
          <Text color="gray.500" fontSize="sm" py="6" textAlign="center">
            暂无任务。下载光影、保存/还原版本、清理内存等后台操作会出现在这里。
          </Text>
        ) : (
          ordered.map((task) => {
            const closing = closingTaskIds.includes(task.id);
            const percent =
              task.kind === "version-operation"
                ? (task.percent ?? null)
                : task.total && task.total > 0
                  ? Math.min(100, Math.round((task.downloaded / task.total) * 100))
                  : null;
            return (
              <Box
                className={["pz-task-slot", closing ? "is-closing" : ""].filter(Boolean).join(" ")}
                key={task.id}
              >
                <Box className={["pz-task-row", "pz-task-" + task.status].join(" ")}>
                  <HStack align="start" gap="2" justify="space-between" wrap="wrap">
                    <Box flex="1" minW="0">
                      <Text color="gray.900" fontSize="sm" fontWeight="700" overflowWrap="anywhere">
                        {task.title}
                      </Text>
                      <Text color="gray.500" fontSize="xs" mt="1" overflowWrap="anywhere">
                        {task.detail}
                      </Text>
                    </Box>
                    <Badge colorPalette={taskStatusPalette(task.status)} flexShrink="0" size="sm">
                      {taskStatusLabel(task.status)}
                    </Badge>
                  </HStack>

                  {task.status === "running" && (
                    <Box mt="2">
                      <Box
                        aria-label={"任务进度：" + task.title}
                        aria-valuemax={100}
                        aria-valuemin={0}
                        aria-valuenow={percent ?? undefined}
                        className={[
                          "pz-download-progress",
                          "pz-task-progress",
                          task.kind === "memory-clean" ? "pz-task-progress-indeterminate" : "",
                        ]
                          .filter(Boolean)
                          .join(" ")}
                        role="progressbar"
                      >
                        <Box as="span" style={{ width: percent !== null ? percent + "%" : "35%" }} />
                      </Box>
                      <Text color="gray.500" fontSize="xs" mt="1">
                        {task.kind === "memory-clean"
                          ? task.usedPercent != null
                            ? `系统占用 ${task.usedPercent}%`
                            : "正在读取内存状态…"
                          : task.kind === "version-operation"
                          ? [
                              percent !== null ? `${percent}%` : null,
                              task.step != null && task.steps != null && task.steps > 1
                                ? `${task.step}/${task.steps}`
                                : null,
                              task.doneFiles != null && task.totalFiles != null
                                ? `${task.doneFiles} / ${task.totalFiles} 个文件`
                                : null,
                            ]
                              .filter(Boolean)
                              .join(" · ")
                          : `${formatDownloadSize(task.downloaded)}${
                              task.total ? " / " + formatDownloadSize(task.total) : ""
                            }${percent !== null ? " · " + percent + "%" : ""}`}
                      </Text>
                    </Box>
                  )}

                  {task.kind === "shader-download" && task.status === "success" && task.savedPath && (
                    <Text color="green.700" fontSize="xs" mt="2" overflowWrap="anywhere" title={task.savedPath}>
                      已保存：{task.savedPath}
                    </Text>
                  )}

                  {task.status === "error" && task.error && (
                    <Text color="red.600" fontSize="xs" mt="2" overflowWrap="anywhere">
                      {task.error}
                    </Text>
                  )}

                  <HStack gap="2" justify="space-between" mt="2" wrap="wrap">
                    <Text color="gray.500" fontSize="xs">
                      {new Date(task.createdAt).toLocaleTimeString("zh-CN", { hour12: false })}
                    </Text>
                    <HStack gap="1">
                      {task.kind === "shader-download" && task.status === "error" && (
                        <Button onClick={() => onRetry(task)} size="xs" variant="outline">重试</Button>
                      )}
                      {task.kind === "version-operation" && task.status === "running" && onInterruptVersion && (
                        <Button onClick={onInterruptVersion} size="xs" variant="outline">中断</Button>
                      )}
                      {task.status !== "running" && (
                        <Button disabled={closing} onClick={() => onRemove(task.id)} size="xs" variant="ghost">移除</Button>
                      )}
                    </HStack>
                  </HStack>
                </Box>
              </Box>
            );
          })
        )}
      </Box>
    </Box>
  );
}
