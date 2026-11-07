/** Viewpoint 光影包配置弹窗：解析包内 shaders.properties（参数 / 预设 / 滑块），
 *  按包保存参数覆写到 .pzmanager/shaderpack-overrides.json。
 *  启动游戏时后端把这些值行级写进 viewpoint-live.properties（只碰 minecraft.* 键），
 *  游戏进程退出后整文件还原。 */
import { Badge, Box, Button, HStack, Input, Portal, Slider, Switch, Text } from "@chakra-ui/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ShaderpackConfigInfo, ShaderpackOptionInfo } from "../app/types";
import { RESOURCE_DIALOG_EXIT_MS } from "../app/constants";
import {
  applyProfileToValues,
  collectOverrideValues,
  controlKindForOption,
  initialOverrideValues,
  isNumericValue,
  langDisplayName,
  NO_PROFILE,
  optionDisplayName,
  parseMinecraftFormatting,
  ShaderpackControlKind,
  stripMinecraftFormatting,
  valueDisplayLabel,
} from "../shaderpackConfigUi";
import { useDialogAutoHeight } from "./shell";

/** 每页行数（列数来自 screen 的 columns，OptiFine 默认 2）。 */
const ROWS_PER_PAGE = 5;

/** 界面标题：主页显示固定名，子屏优先 lang 的 screen.<name>。 */
function screenTitle(
  screen: { name: string; label: string | null } | undefined,
  fallbackName: string | undefined,
): string {
  if (!screen) {
    return fallbackName ?? "光影设置";
  }
  if (screen.name === "screen") {
    return "光影设置";
  }
  return screen.label?.trim() || screen.name;
}

/** 渲染带 MC § 彩字/格式码的文本：解析成 span 段上色加样式；纯文本原样输出。
 *  乱码码（§k）没有对应的静态样式，按普通文字显示。 */
function McText({ text }: { text: string }) {
  const segments = useMemo(() => parseMinecraftFormatting(text), [text]);
  if (segments.length === 1 && !hasSegmentFormatting(segments[0])) {
    return <>{segments[0].text}</>;
  }
  return (
    <>
      {segments.map((segment, index) => {
        const decoration =
          [segment.underline ? "underline" : "", segment.strikethrough ? "line-through" : ""]
            .filter(Boolean)
            .join(" ") || undefined;
        return (
          <Text
            as="span"
            color={segment.color}
            fontStyle={segment.italic ? "italic" : undefined}
            fontWeight={segment.bold ? "bold" : undefined}
            key={index}
            textDecoration={decoration}
          >
            {segment.text}
          </Text>
        );
      })}
    </>
  );
}

function hasSegmentFormatting(segment: {
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strikethrough?: boolean;
  obfuscated?: boolean;
}): boolean {
  return Boolean(
    segment.color ||
      segment.bold ||
      segment.italic ||
      segment.underline ||
      segment.strikethrough ||
      segment.obfuscated,
  );
}

export function ShaderpackConfigDialog({
  packName,
  onDismiss,
  onSaved,
}: {
  packName: string;
  onDismiss: () => void;
  /** 保存 / 清除覆写成功后回调（父层刷新覆写列表）。 */
  onSaved: () => void;
}) {
  const [info, setInfo] = useState<ShaderpackConfigInfo | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [initialValues, setInitialValues] = useState<Record<string, string>>({});
  const [values, setValues] = useState<Record<string, string>>({});
  const [profile, setProfile] = useState<string>(NO_PROFILE);
  /** 描述语言（lang 文件名字干）；null = 默认（中文优先）。 */
  const [lang, setLang] = useState<string | null>(null);
  const [switchingLang, setSwitchingLang] = useState(false);
  /** Iris 同款分页：当前所在的子屏栈（空 = 主页）与页码。 */
  const [screenStack, setScreenStack] = useState<string[]>([]);
  const [page, setPage] = useState(0);
  const [saving, setSaving] = useState(false);
  const [closing, setClosing] = useState(false);

  const dialogRef = useRef<HTMLDivElement | null>(null);
  const dialogHeightRef = useRef<number | null>(null);
  useDialogAutoHeight(dialogRef, dialogHeightRef, [info, values, profile, loadError, screenStack, page]);

  const requestClose = () => {
    if (closing) return;
    setClosing(true);
    window.setTimeout(onDismiss, RESOURCE_DIALOG_EXIT_MS);
  };

  useEffect(() => {
    let active = true;
    setInfo(null);
    setLoadError(null);
    invoke<ShaderpackConfigInfo>("read_shaderpack_config", { packName })
      .then((config) => {
        if (!active) return;
        setInfo(config);
        setLang(config.lang);
        setScreenStack([]);
        setPage(0);
        const base = initialOverrideValues(config.options);
        // 已存覆写盖在 live 当前值之上（弹窗显示的就是"下次启动会写入的值"）
        const merged = { ...base, ...(config.saved?.values ?? {}) };
        setInitialValues(merged);
        setValues(merged);
        setProfile(config.saved?.profile ?? NO_PROFILE);
      })
      .catch((error: unknown) => {
        if (active) setLoadError(error instanceof Error ? error.message : String(error));
      });
    return () => {
      active = false;
    };
  }, [packName]);

  /** 切换描述语言：只换标签数据，用户已改的参数值 / 预设保持不动。 */
  async function switchLang(stem: string) {
    if (!info || switchingLang || closing || stem === lang) return;
    setSwitchingLang(true);
    try {
      const next = await invoke<ShaderpackConfigInfo>("read_shaderpack_config", {
        packName: info.packName,
        lang: stem,
      });
      setInfo(next);
      setLang(next.lang);
    } catch (error: unknown) {
      window.alert(error instanceof Error ? error.message : String(error));
    } finally {
      setSwitchingLang(false);
    }
  }

  useEffect(() => {
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        requestClose();
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
    // requestClose 依赖 closing，闭包里已经挡住重复触发
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [closing]);

  /** 选中预设：写预设名并把预设 token 预填进参数值（null = 不改预设，只清掉选择）。 */
  function pickProfile(profileName: string | null) {
    setProfile(profileName ?? NO_PROFILE);
    if (!info || profileName == null) return;
    const entry = info.profiles.find((candidate) => candidate.name === profileName);
    if (entry) {
      setValues((current) => applyProfileToValues(info.options, entry, current));
    }
  }

  const changedCount = info
    ? Object.keys(collectOverrideValues(info.options, initialValues, values)).length
    : 0;
  const profileValue = profile === NO_PROFILE ? null : profile;
  const dirty = changedCount > 0 || profileValue !== (info?.saved?.profile ?? null);

  // Iris 同款分页：主页是 screens[0]，[子屏] 格子进栈导航
  const screens = info?.screens ?? [];
  const mainScreen = screens[0];
  const currentName = screenStack[screenStack.length - 1] ?? mainScreen?.name;
  const currentScreen = screens.find((screen) => screen.name === currentName) ?? mainScreen;
  const isMainScreen = currentScreen != null && currentScreen.name === mainScreen?.name;
  const mainScreenHasProfile =
    mainScreen?.tokens.some((token) => token.kind === "profile") ?? false;
  const columns = Math.min(Math.max(currentScreen?.columns ?? 2, 1), 8);
  const tokens = currentScreen?.tokens ?? [];
  const pageSize = columns * ROWS_PER_PAGE;
  const pageCount = Math.max(1, Math.ceil(tokens.length / pageSize));
  const safePage = Math.min(page, pageCount - 1);
  const pageTokens = tokens.slice(safePage * pageSize, (safePage + 1) * pageSize);
  const optionByName = new Map((info?.options ?? []).map((option) => [option.name, option]));

  async function saveOverride() {
    if (!info || closing) return;
    setSaving(true);
    try {
      await invoke("save_shaderpack_override", {
        packName: info.packName,
        profile: profileValue,
        values: collectOverrideValues(info.options, initialValues, values),
      });
      onSaved();
      requestClose();
    } catch (error: unknown) {
      window.alert(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  }

  async function clearOverride() {
    if (!info || closing) return;
    setSaving(true);
    try {
      await invoke("clear_shaderpack_override", { packName: info.packName });
      onSaved();
      requestClose();
    } catch (error: unknown) {
      window.alert(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Portal>
      <Box
        className={`pz-resource-file-overlay ${closing ? "is-closing" : ""}`}
        onClick={requestClose}
      >
        <Box
          aria-label={`配置光影包 ${packName}`}
          aria-modal="true"
          className="pz-resource-file-dialog pz-shaderpack-config-dialog"
          onClick={(event) => event.stopPropagation()}
          ref={dialogRef}
          role="dialog"
        >
          <HStack align="flex-start" gap="3" justify="space-between">
            <Box minW="0">
              <Text color="gray.900" fontSize="lg" fontWeight="700">
                配置光影包
              </Text>
              <Text color="gray.600" fontSize="sm" mt="1" overflowWrap="anywhere">
                {packName}
              </Text>
            </Box>
            <Button aria-label="关闭光影包配置" onClick={requestClose} size="sm" variant="ghost">
              ×
            </Button>
          </HStack>

          <Box flex="1" minH="0" mt="3" overflowY="auto">
            {loadError ? (
              <Text color="red.500" fontSize="sm">
                {loadError}
              </Text>
            ) : !info ? (
              <Text color="gray.500" fontSize="sm">
                正在解析光影包…
              </Text>
            ) : (
              <>
                <HStack align="center" gap="2" wrap="wrap">
                  <Badge colorPalette={info.packIdMatched ? "blue" : "orange"} variant="subtle">
                    packId：{info.packId}
                  </Badge>
                  {!info.packIdMatched ? (
                    <Text color="orange.500" fontSize="xs" flex="1" minW="0">
                      按文件名推导的 packId，live 文件里还没有它的键；建议先在游戏内打开一次该包的
                      设置，让 Viewpoint 写入真实键后再核对。
                    </Text>
                  ) : null}
                </HStack>

                {info.availableLangs.length > 1 ? (
                  <HStack align="center" gap="1" mt="2" wrap="wrap">
                    <Text color="gray.500" fontSize="xs" flexShrink={0}>
                      描述语言
                    </Text>
                    {info.availableLangs.map((stem) => (
                      <Button
                        colorPalette={stem === lang ? "blue" : "gray"}
                        disabled={switchingLang}
                        key={stem}
                        loading={switchingLang && stem === lang}
                        onClick={() => void switchLang(stem)}
                        size="2xs"
                        variant={stem === lang ? "solid" : "outline"}
                      >
                        {langDisplayName(stem)}
                      </Button>
                    ))}
                  </HStack>
                ) : null}

                {/* Iris 同款分页：主页是 screens[0]，子屏经 [子屏] 格子进栈导航 */}
                {screenStack.length > 0 ? (
                  <HStack align="center" gap="2" mt="3">
                    <Button
                      onClick={() => {
                        setScreenStack((stack) => stack.slice(0, -1));
                        setPage(0);
                      }}
                      size="xs"
                      variant="outline"
                    >
                      ← 返回
                    </Button>
                    <Text color="gray.700" fontSize="sm" fontWeight="600" lineClamp={1}>
                      <McText text={screenTitle(currentScreen, currentName)} />
                    </Text>
                  </HStack>
                ) : null}

                {/* 预设：主页布局里有 <profile> 格子就在格子里渲染；没有则主页顶部兜底 */}
                {isMainScreen && !mainScreenHasProfile && info.profiles.length > 0 ? (
                  <ProfilePicker info={info} onPick={pickProfile} profile={profile} />
                ) : null}

                {pageTokens.length > 0 ? (
                  <Box
                    display="grid"
                    gap="2"
                    gridTemplateColumns={`repeat(${columns}, minmax(0, 1fr))`}
                    mt="3"
                  >
                    {pageTokens.map((token, index) => {
                      if (token.kind === "empty") {
                        return <Box key={index} />;
                      }
                      if (token.kind === "profile") {
                        return info.profiles.length > 0 ? (
                          <ProfilePicker info={info} key={index} onPick={pickProfile} profile={profile} compact />
                        ) : (
                          <Box key={index} />
                        );
                      }
                      if (token.kind === "screen") {
                        const target = screens.find((screen) => screen.name === token.name);
                        if (!target) {
                          return <Box key={index} />;
                        }
                        return (
                          <Button
                            alignItems="center"
                            borderColor="gray.300"
                            colorPalette="blue"
                            h="auto"
                            justifyContent="center"
                            key={index}
                            minH="10"
                            onClick={() => {
                              setScreenStack((stack) => [...stack, target.name]);
                              setPage(0);
                            }}
                            size="xs"
                            textAlign="center"
                            variant="outline"
                          >
                            <McText text={target.label?.trim() || target.name} /> ›
                          </Button>
                        );
                      }
                      const option = token.name ? optionByName.get(token.name) : undefined;
                      if (!option) {
                        return <Box key={index} />;
                      }
                      return (
                        <OptionCell
                          key={`${token.name}-${index}`}
                          onChange={(value) =>
                            setValues((current) => ({ ...current, [option.name]: value }))
                          }
                          option={option}
                          value={values[option.name] ?? ""}
                        />
                      );
                    })}
                  </Box>
                ) : (
                  <Text color="gray.500" fontSize="sm" mt="4">
                    这个包没有解析到可调参数。
                  </Text>
                )}

                {/* 分页设计强制：分页条常驻（哪怕只有一页），所有光影包的配置界面保持同一套分页形态 */}
                <HStack justify="center" mt="3" gap="2">
                  <Button
                    disabled={safePage === 0}
                    onClick={() => setPage(safePage - 1)}
                    size="xs"
                    variant="outline"
                  >
                    ‹
                  </Button>
                  <Text color="gray.500" fontSize="xs">
                    {safePage + 1} / {pageCount}
                  </Text>
                  <Button
                    disabled={safePage >= pageCount - 1}
                    onClick={() => setPage(safePage + 1)}
                    size="xs"
                    variant="outline"
                  >
                    ›
                  </Button>
                </HStack>
              </>
            )}
          </Box>

          <Box borderTopColor="gray.200" borderTopWidth="1px" flexShrink={0} mt="3" pt="3">
            <Text color="gray.500" fontSize="xs">
              保存的覆写在下次启动游戏时写入 viewpoint-live.properties（只改该包的 minecraft
              参数），游戏退出后整文件还原——游戏内当次对该包的改动不会保留。
              {info && changedCount > 0 ? ` 本次将覆写 ${changedCount} 项。` : ""}
            </Text>
            <HStack gap="2" justify="flex-end" mt="2" wrap="wrap">
              {info?.saved ? (
                <Button
                  colorPalette="red"
                  disabled={saving}
                  onClick={() => void clearOverride()}
                  size="sm"
                  variant="outline"
                >
                  清除覆写
                </Button>
              ) : null}
              <Button disabled={saving} onClick={requestClose} size="sm" variant="ghost">
                取消
              </Button>
              <Button
                colorPalette="blue"
                disabled={!info || saving || !dirty}
                loading={saving}
                onClick={() => void saveOverride()}
                size="sm"
              >
                保存覆写
              </Button>
            </HStack>
          </Box>
        </Box>
      </Box>
    </Portal>
  );
}

/** 网格里的单个参数卡片（Iris 风格：紧凑标题 + 控件，详细信息放悬停提示）。 */
function OptionCell({
  option,
  value,
  onChange,
}: {
  option: ShaderpackOptionInfo;
  value: string;
  onChange: (value: string) => void;
}) {
  const kind = controlKindForOption(option);
  const displayName = optionDisplayName(option);
  const tooltip = [
    stripMinecraftFormatting(displayName),
    option.comment?.trim() ? stripMinecraftFormatting(option.comment.trim()) : null,
    option.liveValue != null
      ? `游戏内当前值：${stripMinecraftFormatting(valueDisplayLabel(option, option.liveValue))}（${option.liveValue}）`
      : option.defaultValue != null
        ? `默认值：${stripMinecraftFormatting(valueDisplayLabel(option, option.defaultValue))}（${option.defaultValue}）`
        : null,
  ]
    .filter(Boolean)
    .join("\n");
  return (
    <Box
      borderColor="gray.200"
      borderRadius="sm"
      borderWidth="1px"
      minW="0"
      overflow="hidden"
      p="2"
      title={tooltip}
    >
      <Text color="gray.800" fontSize="xs" fontWeight="600" lineClamp={1}>
        <McText text={displayName} />
      </Text>
      <Box mt="1">
        <OptionControl kind={kind} onChange={onChange} option={option} value={value} />
      </Box>
    </Box>
  );
}

/** 预设选择（主页兜底展示 或 <profile> 格子里的紧凑形态）。 */
function ProfilePicker({
  info,
  profile,
  onPick,
  compact,
}: {
  info: ShaderpackConfigInfo;
  /** 选中的预设名；null = 不改预设（对话框负责预填参数值）。 */
  onPick: (profile: string | null) => void;
  profile: string;
  /** <profile> 格子里的紧凑形态：只显示按钮组。 */
  compact?: boolean;
}) {
  const buttons = (
    <HStack gap="1" wrap="wrap">
      <Button
        colorPalette={profile === NO_PROFILE ? "gray" : "blue"}
        onClick={() => onPick(null)}
        size={compact ? "2xs" : "xs"}
        variant={profile === NO_PROFILE ? "solid" : "outline"}
      >
        不改预设
      </Button>
      {info.profiles.map((entry) => (
        <Button
          colorPalette={profile === entry.name ? "blue" : "gray"}
          key={entry.name}
          onClick={() => onPick(entry.name)}
          size={compact ? "2xs" : "xs"}
          title={entry.label ? entry.name : undefined}
          variant={profile === entry.name ? "solid" : "outline"}
        >
          <McText text={entry.label?.trim() || entry.name} />
        </Button>
      ))}
    </HStack>
  );
  if (compact) {
    return buttons;
  }
  return (
    <Box mt="3">
      <Text color="gray.800" fontSize="sm" fontWeight="600">
        预设
      </Text>
      {buttons}
      <Text color="gray.500" fontSize="xs" mt="1">
        {info.liveProfile
          ? `游戏内当前预设：${info.liveProfile}`
          : "live 文件里还没有该包的预设记录。"}
        选预设会预填参数值，点「保存覆写」才写入。
      </Text>
    </Box>
  );
}

function OptionControl({
  kind,
  option,
  value,
  onChange,
}: {
  kind: ShaderpackControlKind;
  option: ShaderpackOptionInfo;
  value: string;
  onChange: (value: string) => void;
}) {
  const name = option.name;
  if (kind === "toggle") {
    const checked = value.toLowerCase() === "true";
    return (
      <HStack gap="2">
        <Switch.Root
          aria-label={`参数 ${name}`}
          checked={checked}
          onCheckedChange={(details) => onChange(details.checked ? "true" : "false")}
          size="sm"
        >
          <Switch.HiddenInput />
          <Switch.Control>
            <Switch.Thumb />
          </Switch.Control>
        </Switch.Root>
        <Text color="gray.600" fontSize="xs">
          <McText text={valueDisplayLabel(option, value)} />
        </Text>
      </HStack>
    );
  }
  if (kind === "slider" && isNumericValue(value)) {
    const numeric = Number(value);
    const clamped = Number.isFinite(numeric) ? Math.min(1, Math.max(0, numeric)) : 0;
    return (
      <HStack gap="2">
        <Slider.Root
          aria-label={[`参数 ${name}`]}
          max={1}
          min={0}
          step={0.01}
          value={[clamped]}
          onValueChange={({ value: next }) => onChange(String(next[0] ?? 0))}
          w="100%"
        >
          <Slider.Control>
            <Slider.Track>
              <Slider.Thumb index={0} />
            </Slider.Track>
          </Slider.Control>
        </Slider.Root>
        <Text color="gray.600" fontSize="xs" textAlign="right" w="12">
          {value}
        </Text>
      </HStack>
    );
  }
  if (kind === "select" && option.values.length > 0) {
    // 观测取值不多时用按钮组（显示 lang 标签），放不下（>6 个）用原生下拉兜底
    if (option.values.length <= 6) {
      const choices = option.values.includes(value) ? option.values : [...option.values, value];
      return (
        <HStack gap="1" justify="flex-end" wrap="wrap">
          {choices.map((choice) => {
            const label = valueDisplayLabel(option, choice);
            return (
              <Button
                colorPalette={choice === value ? "blue" : "gray"}
                key={choice}
                onClick={() => onChange(choice)}
                size="xs"
                title={label !== choice ? choice : undefined}
                variant={choice === value ? "solid" : "outline"}
              >
                <McText text={label} />
              </Button>
            );
          })}
        </HStack>
      );
    }
    return (
      <select
        aria-label={`参数 ${name}`}
        onChange={(event) => onChange(event.target.value)}
        style={{
          border: "1px solid var(--pz-line-strong, #cbd5e0)",
          borderRadius: 6,
          fontSize: 13,
          maxWidth: "100%",
          padding: "2px 6px",
        }}
        value={value}
      >
        {(option.values.includes(value) ? option.values : [value, ...option.values]).map(
          (choice) => {
            const label = valueDisplayLabel(option, choice);
            // 原生 <option> 渲染不了样式，§ 码剥掉只留文字
            const plain = stripMinecraftFormatting(label);
            return (
              <option key={choice} value={choice}>
                {plain !== choice ? `${plain}（${choice}）` : choice}
              </option>
            );
          },
        )}
      </select>
    );
  }
  // 文本输入：也兜底"非数值的滑块参数"
  return (
    <Input
      aria-label={`参数 ${name}`}
      onChange={(event) => onChange(event.target.value)}
      size="sm"
      value={value}
      w="100%"
    />
  );
}
