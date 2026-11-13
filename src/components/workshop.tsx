/** 从 src/App.tsx 拆出的类型 / 常量 / 工具 / 独立组件，行为保持不变。 */
import {
  Badge,
  Box,
  Button,
  HStack,
  Input,
  Menu,
  Portal,
  Text,
} from "@chakra-ui/react";
import {
  MinecraftVersionOption,
  ResourceFile,
  ResourceFilters,
  ResourceItem,
  ResourceOptions,
  ResourceSearchResponse,
  ResourceVersionPack,
  ToastKind,
  WorkshopMenuOption,
  WorkshopTagMode,
} from "../app/types";
import { EmptyState, SkeletonGate, SkeletonList } from "./common";
import { FallbackImage } from "../FallbackImage";
import { RESOURCE_DIALOG_EXIT_MS, initialShaderDownloadFilters } from "../app/constants";
import { invoke } from "@tauri-apps/api/core";
import { Pagination } from "./pagination";
import { useDialogAutoHeight } from "./shell";
import { useEffect, useRef, useState } from "react";

export function WorkshopTagPicker({
  tags,
  requiredTags,
  excludedTags,
  onToggle,
  onRemove,
  compact = false,
}: {
  tags: string[];
  requiredTags: string[];
  excludedTags: string[];
  onToggle: (tag: string, mode: WorkshopTagMode) => void;
  onRemove: (tag: string, mode: WorkshopTagMode) => void;
  compact?: boolean;
}) {
  const renderTagMenu = (mode: WorkshopTagMode) => {
    const selectedTags = mode === "requiredTags" ? requiredTags : excludedTags;
    const colorPalette = mode === "requiredTags" ? "blue" : "red";
    const label = mode === "requiredTags" ? "包含" : "排除";

    return (
      <Menu.Root>
        <Menu.Trigger asChild>
          <Button
            colorPalette={colorPalette}
            size="xs"
            variant="outline"
          >
            <HStack gap="1">
              <Text as="span">{label}</Text>
              <Text aria-hidden="true" as="span" className="pz-workshop-tag-menu-chevron">
                ▾
              </Text>
            </HStack>
          </Button>
        </Menu.Trigger>
        <Menu.Positioner className="pz-menu-positioner">
          <Menu.Content className="pz-workshop-tag-menu-content" minW="220px">
            {tags.length > 0 ? (
              tags.map((tag) => {
                const selected = selectedTags.includes(tag);
                return (
                  <Menu.Item
                    key={tag}
                    value={tag}
                    onSelect={() => onToggle(tag, mode)}
                  >
                    <Text flex="1">{tag}</Text>
                    {selected && (
                      <Text color={`${colorPalette}.600`} fontSize="xs">
                        已选择
                      </Text>
                    )}
                  </Menu.Item>
                );
              })
            ) : (
              <Text color="gray.500" fontSize="sm" px="3" py="2">
                暂无可用标签
              </Text>
            )}
          </Menu.Content>
        </Menu.Positioner>
      </Menu.Root>
    );
  };

  return (
    <Box className={`pz-workshop-tag-picker${compact ? " pz-workshop-tag-picker-compact" : ""}`}>
      <HStack align="center" gap="3" wrap="wrap">
        <Text className="pz-filter-label" fontSize="xs" fontWeight="700">
          标签
        </Text>
        <HStack gap="1.5">
          {renderTagMenu("requiredTags")}
          {renderTagMenu("excludedTags")}
        </HStack>
        {!compact && (
          <Text className="pz-workshop-tag-hint" color="gray.500" fontSize="xs">
            从菜单中选择标签
          </Text>
        )}
      </HStack>

      {!compact && (
        <WorkshopTagSelectionSummary
          excludedTags={excludedTags}
          onRemove={onRemove}
          requiredTags={requiredTags}
        />
      )}
    </Box>
  );
}

export function WorkshopTagSelectionSummary({
  requiredTags,
  excludedTags,
  onRemove,
}: {
  requiredTags: string[];
  excludedTags: string[];
  onRemove: (tag: string, mode: WorkshopTagMode) => void;
}) {
  if (requiredTags.length === 0 && excludedTags.length === 0) {
    return null;
  }

  return (
    <HStack className="pz-workshop-current-filters" gap="1.5" mt="3" wrap="wrap">
      <Text color="gray.500" fontSize="xs" whiteSpace="nowrap">
        当前筛选
      </Text>
      {requiredTags.map((tag) => (
        <Button
          key={`required-${tag}`}
          colorPalette="blue"
          onClick={() => onRemove(tag, "requiredTags")}
          size="xs"
          variant="outline"
        >
          包含 · {tag} ×
        </Button>
      ))}
      {excludedTags.map((tag) => (
        <Button
          key={`excluded-${tag}`}
          colorPalette="red"
          onClick={() => onRemove(tag, "excludedTags")}
          size="xs"
          variant="outline"
        >
          排除 · {tag} ×
        </Button>
      ))}
    </HStack>
  );
}

export function WorkshopDropdown({
  ariaLabel,
  options,
  value,
  width,
  disabled,
  onChange,
}: {
  ariaLabel: string;
  options: WorkshopMenuOption[];
  value: string;
  width: string | Record<string, string>;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const selectedOption = options.find((option) => option.value === value);

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button
          aria-label={ariaLabel}
          className="pz-workshop-menu-trigger"
          disabled={disabled}
          size="sm"
          variant="outline"
          width={width}
        >
          <HStack gap="2" justify="space-between" width="100%">
            <Text flex="1" minW="0" overflow="hidden" textOverflow="ellipsis" whiteSpace="nowrap">
              {selectedOption?.label ?? value}
            </Text>
            <Text aria-hidden="true" className="pz-workshop-tag-menu-chevron">
              ▾
            </Text>
          </HStack>
        </Button>
      </Menu.Trigger>
      <Menu.Positioner className="pz-menu-positioner">
        <Menu.Content className="pz-workshop-tag-menu-content pz-dropdown-menu-content">
          {options.map((option) => (
            <Menu.Item
              key={option.value}
              value={option.value}
              onSelect={() => onChange(option.value)}
            >
              <Text flex="1">{option.label}</Text>
              {option.value === value && (
                <Text color="blue.600" fontSize="xs">
                  已选择
                </Text>
              )}
            </Menu.Item>
          ))}
        </Menu.Content>
      </Menu.Positioner>
    </Menu.Root>
  );
}

export function ResourceDownloadBrowser({
  options,
  versions,
  onToast,
  onDownload,
}: {
  options: ResourceOptions | null;
  versions: MinecraftVersionOption[];
  onToast: (message: string, kind?: ToastKind) => void;
  onDownload: (file: ResourceFile) => void;
}) {
  const [filters, setFilters] = useState<ResourceFilters>(initialShaderDownloadFilters);
  const [response, setResponse] = useState<ResourceSearchResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedResource, setSelectedResource] = useState<ResourceItem | null>(null);
  const [versionPacks, setVersionPacks] = useState<ResourceVersionPack[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  // 关闭时先播退场动画再卸载，所以需要一个"正在关闭"的中间状态。
  const [closingResourceDialog, setClosingResourceDialog] = useState(false);
  const requestId = useRef(0);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const resourceDialogRef = useRef<HTMLDivElement | null>(null);
  const resourceDialogCloseTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 上一次稳定下来的弹框高度（px）：把"加载中 → 版本列表就绪"的高度跳变做成过渡动画。
  const resourceDialogHeightRef = useRef<number | null>(null);

  const sourceOptions = options?.sources ?? [];
  const tagOptions = (options?.tags[filters.downloadSource] ?? ["All"]).map((tag) => ({
    value: tag,
    label: tag === "All" ? "全部" : tag,
  }));
  const sortOptions = options?.sortOptions[filters.downloadSource] ?? [];
  const selectedSource = sourceOptions.find((option) => option.value === filters.downloadSource);

  async function loadResources(nextFilters: ResourceFilters = filters) {
    const currentRequest = ++requestId.current;
    setLoading(true);
    try {
      const nextResponse = await invoke<ResourceSearchResponse>("fetch_shader_resources", {
        query: {
          resourceType: "shader",
          searchQuery: nextFilters.searchQuery,
          gameVersion: nextFilters.gameVersion,
          selectedTag: nextFilters.selectedTag,
          sortBy: nextFilters.sortBy,
          downloadSource: nextFilters.downloadSource,
          page: nextFilters.page,
          pageSize: nextFilters.pageSize,
        },
      });
      if (currentRequest === requestId.current) {
        setResponse(nextResponse);
      }
    } catch (error: unknown) {
      if (currentRequest === requestId.current) {
        setResponse(null);
        onToast(error instanceof Error ? error.message : String(error), "error");
      }
    } finally {
      if (currentRequest === requestId.current) {
        setLoading(false);
      }
    }
  }

  function updateFilters(changes: Partial<ResourceFilters>, delay = 250, resetPage = true) {
    const nextFilters = {
      ...filters,
      ...changes,
      page: resetPage ? 1 : changes.page ?? filters.page,
    };
    setFilters(nextFilters);
    if (searchTimer.current) {
      clearTimeout(searchTimer.current);
    }
    searchTimer.current = setTimeout(() => void loadResources(nextFilters), delay);
  }

  function resetFilters() {
    const nextFilters = {
      ...initialShaderDownloadFilters,
      downloadSource: filters.downloadSource,
      sortBy: filters.downloadSource === "CurseForge" ? "Popularity" : "relevance",
    };
    setFilters(nextFilters);
    void loadResources(nextFilters);
  }

  async function openResource(resource: ResourceItem) {
    // 退场动画还没播完就又点了另一个光影包：取消关闭，直接换内容。
    if (resourceDialogCloseTimer.current != null) {
      clearTimeout(resourceDialogCloseTimer.current);
      resourceDialogCloseTimer.current = null;
    }
    setClosingResourceDialog(false);
    // 请求序号守卫：A 的请求慢/超时几秒，用户已关掉它去点 B —— A 的响应晚到时
    // 绝不能把 A 的文件列表写进 B 的弹窗（那会照着 A 的下载直链下错文件）。
    const requestIdForThisOpen = ++requestId.current;
    setSelectedResource(resource);
    setVersionPacks([]);
    setLoadingFiles(true);
    try {
      const files = await invoke<ResourceVersionPack[]>("fetch_shader_resource_files", {
        resourceId: resource.id,
        downloadSource: resource.source,
      });
      if (requestId.current !== requestIdForThisOpen) {
        return;
      }
      setVersionPacks(files);
    } catch (error: unknown) {
      if (requestId.current !== requestIdForThisOpen) {
        return;
      }
      onToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      if (requestId.current === requestIdForThisOpen) {
        setLoadingFiles(false);
      }
    }
  }

  function closeResourceDialog() {
    if (resourceDialogCloseTimer.current != null) {
      return;
    }
    setClosingResourceDialog(true);
    resourceDialogCloseTimer.current = setTimeout(() => {
      resourceDialogCloseTimer.current = null;
      setClosingResourceDialog(false);
      setSelectedResource(null);
    }, RESOURCE_DIALOG_EXIT_MS);
  }

  useEffect(
    () => () => {
      if (resourceDialogCloseTimer.current != null) {
        clearTimeout(resourceDialogCloseTimer.current);
        resourceDialogCloseTimer.current = null;
      }
    },
    [],
  );

  // 弹框高度随内容变化（读取版本文件 → 列表就绪），量高后显式过渡，见 useDialogAutoHeight。
  useDialogAutoHeight(resourceDialogRef, resourceDialogHeightRef, [
    selectedResource,
    loadingFiles,
    versionPacks,
  ]);
  useEffect(() => {
    if (options) {
      void loadResources();
    }
  }, [options, versions.length]);

  return (
    <Box className="pz-resource-browser">
      <Box className="pz-panel-header" p={{ base: "3", md: "4" }}>
        <HStack align="start" justify="space-between" gap="3" wrap="wrap">
          <Box>
            <Text color="gray.900" fontSize="lg" fontWeight="700">
              光影包下载
            </Text>
          </Box>
          <HStack gap="2">
            <Button loading={loading} onClick={() => void loadResources()} size="sm" variant="outline">
              刷新
            </Button>
            <Button onClick={resetFilters} size="sm" variant="ghost">
              重置筛选
            </Button>
          </HStack>
        </HStack>
      </Box>

      <Box className="pz-resource-download-body">
        <HStack className="pz-resource-filter-row" gap="2" wrap="wrap">
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">标签</Text>
            <WorkshopDropdown
              ariaLabel="选择光影标签"
              onChange={(value) => updateFilters({ selectedTag: value })}
              options={tagOptions}
              value={filters.selectedTag}
              width={{ base: "92px", md: "100px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">游戏版本</Text>
            <WorkshopDropdown
              ariaLabel="选择 Minecraft 游戏版本"
              onChange={(value) => updateFilters({ gameVersion: value })}
              options={versions.map((version) => ({ value: version.value, label: version.label }))}
              value={filters.gameVersion}
              width={{ base: "76px", md: "80px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">下载源</Text>
            <WorkshopDropdown
              ariaLabel="选择下载源"
              onChange={(value) =>
                updateFilters({
                  downloadSource: value,
                  selectedTag: "All",
                  sortBy: value === "CurseForge" ? "Popularity" : "relevance",
                })
              }
              options={sourceOptions}
              value={filters.downloadSource}
              width={{ base: "116px", md: "124px" }}
            />
          </HStack>
          <HStack gap="2">
            <Text className="pz-filter-label" whiteSpace="nowrap">排序</Text>
            <WorkshopDropdown
              ariaLabel="选择资源排序方式"
              onChange={(value) => updateFilters({ sortBy: value })}
              options={sortOptions}
              value={filters.sortBy}
              width={{ base: "92px", md: "100px" }}
            />
          </HStack>
        </HStack>

        <HStack className="pz-resource-search-row" gap="2" mt="2" wrap="wrap">
          <Text className="pz-filter-label" whiteSpace="nowrap">名称</Text>
          <Input
            flex="1"
            maxW={{ base: "100%", md: "340px" }}
            onChange={(event) => updateFilters({ searchQuery: event.currentTarget.value }, 400)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                void loadResources();
              }
            }}
            placeholder="搜索光影包名称"
            size="sm"
            value={filters.searchQuery}
          />
          <Button colorPalette="blue" loading={loading} onClick={() => void loadResources()} size="sm">
            搜索
          </Button>
        </HStack>

        <HStack className="pz-resource-meta-row" color="gray.500" fontSize="xs" justify="space-between" mt="3" wrap="wrap">
          <Text>{selectedSource?.label ?? filters.downloadSource} · Minecraft Shader Packs</Text>
          <Text>{response ? `共 ${response.total.toLocaleString()} 个资源` : "等待加载"}</Text>
        </HStack>

        <Box className="pz-resource-download-list">
          <SkeletonGate
            loading={loading && !response}
            skeleton={<SkeletonList rows={6} thumb="icon" />}
          >
          {response && response.items.length > 0 ? (
            response.items.map((resource) => (
              <Box
                className="pz-resource-download-item"
                key={`${resource.source}-${resource.id}`}
                onClick={() => void openResource(resource)}
                role="button"
                tabIndex={0}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    void openResource(resource);
                  }
                }}
              >
                <Box className="pz-resource-download-icon">
                  {resource.iconUrl ? <FallbackImage alt="" fallbackSrc={resource.iconFallbackUrl} loading="lazy" src={resource.iconUrl} /> : <Text>MC</Text>}
                </Box>
                <Box minW="0" flex="1">
                  <HStack align="start" className="pz-resource-download-title-row" gap="2" wrap="wrap">
                    <Text color="gray.900" flex="1" fontSize="sm" fontWeight="700" lineClamp="1" minW="0">
                      {resource.name}
                    </Text>
                    <HStack className="pz-resource-download-tags" gap="1" flexShrink="0" justify="flex-end" wrap="wrap">
                      <Badge colorPalette={resource.source === "Modrinth" ? "green" : "orange"} size="sm">
                        {resource.source}
                      </Badge>
                      {resource.tags.slice(0, 3).map((tag) => (
                        <Badge key={tag} colorPalette="blue" size="sm" variant="subtle">{tag}</Badge>
                      ))}
                    </HStack>
                  </HStack>
                  <Text color="gray.500" fontSize="xs" lineClamp="2" mt="1">
                    {resource.description || "暂无描述"}
                  </Text>
                </Box>
              </Box>
            ))
          ) : (
            <EmptyState text="没有匹配的光影包。" />
          )}
          </SkeletonGate>
        </Box>

        {response ? (
          <Pagination
            onChange={(page) => updateFilters({ page }, 0, false)}
            page={response.page}
            totalPages={Math.ceil(response.total / response.pageSize)}
          />
        ) : null}
      </Box>

      {/* 正文框 .pz-main 带 backdrop-filter + overflow:hidden，会让 position:fixed 的弹层
          以它为包含块并被裁掉，所以这里必须 Portal 到 body 才能覆盖整个窗口。 */}
      {selectedResource && (
        <Portal>
          <Box
            className={`pz-resource-file-overlay ${closingResourceDialog ? "is-closing" : ""}`}
            onClick={closeResourceDialog}
          >
            <Box className="pz-resource-file-dialog" onClick={(event) => event.stopPropagation()} ref={resourceDialogRef}>
              <HStack justify="space-between" gap="3">
                <Box minW="0">
                  <Text color="gray.900" fontWeight="700" lineClamp="1">{selectedResource.name}</Text>
                  <Text color="gray.500" fontSize="xs" mt="1">{selectedResource.source} · 选择文件下载</Text>
                  <HStack color="gray.500" fontSize="xs" gap="3" mt="1" wrap="wrap">
                    <Text>更新 {new Date(selectedResource.lastUpdated).toLocaleDateString("zh-CN")}</Text>
                    <Text>下载 {selectedResource.downloads.toLocaleString()}</Text>
                    <Text>作者 {selectedResource.author || "未知作者"}</Text>
                  </HStack>
                </Box>
                <Button aria-label="关闭文件列表" onClick={closeResourceDialog} size="sm" variant="ghost">×</Button>
              </HStack>
              <Box className="pz-resource-file-list">
                <SkeletonGate loading={loadingFiles} skeleton={<SkeletonList rows={4} />}>
                {versionPacks.length > 0 ? (
                  versionPacks.flatMap((pack) => pack.items).map((file) => (
                    <HStack className="pz-resource-file-row" key={`${file.fileName}-${file.fileDate}`} justify="space-between" gap="3">
                      <Box minW="0">
                        <Text color="gray.800" fontSize="sm" fontWeight="600" overflowWrap="anywhere">{file.fileName}</Text>
                        <Text color="gray.500" fontSize="xs" mt="1">
                          {file.releaseType} · {file.gameVersions.slice(0, 3).join("、") || "版本未知"} · 下载 {file.downloads.toLocaleString()}
                        </Text>
                      </Box>
                      <Button
                        colorPalette="blue"
                        flexShrink="0"
                        onClick={() => {
                          onDownload(file);
                          closeResourceDialog();
                        }}
                        size="sm"
                      >
                        下载
                      </Button>
                    </HStack>
                  ))
                ) : (
                  <Text color="gray.500" py="5" textAlign="center">没有可用下载文件。</Text>
                )}
                </SkeletonGate>
              </Box>
            </Box>
          </Box>
        </Portal>
      )}
    </Box>
  );
}
