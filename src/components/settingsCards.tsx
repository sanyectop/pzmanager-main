/** 设置页的分区卡片：分区标题在卡片外，行放白卡片里、行间细分隔线（参考 PCL2 的分组布局）。 */
import { Box, HStack, Text } from "@chakra-ui/react";
import type { ReactNode } from "react";

/** 一个分区：标题在卡片外（加粗深色），下面是一张白色圆角卡片。 */
export function SettingsSection({
  title,
  titleExtra,
  children,
}: {
  title: string;
  /** 标题右侧的附加元素（如「网络」的说明气泡）。 */
  titleExtra?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Box className="pz-settings-section" mt="6">
      <HStack align="center" gap="2">
        <Text color="gray.900" fontSize="sm" fontWeight="700">
          {title}
        </Text>
        {titleExtra}
      </HStack>
      <Box className="pz-settings-card">{children}</Box>
    </Box>
  );
}

/** 卡片里的一行：左侧标签 + 灰色说明，右侧控件；行与行之间是细分隔线。 */
export function SettingsRow({
  label,
  hint,
  children,
}: {
  label: string;
  /** 标签下方的灰色说明文字。 */
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <HStack className="pz-settings-row" align="center" gap="4" justify="space-between" p="4" wrap="wrap">
      <Box flex="1" minW="240px">
        <Text color="gray.800" fontSize="sm" fontWeight="700">
          {label}
        </Text>
        {hint && (
          <Text color="gray.500" fontSize="xs" mt="1">
            {hint}
          </Text>
        )}
      </Box>
      {children}
    </HStack>
  );
}
