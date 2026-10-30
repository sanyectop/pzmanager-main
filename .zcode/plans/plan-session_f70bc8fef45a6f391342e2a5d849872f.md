# 模组版本隔离 + 游戏大版本关联模组

## 一、复用性分析（结论）

`versioning.rs` 的引擎本身与“游戏”无关，所有入口都是 `root + store`：`scan_directory`、`create_full_snapshot`、`capture_version`、`restore_version`、`verify_current_version`、`get_status`、`delete_version`。CDC 增量（16–128 KiB 分块）、manifest、trash、`Materializer` 链式重建、并行开关、取消回调都可以直接用在模组目录上。

真正绑定游戏的只有 `lib.rs` 里的胶水层：游戏路径探测、`appmanifest` buildid、运行中进程检查（`ensure_game_closed`）、PZ Optimization 卸载/重装、`versionStorePath` 设置、以及“游戏目录”相关文案。

**结论：按单个模组复用是可行的，引擎几乎不改，改动集中在三处**：目标目录参数化（命令层接受 `rootPath`）、存储分区（`<store>/mods/<key>`）、独立事件通道（`mod-version-progress`，避免污染游戏页状态）。

需要新增、而不是复用的部分：模组根目录枚举与稳定 store 键、`jar-mod-state.json` 回写（否则下次打开模组页会把还原好的 `mod.info` 又改名回 `disabled`）、创意工坊目录会被 Steam 覆盖所以还原需要 force 确认。

“游戏大版本包含模组”用一个轻量关联实现：每个模组仍是一条独立版本链，游戏版本记录里只保存 `模组键 → 模组版本 ID` 的引用表，不把模组文件塞进游戏快照。

## 二、已定决策

- 粒度：**单模组版本链** + **游戏大版本可选关联模组**（开关在游戏页，默认开启，持久化到设置）。
- 快照内容：**只存文件内容**；启用顺序/开关继续由「模组预设」负责。还原后只回写 jar 状态，避免启动器把文件改回去。
- 页面：设置 › 版本 下两个分页「游戏版本 / 模组版本」，模组分页是**模组列表 + 行内展开**；模组管理的行右键菜单新增**「版本记录」**，点开是**模态框时间线**。
- 大版本捕获的模组集合：**只捕获当前已启用的模组**（Lua 在 `default.txt` 里启用 + JAR 已批准）。未启用的模组不冻结，还原时保持现状。这保证默认开启不会突然复制整个工坊（本机 11 GB / 17 万文件）。要改成“全部已安装”只需换一个取根集合的函数。

## 三、存储布局

沿用现有 `versionStorePath`（默认 `<Zomboid>/.pzmanager/versions`）：

```
versions/
├── base/ checkpoints/ deltas/ manifests/ trash/ tmp/ meta.json   ← 游戏大版本（现状不动）
└── mods/
    ├── workshop-<workshopId>-<modId>/   ← 与游戏相同的完整布局 + meta.json
    └── local-<modId>/
```

store 键规则（`mod_versioning::store_key`）：创意工坊 `workshop-<id>-<modId>`，本地 `local-<modId>`；`modId` 统一小写、只保留 `[a-z0-9._-]`，其余转 `_` 并截断；拿不到 modId 时退化为 `<source>-<路径 SHA-256 前 8 位>`。键稳定跨 Steam 库迁移，删模组后对应目录保留（暂不做孤立清理）。

## 四、后端改造

### 4.1 `src-tauri/src/versioning.rs`（小改）

1. `ensure_store_outside_game` → `ensure_store_outside_root`，文案改为“版本存储目录不能放在目标目录内”，更新 `lib.rs` 里 3 处调用。
2. `scan_directory` 空目录文案去掉“游戏目录/安装目录”字样，改为中性 `目录里没有可扫描的文件：<path>`。
3. `VersionRecord` 新增 `#[serde(default)] mod_versions: Vec<ModVersionRef>`；新增 `ModVersionRef { key, mod_id, name, source, workshop_id, version_id, root_path }`（camelCase）。模组链里该字段为空。
4. 新增 `pub fn attach_mod_versions(store, version_id, refs)`：载入 meta → 找到记录 → 写引用表 → 保存（游戏版本创建后再回填，避免改 `create_full_snapshot` 签名）。

### 4.2 新模块 `src-tauri/src/mod_versioning.rs`

- `ModVersionTarget { key, rootPath, name, modId, source, workshopId, hasStore, versionCount, latestVersionId, latestLabel, latestCreatedAt }`
- `ModVersionStatus { target, storePath, versions, currentMatch, pendingChanges, operationRunning }`
- `store_key(source, workshop_id, mod_id, root)`
- `list_targets(roots: &[(PathBuf, source, workshop_id, meta)], store_base) -> Vec<ModVersionTarget>`（只读各 store 的 `meta.json`，不扫描文件）
- `status(root, store, running)` → 调 `versioning::get_status(Some(root), store, None, "mods", running)` 并包装
- `#[cfg(test)]` 单测：键的稳定性/清洗/来源区分、meta 读取不触发扫描。

### 4.3 `src-tauri/src/lib.rs`

1. `collect_mod_roots() -> Vec<ModRootInfo>`：`workshop_background_roots()` 下数字目录 → `discover_mod_roots()`，加上 `%USERPROFILE%\Zomboid\mods` 的一级目录；用 `lua_mod_metadata()` 取 name/id，按规范化路径去重（**按物理根目录去重**，不像 `lua_mod_entries` 按 ID 合并）。
2. `enabled_mod_roots()`：`read_default_mod_ids(default.txt)` 的 ID 集合 + `jar-mod-state.json` 中 `!disabled` 的条目，映射到根目录。
3. `mod_root_for_info_path()`：从 `mod.info` 路径向上找到父目录名为 `mods` 的那一层，返回其子目录；`ScannedJarMod` 新增 `mod_root: String` 字段供前端右键菜单直接使用。
4. `reconcile_jar_state_for_root(root)`：还原后按 `mod.info`/`mod.info.disabled` 实际存在情况更新 `jar-mod-state.json` 的 `disabled`（Windows 大小写不敏感，直接 `root.join("mod.info").is_file()`）。
5. `spawn_version_operation` 参数化：增加事件名与可选 `(rootKey, rootPath)`，游戏走 `version-progress`，模组走 `mod-version-progress`；互斥仍共用 `VERSION_OPERATION_ACTIVE`（游戏/模组操作串行），`cancel_version_operation` 两者都能取消。
6. 新命令（注册进 `generate_handler!`）：
   - `list_mod_version_targets()`
   - `get_mod_version_status(root_path)`
   - `start_mod_version_snapshot(app, root_path, label)` / `start_mod_version_capture(...)` / `start_mod_version_restore(app, root_path, version_id, force)` / `start_mod_version_verify(app, root_path)`
   - `delete_mod_version(root_path, version_id)`
   - `save_version_include_mods(enabled)`（写入设置 `version_include_mods`，默认 true）
7. 扩展游戏命令：`start_version_snapshot` / `start_version_capture` 增加 `include_mods: Option<bool>`；`start_version_restore` 增加 `restore_mods: Option<bool>`。
8. 大版本捕获流程（在游戏操作成功后）：
   - 仅当 `outcome.created` 为真；逐个已启用模组：store 为空 → `create_full_snapshot`，否则 `capture_version`（label `大版本 <label|build>`）；无变化时取该链最新版本 ID。
   - 收集 `ModVersionRef` → `attach_mod_versions(游戏 store, 新版本 ID, refs)`。
   - 进度 phase 新增 `"mods"`，`current_path` 写“模组名（3/19）”；单个模组失败只记录并继续，最终消息追加“已记录 N 个模组版本（M 个失败）”。
9. 大版本还原流程（游戏文件 + PZ Optimization 重装之后）：按 `mod_versions` 逐条解析当前根目录（找不到再退回记录的 `root_path`），`restore_version(..., force=true, ...)`，随后 `reconcile_jar_state_for_root`；模组已不存在则跳过并汇报。模组还原前也要求游戏已关闭。

## 五、前端改造（`src/App.tsx` / `src/App.css` / `src/versioning.ts`）

### 5.1 类型与纯函数（`src/versioning.ts`）

新增 `ModVersionTarget`、`ModVersionStatus`、`ModVersionRef`、`ModVersionOperationUpdate`；`VersionRecord` 加 `modVersions?: ModVersionRef[]`。新增纯函数并单测：`sortVersionsNewestFirst`、`versionOrdinal`（1 起，最新=1）、`versionDotSize(index)`（`max(10, 18 - index)` px）、`formatVersionDate`（`YYYY/MM/DD`）；`versionPhaseLabel` 增加 `mods: "处理模组版本"`。

### 5.2 分页外壳

`versionSettingsContent` 拆成外壳 + 两个内容常量：

```tsx
<Box className="pz-panel pz-version-settings-panel">
  <HStack className="pz-version-tabs" role="tablist" aria-label="版本隔离分类">
    <Button role="tab" aria-selected={versionTab === "game"}>游戏版本</Button>
    <Button role="tab" aria-selected={versionTab === "mods"}>模组版本</Button>
  </HStack>
  {versionTab === "game" ? gameVersionContent : modVersionContent}
</Box>
```

`.pz-version-settings-panel` 保持“平铺、无卡面”（`acrylicVersionToast.test.ts` 已锁）。新增 `.pz-version-tabs` / `.pz-version-tab` / `.pz-version-tab-active`（胶囊分段，复用 `--pz-fill-accent-active` / `--pz-accent-deep` 令牌）。

### 5.3 游戏分页

- 现有内容整体搬进 `gameVersionContent`，标题改「游戏版本隔离」。
- 创建/捕获按钮旁新增开关「捕获时包含模组」（默认开，`versionIncludeMods`，`save_version_include_mods` 落盘，与 `version_parallel` 同款乐观更新/回滚）。
- 确认文案补一句：包含模组时会为 N 个已启用模组记录版本。
- 版本记录行在 `record.modVersions?.length` 时加徽标「含 N 个模组」；还原确认里写明会一起还原关联模组。
- `start_version_snapshot` / `start_version_capture` 传 `includeMods`，`start_version_restore` 传 `restoreMods: true`。

### 5.4 模组分页

- 顶部：标题「模组版本隔离」+ 说明 + 刷新；搜索框 + 来源筛选（全部/创意工坊/本地/已隔离）。
- 列表：每行 模组名、来源徽标、模组 ID、`N 个版本` 或 `未记录`、最后记录时间；按钮「记录当前版本」「展开」。
- 展开：当前状态（复用 `versionMatchLabel`）、与最新版本的待捕获变化、`<ModVersionTimeline>`、操作「校验当前目录 / 还原 / 删除（仅最新）」。
- 状态按需加载：`get_mod_version_status(rootPath)` 在展开时调用（避免一次扫 164 个根目录）。

### 5.5 时间线组件（模态框与行内共用）

`ModVersionTimeline({ versions, currentMatch, busy, onRestore, onDelete })`：

- 顺序：**最新在上**；序号 `index + 1`（最新 = 1）。
- 左侧轨道：圆形胶囊内写序号，直径 `max(10, 18 - index)px`（越新越大，最小 10px），相邻圆形之间用 2px 竖线连接（最后一条不画线）。
- 紧接圆形是一个矩形日期框（`.pz-version-timeline-date`，`Y-m-d`，等宽数字，细描边小圆角）。
- 日期框右侧/下一行：`versionBaseTypeLabel` 徽标、变更摘要（`versionChangeSummary`）、`完整快照` 说明、按钮「还原到此版本」「删除」（仅最新）。
- 空状态：“还没有版本记录。先记录一次当前模组内容。”

### 5.6 模态框

`ModVersionDialog`：Portal 到 body，复用 `.pz-resource-file-overlay` / `.pz-resource-file-dialog` + `useDialogAutoHeight`，Esc/遮罩关闭，`is-closing` 退场。内容：标题「<模组名> · 版本记录」+ 来源/路径 + 「记录当前版本」「校验当前目录」+ `ModVersionTimeline`。

### 5.7 模组管理右键菜单

`ModRowContextMenu` 增加第 4 项「版本记录」（始终可用），新增 `onOpenVersionHistory(rootPath)` 回调：Lua 行传 `mod.path`，JAR 行传后端新增的 `modRoot`。点击打开 `ModVersionDialog` 并加载该模组状态。

### 5.8 状态与事件

新增 `versionTab`、`versionIncludeMods`、`modVersionTargets/Loading/Search/Filter`、`modVersionStatuses`（按 rootPath 缓存）、`modVersionExpandedKey`、`modVersionDialog`、`modVersionOperation/Running`。新增 `mod-version-progress` 监听：过滤 `rootPath`、完成后刷新目标列表与该模组状态；互斥沿用 `versionBusy` 的全局判断。进入分页时加载目标列表（effect 依赖 `[settingsPage, versionTab]`）。

## 六、安全与副作用

- 捕获/校验只读，可游戏运行中进行；**模组还原要求游戏已关闭**（复用 `ensure_game_closed`）。
- 存储目录必须不在目标模组根目录内（`ensure_store_outside_root` 对每个模组检查）。
- 还原模组后同步 `jar-mod-state.json`，避免下次打开模组页把 `mod.info` 改名回去。
- 创意工坊目录可能被 Steam 校验/更新覆盖；还原确认文案说明这一点。
- 游戏与模组版本操作共用一个互斥，不能并发；删除仍只允许最新版本。
- 大版本还原若某个模组已卸载，跳过并写入结果消息，不整单失败。

## 七、测试

- Rust：`mod_versioning` 的键与目标列表测试；`versioning.rs` 增加 `attach_mod_versions` 往返测试；现有 7 个引擎测试不改。
- JS：新增 `tests/modVersionUi.test.ts`（分页结构与 aria、右键「版本记录」两行接线、`mod-version-progress` 过滤、`include_mods` 透传、时间线纯函数）；`tests/versioningUi.test.ts` 补 `mods` 阶段标签。
- 必须保持通过的既有断言：`versionParallel`、`versionPzopt`、`acrylicVersionToast`、`launcherLog`、`modPresets`（不重命名被匹配的状态/类名/文案）。
- 验证：`cargo test`（src-tauri）、`npm test`、`npx tsc --noEmit`；真机 CDP 截图核对两个分页、行内时间线、模态框时间线（圆圈大小/连线/日期矩形）。

## 八、文档与文案

- `docs/version-isolation.md` 新增“模组版本隔离”章节：存储布局、store 键、命令、关联表、还原流程、限制。
- `docs/ui-style-guide.md` 补：设置子页内的二级分页规范（胶囊分段 + `role=tablist`）、版本时间线规范（最新在上、序号从 1、越新越大、连线、日期矩形）。
- `docs/copy-backups/ui-copy.md` 记录新增/改名文案（游戏版本隔离、模组版本隔离、捕获时包含模组、版本记录、记录当前版本等）。
- `PROJECT_OVERVIEW.md` 设置章节补一段版本隔离说明。

## 九、实施顺序

1. 引擎小改 + `mod_versioning.rs` + 命令/事件注册，`cargo test` 通过。
2. 大版本 `include_mods` 捕获、关联表、还原关联模组、jar 状态回写。
3. 前端类型/纯函数 + 分页外壳 + 游戏分页拆出 + 包含模组开关。
4. 模组分页列表 + 行内时间线。
5. 模组管理右键「版本记录」+ 模态框。
6. 文档、文案快照、全量测试与真机验证。

## 十、风险

- 模组目录可能很大（单个最大约 1.9 GB），首次记录为完整快照，后续为增量；确认框会写明文件数/大小（进度里可见）。
- 大版本捕获模组会使一次操作变长，失败按模组逐个跳过并在结果里列出。
- 创意工坊更新后模组目录根哈希不匹配，还原需要确认覆盖（沿用游戏页的二次确认）。
- 若模组目录改名/移动，`workshop-…`/`local-…` 键仍能定位；纯路径哈希键会失配，退化为“未记录”。