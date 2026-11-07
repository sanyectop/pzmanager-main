/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  Box,
  Button,
  HStack,
  Menu,
  Portal,
  Slider,
  Text,
} from "@chakra-ui/react";
import { BUILTIN_BACKGROUNDS, MINUTES_PER_DAY } from "../app/constants";
import { BackgroundEntry, BuiltinBackground } from "../app/types";
import { FallbackImage } from "../FallbackImage";
import { WALLPAPER_DAY_MAX, WallpaperEffect, WallpaperEffectSettings, formatClock, formatWallpaperDate } from "../builtinWallpaper";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

export function BackgroundThumbnail({ path }: { path: string }) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;
    invoke<ArrayBuffer>("read_background", { path })
      .then((bytes) => {
        if (!active) {
          return;
        }
        objectUrl = URL.createObjectURL(new Blob([bytes]));
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

  return url ? (
    <FallbackImage alt="" className="pz-background-menu-thumbnail" loading="lazy" src={url} />
  ) : (
    <Box className="pz-background-menu-thumbnail-placeholder" />
  );
}

export function BackgroundMenu({
  customBackgrounds,
  workshopBackgrounds,
  workshopLoading,
  workshopScanProgress,
  onChooseCustom,
  onOpenRecommendedWorkshop,
  onScanWorkshop,
  onSelect,
  onSelectBuiltin,
  wallpaperEffect,
  wallpaperSettings,
  onSetWallpaperDay,
  onSetWallpaperTime,
  onToggleWallpaperPin,
  onToggleWallpaperAutoContrast,
}: {
  customBackgrounds: BackgroundEntry[];
  workshopBackgrounds: BackgroundEntry[];
  workshopLoading: boolean;
  workshopScanProgress: { scanned: number; total: number };
  onChooseCustom: () => void;
  onOpenRecommendedWorkshop: (workshopId: string) => void;
  onScanWorkshop: () => void;
  onSelect: (background: BackgroundEntry) => void;
  onSelectBuiltin: (background: BuiltinBackground) => void;
  wallpaperEffect: WallpaperEffect;
  wallpaperSettings: WallpaperEffectSettings;
  onSetWallpaperDay: (dayOfYear: number | null) => void;
  onSetWallpaperTime: (timeMinutes: number | null) => void;
  onToggleWallpaperPin: () => void;
  onToggleWallpaperAutoContrast: () => void;
}) {
  const workshopGroups = Array.from(
    workshopBackgrounds.reduce((groups, background) => {
      const key = `${background.workshopId ?? "unknown"}:${background.modName ?? background.name}`;
      const current = groups.get(key);
      if (current) {
        current.push(background);
      } else {
        groups.set(key, [background]);
      }
      return groups;
    }, new Map<string, BackgroundEntry[]>()),
  );

  const renderBackgroundItem = (background: BackgroundEntry, detailOnly = false) => (
    <Menu.Item
      key={background.id}
      value={background.id}
      onSelect={() => onSelect(background)}
    >
      <BackgroundThumbnail path={background.path} />
      <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
        {detailOnly ? (background.name.split("·", 2)[1]?.trim() ?? background.name) : background.name}
      </Text>
    </Menu.Item>
  );

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button className="pz-background-menu-trigger" size="sm" variant="outline">
          <HStack gap="1.5">
            <Text>自定义背景</Text>
            <Text aria-hidden="true" className="pz-workshop-tag-menu-chevron">
              ▾
            </Text>
          </HStack>
        </Button>
      </Menu.Trigger>
      <Portal>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 0,
              strategy: "fixed",
              flip: true,
              shift: -14,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">内置背景</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                {BUILTIN_BACKGROUNDS.map((background) => (
                  <Menu.Item
                    key={background.id}
                    value={`builtin-${background.id}`}
                    onSelect={() => onSelectBuiltin(background)}
                  >
                    <FallbackImage alt="" className="pz-background-menu-thumbnail" loading="lazy" src={background.url} />
                    <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                      {background.name}
                    </Text>
                  </Menu.Item>
                ))}
                <Menu.Separator />
                <Text color="gray.500" fontSize="xs" px="3" py="1">
                  四季轮转 · {wallpaperEffect.blendLabel} · {wallpaperEffect.clockLabel}
                  {wallpaperEffect.pinned ? " · 已固定" : wallpaperSettings.mode === "manual" ? " · 手动" : ""}
                </Text>
                <Menu.Item value="wallpaper-pin" onSelect={onToggleWallpaperPin}>
                  {wallpaperEffect.pinned ? "取消固定" : "固定当前效果"}
                </Menu.Item>
                <Menu.Item value="wallpaper-auto-contrast" onSelect={onToggleWallpaperAutoContrast}>
                  {wallpaperEffect.autoContrast ? "✓ " : ""}自动对比度（{wallpaperEffect.contrast.toFixed(2)}x）
                </Menu.Item>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 0,
                    strategy: "fixed",
                    flip: true,
                    shift: -14,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">手动季节</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item value="season-auto" onSelect={() => onSetWallpaperDay(null)}>
                          {wallpaperSettings.dayOfYear === null ? "✓ " : ""}自动（跟随日期）
                        </Menu.Item>
                        <Box px="3" py="2" minW="240px" onKeyDown={(event) => event.stopPropagation()}>
                          <Text color="gray.500" fontSize="xs" mb="2">
                            {wallpaperSettings.dayOfYear === null
                              ? "拖动选择日期（季节随之变化）"
                              : `${formatWallpaperDate(wallpaperSettings.dayOfYear)} · ${wallpaperEffect.blendLabel}`}
                          </Text>
                          <Slider.Root
                            aria-label={["壁纸日期"]}
                            className="pz-memory-slider"
                            max={WALLPAPER_DAY_MAX}
                            min={0}
                            step={1}
                            value={[wallpaperSettings.dayOfYear ?? wallpaperEffect.dayOfYear]}
                            onValueChange={({ value }) => {
                              const nextValue = value[0];
                              if (nextValue !== undefined) {
                                onSetWallpaperDay(nextValue);
                              }
                            }}
                          >
                            <Slider.Control>
                              <Slider.Track>
                                <Slider.Range />
                              </Slider.Track>
                              <Slider.Thumb index={0}>
                                <Slider.HiddenInput />
                              </Slider.Thumb>
                            </Slider.Control>
                          </Slider.Root>
                        </Box>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 0,
                    strategy: "fixed",
                    flip: true,
                    shift: -14,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">手动时刻</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item value="time-auto" onSelect={() => onSetWallpaperTime(null)}>
                          {wallpaperSettings.timeMinutes === null ? "✓ " : ""}自动（跟随真实时间）
                        </Menu.Item>
                        <Box px="3" py="2" minW="240px" onKeyDown={(event) => event.stopPropagation()}>
                          <Text color="gray.500" fontSize="xs" mb="2">
                            {wallpaperSettings.timeMinutes === null
                              ? "拖动选择时刻（时间随之变化）"
                              : `${formatClock(wallpaperSettings.timeMinutes)} · ${wallpaperEffect.phaseLabel}`}
                          </Text>
                          <Slider.Root
                            aria-label={["壁纸时刻"]}
                            className="pz-memory-slider"
                            max={MINUTES_PER_DAY - 1}
                            min={0}
                            step={1}
                            value={[wallpaperSettings.timeMinutes ?? wallpaperEffect.minutes]}
                            onValueChange={({ value }) => {
                              const nextValue = value[0];
                              if (nextValue !== undefined) {
                                onSetWallpaperTime(nextValue);
                              }
                            }}
                          >
                            <Slider.Control>
                              <Slider.Track>
                                <Slider.Range />
                              </Slider.Track>
                              <Slider.Thumb index={0}>
                                <Slider.HiddenInput />
                              </Slider.Thumb>
                            </Slider.Control>
                          </Slider.Root>
                        </Box>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>
          <Menu.Separator />
          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 0,
              strategy: "fixed",
              flip: true,
              shift: -14,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">自定义图片</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                <Menu.Item value="choose-custom" onSelect={onChooseCustom}>
                  选择图片
                </Menu.Item>
                {customBackgrounds.length > 0 && <Menu.Separator />}
                {customBackgrounds.map((background) => renderBackgroundItem(background))}
                {customBackgrounds.length === 0 && (
                  <Text color="gray.500" fontSize="sm" px="3" py="2">
                    暂无自定义图片
                  </Text>
                )}
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>

          <Menu.Root
            positioning={{
              placement: "right-start",
              gutter: 0,
              strategy: "fixed",
              flip: true,
              shift: -14,
              overflowPadding: 8,
            }}
          >
            <Menu.TriggerItem>
              <Text flex="1">创意工坊订阅</Text>
              <Text aria-hidden="true">›</Text>
            </Menu.TriggerItem>
            <Portal>
              <Menu.Positioner className="pz-menu-positioner">
                <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                <Menu.Item value="scan-workshop" onSelect={onScanWorkshop}>
                  {workshopLoading
                    ? workshopScanProgress.scanned > 0
                      ? `正在查找... 已发现 ${workshopScanProgress.scanned} 个`
                      : "正在查找..."
                    : "查找创意工坊背景目录"}
                </Menu.Item>
                <Menu.Root
                  positioning={{
                    placement: "right-start",
                    gutter: 0,
                    strategy: "fixed",
                    flip: true,
                    shift: -14,
                    overflowPadding: 8,
                  }}
                >
                  <Menu.TriggerItem>
                    <Text flex="1">推荐的创意工坊项目</Text>
                    <Text aria-hidden="true">›</Text>
                  </Menu.TriggerItem>
                  <Portal>
                    <Menu.Positioner className="pz-menu-positioner">
                      <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        <Menu.Item
                          value="project-anime-backgrounds"
                          onSelect={() => onOpenRecommendedWorkshop("3498201101")}
                        >
                          <Text flex="1">Project Anime Backgrounds</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="l4d2-zombie-siege"
                          onSelect={() => onOpenRecommendedWorkshop("3388808256")}
                        >
                          <Text flex="1">L4D2丧尸围攻 主菜单壁纸</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="weeb-background"
                          onSelect={() => onOpenRecommendedWorkshop("3791676161")}
                        >
                          <Text flex="1">Weeb Background</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="blue-archive-visual-innovation"
                          onSelect={() => onOpenRecommendedWorkshop("3410321790")}
                        >
                          <Text flex="1">Blue Archive Visual Innovation</Text>
                        </Menu.Item>
                        <Menu.Item
                          value="dying-light-main-menu-background"
                          onSelect={() => onOpenRecommendedWorkshop("3141620300")}
                        >
                          <Text flex="1">Dying Light Main Menu Background</Text>
                        </Menu.Item>
                      </Menu.Content>
                    </Menu.Positioner>
                  </Portal>
                </Menu.Root>
                {workshopGroups.length > 0 && <Menu.Separator />}
                {workshopGroups.map(([key, backgrounds]) => (
                  <Menu.Root
                    key={key}
                    positioning={{
                      placement: "right-start",
                      gutter: 0,
                      strategy: "fixed",
                      flip: true,
                      shift: -14,
                      overflowPadding: 8,
                    }}
                  >
                    <Menu.TriggerItem>
                      <Text flex="1" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
                        {backgrounds[0].modName ?? "未命名模组"}
                      </Text>
                      <Text aria-hidden="true">›</Text>
                    </Menu.TriggerItem>
                    <Portal>
                      <Menu.Positioner className="pz-menu-positioner">
                        <Menu.Content className="pz-workshop-tag-menu-content pz-background-menu-content pz-background-menu-layer">
                        {backgrounds.map((background) => renderBackgroundItem(background, true))}
                        </Menu.Content>
                      </Menu.Positioner>
                    </Portal>
                  </Menu.Root>
                ))}
                {workshopBackgrounds.length === 0 && !workshopLoading && (
                  <Text color="gray.500" fontSize="sm" px="3" py="2">
                    请先查找创意工坊背景
                  </Text>
                )}
                </Menu.Content>
              </Menu.Positioner>
            </Portal>
          </Menu.Root>

          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  );
}
