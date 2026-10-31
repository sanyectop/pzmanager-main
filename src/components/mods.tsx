/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  Badge,
  Box,
  Button,
  HStack,
  Menu,
  Portal,
  Text,
} from "@chakra-ui/react";
import { DetectionState, LuaModEntry } from "../app/types";
import { FallbackImage } from "../FallbackImage";
import { ModWorkshopLink, modWorkshopLink } from "../modWorkshop";
import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ZOMBIE_BUDDY_WORKSHOP_ID } from "../app/constants";
import { invoke } from "@tauri-apps/api/core";
import { mimeTypeFor } from "../app/utils";

export function JarModPoster({ name, path }: { name: string; path: string | null }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    if (!path) {
      setUrl(null);
      return () => {
        active = false;
      };
    }

    invoke<ArrayBuffer>("read_jar_mod_poster", { path })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(path) }));
        setUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setUrl(null);
        }
      });

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [path]);

  const initial = name.trim().charAt(0).toUpperCase() || "?";
  return (
    <Box className="pz-jar-mod-poster" aria-hidden="true">
      {url ? <FallbackImage alt="" loading="lazy" src={url} /> : <Box className="pz-jar-mod-poster-placeholder"><Text>{initial}</Text></Box>}
    </Box>
  );
}

export function LuaModPoster({ name, path }: { name: string; path: string | null }) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;
    if (!path) { setUrl(null); return () => { active = false; }; }
    invoke<ArrayBuffer>("read_lua_mod_poster", { path })
      .then((bytes) => { if (active) { objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(path) })); setUrl(objectUrl); } })
      .catch(() => { if (active) setUrl(null); });
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [path]);
  const initial = name.trim().charAt(0).toUpperCase() || "?";
  return <Box className="pz-jar-mod-poster" aria-hidden="true">{url ? <FallbackImage alt="" loading="lazy" src={url} /> : <Box className="pz-jar-mod-poster-placeholder"><Text>{initial}</Text></Box>}</Box>;
}

export function ModRowContextMenu({
  busy,
  canEnable,
  children,
  enabled,
  onDisable,
  onEnable,
  onOpenVersionHistory,
  onOpenWorkshop,
  workshopId,
}: {
  busy: boolean;
  /** 例如非 B42 的 Lua 模组，启动器不能启用。 */
  canEnable: boolean;
  children: ReactNode;
  enabled: boolean;
  onDisable: () => void;
  onEnable: () => void;
  /** 打开这个模组的「版本记录」模态框。 */
  onOpenVersionHistory: () => void;
  onOpenWorkshop: (link: ModWorkshopLink) => void;
  workshopId: string | number | null;
}) {
  const workshopLink = modWorkshopLink(workshopId);
  return (
    <Menu.Root positioning={{ strategy: "fixed", flip: true, shift: 8, overflowPadding: 8 }}>
      {/* 整行都是右键触发区；左键的选中、拖动和开关逻辑仍留在行自己身上。 */}
      <Menu.ContextTrigger asChild>{children}</Menu.ContextTrigger>
      {/* 挂到 Portal：模组行自身的位移动画与列表的滚动裁剪会把就地渲染的浮层切掉。 */}
      <Portal>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-mod-context-menu">
            <Menu.Item
              disabled={busy || enabled || !canEnable}
              onSelect={onEnable}
              value="enable"
            >
              <Text>启用</Text>
            </Menu.Item>
            <Menu.Item disabled={busy || !enabled} onSelect={onDisable} value="disable">
              <Text>禁用</Text>
            </Menu.Item>
            <Menu.Item
              disabled={workshopLink === null}
              onSelect={() => {
                if (workshopLink) {
                  onOpenWorkshop(workshopLink);
                }
              }}
              title={workshopLink ? "在 Steam 创意工坊中打开" : "本地模组没有创意工坊页面"}
              value="workshop"
            >
              <Text>创意工坊打开</Text>
            </Menu.Item>
            <Menu.Item onSelect={onOpenVersionHistory} value="versions">
              <Text>版本记录</Text>
            </Menu.Item>
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  );
}

export function SleepingModRow({
  render,
}: {
  render: () => ReactNode;
}) {
  const shellRef = useRef<HTMLDivElement | null>(null);
  const [awake, setAwake] = useState(false);
  const [sleepHeight, setSleepHeight] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (awake && shellRef.current) {
      setSleepHeight(shellRef.current.offsetHeight);
    }
  }, [awake]);

  useEffect(() => {
    const element = shellRef.current;
    if (!element) {
      return;
    }
    if (typeof IntersectionObserver === "undefined") {
      setAwake(true);
      return;
    }

    const root = element.closest(".pz-lua-mod-list, .pz-jar-mods-body");
    const observer = new IntersectionObserver(
      ([entry]) => {
        const nextAwake = entry?.isIntersecting ?? true;
        if (!nextAwake && shellRef.current) {
          setSleepHeight(shellRef.current.offsetHeight);
        }
        setAwake(nextAwake);
      },
      {
        root,
        rootMargin: "480px 0px",
      },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <Box
      className={`pz-mod-sleep-row${awake ? " is-awake" : " is-sleeping"}`}
      ref={shellRef}
      style={!awake && sleepHeight !== null ? { height: `${sleepHeight}px` } : undefined}
    >
      {awake ? render() : <Box className="pz-mod-sleep-placeholder" />}
    </Box>
  );
}

export function ModBatchBar({
  busyAction,
  negativeLabel,
  onAutoSort,
  onClear,
  onInvert,
  onNegative,
  onPositive,
  onSelectAll,
  positiveLabel,
  selectedCount,
  totalCount,
}: {
  busyAction: "positive" | "negative" | "sort" | null;
  negativeLabel: string;
  /** 自动排序（只对 Lua 模组页提供）：对全部已启用模组重排加载顺序。 */
  onAutoSort?: () => void;
  onClear: () => void;
  onInvert: () => void;
  onNegative: () => void;
  onPositive: () => void;
  onSelectAll: () => void;
  positiveLabel: string;
  selectedCount: number;
  totalCount: number;
}) {
  const busy = busyAction !== null;
  return (
    <HStack className="pz-mod-batch-bar" gap="1" wrap="wrap">
      <Text className="pz-mod-batch-count">已选 {selectedCount} / {totalCount}</Text>
      <Menu.Root>
        <Menu.Trigger asChild>
          <Button disabled={busy || totalCount === 0} size="xs" variant="ghost">
            选择操作
          </Button>
        </Menu.Trigger>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content" minW="150px">
            {onAutoSort ? (
              <Menu.Item disabled={busyAction === "sort"} onSelect={onAutoSort} value="auto-sort">
                自动排序加载顺序
              </Menu.Item>
            ) : null}
            <Menu.Item onSelect={onSelectAll} value="select-all">全选当前列表</Menu.Item>
            <Menu.Item onSelect={onInvert} value="invert">反选当前列表</Menu.Item>
            <Menu.Item disabled={selectedCount === 0} onSelect={onClear} value="clear">取消选择</Menu.Item>
          </Menu.Content>
        </Menu.Positioner>
      </Menu.Root>
      <Box className="pz-mod-batch-spacer" />
      <Button
        colorPalette="green"
        disabled={busy || selectedCount === 0}
        loading={busyAction === "positive"}
        onClick={onPositive}
        size="xs"
        variant="subtle"
      >
        {positiveLabel}
      </Button>
      <Button
        colorPalette="red"
        disabled={busy || selectedCount === 0}
        loading={busyAction === "negative"}
        onClick={onNegative}
        size="xs"
        variant="subtle"
      >
        {negativeLabel}
      </Button>
    </HStack>
  );
}

export function SupportOptionRow({
  badge,
  badgePalette,
  description,
  expanded,
  name,
  onToggle,
  children,
}: {
  badge: string;
  badgePalette: "green" | "orange" | "gray" | "blue";
  description: string;
  expanded: boolean;
  name: string;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <Box
      className={`pz-mod-row pz-support-option${expanded ? " pz-jar-mod-row-expanded" : ""}`}
      onClick={(event) => {
        const target = event.target as HTMLElement;
        if (target.closest("button, .pz-jar-mod-details-shell")) {
          return;
        }
        onToggle();
      }}
    >
      <Box className="pz-mod-summary pz-compact-mod-summary">
        <Box className="pz-mod-select-area">
          <Box className="pz-jar-mod-summary-content" minW="0">
            <Text className="pz-jar-mod-name" color="gray.900" fontWeight="700" overflowWrap="anywhere">
              {name}
            </Text>
            <Text className="pz-jar-mod-description" color="gray.500">
              {description}
            </Text>
          </Box>
        </Box>
        <HStack className="pz-jar-mod-badges" gap="1" wrap="nowrap">
          <Badge colorPalette={badgePalette}>{badge}</Badge>
        </HStack>
        <HStack align="center" className="pz-mod-actions" gap="1">
          <Button
            aria-expanded={expanded}
            aria-label={`${expanded ? "收起" : "展开"} ${name}详情`}
            className="pz-jar-mod-detail-button"
            onClick={(event) => {
              event.stopPropagation();
              onToggle();
            }}
            size="xs"
            title={expanded ? "收起" : "详情"}
            variant="ghost"
          >
            {expanded ? "×" : "⋯"}
          </Button>
        </HStack>
      </Box>
      <Box aria-hidden={!expanded} className="pz-jar-mod-details-shell">
        <Box className="pz-jar-mod-details">
          <Box className="pz-jar-mod-details-body">{children}</Box>
        </Box>
      </Box>
    </Box>
  );
}

export function ZombieBuddyLoaderPanel({
  busy,
  detection,
  developerMode,
  expanded,
  luaMod,
  schemeEnabled,
  onOpenModLoaderSettings,
  onToggle,
  onToggleLuaMod,
}: {
  busy: boolean;
  detection: DetectionState;
  developerMode: boolean;
  expanded: boolean;
  luaMod: LuaModEntry | null;
  /** 「模组加载器方案」是否选中了 ZombieBuddy。 */
  schemeEnabled: boolean;
  onOpenModLoaderSettings: () => void;
  onToggle: () => void;
  onToggleLuaMod: (mod: LuaModEntry) => void;
}) {
  const loader = detection.status === "found" ? detection.paths.zombieBuddy : null;
  const statusLabel =
    detection.status === "checking"
      ? "检测中"
      : loader?.installed
        ? "已安装"
        : loader?.available
          ? "待安装"
          : "未检测到";
  const statusPalette =
    loader?.installed ? "green" : loader?.available ? "orange" : "gray";

  return (
    <Box className={`pz-jar-conflict-panel pz-zombiebuddy-loader-panel${expanded ? " is-expanded" : ""}`} role="status">
      <Button
        aria-expanded={expanded}
        className="pz-jar-conflict-summary pz-zombiebuddy-loader-summary"
        onClick={onToggle}
        size="sm"
        variant="ghost"
      >
        <HStack gap="2" minW="0" wrap="wrap">
          <Box className="pz-jar-conflict-indicator pz-zombiebuddy-loader-indicator" />
          <Text color="gray.800" fontSize="sm" fontWeight="700">ZombieBuddy 模组加载器</Text>
          <Badge colorPalette={statusPalette} size="sm">{statusLabel}</Badge>
          {!schemeEnabled && <Badge colorPalette="gray" size="sm">未启用</Badge>}
          {developerMode && (
            <Badge colorPalette={luaMod?.enabled ? "green" : "gray"} size="sm">
              {luaMod?.enabled ? "Lua 模组已启用" : "Lua 模组未启用"}
            </Badge>
          )}
        </HStack>
        <Text aria-hidden="true" className="pz-jar-conflict-chevron">{expanded ? "▴" : "▾"}</Text>
      </Button>
      <Box className="pz-jar-conflict-details-shell" aria-hidden={!expanded}>
        <Box className="pz-jar-conflict-details">
          {developerMode && (
            <>
              <Text color="gray.500" fontSize="xs">
                这是 JAR 模组加载器，不属于普通 Lua 可排序模组。安装并启用后，ZombieBuddy 会负责审批和加载带 @Patch / @Exposer 注解的 JAR。
              </Text>
              <HStack gap="3" mt="2" wrap="wrap">
                <Text color="gray.600" fontSize="xs">Workshop ID：{ZOMBIE_BUDDY_WORKSHOP_ID}</Text>
                <Text color="gray.600" fontSize="xs">加载器 ID：ZombieBuddy</Text>
                <Text color="gray.600" fontSize="xs">
                  加载器方案：{schemeEnabled ? "ZombieBuddy" : "不使用（原版加载）"}
                </Text>
                {luaMod && (
                  <Text color="gray.600" fontSize="xs">
                    Lua 模组：{luaMod.enabled ? "已启用" : "未启用"}
                  </Text>
                )}
              </HStack>
              {loader?.sourcePath && (
                <Text color="gray.500" fontSize="xs" mt="2" overflowWrap="anywhere">
                  来源：{loader.sourcePath}
                </Text>
              )}
            </>
          )}
          {!loader?.installed && loader?.missingComponents.length ? (
            <Text color="gray.500" fontSize="xs" mt="2">
              缺少：{loader.missingComponents.join("、")}
            </Text>
          ) : null}
          <HStack gap="2" mt="3" wrap="wrap">
            {luaMod && (
              <Button
                colorPalette={luaMod.enabled ? "red" : "green"}
                disabled={busy}
                loading={busy}
                onClick={() => onToggleLuaMod(luaMod)}
                size="xs"
                variant="outline"
              >
                {luaMod.enabled ? "停用 Lua 模组" : "启用 Lua 模组"}
              </Button>
            )}
            {!loader?.installed && (
              <Button colorPalette="blue" onClick={onOpenModLoaderSettings} size="xs" variant="outline">
                前往 设置 › 模组加载器
              </Button>
            )}
          </HStack>
        </Box>
      </Box>
    </Box>
  );
}
