/** 应用内确认对话框：替代原生 window.confirm（同步阻塞、样式割裂、按钮语义固定）。
 *  支持最多三个动作（value 由调用方定义）、危险主按钮标红、Esc/遮罩返回 cancelValue。 */
import { Box, Button, HStack, Portal, Text } from "@chakra-ui/react";
import { useEffect, useRef } from "react";

export type ConfirmAction = {
  label: string;
  value: string;
  /** 危险动作（删除 / 卸载 / 覆盖）标红。 */
  danger?: boolean;
};

export type ConfirmRequest = {
  title: string;
  /** 支持多行（\n）。 */
  body: string;
  actions: ConfirmAction[];
  /** Esc / 点遮罩 / Enter 前无焦点时返回的值；默认最后一个动作的 value（通常是「取消」）。 */
  cancelValue?: string;
};

export function ConfirmDialog({
  closing,
  request,
  onAction,
}: {
  closing: boolean;
  request: ConfirmRequest;
  onAction: (value: string) => void;
}) {
  const cancelValue = request.cancelValue ?? request.actions[request.actions.length - 1].value;
  const primaryRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    primaryRef.current?.focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onAction(cancelValue);
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [cancelValue, onAction]);

  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay${closing ? " is-closing" : ""}`}
        onClick={() => onAction(cancelValue)}
      >
        <Box
          aria-label={request.title}
          aria-modal="true"
          className="pz-resource-file-dialog pz-confirm-dialog"
          onClick={(event) => event.stopPropagation()}
          role="alertdialog"
        >
          <Text color="gray.900" fontSize="md" fontWeight="700">
            {request.title}
          </Text>
          <Text color="gray.700" fontSize="sm" mt="2" whiteSpace="pre-wrap">
            {request.body}
          </Text>
          <HStack gap="2" justify="flex-end" mt="4" wrap="wrap">
            {request.actions.map((action, index) => (
              <Button
                colorPalette={action.danger ? "red" : "blue"}
                onClick={() => onAction(action.value)}
                ref={index === 0 ? primaryRef : undefined}
                size="sm"
                variant={index === 0 ? "solid" : "outline"}
              >
                {action.label}
              </Button>
            ))}
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}
