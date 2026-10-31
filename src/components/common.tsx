/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Box, Portal, Text, Tooltip } from "@chakra-ui/react";
import type { BoxProps } from "@chakra-ui/react";
import { FallbackImage } from "../FallbackImage";
import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { mimeTypeFor } from "../app/utils";

/**
 * 后端拼好的富文本（新闻正文等）：里面的 <img> 与 FallbackImage 一样，
 * 加载完成后才渐显（.pz-image-reveal / .is-loaded，样式见 App.css）。
 * 加载失败时退回普通显示，让浏览器展示破图状态，不把图藏掉。
 */
export function HtmlContentWithImageReveal({ html, ...rest }: { html: string } & BoxProps) {
  const contentRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const root = contentRef.current;
    if (!root) return;
    for (const img of Array.from(root.querySelectorAll("img"))) {
      img.classList.add("pz-image-reveal");
      if (img.complete && img.naturalWidth > 0) {
        img.classList.add("is-loaded");
        continue;
      }
      img.addEventListener(
        "load",
        () => {
          img.classList.add("is-loaded");
        },
        { once: true },
      );
      img.addEventListener(
        "error",
        () => {
          img.classList.remove("pz-image-reveal", "is-loaded");
        },
        { once: true },
      );
    }
  }, [html]);

  return <Box ref={contentRef} {...rest} dangerouslySetInnerHTML={{ __html: html }} />;
}

export function DeveloperNewsCard({ revealIndex, children }: { revealIndex: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const header = Array.from(element.children).filter(
      (child): child is HTMLElement => child instanceof HTMLElement && !child.classList.contains("pz-developer-news-content"),
    );
    // img 不参加条目弹出：图片走 HtmlContentWithImageReveal 的加载门控渐显，
    // 两个动画叠在同一段 opacity 上会「弹出 → 消失 → 再淡入」。
    const body = element.querySelectorAll<HTMLElement>(
      ".pz-developer-news-content p, .pz-developer-news-content h1, .pz-developer-news-content h2, .pz-developer-news-content h3, .pz-developer-news-content h4, .pz-developer-news-content li, .pz-developer-news-content blockquote",
    );
    const targets = [...header, ...Array.from(body)];
    targets.forEach((target, index) => {
      target.classList.add("pz-news-reveal");
      target.style.setProperty("--pz-reveal-index", String(revealIndex * 4 + Math.min(index, 11)));
    });
  }, [revealIndex]);
  return (
    <Box className="pz-developer-news-item" ref={ref}>
      {children}
    </Box>
  );
}

export function EmptyState({ text }: { text: string }) {
  return (
    <Box className="pz-empty-state" borderColor="gray.200" borderRadius="sm" borderWidth="1px" p="6" textAlign="center">
      <Text color="gray.500">{text}</Text>
    </Box>
  );
}

/** 骨架屏退场动画时长，与 App.css 里 .pz-skeleton-stage 的过渡时长保持一致。 */
const SKELETON_EXIT_MS = 180;

/**
 * 骨架屏装载器：loading 期间显示骨架；数据就绪后骨架先播退场动画
 * （.is-exiting，淡出 + 下移），播完再卸载并渲染正文。
 */
export function SkeletonGate({
  loading,
  skeleton,
  children,
}: {
  loading: boolean;
  skeleton: ReactNode;
  children: ReactNode;
}) {
  const [visible, setVisible] = useState(loading);
  const [exiting, setExiting] = useState(false);

  // loading 开始：重新显示骨架。
  useEffect(() => {
    if (!loading) {
      return;
    }
    setVisible(true);
    setExiting(false);
  }, [loading]);

  // loading 结束：先进入退场态，等过渡播完再卸载（不在 updater 里嵌套 setState）。
  useEffect(() => {
    if (loading || !visible || exiting) {
      return;
    }
    setExiting(true);
  }, [loading, visible, exiting]);

  useEffect(() => {
    if (!exiting) {
      return;
    }
    const timer = window.setTimeout(() => {
      setVisible(false);
      setExiting(false);
    }, SKELETON_EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [exiting]);

  if (!visible) {
    return <>{children}</>;
  }
  return (
    <Box aria-hidden="true" className={`pz-skeleton-stage${exiting ? " is-exiting" : ""}`}>
      {skeleton}
    </Box>
  );
}

/**
 * 列表骨架：缩略图（poster 1.9rem / icon 42px / preview 64px）+ 若干文字条，
 * 行高与各列表行一致，行间加入微小的宽度错位避免机械感。
 */
export function SkeletonList({
  rows,
  thumb,
  lines = 2,
}: {
  rows: number;
  /** 行首缩略图占位：poster（模组封面）/ icon（光影图标）/ preview（工坊预览图）；不传则只有文字条。 */
  thumb?: "poster" | "icon" | "preview";
  lines?: number;
}) {
  return (
    <Box className="pz-skeleton-list">
      {Array.from({ length: rows }).map((_, index) => (
        <Box className="pz-skeleton-row" key={index}>
          {thumb ? <Box className={`pz-skeleton-shine pz-skeleton-thumb-${thumb}`} /> : null}
          <Box className="pz-skeleton-lines">
            <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: index % 3 === 1 ? "44%" : "62%" }} />
            {lines > 1 && <Box className="pz-skeleton-shine pz-skeleton-line pz-skeleton-line-short" />}
          </Box>
        </Box>
      ))}
    </Box>
  );
}

/** 新闻卡片骨架：标题条 + 元信息条 + 三段正文条。 */
export function SkeletonNews({ cards = 3 }: { cards?: number }) {
  return (
    <Box className="pz-skeleton-list">
      {Array.from({ length: cards }).map((_, index) => (
        <Box className="pz-skeleton-news-card" key={index}>
          <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: "52%" }} />
          <Box className="pz-skeleton-shine pz-skeleton-line pz-skeleton-line-short" />
          <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: "94%" }} />
          <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: "88%" }} />
          <Box className="pz-skeleton-shine pz-skeleton-line" style={{ width: "70%" }} />
        </Box>
      ))}
    </Box>
  );
}

export function PathValue({
  label,
  value,
  fallback,
  error,
}: {
  label: string;
  value: string | null;
  fallback: string | null;
  error: boolean;
}) {
  return (
    <Box>
      <Text color="gray.500" fontSize="sm" fontWeight="600">
        {label}
      </Text>
      {value ? (
        <Box className="pz-value-box" bg="gray.50" borderColor="gray.200" borderRadius="sm" borderWidth="1px" mt="2" p="3">
          <Text color="gray.800" fontFamily="mono" fontSize="sm" overflowWrap="anywhere">
            {value}
          </Text>
        </Box>
      ) : (
        <Text color={error ? "red.600" : "gray.600"} mt="2">
          {fallback}
        </Text>
      )}
    </Box>
  );
}

export function MediaPanel({
  title,
  description,
  directory,
  developerMode = false,
  children,
}: {
  title: string;
  description: string;
  directory?: string;
  developerMode?: boolean;
  children: ReactNode;
}) {
  return (
    <Box className="pz-panel pz-media-panel">
      <Box className="pz-media-panel-header">
        <Text color="gray.900" fontSize="lg" fontWeight="700">
          {title}
        </Text>
        {developerMode && (
          <Text color="gray.500" fontSize="sm" mt="1">
            {description}
          </Text>
        )}
        {developerMode && directory && (
          <Text color="gray.400" fontFamily="mono" fontSize="xs" mt="2" overflowWrap="anywhere">
            {directory}
          </Text>
        )}
      </Box>
      <Box className="pz-media-panel-body">{children}</Box>
    </Box>
  );
}

/** 截图来源：Viewpoint 启动器截图，或 Steam 云端截图（userdata/<账号>/760/remote/108600）。 */
export type ShotsSource = "viewpoint" | "steam";

const SHOTS_READ_COMMAND: Record<ShotsSource, string> = {
  viewpoint: "read_viewpoint_shot",
  steam: "read_steam_screenshot",
};

export function ShotThumbnail({ name, source = "viewpoint" }: { name: string; source?: ShotsSource }) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    invoke<ArrayBuffer>(SHOTS_READ_COMMAND[source], { name })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: mimeTypeFor(name) }));
        setUrl(objectUrl);
      })
      .catch(() => {
        if (active) {
          setFailed(true);
        }
      });

    return () => {
      active = false;
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl);
      }
    };
  }, [name, source]);

  return (
    <Box
      alignItems="center"
      aspectRatio="16 / 10"
      bg="gray.200"
      className="pz-thumbnail"
      display="flex"
      justifyContent="center"
      overflow="hidden"
    >
      {url ? (
        <FallbackImage
          alt={name}
          src={url}
          style={{ height: "100%", objectFit: "cover", width: "100%" }}
        />
      ) : (
        <Text color="gray.500" fontSize="xs">
          {failed ? "无法加载缩略图" : "正在加载缩略图..."}
        </Text>
      )}
    </Box>
  );
}

export function HintDot({ label, text }: { label: string; text: string }) {
  return (
    <Tooltip.Root
      closeDelay={80}
      openDelay={120}
      positioning={{ placement: "top", flip: true, shift: 8, overflowPadding: 8 }}
    >
      <Tooltip.Trigger asChild>
        <Box aria-label={label} as="span" className="pz-hint-dot" role="img" tabIndex={0}>
          ?
        </Box>
      </Tooltip.Trigger>
      <Portal>
        <Tooltip.Positioner className="pz-menu-positioner">
          <Tooltip.Content className="pz-hint-tooltip">{text}</Tooltip.Content>
        </Tooltip.Positioner>
      </Portal>
    </Tooltip.Root>
  );
}
