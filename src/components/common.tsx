/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import { Box, Portal, Text, Tooltip } from "@chakra-ui/react";
import { FallbackImage } from "../FallbackImage";
import { ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { mimeTypeFor } from "../app/utils";

export function DeveloperNewsCard({ revealIndex, children }: { revealIndex: number; children: ReactNode }) {
  const ref = useRef<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const header = Array.from(element.children).filter(
      (child): child is HTMLElement => child instanceof HTMLElement && !child.classList.contains("pz-developer-news-content"),
    );
    const body = element.querySelectorAll<HTMLElement>(
      ".pz-developer-news-content p, .pz-developer-news-content h1, .pz-developer-news-content h2, .pz-developer-news-content h3, .pz-developer-news-content h4, .pz-developer-news-content li, .pz-developer-news-content img, .pz-developer-news-content blockquote",
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

export function ShotThumbnail({ name }: { name: string }) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    invoke<ArrayBuffer>("read_viewpoint_shot", { name })
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
  }, [name]);

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
