/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  Badge,
  Box,
  Button,
  Checkbox,
  HStack,
  Portal,
  Text,
} from "@chakra-ui/react";
import {
  ModVersionStatus,
  VersionCurrentMatch,
  VersionRecord,
  formatVersionBytes,
  formatVersionDate,
  formatVersionTime,
  modSourceLabel,
  shortVersionHash,
  sortVersionsNewestFirst,
  versionBaseTypeLabel,
  versionChangeSummary,
  versionDotSize,
  versionMatchLabel,
  versionOrdinal,
} from "../versioning";
import { HintDot } from "./common";
import { useDialogAutoHeight } from "./shell";
import { useEffect, useRef } from "react";

export function ModVersionTimeline({
  busy,
  currentMatch,
  onDelete,
  onRestore,
  versions,
}: {
  busy: boolean;
  currentMatch?: VersionCurrentMatch | null;
  onDelete: (record: VersionRecord) => void;
  onRestore: (record: VersionRecord) => void;
  versions: VersionRecord[];
}) {
  if (versions.length === 0) {
    return (
      <Text color="gray.500" fontSize="sm" mt="2">
        还没保存过版本。先保存一次当前模组吧。
      </Text>
    );
  }
  const ordered = sortVersionsNewestFirst(versions);
  return (
    <Box className="pz-version-timeline">
      {ordered.map((record, index) => {
        const dotSize = versionDotSize(index);
        return (
          <Box className="pz-version-timeline-item" key={record.id}>
            <Box aria-hidden="true" className="pz-version-timeline-rail">
              <Box
                className={`pz-version-timeline-dot${index === 0 ? " is-latest" : ""}`}
                style={{ width: `${dotSize}px`, height: `${dotSize}px`, fontSize: `${Math.max(9, dotSize - 7)}px` }}
              >
                {versionOrdinal(index)}
              </Box>
              {index < ordered.length - 1 ? <Box className="pz-version-timeline-line" /> : null}
            </Box>
            <Box className="pz-version-timeline-body">
              <HStack align="center" gap="2" justify="space-between" wrap="nowrap">
                <Box className="pz-version-timeline-date">{formatVersionDate(record.createdAt)}</Box>
                <HStack align="center" flexShrink="0" gap="1" justify="flex-end" wrap="wrap">
                  <Badge colorPalette={record.baseType === "full" ? "purple" : "blue"} size="sm">
                    {versionBaseTypeLabel(record.baseType, record.parentId)}
                  </Badge>
                  {currentMatch?.versionId === record.id ? (
                    <Badge colorPalette="green" size="sm">当前</Badge>
                  ) : null}
                </HStack>
              </HStack>
              <Text color="gray.500" fontSize="xs" mt="1">
                {formatVersionTime(record.createdAt)} · {record.fileCount} 个文件 ·{" "}
                {formatVersionBytes(record.totalBytes)} · {shortVersionHash(record.rootHash)}
              </Text>
              <Text color="gray.500" fontSize="xs" mt="0.5">
                {record.changeLog
                  ? `相比上一个版本：${versionChangeSummary(record.changeLog)}`
                  : "完整备份"}
              </Text>
              <HStack gap="1" mt="1" wrap="wrap">
                <Button
                  disabled={busy}
                  onClick={() => onRestore(record)}
                  size="xs"
                  variant="outline"
                >
                  还原到这个版本
                </Button>
                {index === 0 ? (
                  <Button
                    colorPalette="red"
                    disabled={busy}
                    onClick={() => onDelete(record)}
                    size="xs"
                    variant="ghost"
                  >
                    删除
                  </Button>
                ) : null}
              </HStack>
            </Box>
          </Box>
        );
      })}
    </Box>
  );
}

export function VersionStoreDriveDialog({
  closing,
  dontAskAgain,
  onCheckDontAskAgain,
  onClose,
  onContinue,
  onGoChange,
  storePath,
}: {
  closing: boolean;
  dontAskAgain: boolean;
  onCheckDontAskAgain: (checked: boolean) => void;
  onClose: () => void;
  onContinue: () => void;
  onGoChange: () => void;
  storePath: string;
}) {
  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay ${closing ? "is-closing" : ""}`}
        onClick={onClose}
      >
        <Box
          aria-label="备份保存在 C 盘"
          aria-modal="true"
          className="pz-resource-file-dialog pz-version-drive-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">备份保存在 C 盘</Text>
            <Button aria-label="关闭换盘提示" onClick={onClose} size="sm" variant="ghost">×</Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3">
            版本备份会占用大量空间，放在 C 盘（系统盘）可能拖慢系统，也可能把系统盘装满。
            建议把「备份保存位置」换到其它盘。
          </Text>
          <Text color="gray.500" fontSize="xs" mt="2" overflowWrap="anywhere">
            当前位置：{storePath}
          </Text>
          <Checkbox.Root
            checked={dontAskAgain}
            className="pz-version-drive-dont-ask"
            mt="4"
            onCheckedChange={(details) => onCheckDontAskAgain(Boolean(details.checked))}
          >
            <Checkbox.HiddenInput />
            <Checkbox.Control>
              <Checkbox.Indicator />
            </Checkbox.Control>
            <Checkbox.Label>不再弹出该提示</Checkbox.Label>
          </Checkbox.Root>
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onGoChange} size="sm" variant="ghost">去换盘</Button>
            <Button colorPalette="blue" onClick={onContinue} size="sm">仍要继续</Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}

export function ModVersionDialog({
  closing,
  loading,
  name,
  onClose,
  onDelete,
  onRecord,
  onRestore,
  onVerify,
  status,
}: {
  closing: boolean;
  loading: boolean;
  name: string;
  onClose: () => void;
  onDelete: (record: VersionRecord) => void;
  onRecord: () => void;
  onRestore: (record: VersionRecord) => void;
  onVerify: () => void;
  status: ModVersionStatus | null;
}) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const dialogHeightRef = useRef<number | null>(null);
  useDialogAutoHeight(dialogRef, dialogHeightRef, [status?.versions.length, loading]);

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onClose]);

  const busy = loading || (status?.operationRunning ?? false);
  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay ${closing ? "is-closing" : ""}`}
        onClick={onClose}
      >
        <Box
          aria-label={`${name} 版本记录`}
          aria-modal="true"
          className="pz-resource-file-dialog pz-mod-version-dialog"
          onClick={(event) => event.stopPropagation()}
          ref={dialogRef}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between" wrap="wrap">
            <Box flex="1" minW="0">
              <Text color="gray.900" fontSize="lg" fontWeight="700" lineClamp={2}>
                {name} · 版本记录
              </Text>
              <HStack align="center" gap="1" mt="1" wrap="wrap">
                <Text color="gray.500" fontSize="xs">
                  {status ? `${modSourceLabel(status.target.source)} · 模组目录` : "正在读取版本信息…"}
                </Text>
                {status ? <HintDot label="模组目录" text={status.target.rootPath} /> : null}
              </HStack>
            </Box>
            <HStack gap="1" wrap="wrap">
              <Button disabled={busy || !status} onClick={onRecord} size="sm" variant="outline">
                保存当前版本
              </Button>
              <Button disabled={busy || !status} onClick={onVerify} size="sm" variant="ghost">
                检查当前版本
              </Button>
              <Button aria-label="关闭" onClick={onClose} size="sm" variant="ghost">×</Button>
            </HStack>
          </HStack>

          {loading && !status ? (
            <Text color="gray.500" fontSize="sm" mt="4">正在读取版本信息…</Text>
          ) : status ? (
            <>
              {/* 没有版本时只留时间线的空状态，避免同一句话出现两遍。 */}
              {status.versions.length > 0 ? (
                <HStack align="center" gap="2" mt="3" wrap="wrap">
                  <Badge colorPalette={status.currentMatch.kind === "unknown" ? "orange" : "blue"} size="sm">
                    {versionMatchLabel(status.currentMatch, status.versions)}
                  </Badge>
                  <Text color="gray.500" fontSize="xs">{status.currentMatch.message}</Text>
                </HStack>
              ) : null}
              {status.pendingChanges ? (
                <Text color="orange.700" fontSize="xs" mt="1">
                  与最新版本相比：新增 {status.pendingChanges.added} · 删除 {status.pendingChanges.removed} · 修改 {status.pendingChanges.modified}。
                </Text>
              ) : null}
              <Box className="pz-mod-version-dialog-body">
                <ModVersionTimeline
                  busy={busy}
                  currentMatch={status.currentMatch}
                  onDelete={onDelete}
                  onRestore={onRestore}
                  versions={status.versions}
                />
              </Box>
            </>
          ) : null}
        </Box>
      </Box>
    </Portal>
  );
}
