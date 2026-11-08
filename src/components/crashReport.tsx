/** 游戏异常停止报告：原因 + 诊断 + 底部可疑 JAR 模组列表。
 *
 *  与 `ConfirmDialog` 同一套遮罩/面板样式（`pz-resource-file-overlay` +
 *  `pz-resource-file-dialog`），但不走 `showConfirm`：这个弹窗在打开后还要异步
 *  反查「哪个模组引用了报错的注入项」，结果回来时得能就地更新内容。
 *
 *  列表区域自己滚动（`.pz-crash-report-suspect-list`），诊断再长也只挤正文，
 *  不会把模组清单推出可视区。 */
import { Badge, Box, Button, HStack, Portal, Stack, Text, VStack } from "@chakra-ui/react";
import { useEffect, useRef } from "react";

import type { JarModInjectionMatch } from "../app/types";

export type CrashReport = {
  title: string;
  /** 已排版好的正文（结果 / 停止原因与退出码 / 诊断，段落之间空行）。 */
  body: string;
  /** 诊断点名、用于反查模组的注入项；null = 与注入项无关，不展示模组列表。 */
  suspectItem: string | null;
  /** 反查结果；`null` = 仍在查找。 */
  suspects: JarModInjectionMatch[] | null;
  suspectsError: string | null;
};

/** 模组的当前状态标签：已批准 / 未批准 / 已禁用。 */
function matchStateLabel(match: JarModInjectionMatch): { label: string; palette: string } {
  if (match.disabled) {
    return { label: "已禁用", palette: "gray" };
  }
  if (match.decision === true) {
    return { label: "已批准", palette: "orange" };
  }
  if (match.decision === false) {
    return { label: "未批准", palette: "gray" };
  }
  return { label: "待批准", palette: "gray" };
}

export function CrashReportDialog({
  closing,
  report,
  onClose,
}: {
  /** 退场中：加 is-closing 让遮罩与面板播关闭动画（与 ConfirmDialog 一致）。 */
  closing: boolean;
  report: CrashReport;
  onClose: () => void;
}) {
  const primaryRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    primaryRef.current?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const suspects = report.suspects;

  return (
    <Portal>
      <Box className={`pz-resource-file-overlay${closing ? " is-closing" : ""}`} onClick={onClose}>
        <Box
          aria-label={report.title}
          aria-modal="true"
          className="pz-resource-file-dialog pz-crash-report-dialog"
          onClick={(event) => event.stopPropagation()}
          role="alertdialog"
        >
          <Text color="gray.900" fontSize="md" fontWeight="700">
            {report.title}
          </Text>
          <Text className="pz-crash-report-body" color="gray.700" fontSize="sm" whiteSpace="pre-wrap">
            {report.body}
          </Text>
          {report.suspectItem && (
            <Box className="pz-crash-report-suspects">
              <HStack gap="2" justify="space-between" minW="0">
                <Text color="gray.800" fontSize="sm" fontWeight="700">
                  引用了该注入项的 JAR 模组
                </Text>
                {suspects && (
                  <Badge colorPalette="gray" size="sm">
                    {suspects.length} 个
                  </Badge>
                )}
              </HStack>
              <Text color="gray.500" fontFamily="mono" fontSize="xs" mt="1" overflowWrap="anywhere">
                {report.suspectItem}
              </Text>
              {report.suspectsError ? (
                <Text color="red.600" fontSize="xs" mt="2">
                  {report.suspectsError}
                </Text>
              ) : suspects === null ? (
                <Text color="gray.600" fontSize="xs" mt="2">
                  正在扫描 JAR 模组…
                </Text>
              ) : suspects.length === 0 ? (
                <Text color="gray.600" fontSize="xs" mt="2">
                  没有找到引用该注入项的 JAR 模组：问题更可能出在游戏本体或 Lua 模组上。
                </Text>
              ) : (
                <Stack className="pz-crash-report-suspect-list" gap="1" mt="2">
                  {suspects.map((match) => {
                    const state = matchStateLabel(match);
                    return (
                      <VStack
                        align="stretch"
                        className="pz-crash-report-suspect-row"
                        gap="0.5"
                        key={`${match.jarPath}-${match.id}`}
                      >
                        <HStack gap="2" minW="0">
                          <Text color="gray.800" fontSize="xs" fontWeight="700" truncate>
                            {match.id}
                          </Text>
                          <Badge colorPalette={state.palette} size="sm">
                            {state.label}
                          </Badge>
                        </HStack>
                        <Text
                          color="gray.500"
                          fontFamily="mono"
                          fontSize="xs"
                          overflowWrap="anywhere"
                        >
                          {match.matched.join("、")}
                        </Text>
                      </VStack>
                    );
                  })}
                </Stack>
              )}
              <Text color="gray.500" fontSize="xs" mt="2">
                建议先禁用或回滚上述模组，再启动游戏验证。
              </Text>
            </Box>
          )}
          <HStack gap="2" justify="flex-end" mt="4">
            <Button onClick={onClose} ref={primaryRef} size="sm">
              知道了
            </Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}
