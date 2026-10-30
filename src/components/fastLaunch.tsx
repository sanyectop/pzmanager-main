/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Box, Button, HStack, Portal, Text } from "@chakra-ui/react";
import { FastLaunchCacheStatus } from "../app/types";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export function FastLaunchCacheStatusLine({ enabled }: { enabled: boolean }) {
  const [status, setStatus] = useState<FastLaunchCacheStatus | null>(null);

  useEffect(() => {
    if (!enabled) {
      setStatus(null);
      return;
    }
    let active = true;
    invoke<FastLaunchCacheStatus>("fast_launch_cache_status")
      .then((value) => {
        if (active) {
          setStatus(value);
        }
      })
      .catch(() => {
        if (active) {
          setStatus(null);
        }
      });
    return () => {
      active = false;
    };
  }, [enabled]);

  if (!enabled) {
    return null;
  }
  if (!status) {
    return (
      <Text color="gray.500" fontSize="xs" mt="1">
        正在检查加速记录…
      </Text>
    );
  }
  if (!status.exists) {
    return (
      <Text color="gray.500" fontSize="xs" mt="1">
        还没有加速记录：开启本开关后，到「模组管理 › JAR」点一次「刷新 JAR 列表」即可生成。
      </Text>
    );
  }
  return (
    <Text color={status.matches ? "gray.500" : "orange.700"} fontSize="xs" mt="1">
      上次加速生成于 {new Date(status.generatedAt * 1000).toLocaleString()} · 记录了 {status.modCount} 个模组
      {status.matches
        ? " · 与当前启用的模组一致，启动时会直接使用"
        : " · 模组列表已变化，建议到「模组管理 › JAR」点一次「刷新 JAR 列表」更新"}
    </Text>
  );
}

export function FastLaunchWarningDialog({
  closing,
  onCancel,
  onConfirm,
}: {
  closing: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCancel();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay ${closing ? "is-closing" : ""}`}
        onClick={onCancel}
      >
        <Box
          aria-label="开启极速启动"
          aria-modal="true"
          className="pz-resource-file-dialog pz-fast-launch-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">开启极速启动？</Text>
            <Button aria-label="关闭极速启动警告" onClick={onCancel} size="sm" variant="ghost">×</Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3">
            极速启动会修改游戏的启动方式，只对通过本启动器「启动游戏」拉起的游戏生效。
          </Text>
          <Text color="gray.600" fontSize="sm" mt="2">
            建议只在单机或自建服务器环境使用。开启时会自动备份启动配置；任何一步出错都会自动退回游戏原本的加载方式，不影响正常游玩。
          </Text>
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onCancel} size="sm" variant="ghost">取消</Button>
            <Button colorPalette="orange" onClick={onConfirm} size="sm">仍要开启</Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}
