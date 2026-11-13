/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  Badge,
  Box,
  Button,
  Checkbox,
  HStack,
  Input,
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
  versionCanDelete,
  versionChangeSummary,
  versionDotSize,
  versionIsStandalone,
  versionMatchLabel,
  versionOrdinal,
} from "../versioning";
import { HintDot, SkeletonGate, SkeletonList } from "./common";
import { useDialogAutoHeight } from "./shell";
import { useEffect, useRef, useState } from "react";

export function ModVersionTimeline({
  busy,
  currentMatch,
  onConvert,
  onDelete,
  onDeleteThrough,
  onRestore,
  versions,
}: {
  busy: boolean;
  currentMatch?: VersionCurrentMatch | null;
  onConvert: (record: VersionRecord) => void;
  onDelete: (record: VersionRecord) => void;
  onDeleteThrough: (record: VersionRecord) => void;
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
                {versionOrdinal(index, ordered.length)}
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
                {record.baseType === "delta" && !versionIsStandalone(record, versions) ? (
                  <Button
                    disabled={busy}
                    onClick={() => onConvert(record)}
                    size="xs"
                    title="和最近的完整备份重新比对、重写增量包；之后可删除它之前的旧版本"
                    variant="ghost"
                  >
                    转换为独立版本
                  </Button>
                ) : null}
                {versionCanDelete(record, versions) ? (
                  <Button
                    colorPalette="red"
                    disabled={busy}
                    onClick={() => onDelete(record)}
                    size="xs"
                    variant="ghost"
                  >
                    删除
                  </Button>
                ) : (
                  <Button
                    colorPalette="red"
                    disabled
                    size="xs"
                    title="第一个完整备份还被后面的版本依赖，不能删除；先删掉它后面的版本。"
                    variant="ghost"
                  >
                    删除
                  </Button>
                )}
                <Button
                  colorPalette="red"
                  disabled={busy}
                  onClick={() => onDeleteThrough(record)}
                  size="xs"
                  title="删除这个版本和它之前的所有版本；之后的版本会重建成新的独立基准"
                  variant="ghost"
                >
                  删除及更早
                </Button>
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

/** 还原完成后 PZ Optimization 未能自动重装的警告：还原本身已成功，只提示手动补救。 */
export function PzOptRestoreWarningDialog({
  error,
  onClose,
  onGoSupport,
}: {
  error: string;
  onClose: () => void;
  onGoSupport: () => void;
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
      <Box className="pz-resource-file-overlay" onClick={onClose}>
        <Box
          aria-label="PZ Optimization 未能自动重装"
          aria-modal="true"
          className="pz-resource-file-dialog pz-version-drive-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">PZ Optimization 未能自动重装</Text>
            <Button aria-label="关闭重装警告" onClick={onClose} size="sm" variant="ghost">×</Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3">
            版本已还原完成，游戏文件不受影响；但 PZ Optimization 没能自动装回去，游戏内的模组功能可能失效。
          </Text>
          <Text color="red.600" fontSize="xs" mt="2" overflowWrap="anywhere">
            原因：{error}
          </Text>
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onClose} size="sm" variant="ghost">知道了</Button>
            <Button colorPalette="blue" onClick={onGoSupport} size="sm">去支持库安装</Button>
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
  onConvert,
  onDelete,
  onDeleteThrough,
  onRecord,
  onRestore,
  onVerify,
  status,
}: {
  closing: boolean;
  loading: boolean;
  name: string;
  onClose: () => void;
  onConvert: (record: VersionRecord) => void;
  onDelete: (record: VersionRecord) => void;
  onDeleteThrough: (record: VersionRecord) => void;
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

          {/* 首次读取用骨架屏占位（复用资源中心那套，模组行带海报占位）。 */}
          <SkeletonGate
            loading={loading && !status}
            skeleton={<SkeletonList rows={3} thumb="poster" />}
          >
          {loading && !status ? null : status ? (
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
                  onConvert={onConvert}
                  onDelete={onDelete}
                  onDeleteThrough={onDeleteThrough}
                  onRestore={onRestore}
                  versions={status.versions}
                />
              </Box>
            </>
          ) : null}
          </SkeletonGate>
        </Box>
      </Box>
    </Portal>
  );
}

/** 历史版本重命名弹窗：只改显示名（版本内容、依赖链、还原范围都不变）。
    与整合包命名弹窗同一套外观；名称必填，回车确认、Esc/遮罩取消。 */
export function VersionRenameDialog({
  closing,
  busy,
  initialLabel,
  onCancel,
  onConfirm,
}: {
  closing: boolean;
  busy: boolean;
  initialLabel: string;
  onCancel: () => void;
  onConfirm: (label: string) => void;
}) {
  const [draft, setDraft] = useState(initialLabel);
  const trimmed = draft.trim();
  const confirmDisabled = busy || !trimmed;

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
          aria-label="重命名版本"
          aria-modal="true"
          className="pz-resource-file-dialog pz-version-drive-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              重命名版本
            </Text>
            <Button aria-label="关闭重命名版本" onClick={onCancel} size="sm" variant="ghost">
              ×
            </Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3">
            只改这条版本的显示名，版本内容和还原范围不变。名称也会显示在启动页的启动按钮下方。
          </Text>
          <Input
            aria-label="版本名称"
            autoFocus
            disabled={busy}
            maxLength={60}
            mt="3"
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !confirmDisabled) {
                onConfirm(trimmed);
              }
            }}
            placeholder="版本名称"
            size="sm"
            value={draft}
          />
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onCancel} size="sm" variant="ghost">取消</Button>
            <Button
              colorPalette="blue"
              disabled={confirmDisabled}
              onClick={() => onConfirm(trimmed)}
              size="sm"
            >
              重命名
            </Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}

/** 整合包命名 / 重命名弹窗：保存全部模组时可以起名，之后也能在列表里改名。
    `mode = "save"` 时名称可留空（后端回落「整合包 N」）；`mode = "rename"` 必填。 */
export function ModPackNameDialog({
  closing,
  busy,
  initialLabel,
  mode,
  onCancel,
  onConfirm,
}: {
  closing: boolean;
  busy: boolean;
  initialLabel: string;
  mode: "save" | "rename";
  onCancel: () => void;
  onConfirm: (label: string) => void;
}) {
  const [draft, setDraft] = useState(initialLabel);
  const trimmed = draft.trim();

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onCancel();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  const saving = mode === "save";
  const confirmDisabled = busy || (!saving && !trimmed);

  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay ${closing ? "is-closing" : ""}`}
        onClick={onCancel}
      >
        <Box
          aria-label={saving ? "保存整合包" : "重命名整合包"}
          aria-modal="true"
          className="pz-resource-file-dialog pz-version-drive-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              {saving ? "保存整合包" : "重命名整合包"}
            </Text>
            <Button
              aria-label={saving ? "关闭保存整合包" : "关闭重命名整合包"}
              onClick={onCancel}
              size="sm"
              variant="ghost"
            >
              ×
            </Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3">
            {saving
              ? "会把当前所有模组各存一个版本（第一次是完整备份，占用空间较大），并记成一条整合包；以后可以一键整套还原。"
              : "只改这条整合包的显示名，里面的模组版本和还原范围不变。"}
          </Text>
          <Input
            aria-label="整合包名称"
            autoFocus
            disabled={busy}
            maxLength={60}
            mt="3"
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !confirmDisabled) {
                onConfirm(trimmed);
              }
            }}
            placeholder={saving ? "给整合包起个名字（可选，留空自动编号）" : "整合包名称"}
            size="sm"
            value={draft}
          />
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onCancel} size="sm" variant="ghost">取消</Button>
            <Button
              colorPalette="blue"
              disabled={confirmDisabled}
              onClick={() => onConfirm(trimmed)}
              size="sm"
            >
              {saving ? "保存整合包" : "重命名"}
            </Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}

/** 游戏版本「完整保存」命名弹窗：名称可留空（后端自动命名「第一次完整备份 / 完整备份」）。
    正文说明由调用方按首次保存 / 重复保存算好传进来（bodyText）。 */
export function VersionNameDialog({
  closing,
  busy,
  bodyText,
  onCancel,
  onConfirm,
}: {
  closing: boolean;
  busy: boolean;
  bodyText: string;
  onCancel: () => void;
  onConfirm: (label: string) => void;
}) {
  const [draft, setDraft] = useState("");
  const trimmed = draft.trim();

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
          aria-label="完整保存游戏版本"
          aria-modal="true"
          className="pz-resource-file-dialog pz-version-drive-dialog"
          onClick={(event) => event.stopPropagation()}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              完整保存
            </Text>
            <Button
              aria-label="关闭完整保存"
              onClick={onCancel}
              size="sm"
              variant="ghost"
            >
              ×
            </Button>
          </HStack>
          <Text color="gray.600" fontSize="sm" mt="3" whiteSpace="pre-line">
            {bodyText}
          </Text>
          <Input
            aria-label="版本名称"
            autoFocus
            disabled={busy}
            maxLength={60}
            mt="3"
            onChange={(event) => setDraft(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !busy) {
                onConfirm(trimmed);
              }
            }}
            placeholder="给这个版本起个名字（可选，留空自动命名）"
            size="sm"
            value={draft}
          />
          <HStack justify="flex-end" gap="2" mt="4" wrap="wrap">
            <Button onClick={onCancel} size="sm" variant="ghost">取消</Button>
            <Button
              colorPalette="blue"
              disabled={busy}
              onClick={() => onConfirm(trimmed)}
              size="sm"
            >
              完整保存
            </Button>
          </HStack>
        </Box>
      </Box>
    </Portal>
  );
}
