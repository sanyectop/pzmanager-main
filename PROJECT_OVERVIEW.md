# pzmanager 项目文档：功能全景与技术细节

> **快照时间：2026-10-20**。本文按工作区当前代码重写，不是设计稿；数字（行数、命令数、测试数）都取自当前文件。
>
> **定位**：一篇把「能做什么」和「怎么做到」放在一起读的全景文档。命令契约、磁盘布局的逐条索引以
> [docs/architecture.md](docs/architecture.md) 为准；覆盖层的注入/Hook/排错细节以
> [docs/overlay-injection.md](docs/overlay-injection.md) 为准；极速启动的实验记录以
> [docs/fast-launch-status.md](docs/fast-launch-status.md) 为准；视觉与交互规范以
> [docs/ui-style-guide.md](docs/ui-style-guide.md) 为准。本文负责把它们串成一条完整的产品与工程叙事。

---

## 摘要

pzmanager 是 **Project Zomboid（僵尸毁灭工程）的 Windows 桌面启动器 + 模组管理器**。它不修改游戏本体，
而是把玩家在启动前后必须手工完成的事情收进一个界面：找 Steam 与游戏目录、配内存与启动参数、管理
Lua/JAR 模组、订阅与浏览创意工坊、下载光影包、翻译开发者新闻，以及**在游戏窗口内部绘制加载进度**。

| 维度 | 内容 |
| --- | --- |
| 形态 | Tauri 2 桌面应用（WebView2 + Rust），无边框自绘标题栏，单窗口 970×600 |
| 前端 | React 19 + TypeScript ~6 + Chakra UI 3 + Emotion + FontAwesome 7，Vite 8 |
| 后端 | Rust 2021（`src-tauri`），61 个 Tauri 命令、7 类事件 |
| 注入器 | 独立 cdylib `pz_overlay`（`overlay-dll`），MinHook + OpenGL + GDI + JVMTI |
| 平台 | 仅 Windows x64（游戏、启动器、DLL 必须同为 64 位） |
| 规模 | 主要手写文件约 **2.8 万行**（见 §4.3），Rust 测试 **131 条**，前端契约测试 **36 条** |
| 界面语言 | 简体中文（代码注释、错误文案、UI 文案一致） |

---

## 1. 项目定位

### 1.1 它解决的四类问题

1. **环境与启动链**：Steam 可能装在任意盘符、游戏可能在任意库、账户可能在多个 `userdata` 目录里。
   启动器从注册表与 VDF 里把这些找出来，并在每次启动前把内存上限、JVM agent 参数写进
   Steam 启动项、`ProjectZomboid64.json`、`ProjectZomboid64.bat` 三处。
2. **模组管理**：Lua 模组要处理加载顺序、依赖前置、B42 差异；JAR 模组（ZombieBuddy）要处理审批、
   `Mod.info` 文件态、注解扫描与冲突分析。两者语义完全不同，不能共用一套「启用/禁用」。
3. **加载过程可见**：原版游戏启动时长时间黑屏/无反馈。启动器在游戏窗口**内部**绘制红色底 + 控制台尾部 +
   进度条，进度来自 `console.txt` 里的 `[ZB]` 输出，加载完成自动撤除。
4. **周边资源**：创意工坊浏览器、Modrinth/CurseForge 光影下载、Steam 新闻批量翻译、Viewpoint 截图与
   光影包管理、自定义启动背景。

### 1.2 设计原则

| 原则 | 在代码里的体现 |
| --- | --- |
| **不碰游戏本体** | 只改启动配置；唯一的例外是用户显式点「安装 PZ Optimization」，且调用的是模组自带的官方安装器 |
| **写前备份、失败回滚** | `game_path\.pzmanager-backups\<时间戳>\` + `apply_file_transaction()` 内存快照回滚 |
| **优化不能变成前置条件** | 内存清理失败、状态文件读不到、DLL 路径解析失败都只记日志，绝不阻断启动 |
| **任何失败回退原版路径** | 覆盖层注入失败 → 游戏照常启动；极速启动任一环节不成立 → 摘掉 `-agentpath` 回到 ZombieBuddy 原生扫描 |
| **最小权限** | Tauri capabilities 只开窗口控制、文件对话框、剪贴板写图/文本，以及两条精确匹配的 Steam URL |
| **渐进披露** | 开发者模式只多显示路径、注解统计、冲突分析、注入方法与详细通知，不隐藏核心操作与错误 |

### 1.3 它不做什么

- 不是模组开发/打包工具，不生成 `mod.info` 或 jar。
- 不改游戏存档、不改 `projectzomboid.jar`（PZ Optimization 是第三方模组自己的类覆盖安装器）。
- 不接管 Steam 的下载与订阅，工坊内容仍由 Steam 下载。
- 不内置作弊或联机绕过；`policy=allow-all` 只是跳过 ZombieBuddy 的审批弹窗，仍受游戏与服务器规则约束。

---

## 2. 功能全景

### 2.1 一级导航与能力地图

```text
pzmanager
├─ 启动          账户 / 内存 / 启动参数 / 背景 / 启动·停止 / 覆盖层会话 / 启动时长
├─ 支持库        路径探测 / ZombieBuddy / PZ Optimization / Viewpoint 3D
├─ 截图管理      Viewpoint 截图列表、预览、复制到剪贴板
├─ 光影管理      ZIP 导入（拖拽/选择）、删除、图标与搜索词
├─ 资源中心      创意工坊 / 光影下载 / 开发者新闻
├─ 模组管理      Lua 模组 / JAR 模组 / 模组预设
├─ 设置          通用 / 翻译器 / 控制台 / 下载器 / 模组加载器 / 启动
└─ 任务中心      标题栏右侧，后台下载等任务进度
```

### 2.2 功能清单与成熟度

| 功能 | 入口 | 状态 | 备注 |
| --- | --- | --- | --- |
| Steam / 游戏 / 工坊 / 账户探测 | 启动页、支持库 | 稳定 | 注册表 + VDF + ACF 多级回退 |
| 内存上限设置 | 启动页滑块 | 稳定 | 1 GB 起、512 MB 步进、按物理内存校验 |
| 启动参数编辑 | 启动详情弹层 | 稳定 | 默认 `-agentlib:zbNative --` |
| 启动 / 停止游戏 | 启动页大按钮 | 稳定 | 0.3 s 轮询进程与窗口 |
| 启动时长对比 | 启动页、设置 › 控制台 | 稳定 | 落盘 `startup-timing.json`，显示与上次的差值 |
| ZombieBuddy 安装 | 支持库 / 设置 › 模组加载器 | 稳定 | 复制 DLL/JAR + 三处启动配置 |
| PZ Optimization 安装/更新/卸载 | 支持库 | 稳定（依赖第三方安装器） | 调 `install.ps1`，按 revision 比对 |
| Viewpoint 3D 一键配置 | 支持库 | 稳定 | 三个工坊模组齐备才可点 |
| Viewpoint 截图 | 截图管理 | 稳定 | 复制走 Canvas → RGBA → 剪贴板 |
| 光影包导入/删除 | 光影管理 | 稳定 | 拖拽或文件选择，不覆盖同名 |
| 创意工坊浏览 | 资源中心 › 创意工坊 | 稳定 | 排序/时间/标签/搜索/分页，300 s 缓存 |
| 工坊详情与翻译 | 工坊列表右键 | 稳定 | 官方 API + BBCode + 按段翻译 |
| 光影资源检索/下载 | 资源中心 › 光影下载 | 稳定 | Modrinth 优先、CurseForge 兜底、镜像回退；下载进入全局任务中心 |
| 开发者新闻与翻译 | 资源中心 › 开发者新闻 | 稳定 | Steam News + 15 条/批翻译 |
| Lua 模组管理 | 模组管理 › Lua | 稳定 | 排序、依赖前置、批量启停 |
| JAR 模组管理 | 模组管理 › JAR | 稳定（需 ZombieBuddy） | 注解扫描、审批、冲突分级 |
| 模组预设 | 模组管理侧栏 | 稳定 | Lua 顺序 + JAR 启停快照 |
| 覆盖层加载界面 | 自动（启动游戏）/ 设置 › 控制台 | 稳定 | 注入 + Hook + 窗口内绘制 |
| 内存清理 | 设置 › 启动 | 稳定 | 静默阈值 + 启动时一次 |
| 极速启动（JVMTI） | 设置 › 启动 | **实验性** | 链路已打通，游戏内仍有已知崩溃风险，见 §5.4 |
| 自定义/工坊背景 | 启动设置弹层 | 稳定 | 内置图、导入图、工坊 `title*.png` 四宫格拼接 |

### 2.3 三条典型使用路径

**首次使用**：启动 → 自动探测（找不到就提示）→ 支持库确认路径 → 设置 › 模组加载器安装 ZombieBuddy
→ 模组管理 › JAR 刷新列表并审批 → 启动游戏。

**日常启动**：启动页确认账户与内存 → 点「启动游戏」→ 游戏窗口出现后自动注入覆盖层 → 看到
`[ZB]` 阶段与 advice 进度 → 进入主菜单后覆盖层自动消失 → 启动时长显示在按钮下方。

**模组调试**：模组管理 › Lua 调顺序/依赖 → JAR 页看冲突分析 → 保存为预设 → 换预设一键切换 →
设置 › 控制台观察覆盖层会话与启动时长。

---

## 3. 功能详解

### 3.1 启动页

**账户面板**：显示 Steam 头像回退、Persona 名与账户名。账户名可点击隐藏/显示（用两条短横线遮挡），
状态持久化到 `settings.json` 的 `steamAccountHidden`。

**游戏内存滑块**：
- 范围 `[1024 MB, 系统物理内存]`，步进 512 MB；拖动时本地即时更新，**250 ms 防抖**后写盘；
- 8 GB 标为「推荐」、16 GB 标为「参考线」；超过 16 GB 显示不稳定警告；
- 后端 `validate_memory_limit_mb` 再次校验（不能为 0、不能超过物理内存），防止前端被绕过。

**启动参数**：弹层里逐条编辑，可增删；启动时按列表顺序以空格拼接。默认
`["-agentlib:zbNative", "--"]`（[src/launchArgs.ts](src/launchArgs.ts)）。

**启动按钮**：
- 点击后禁用 3 秒（`launchCooldown`），并开始每 300 ms 轮询游戏进程/窗口；
- 检测到窗口就绪后，「启动游戏」左滑、「停止游戏进程」划入（300–320 ms）；
- 游戏自行退出时按钮复位（[src/gameProcess.ts](src/gameProcess.ts) 的 `shouldResetGameWindow()` 对账）。

**启动设置弹层**：内存滑块 + 背景选择（内置、导入的自定义图、创意工坊背景）。选中工坊背景时，
左下角出现可折叠的「创意工坊背景」信息胶囊，显示模组名与 Workshop ID。

**启动时长**：右下角显示「本次启动 xx.x s」并在有上次记录时显示对比。计时从检测到游戏窗口开始，
到发停止事件（移除覆盖层）结束；后端落盘、事件推送、命令读回三条路共用一份数据。

### 3.2 支持库

三项安装内容收在同一个「支持组件」面板里，每行是一张可折叠卡片（默认折叠：名称 + 一句话 + 状态标签 + ⋯）：

| 行 | 状态判定 | 动作 |
| --- | --- | --- |
| ZombieBuddy 模组加载器 | 已安装 / 待安装 / 模组未下载（缺哪个文件会列出） | 只报状态并指路「设置 › 模组加载器」，不在本页安装 |
| PZ Optimization | 已安装 / 可更新 / 可安装 / 未下载 | 一键安装、更新、卸载；显示来源与前后 revision |
| Viewpoint 3D 模组配置 | 三个模组是否都订阅：Project Viewpoint、ZombieBuddy、Temporary 42.21 Fix | 一键配置 3D 模组（复制 libs、写入启动文件） |

开发者模式额外显示 Steam 安装路径与创意工坊路径。

### 3.3 截图管理

读取游戏 Viewpoint 截图目录中的图片，按文件名列出缩略图、大小与目录路径。点击打开预览；
预览里「复制到剪贴板」的链路是：字节 → `Blob` → `createImageBitmap` → Canvas 2D →
`getImageData` → `TauriImage` → `writeImage`，避免直接传 PNG 字节导致剪贴板格式不一致。
只允许读取托管目录内的文件（目录穿越防护）。

### 3.4 光影管理

- 拖拽 ZIP 到虚线框，或点「选择 ZIP 文件」；
- 后端 `import_viewpoint_shaderpack` 把文件复制到 `%USERPROFILE%\Zomboid\viewpoint-shaderpacks`，
  **同名不覆盖**（返回错误提示），并校验扩展名；
- 列表显示图标、文件名、大小与搜索词（悬停 title）；图标由包内 `shaders` 目录结构推导；
- 删除只操作托管目录内的 ZIP。

### 3.5 资源中心

资源中心是「左侧分类 + 右侧正文」结构，三个分类共用一个外壳与切换动画。

#### 3.5.1 创意工坊

**筛选维度**（[src-tauri/src/workshop.rs](src-tauri/src/workshop.rs)）：

| 维度 | 取值 |
| --- | --- |
| 排序 `browsesort` | 最热门（趋势）、最新发布、最近更新、订阅最多、评分最高 |
| 趋势时间 `days` | 1 / 7 / 30 / 90 / 365 天（仅趋势排序时出现） |
| 每页 `numPerPage` | 15 / 30 |
| 标签 | 31 个官方标签，支持「必需」与「排除」两种选择 |
| 搜索 | 标题/描述关键词，输入 400 ms 防抖后自动查询，回车立即查询 |

**实现要点**：
- 直接抓 `steamcommunity.com/workshop/browse/?appid=108600&section=readytouseitems...`，无需 API Key；
- 解析 Steam 内嵌的 `<script id="valve-ssr-data">` 里的 `renderContext`，并保留旧版
  `window.SSR.renderContext = JSON.parse("...")` 的回退解析；
- 结果按 URL 做 **300 秒内存缓存**（`CACHE_TTL`），翻页/改筛选会生成不同的 key；
- 列表项：预览图、标题、3 个标签、一行描述、更新时间/订阅/浏览/作者/大小；
- 左键直接 `steam://openurl/...` 打开工坊页；右键菜单提供「在 Steam 中打开」「在窗口内打开」；
- 分页信息来自 `meta.totalCount/currentPage/totalPages`，切换页时回到列表顶部。

**窗口内详情**：右键「在窗口内打开」→ `fetch_workshop_item_detail` 调
`ISteamRemoteStorage/GetPublishedFileDetails/v1`（POST，一次拿全文）→ 正文 BBCode 转 HTML →
按段落包 `<span data-sid="...">` → 复用新闻翻译管线逐批翻译 → 按 `sid` 原位替换。
标题与正文共用 sid 方案（`w<id>_title` / `w<id>_t<node>_s<idx>`），所以标题也会翻译；
工坊标题是纯文本，**不做** BBCode 剥离，避免吃掉 `[B42]` 这类后缀。

#### 3.5.2 光影下载

- 数据源：Modrinth 优先、CurseForge 兜底（[src-tauri/src/resource.rs](src-tauri/src/resource.rs)）；两者都经过 PCL2 同款镜像
  `mod.mcimirror.top`，失败再回退官方域名；
- 可按 Minecraft 版本、加载器/标签、排序、下载源筛选；`fetch_shader_resource_files` 拿版本与文件列表；
- 点击版本文件旁的「下载」会立即把该文件加入右上角**全局任务中心**并关闭版本弹窗，下载在后台继续；
- 下载走 `download_shader_file`（多线程分块，见 §5.8，任务 ID 作为 `downloadId` 传给后端，
  以便把 `shader-download-progress` 精确对回任务），默认保存到
  `%USERPROFILE%\Zomboid\viewpoint-shaderpacks`，也可在设置 › 下载器改目录；
- 任务中心显示每条下载的进行中/已完成/失败状态、进度、保存路径，支持失败重试与清除已完成；
- 「自定义下载」表单：URL、可选文件名、目录、1–16 线程；设置持久化到 `settings.json`。

#### 3.5.3 开发者新闻

- 取 `ISteamNews/GetNewsForApp/v2`（AppID 108600），可限制条数与单条 gid；
- 正文先清洗（白名单标签、URL 校验、属性过滤），再按可翻译片段切分并包 `data-sid`；
- 前端按 **15 条/批**调 `translate_developer_news_batch`，后台按 `BATCH_SIZE=15`、
  单条 `MAX_TEXT_LEN=2000` 再切批；Google 优先、失败回退 Yandex；
- 每条新闻显示「翻译来源 Google 62% · Yandex 38%」的引擎占比（`engineTally`）；
- 翻译语言取设置 › 通用的全局值；选 English 时不翻译、直接显示原文；
- 翻译结果与源文按 `(语言, 文本)` 做内存缓存，重复打开页面不会重复请求。

### 3.6 模组管理

模组管理是「类型 + 筛选 + 排序 + 预设」的左侧栏结构，右侧正文在 Lua / JAR 两个完全不同的面板间切换。

#### 3.6.1 Lua 模组

数据源是 `%USERPROFILE%\Zomboid\mods` 与创意工坊目录；启用状态来自 `mods\default.txt` 的 `mods { ... }` 块。

| 能力 | 行为 |
| --- | --- |
| 列取 | 读 `mod.info` / `common\mod.info`，解析 `id/name/poster/require/loadAfter/loadBefore/loadFirst/loadLast` |
| 副本合并 | 同一个 ID 的 common 与 B42 副本合并：B42 优先提供元数据，其余字段取并集（`merge_lua_mod_entry`） |
| 筛选 | 全部 / 已启用 / 未启用 / B42（可多选组合） |
| 排序 | 加载顺序（启用项按 `loadOrder`）或名称（`localeCompare("zh-CN")`） |
| 启停 | 单个开关；非 B42 模组禁止从启动器启用（游戏不认） |
| 调序 | 悬停行出现 ↑/↓，只对已启用项生效 |
| 批量 | 全选当前可见、反选、批量启用/停用；可按住鼠标跨行拖选 |
| 搜索 | 名称 / ID / 作者 / 简介（后两者当前仅前端类型预留） |
| 详情 | 展开显示 ID、路径（开发者模式）、来源、common/B42、前置列表、加载序号 |

**写回**：`write_default_mod_ids` 只替换 `mods{...}` 块，保留文件其余内容、换行符与缩进；
写出的行是**不带引号**的 `mod = 模组ID,`——加引号会让游戏把引号当成 ID 的一部分。
启用列表在写回前经过 `order_mod_ids_by_requires` 的拓扑排序：
`require` 与 `loadAfter` 是正向边、`loadBefore` 反向，`loadFirst/loadLast` 只影响稳定兜底顺序；
缺失的依赖会在结果里报告，而不是静默丢弃。

#### 3.6.2 JAR 模组（ZombieBuddy）

- **前置门禁**：只有「模组加载器方案 = ZombieBuddy」且加载器已安装时才可管理；否则显示锁定页并指路设置。
- **扫描**：`start_zombie_buddy_jar_scan` 后台线程扫描工坊与本地模组根，读 `Mod.info`
  （回退 `Mod.info.disabled`），用 `java-class-rs` 解析 class 字节，识别
  `@Patch` / `@Exposer$LuaClass` 与 `@Overwrite/@Redirect/@Inject/@ModifyConstant/@ModifyVariable`。
- **进度**：命令返回 `scanId`，逐条 emit `jar-mod-scan-update`；前端画扫描进度条与
  四帧点状「后台扫描中…」动画。
- **审批**：决策存 `%USERPROFILE%\Zomboid\.pzmanager\jar-mod-state.json`
  （`decision: true/false/null` + `disabled`）。工坊重下覆盖 `Mod.info` 后，
  `apply_jar_mod_file_state` 会把用户选择重新套用回文件。
- **文件态**：启用 = `Mod.info`；禁用 = 改名成 `Mod.info.disabled`（其他启动器也不会读取）。
- **冲突分析**（开发者模式）：只统计**已批准**模组，按 `(注解, 方法名)` 求交集，
  风险分级 `@Overwrite = 高`、`@Redirect/@ModifyConstant = 中高`、
  `@Inject/@ModifyVariable = 中低`，稳定排序后展示。
- **刷新**：`刷新 JAR 列表` 是唯一会重建极速启动预扫描缓存的入口（见 §5.4）。

#### 3.6.3 模组预设

预设是 **Lua 启用顺序 + 每个 JAR 模组的启停** 的快照，存 `.pzmanager\mod-presets.json`（`formatVersion: 1`）。

- 保存：按当前 `default.txt` 原始顺序记录 Lua ID，并记录所有 JAR 模组的 `modDir/modId/workshopId/packageName/enabled`；
- 应用：Lua 按预设顺序重写 `default.txt`（应用前备份到 `.pzmanager\default-backups\<时间戳>`），
  只保留当前存在且是 B42 的 ID，其余计入 `luaSkipped`；JAR 侧不在预设里的全部停用；
- 匹配：优先按目录大小写不敏感相等，否则按 `(modId, workshopId, packageName)` 三元组；
- 删除：按 ID 移除并校验存在性。

### 3.7 设置

设置页同样是「左侧分类 + 右侧正文」，六个分类共用一个滚动容器。

#### 3.7.1 通用

| 项 | 说明 |
| --- | --- |
| 开发者模式 | 打开注入方法、模组标签、冲突分析、路径、详细通知；关闭时只显示错误 Toast，且正文裁到首行 140 字符 |
| 翻译语言 | 简体中文 / 繁體中文 / English / 日本語 / 한국어 / Русский；工坊详情与开发者新闻共用；English 表示不翻译 |
| 字体对比度 | 0–100、步进 5；写到 `documentElement` 的 `--pz-text-contrast`，CSS 用 `color-mix()` 把浅色面上的文字往近黑推，0 = 保持原配色 |
| 模糊度 | 0–200%、步进 10，默认 100%；写到 `documentElement` 的 `--pz-blur-scale`，App.css 里每一处 `blur()` 都是 `calc(<基准> * var(--pz-blur-scale, 1))`，所以面板、菜单、弹层、标题栏、Toast 一起变；0 = 关掉模糊 |

#### 3.7.2 翻译器

- 自定义文本翻译，复用新闻/工坊那条链路（Google 优先、Yandex 兜底）；
- 上限 20,000 字符，单块 1,800 字符；按行拆分，空行原样保留，纯符号/数字行不送翻译；
- Ctrl + Enter 触发；显示「正在翻译 x / y 段」与翻译来源徽章；可一键复制译文；
- 目标语言默认跟随全局设置，在本页改动只影响本页。

#### 3.7.3 控制台

原启动页的覆盖层面板移到这里：会话状态徽章（等待/注入/加载/完成/错误）、阶段消息、
`[ZB]` 行数与最近一行（开发者模式）、「立即开始覆盖层会话」按钮、
「选择 DLL…」与当前 DLL 路径（开发者模式）。会话进行中按钮禁用，避免并发注入。

下方是**通知记录**：本次运行产生的所有 Toast 都会留档（包括简单模式下被隐藏的信息/成功提示），
显示时间、类型、完整正文，并可清空。

#### 3.7.4 下载器

自定义下载：URL、可选文件名（留空从链接提取）、保存目录、并行线程 1–16；
「开始下载」同样加入全局任务中心（不阻塞当前页面），「保存设置」写入 `settings.json`。

#### 3.7.5 模组加载器

- 方案下拉：ZombieBuddy / 不使用（原版加载），改完在**下一次启动**生效；
- 当前方案状态：已安装 / 待安装 / 模组未下载，列出缺失文件；Workshop ID 与「安装 Both」；
- 风险提示：安装会备份并修改启动配置，Java 模组可执行本机代码；
- 说明文字明确「加载器相关只在这里，其他页面只报状态并指路」，避免各页各写一套。

#### 3.7.6 启动

| 项 | 行为 |
| --- | --- |
| 极速启动（实验性） | 仅方案 = ZombieBuddy 可开；显示缓存状态行（时间/模组数/指纹是否匹配）；关闭时会**当场**摘掉已写入的 `-agentpath:` |
| 静默内存清理 | 后台每 10 s 读系统占用，≥ 阈值（30–95%，默认 80%）清一次；两次至少隔 60 s；可「立即清理一次」并显示释放量与耗时 |
| 启动时清理内存 | 点「启动游戏」时、拉起 Steam 之前清理一次；失败不影响启动 |

### 3.8 外壳与交互

- **无边框窗口**：自绘标题栏 32 px，品牌 + 主导航 + 窗口按钮；拖动区支持单击拖动、双击最大化；
  浏览器预览下不渲染拖动区与窗口按钮（[src/App.tsx](src/App.tsx) 的 `WindowTitlebar`）。
- **页面过渡**：200 ms；切到启动页时只让旧页滑出、启动页不做入场——启动页是整页固定层，整页淡入会让
  壁纸与亚克力面板先空一帧再补上，看起来「闪一下」。启动页面板已不再使用 `backdrop-filter`，
  材质方案见 [docs/acrylic-panel-options.md](docs/acrylic-panel-options.md)。
- **启动页亚克力**：不用 `backdrop-filter`——把壁纸再渲染一份、整层 `blur()` 后按面板圆角矩形裁出来垫在
  面板下面（`src/AcrylicGlass.tsx` + `.pz-acrylic-glass`），面板的半透明底色 / 颗粒层 / 描边保持不变。
  详见 [docs/acrylic-panel-options.md](docs/acrylic-panel-options.md)。
- **菜单材质**：所有 `Menu.Content`（下拉、多级子菜单、右键菜单）与启动页上弹面板共用一套亚克力——
  `backdrop-filter` 模糊 + `--pz-menu-tint` 半透明底 + 烘进 SVG 的极淡颗粒 + 顶边内高光。
  浮层是短暂的小面积层，不参与页面过渡，所以不会重现启动页常驻面板那种"切换时丢模糊"。
- **启动画面**：内联在 `index.html` 的 splash，先于 JS/CSS 绘制；最短 260 ms + 200 ms 淡出，
  双 `requestAnimationFrame` 后卸载，并有 8 s 兜底移除。
- **任务中心**：标题栏右侧的「任务」按钮 + 右上角浮层；光影下载以后台任务运行，显示进度、结果、
  失败重试与清除已完成，不影响页面切换。
- **通知**：右上角 Toast，位于顶部导航下方；非开发者模式只显示错误，正文取首行并截断到 140 字符；
  完整通知同时写入「设置 › 控制台 › 通知记录」。
- **响应式**：900 px 以下压缩布局，640 px 以下单列；控件高度统一 `--pz-control-height: 26px`。
- **无障碍**：导航用 `aria-current`、列表行用 `role="checkbox"` + 键盘 Enter/Space、
  图标按钮同时有 `aria-label` 与 `title`、所有异步区都有加载/空/错误状态；
  支持 `prefers-reduced-motion`。

---

## 4. 技术架构

### 4.1 技术栈与硬约束

| 层 | 选型 |
| --- | --- |
| 壳 | Tauri 2，插件 `opener` / `dialog` / `clipboard-manager` |
| 前端 | React 19、TypeScript ~6、Chakra UI 3、Emotion、FontAwesome 7、Vite 8 |
| 后端 | Rust 2021：`reqwest 0.13.5`（native-tls/schannel）、`futures-util`、`serde`、`regex`、`sha2`、`zip 8`、`image 0.25`、`java-class-rs 0.1`、`winreg 0.55`、`windows 0.62.2` |
| 注入器 | `pz_overlay`（cdylib）：`minhook 0.9`、`jni 0.21`、`windows 0.62.2` |

**硬约束**：

1. **仅 Windows x64**：非 Windows 目标在 `overlay.rs` / `overlay-dll` 里直接 `compile_error!`；
2. **注入默认后端必须 stable 可编译**：`CreateRemoteThread` + `LoadLibraryW`；
   `dll-syringe` 所有发布版都带 nightly feature，只在 `--features dll-syringe-inject` 下参与编译；
3. **覆盖层 DLL release 用 `panic = "unwind"`**：注入后初始化线程 panic 只能死一个线程，不能 abort 掉游戏；
4. **`reqwest` 用 native-tls 而非 rustls**：避免构建期依赖 `cl.exe` 编译 aws-lc。

### 4.2 运行时拓扑

```text
┌──────────────────────────────────────┐
│ pzmanager.exe（WebView2 + Rust）      │◄──────── invoke / emit ────────► React
│  · Steam/VDF 解析、配置改写           │
│  · 工坊/资源/新闻 HTTP                │
│  · overlay_session 会话线程           │
└──────┬──────────────────┬────────────┘
       │ 1. 写启动配置     │ 2. CreateRemoteThread + LoadLibraryW
       ▼                  ▼
 Steam localconfig     ProjectZomboid64.exe（JVM）
 ProjectZomboid64.json   └─ pz_overlay.dll
 ProjectZomboid64.bat        · Hook opengl32!wglSwapBuffers
                             · 读 %TEMP%\pz_overlay_status.txt
                             · 等 Local\PZManagerOverlayStop_<pid>
       │ 3. tail %USERPROFILE%\Zomboid\console.txt（解析 [ZB]）
       └──────────────────────────────► overlay_session 线程
```

### 4.3 代码地图与规模

| 文件 | 行数 | 职责 |
| --- | ---: | --- |
| [src/App.tsx](src/App.tsx) | 7,178 | 全部页面、状态、IPC 调用与局部组件 |
| [src/App.css](src/App.css) | 3,433 | 手写样式，类名统一 `pz-` 前缀 |
| [src-tauri/src/lib.rs](src-tauri/src/lib.rs) | 8,130 | Steam 发现、配置事务、模组、背景、命令注册、内联测试 |
| [src-tauri/src/overlay_session.rs](src-tauri/src/overlay_session.rs) | 1,162 | 会话状态机、控制台解析、进度估算、启动时长 |
| [overlay-dll/src/classfile.rs](overlay-dll/src/classfile.rs) | 1,478 | class 文件解析与字节码改写 |
| [overlay-dll/src/fastlaunch.rs](overlay-dll/src/fastlaunch.rs) | 1,502 | JVMTI agent、函数表、类加载 hook |
| [overlay-dll/src/ui.rs](overlay-dll/src/ui.rs) | 1,000 | GDI DIB 绘制与 GL 纹理上传 |
| [overlay-dll/src/lib.rs](overlay-dll/src/lib.rs) | 889 | DllMain、MinHook、GL 覆盖、停止事件 |
| [src-tauri/src/resource.rs](src-tauri/src/resource.rs) | 846 | Modrinth / CurseForge 检索与镜像 |
| [src-tauri/src/workshop.rs](src-tauri/src/workshop.rs) | 804 | 工坊爬虫、SSR 解析、详情 API |
| [src-tauri/src/news_translation.rs](src-tauri/src/news_translation.rs) | 549 | 分段、批处理、Google/Yandex、缓存 |
| [src-tauri/src/news.rs](src-tauri/src/news.rs) | 440 | 新闻 API、HTML 清洗、BBCode |
| [src-tauri/src/memclean.rs](src-tauri/src/memclean.rs) | 362 | 内存状态、工作集清理、静默线程 |
| [src-tauri/src/overlay.rs](src-tauri/src/overlay.rs) | 343 | 注入入口、路径校验、进程轮询、重复注入 |
| [src-tauri/src/downloader.rs](src-tauri/src/downloader.rs) | 183 | Range 分块下载、进度事件、合并 |

合计约 2.83 万行（不含测试夹具与生成代码）。

### 4.4 后端模块职责

| 模块 | 职责 | 备注 |
| --- | --- | --- |
| `lib.rs` | 环境探测、配置改写事务、模组、背景/截图/光影、class 注解、命令注册 | 单一巨石文件；内联测试跨模块合计 117 条 |
| `workshop.rs` | 工坊浏览（HTML/SSR 解析，无 API Key）、详情 API | 300 s 缓存 |
| `resource.rs` | Modrinth + CurseForge 光影检索 | 对齐 PCL2 的 `DlSourceModGet` |
| `downloader.rs` | 多线程分块下载 + 进度事件 | 供下载器与资源中心共用 |
| `news.rs` | Steam 新闻 + 正文清洗 + 可翻译片段 | 夹具 `tests/fixtures` |
| `news_translation.rs` | 批量翻译、HTML span 回填、缓存 | Google + Yandex |
| `overlay.rs` | 注入入口、路径规范化、进程轮询、重复注入检测 | 后端编译期二选一 |
| `overlay_inject_win32.rs` / `..._dll_syringe.rs` | 等价注入后端 | 后者需 nightly |
| `overlay_session.rs` | 等窗口 → 注入 → 解析控制台 → 写状态 → 判定完成 → 发停止事件 | 另有进程查询/停止命令 |
| `memclean.rs` | 系统内存状态、工作集清理、静默循环 | 跳过游戏与自身 |
| `main.rs` | 仅调用 `pzmanager_lib::run()` | 7 行 |

### 4.5 前端组织

- **单组件 + 纯函数模块**：`App()` 内约 138 个 `useState` / 28 个 `useRef`，
  页面内容先构造成 `launchContent` / `overviewContent` / … 再按 `activePage` 渲染；
- **可测试的纯函数**（[src/](src/)）：模组排序/多选/冲突分析/通知策略/启动参数/工坊链接/窗口状态/注入封装；
- **竞态处理**：长请求用 `useRef` 保存请求序号或扫描 ID（`workshopRequestId`、
  `developerNewsRequestId`、`jarScanIdRef`、`workshopBackgroundScanIdRef`），回包时比对后丢弃过期结果；
- **大列表**：`SleepingModRow` 用 `IntersectionObserver`（rootMargin 480 px）让视口外的行休眠，
  保留测量高度占位，避免几百个模组同时挂载；
- **选择集**：批量操作用 `startTransition` 更新 Set，避免大列表同步重渲染卡顿；
- **二进制媒体**：海报/截图/背景走 `invoke<ArrayBuffer>` → Blob URL，卸载时 `revokeObjectURL`。

### 4.6 IPC 契约

| 项 | 约定 |
| --- | --- |
| 调用 | `invoke<T>("命令名", { camelCase 参数 })`，后端参数用 `snake_case`，Tauri 自动转换 |
| 错误 | 后端 `Err(String)` 原样抛出；前端统一转 `Error` 后交给 Toast |
| 二进制 | `tauri::ipc::Response` 返回字节，前端转 Blob |
| 长任务 | 返回 `scanId` / `downloadId`，进度用事件推送 |
| 事件 | 7 类：`overlay-session`、`overlay-zb-progress`、`startup-timing`、`jar-mod-scan-update`、`workshop-background-scan-update`、`shader-download-progress`、`memory-cleaned` |
| 注册 | 61 条命令集中在 `lib.rs` 的 `generate_handler!`，新增命令必须同时改这里与前端调用 |

完整命令清单见附录 B。

### 4.7 跨进程契约

三条约定，改任何一边都必须同步另一边：

1. **启动器 → DLL**：`%TEMP%\pz_overlay_status.txt`。格式为逐行 `key=value`：
   `progress=0.000–1.000`（有精确分母时才写）、`phase=阶段文字`、`memmax=内存MB`、
   `line=控制台行`（最多 6 行）。写入用「临时文件 + rename」避免 DLL 读到半截。
2. **启动器 → DLL**：命名事件 `Local\PZManagerOverlayStop_<游戏PID>`，`SetEvent` 即停止覆盖。
3. **DLL → 启动器**：命名事件 `Local\PZManagerOverlayStarted_<PID>`（首帧已画），
   启动器在收到它之前**不做任何加载完成判定**；另有 `%TEMP%\overlay.log` 供人读。

---

## 5. 关键实现

### 5.1 Steam 发现链

```text
注册表 / 常见路径
   └─ steam_roots()                     候选 Steam 根目录
        └─ libraryfolders.vdf           所有库（可跨盘）
             ├─ appmanifest_108600.acf  游戏安装目录
             ├─ workshop/content/108600 工坊目录
             └─ userdata/*/config/loginusers.vdf
                    └─ 最近账户（SteamID64 → accountId）
                           └─ userdata/<accountId>/config/localconfig.vdf
                                  └─ LaunchOptions 写入点
```

- 缺失任何一环都按「正常的未找到」处理，不抛错；
- 账户选择取 `MostRecent` 标记，找不到则取第一个可用账户；
- 路径解析全部容忍大小写差异与引号转义，VDF 用自写的最小解析器（不引第三方 crate）。

### 5.2 启动链与配置事务

`launch_project_zomboid(memoryMb, launchArgs, fastLaunchDllPath)` 的顺序是刻意固定的：

1. 读系统物理内存，校验内存上限；
2. 找 Steam `localconfig.vdf`（找不到直接报错，因为 Steam 启动方式必须写这里）；
3. 读 `settings.json` 的 `modLoaderScheme` 与 `fastLaunch`；
   `policyAgent = scheme == "zombieBuddy" ? "-agentlib:zbNative=policy=allow-all" : None`；
4. 决定 `fastLaunchAgent`：**开关打开 + 方案是 ZombieBuddy + DLL 真实存在**，三者缺一即 `None`；
5. 写 Steam 启动项（内存 + 参数；方案为 `none` 时先摘掉前端默认带上的 `-agentlib:zbNative`）；
6. `apply_launch_loader` 写游戏目录的 `ProjectZomboid64.json` 与 `.bat`：
   - 内存**始终**写（JSON 里旧的 `-Xmx` 会覆盖启动器设置）；
   - agent 只在方案启用加载器时归一化，且保证**只有一条**、带 `policy=allow-all`；
   - `-agentpath:` 被插到 `vmArgs[0]`（JVMTI 按命令行顺序加载，必须排在 ZB 之前）；
     路径含空格时只写 JSON、不写 `.bat`（`SET _JAVA_OPTIONS` 按空白切分）；
   - 写入前备份到 `.pzmanager-backups\<时间戳>`；多文件走 `apply_file_transaction`；
7. 若开启「启动时清理内存」，清理一次（失败只记日志）；
8. `cmd /C start steam://rungameid/108600`。

**事务实现**（`apply_file_transaction`）：

- 先对所有目标做内存快照（原内容或「不存在」）；
- 逐个写：同目录临时文件 → rename；失败时**逆序**恢复已写的文件，报告原始错误与回滚错误；
- 因此不存在「JSON 改了、BAT 没改」的半套配置。

**幂等**：内容已经正确时不重复写，避免每次启动都产生新备份目录。

### 5.3 JAR 注解扫描

`scan_java_mod_jar` 对每个 class 调用 `class_annotations`：

- 类级注解识别 `Lme/zed_0xff/zombie_buddy/Patch;` 与 `Lme/zed_0xff/zombie_buddy/Exposer$LuaClass;`；
- 方法级注解收集 `Overwrite / Redirect / Inject / ModifyConstant / ModifyVariable`，用于冲突分析；
- 类名从常量池 `this_class` 取内部名后转点号；包名同时保留内部名，供预扫描缓存使用；
- 收**声明包及其子包**的类（对齐 ClassGraph 的 `acceptPackages` 语义），否则 ZB 自己的补丁类会被漏掉。

扫描结果同时喂给三处：JAR 列表、冲突分析、极速启动缓存。

### 5.4 极速启动（实验性）

**目标**：启动器已经预扫描过 JAR，知道每个模组有哪些 `@Patch` 类；把这份清单喂给
ZombieBuddy 的 `PatchEngine.collectPatches(package, loader)`，短路其中的 ClassGraph 全量扫描，
省掉约 20 秒。

**链路**：

```text
ProjectZomboid64.json vmArgs[0] = -agentpath:<pz_overlay.dll>
  └─ JVM 调 Agent_OnLoad
       ├─ 读 settings.json 再判门禁（fastLaunch && zombieBuddy）
       ├─ 校验缓存 format + modSetHash
       ├─ AddCapabilities / SetEventCallbacks
       ├─ 立刻打开 ClassFileLoadHook（不能等 VMInit）
       └─ VMInit：DefineClass 辅助类 FastLaunchBridge
              └─ PatchEngine 首次加载时改写字节码：
                   原 collectPatches → collectPatches$pzm（只改 name_index）
                   追加同名同签名包装方法（aload_0, aload_1, invokestatic collect, areturn）
                   包装体带 catch(Throwable) 兜底 → 调原实现
```

为什么必须这样：

- **不能用 `RetransformClasses` 事后补救**：JVMTI 规范禁止 retransform 新增/删除/改名方法，
  而「包装式装配」正是新增方法 + 改名，只能在类加载那一刻改写；
- **`-agentpath:` 必须排最前**：ZB 的 `-agentlib:zbNative` 在它自己的 onload 里就加载 `PatchEngine`，
  排后面时 hook 只能等到 `VMInit`，那时类早已定义完（实测 hook 一次都没触发）；
- **hook 必须在 `Agent_OnLoad` 打开**：`VMInit` 晚于所有 agent 的 onload；
- **桥接类按 `调用方 loader → 系统 loader → 线程上下文 loader` 依次取类**：ZB 传进来的
  `loader` 实际是 `null`（反编译证实），直接 `Class.forName(name,false,null)` 会用 bootstrap，
  43/43 全部 CNFE；
- **缓存类名必须是二进制名（点号）**：扫描产出内部名，`Class.forName` 只认点号；两侧都做转换兜底。

**缓存**：`%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`

| 字段 | 含义 |
| --- | --- |
| `format` | 当前为 1 |
| `generatedAt` | 生成时间 |
| `modSetHash` | 模组集合指纹（FNV-1a） |
| `mods[]` | `modName/jarPath/basePackage/jarHash/patchClasses/luaClasses` |

- 只收**已批准且启用**的模组；额外并入 ZombieBuddy 加载器本体（否则强制模式下 ZB 自己的补丁会丢，游戏弹「未安装」）；
- 缓存**持久化**：只有「刷新 JAR 列表」重建；自动扫描仅在缓存不存在时补一份；改开关/审批/启停都不删除；
- 指纹不一致时设置页状态行提示重建，但**不会**自动放弃缓存。

**默认是「强制模式」**：缓存未命中/不可用 → 返回空列表，**不回退**原生扫描；逃生开关
`PZM_FAST_LAUNCH_ALLOW_FALLBACK=1` 恢复回退。代价是模组集合变更后必须刷新，否则新模组没有补丁。

**当前成熟度（必须如实告知）**：

| 环节 | 状态 |
| --- | --- |
| 缓存产出与持久化 | 已完成并实测（一次扫描约 25–26 个模组、Viewpoint 92 patches / 8 exposers） |
| 字节码工具链（包装式改写） | 已完成，对真实 `PatchEngine.class` 逐字节断言 |
| 辅助类与缓存命中 | 已完成；harness 实测命中 43/43、92/92 |
| 启动器接线（`-agentpath` 最前、关开关即清理） | 已完成并有单测 |
| 游戏内「真的加速」 | ⚠️ 未证明；且出现 `LinkageError: duplicate class definition for zombie.vehicles.UI3DScene` |

崩溃机制：原生扫描要跑几十秒，期间游戏自己已加载目标类，ZB 走 `retransform`（安全）；
极速启动瞬间返回清单，ZB 撞上「还没加载」的类，走 ByteBuddy `new load` 路径，随后游戏自己再
`loadClass` 同一个类 → duplicate。实验开关 `PZM_FAST_LAUNCH_PRELOAD_TARGETS=1`（或
`%TEMP%\pzm-fast-launch-preload.flag`）会尝试提前加载目标类，但**未验证**能消除崩溃。
应急：关设置开关（会当场摘掉 `-agentpath`）或 `PZM_FAST_LAUNCH_DISABLE=1`。

### 5.5 覆盖层：注入 → Hook → 窗口内绘制

**注入侧**（[src-tauri/src/overlay.rs](src-tauri/src/overlay.rs)）：

- 路径规范化：绝对路径校验；相对路径依次尝试「当前工作目录 → exe 目录及其 6 级祖先」；
  刻意不用 `canonicalize`（`\\?\` 前缀会让 `LoadLibraryW` 与模块比对不一致）；
- 60 秒内每 500 ms 轮询 `ProjectZomboid64.exe`，找不到再退 `javaw.exe`；
- 若目标进程已加载同名模块，直接返回「跳过重复注入」（避免多一份引用计数与重复 Hook）；
- 默认后端：`CreateRemoteThread` + `LoadLibraryW`。

**DLL 侧**（[overlay-dll/src/lib.rs](overlay-dll/src/lib.rs)）：

- `DllMain` 只记模块句柄、`DisableThreadLibraryCalls`、起后台线程；
- 等 `opengl32.dll` 与 `wglSwapBuffers` 导出（最多 60 s），MinHook 安装，原始指针存原子量；
- 首次回调时用 `wglGetProcAddress` 解析并缓存 `glBindFramebuffer`；解析不到就永久降级为「不动 FBO」；
- 回调只用原子操作，不做分配、不 `unwrap`；
- 另有一条 GDI 线程在「窗口已出现但还没换帧」期间直接刷窗口 DC；
- 任何失败只写 `%TEMP%\overlay.log` 后静默退出，绝不影响游戏。

**窗口内界面**（[overlay-dll/src/ui.rs](overlay-dll/src/ui.rs)）：

- 同一张 GDI DIB 两用：换帧前 `BitBlt` 到窗口 DC；换帧后上传为 GL 纹理、画满屏四边形；
- 40 ms 刷新（≈25 fps），显示阶段文字、进度条、最新 6 行控制台；
- 字体 Consolas，DPI 用 `GetDpiForWindow` 缩放；窗口非前台时每 200 ms 主动重绘一次；
- 红色覆盖不是全屏黑幕，而是画在游戏窗口内部，不会遮挡其他窗口。

**进度来源**：

- 分母：解析 `[ZB] java mod list to load:` 列出的 jar，统计其中声明的 advice 方法总数，
  再加上 ZB 自带的 bundled patches（实测 614，运行期累加 611，误差可接受）；
- 分子：累加 `[ZB] patching <目标> with N advice(s)` 的 N；
- 精确进度 = 分子 / 分母，封顶 0.97；≥97% 即提前判定完成；
- 分母就绪前用「阶段里程碑 + 行数缓慢增长」兜底，且进度只增不减。

**完成判定**（满足任一，且必须先收到 `Started` 事件）：

| 条件 | 阈值 |
| --- | --- |
| 控制台主菜单标记 | `STATE: exit zombie.gameStates.TermsOfServiceState` / `added to the main menu` / `ZombieBuddy.getVersion()` |
| `[ZB]` 静默（仅收尾阶段：注册 Lua 接口 / 初始化完成回调） | 10 s |
| `[ZB]` 静默（其它阶段） | 120 s |
| 完全没有 `[ZB]` 输出 | 注入后 180 s |
| 会话上限 | 540 s |
| DLL 内部安全上限 | 600 s |

另外：窗口等待上限 300 s、控制台被本次启动重写等待 180 s、首帧等待 180 s；
完成判定后先写 100% 并保留 2 s 再发停止事件，避免最后一帧来不及渲染。

**实测数据**（[docs/overlay-injection.md](docs/overlay-injection.md)）：

- B42 使用 LWJGL3 + GLFW，GLFW 的 WGL 后端确实进入 `opengl32!wglSwapBuffers`（120 帧命中 120 次）；
- 最小 GL 宿主注入前后窗口平均 RGB：59,79,128 → 229,52,56 → 停止后 57,79,128，无残留；
- 二次注入不会重复 Hook，也不会崩溃。

### 5.6 内存清理

- `GlobalMemoryStatusEx` 读总/可用物理内存；`usedPercent` 四舍五入；
- 清理 = 对每个可打开进程调用 `SetProcessWorkingSetSizeEx(h, -1, -1)`（等价 `EmptyWorkingSet`），
  **不结束进程**；
- 跳过 `java.exe / javaw.exe / ProjectZomboid64.exe …` 与自身 PID；
- 释放量按「清理前后可用物理内存差」计算（PCL2 口径），只有变多才算；
- 静默线程每 10 s 检查，达到阈值且距上次 ≥60 s 才清；设置保存后原子变量立即生效，不用重启；
- `get_memory_status` 5 s 轮询仅用于弹窗展示；清理完成推 `memory-cleaned`。

### 5.7 多线程下载器

`download_file` 的流程：

1. 校验 URL 只允许 http/https、文件名不能含路径分隔符、线程数 1–16；
2. 目录不存在则创建；同名文件自动改名 `name-2.zip`、`name-3.zip` …；
3. 先发 `Range: bytes=0-0` 探测；服务器返回 206 才走分块，否则退化为单流下载；
4. 从 `Content-Range` 解析总长度，按线程数均分，每块写 `part-NN`，边收边累加进度；
5. 每块完成后校验字节数是否等于请求区间；全部完成按顺序合并，删除临时目录；
6. 进度事件 `shader-download-progress` 携带 `downloadId/fileName/downloadedBytes/totalBytes`。

### 5.8 资源检索与镜像

- Modrinth 搜索：`api.modrinth.com/v2/search`；版本/文件：`/project/{id}/version`；
- CurseForge：类 ID `6552`（光影），Minecraft gameId `432`；API Key 可选；
- 镜像优先：`mod.mcimirror.top` 替换 API/CDN 主机；失败按顺序回退官方域名（与 PCL2 一致）；
- 光影包列表、版本分组、下载 URL 与备用 URL 一并返回，前端下载失败可换源重试。

### 5.9 新闻清洗与翻译

- **清洗**：只保留白名单标签，`href/src` 走 `safe_url`（仅 http/https），
  其他属性过滤；BBCode（`[b][i][url][img]...`）转 HTML；
- **切段**：按句子/标点切分，合并过短片段，纯数字/符号不送翻译；每段包
  `<span data-sid="...">`，译文按 sid 原位替换；
- **批处理**：前端 15 条/批，后端再按 15 条或 2000 字符切批；超时 8 s、单次翻译 5 s；
- **引擎**：Google `translate_a/single` 优先；失败用 Yandex `detect + translate`；
  结果记录引擎到 `engineTally`，前端展示占比；
- **缓存**：`(语言, 文本) → (译文, 引擎)`，进程内 HashMap，避免重复请求。

### 5.10 背景图流水线

| 来源 | 处理 |
| --- | --- |
| 内置 | 图片在 `public/backgrounds/`，设置里存哨兵 `builtin:<id>`，磁盘无对应文件；前端直接用打包 URL |
| 自定义 | 存 `.pzmanager\backgrounds\custom`，支持 png/jpg/jpeg/webp/bmp |
| 创意工坊 | 扫描模组内 `backgrounds` 目录的图片；`title.png/title2/title3/title4` 四张齐备时拼成 2×2 PNG，缓存到 `.pzmanager\backgrounds\workshop`，文件名带 workshopId + 模组名 + 目录哈希前 8 位 |

工坊背景扫描是后台任务：返回 `scanId`，逐条 emit `workshop-background-scan-update`，
前端实时插入结果并显示进度。`set_background_path` 对 `builtin:` 前缀跳过目录校验，
其余路径必须在托管目录内。

### 5.11 前端性能与竞态

| 手段 | 位置 | 目的 |
| --- | --- | --- |
| `IntersectionObserver` 休眠行 | `SleepingModRow` | 几百个模组只渲染视口附近，保留占位高度 |
| 请求序号 / 扫描 ID | 工坊、新闻、JAR、背景扫描 | 丢弃过期回包 |
| 400 ms 防抖 | 工坊搜索 | 减少抓取与解析 |
| 250 ms 防抖 | 内存滑块 | 避免拖动时刷盘 |
| 200 ms 事件节流 | 覆盖层进度 | 不刷爆前端 |
| `startTransition` | 批量选择 | 大 Set 更新不阻塞输入 |
| 双 rAF 卸载 splash | `main.tsx` | 首帧真正绘制后再淡出 |

---

## 6. 数据与磁盘布局

| 路径 | 写入者 | 读取者 | 说明 |
| --- | --- | --- | --- |
| `%USERPROFILE%\Zomboid\console.txt` | 游戏 / 启动器 | 启动器 tail | `[ZB]` 进度与完成标记的唯一来源；启动器只追加 `[PZManager]` 记录 |
| `...\mods\default.txt` | 游戏 / 启动器 | 启动器 | Lua 启用列表与顺序；只替换 `mods{}` 块 |
| `...\viewpoint-shaderpacks\` | 启动器 | 启动器 | 光影包托管目录（下载与导入都落这里） |
| `...\.pzmanager\settings.json` | 启动器 | 启动器 | 内存、背景、开发模式、账户隐藏、翻译语言、对比度、加载器方案、极速启动、内存清理、下载器设置 |
| `...\.pzmanager\jar-mod-state.json` | 启动器 | 启动器 | JAR 审批决策（`decision` + `disabled`），工坊重下后可重新套用 |
| `...\.pzmanager\mod-presets.json` | 启动器 | 启动器 | 模组预设（formatVersion 1） |
| `...\.pzmanager\startup-timing.json` | 启动器 | 启动器 | 最近一次与上一次启动时长 |
| `...\.pzmanager\default-backups\<时间戳>` | 启动器 | 人 | 应用预设前备份 `default.txt` |
| `...\.pzmanager\backgrounds\custom` / `workshop` | 启动器 | 启动器 | 导入图 / 拼接后的工坊背景 |
| `<游戏目录>\.pzmanager-backups\<时间戳>` | 启动器 | 人 | 改写 `ProjectZomboid64.json`/`.bat` 前备份 |
| `%TEMP%\pz_overlay_status.txt` | 启动器 | DLL | 阶段 / 进度 / 内存上限 / 控制台尾部 |
| `%TEMP%\overlay.log` | DLL | 人 | Hook 安装、首帧、停止信号 |
| `%TEMP%\fast-launch.log` | DLL | 人 | JVMTI 门禁、缓存命中、类改写 |
| Steam `localconfig.vdf` | 启动器 | Steam | 启动参数与内存；写前备份 |
| `ProjectZomboid64.json` / `.bat` | 启动器 | JVM | agent 策略、`-agentpath`、`-Xmx` |

**跨进程事件**：

- `Local\PZManagerOverlayStop_<pid>`：启动器 → DLL，停止覆盖；
- `Local\PZManagerOverlayStarted_<pid>`：DLL → 启动器，首帧已画。

---

## 7. 外部服务依赖

| 服务 | 用途 | 位置 |
| --- | --- | --- |
| Steam 注册表 / `libraryfolders.vdf` / `appmanifest_108600.acf` / `loginusers.vdf` / `localconfig.vdf` | 发现与启动配置 | `lib.rs` |
| `steamcommunity.com/workshop/browse/` | 工坊浏览（HTML/SSR 解析） | `workshop.rs` |
| `api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1` | 工坊详情正文与标签 | `workshop.rs` |
| `api.steampowered.com/ISteamNews/GetNewsForApp/v2` + `store.steampowered.com` | 开发者新闻 | `news.rs` |
| `translate.googleapis.com`、`translate.yandex.net` | 翻译 | `news_translation.rs` |
| `api.modrinth.com/v2`、`api.curseforge.com/v1`、`mod.mcimirror.top`、`cdn.modrinth.com`、`edge.forgecdn.net` | 光影检索与下载 | `resource.rs` |
| `piston-meta.mojang.com/mc/game/version_manifest_v2.json` | Minecraft 版本下拉 | `resource.rs` |

所有网络请求都在 Rust 侧；前端不直接发跨域请求。

---

## 8. 安全设计与红线

### 8.1 文件与命令

- **目录穿越防护**：截图、光影包、背景的读取/删除都先解析到托管目录，再校验父路径；
  `set_background_path` 只对 `builtin:` 哨兵跳过目录校验；
- **文件名防护**：下载文件名拒绝 `/`、`\`、`..`；同名自动编号，不覆盖；
- **URL 防护**：下载只允许 http/https；新闻 HTML 的链接只保留安全协议；
- **外部程序**：只调 `cmd /C start steam://...` 与模组自带 PowerShell 安装器（固定参数、无用户输入拼接）；
- **权限白名单**：Tauri capabilities 只允许窗口控制、打开对话框、写剪贴板，以及两条精确匹配的
  Steam 社区 URL（`steam://url/CommunityFilePage/*`、`steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=*`）。

### 8.2 进程注入

- 注入失败、DLL 初始化失败、Hook 失败都只写日志并退出，游戏继续运行；
- DLL `release` 使用 `panic = "unwind"`，绝不 `abort` 游戏进程；
- 覆盖层有 10 分钟硬上限，启动器异常时不会永久红屏；
- 重复注入检测：目标进程已有同名模块时跳过，避免多份 Hook。

### 8.3 需要知情的取舍

| 取舍 | 影响 | 逃生方式 |
| --- | --- | --- |
| `policy=allow-all` | 所有 Java 模组（含未签名/来源不明）直接加载，跳过 ZBS 签名校验；这是 ZombieBuddy 官方标注「不推荐」的模式 | 改回 `deny-new`（需改代码/配置），或不用加载器方案 |
| 极速启动强制模式 | 缓存未命中返回空列表，不回退原生扫描；模组更新后不刷新会缺补丁 | 关开关；`PZM_FAST_LAUNCH_ALLOW_FALLBACK=1` |
| 极速启动的 `new load` 崩溃 | 已观测 `LinkageError: duplicate class definition` | 关开关；`PZM_FAST_LAUNCH_DISABLE=1`；实验性 preload |
| PZ Optimization 类覆盖 | 替换游戏根目录类文件，其他模组/多人兼容性未测试 | 卸载（按安装清单还原） |
| 内存清理 | 压掉其他进程工作集，可能让它们短暂变慢 | 关开关 |

---

## 9. 测试

| 层 | 规模 | 命令 | 覆盖 |
| --- | ---: | --- | --- |
| Rust（`src-tauri`） | 117 条 `#[test]` | `cd src-tauri; cargo test` | VDF/转义路径、mod.info 大小写、JSON/BAT/Steam 改写、agent 归并、事务回滚、工坊 SSR、class 注解、JAR 扫描、default.txt 块替换与依赖排序、预设、内存阈值 |
| Rust（`overlay-dll`） | 14 条 | `cd overlay-dll; cargo test` | class 解析、包装式改写逐字节断言、异常表/StackMapTable、辅助类解析、缓存门禁 |
| 前端 | 19 个文件 / 36 条 | `npm test` | 源码/配置契约：ARIA 角色、选择器、端口、窗口尺寸、能力白名单、预设逻辑等 |

**前端测试的性质**：全部是「把 `App.tsx` / `App.css` / 配置文件读成字符串再做正则断言」，
零依赖、能锁死样式与配置契约，但重构 JSX/CSS 就容易碎，且验证不了运行时行为。写新断言时优先锁语义
（ARIA、必需属性、配置项），少锁具体色值。

**注意**：`cargo test` 不会重新链接 cdylib，验证 DLL 改动前必须 `cargo build`，否则会加载旧 DLL
得出假结论。

---

## 10. 构建与运行

| 目的 | 命令 |
| --- | --- |
| 安装依赖 | `npm install` |
| 前端开发 | `npm run dev`（固定端口 10002，`strictPort`） |
| 完整开发 | `npm run tauri:dev`（`tauri.dev.conf.json`，WebView2 调试端口 10001） |
| 打包 | `npm run tauri:build` |
| 仅检查后端 | `cd src-tauri; cargo check` |
| 构建覆盖层 DLL | `cd overlay-dll; cargo build --release --target x86_64-pc-windows-msvc` |
| 切 nightly 注入后端 | `cargo +nightly build --features dll-syringe-inject` |

**环境要求**：Windows x64、Rust stable（默认后端）、Node ≥ 22（前端测试用类型剥离）、
MSVC 工具链（覆盖层 DLL 的 MinHook C 源码需要 `cl.exe`）。

**发布缺口**：`tauri.conf.json` 的 `bundle` 没有配置 `resources`，覆盖层 DLL 不会被打进安装包；
前端默认 DLL 路径仍是开发期相对路径（`../overlay-dll/target/...`）。发布「开箱即用」版本需要
把 DLL 作为资源打包，并持久化/解析它的安装路径。

---

## 11. 已知边界与技术债

1. **巨石文件**：`App.tsx` 7,178 行、单组件约 138 个 state；`lib.rs` 8,130 行。抽取方向已存在
   （`src/*.ts` 纯函数、`src-tauri/src/*.rs` 子模块），继续沿边界拆即可。
2. **源码文本断言**：重构成本被放大；见 §9。
3. **极速启动未收敛**：游戏内加速未被证明，且有 duplicate class definition 崩溃；
   默认开关关闭，文档必须保持「实验性」措辞。
4. **覆盖层 DLL 未打包**：发布包不含 DLL；开发期路径依赖工作目录与 exe 祖先目录探测。
5. **开发期 WebView2 数据目录硬编码**：`ensure_webview2_data_folder` 在 debug 下固定
   `F:\webview2-data\pzmanager`，换机器需要改代码。
6. **命令注册单点**：61 条命令集中在一个 `generate_handler!`，新增命令容易漏改前端或漏测。
7. **平台限定**：非 Windows 目标直接编译失败（有意为之），因此也没有跨平台 CI 的可能。
8. **文档漂移**：[docs/superpowers/](docs/superpowers/) 是早期「路径探测」阶段的计划，已过时；
   [docs/fast-launch-status.md](docs/fast-launch-status.md) 是过程记录，结论以本文 §5.4 为准。

---

## 12. 文档索引

| 想知道 | 看 |
| --- | --- |
| 全功能与技术全景（本文） | `PROJECT_OVERVIEW.md` |
| 分层、模块、命令、磁盘契约 | [docs/architecture.md](docs/architecture.md) |
| 覆盖层注入 / Hook / 绘制 / 排错 | [docs/overlay-injection.md](docs/overlay-injection.md) |
| 极速启动的实现状态与实测记录 | [docs/fast-launch-status.md](docs/fast-launch-status.md) |
| 视觉、布局、动效、无障碍规范 | [docs/ui-style-guide.md](docs/ui-style-guide.md) |
| 快速开始与常用命令 | [README.md](README.md) |
| 历史计划（过时） | [docs/superpowers/](docs/superpowers/) |

---

## 附录 A：常量与 ID

| 常量 | 值 | 含义 |
| --- | --- | --- |
| `PROJECT_ZOMBOID_APP_ID` | `108600` | Steam AppID |
| `ZOMBIE_BUDDY_MOD_ID` / `ZOMBIE_BUDDY_WORKSHOP_ID` | `3619862853` | ZombieBuddy 加载器 |
| `PZ_OPTIMIZATION_MOD_ID` | `3805285544` | PZ Optimization |
| `VIEWPOINT_MOD_ID` | `3809306528` | Viewpoint |
| `TEMPORARY_VIEWPOINT_FIX_MOD_ID` | `3807686870` | Temporary 42.21 Fix |
| `ZOMBIE_BUDDY_AGENT` | `-agentlib:zbNative` | agent 前缀 |
| `ZOMBIE_BUDDY_POLICY_AGENT` | `-agentlib:zbNative=policy=allow-all` | 强制策略 |
| `MOD_LOADER_SCHEMES` | `["zombieBuddy", "none"]` | 加载器方案 |
| `TRANSLATION_LANGUAGES` | 6 种 | 设置页可选翻译语言 |
| `FAST_LAUNCH_CACHE_FORMAT` | `1` | 预扫描缓存版本 |
| `FAST_LAUNCH_CACHE_FILE` | `zb-prescan-cache.json` | 缓存文件 |
| 开发端口 / 调试端口 | `10002` / `10001` | Vite / WebView2 |
| 内存范围 | `1024` 起、步进 `512` | 启动页滑块 |

---

## 附录 B：Tauri 命令（61）

**环境 / 设置 / 启动（17）**

`detect_project_zomboid`、`get_system_memory_mb`、`get_app_settings`、`save_memory_limit`、
`save_developer_mode`、`save_steam_account_hidden`、`save_translation_language`、`save_font_contrast`、
`save_mod_loader_scheme`、`save_fast_launch`、`save_silent_memory_cleanup`、`save_clean_memory_on_launch`、
`get_memory_status`、`clean_memory_now`、`fast_launch_cache_status`、`startup_timing`、`launch_project_zomboid`

**背景（6）**

`set_background_path`、`list_custom_backgrounds`、`scan_workshop_backgrounds`、`start_workshop_background_scan`、
`import_custom_background`、`read_background`

**模组（8）**

`start_zombie_buddy_jar_scan`、`list_lua_mods`、`set_lua_mod_enabled`、`set_lua_mods_enabled`、
`save_lua_mod_order`、`set_zombie_buddy_mod_decision`、`set_zombie_buddy_mods_decisions`、`apply_jar_mod_file_state`

**模组预设（4）**

`list_mod_presets`、`save_mod_preset`、`apply_mod_preset`、`delete_mod_preset`

**媒体读取（3）**

`read_jar_mod_poster`、`read_lua_mod_poster`、`read_viewpoint_shot`

**一键安装 / 配置（4）**

`install_zombie_buddy`、`install_pz_optimization`、`uninstall_pz_optimization`、`configure_viewpoint_three_d`

**覆盖层（4）**

`overlay::inject_overlay`、`overlay_session::start_overlay_session`、`overlay_session::check_game_process`、
`overlay_session::stop_game_process`

**截图 / 光影（4）**

`list_viewpoint_shots`、`list_viewpoint_shaderpacks`、`import_viewpoint_shaderpack`、`delete_viewpoint_shaderpack`

**资源中心（9）**

`workshop_options`、`browse_workshop`、`fetch_workshop_item_detail`、`resource_options`、
`fetch_shader_resources`、`fetch_shader_resource_files`、`save_shader_download_settings`、
`download_shader_file`、`fetch_minecraft_versions`

**新闻（2）**

`fetch_developer_news`、`translate_developer_news_batch`

---

## 附录 C：事件、状态文件与环境开关

**事件**

| 事件 | 载荷要点 |
| --- | --- |
| `overlay-session` | `phase/message/pid/processName/zbLines/elapsedMs` |
| `overlay-zb-progress` | `zbLines/phase/lastLine/elapsedMs` |
| `startup-timing` | `lastMs/previousMs` |
| `jar-mod-scan-update` | `scanId/status/scanned/total/item/error` |
| `workshop-background-scan-update` | 同上结构 |
| `shader-download-progress` | `downloadId/fileName/downloadedBytes/totalBytes` |
| `memory-cleaned` | 清理报告（释放量、进程数、耗时） |

**状态文件格式**（`%TEMP%\pz_overlay_status.txt`）

```text
progress=0.873
phase=注入游戏类 · 注入 612/701
memmax=8192
line=[ZB] patching zombie.vehicles.UI3DScene with 2 advice(s)
line=...
```

**环境开关**（仅极速启动相关，均不影响默认启动路径）

| 变量 | 作用 |
| --- | --- |
| `PZM_FAST_LAUNCH_DISABLE=1` | agent 立即什么都不做 |
| `PZM_FAST_LAUNCH_ALLOW_FALLBACK=1` | 缓存未命中时回退 ZB 原生扫描（默认强制模式不回退） |
| `PZM_FAST_LAUNCH_PRELOAD_TARGETS=1` | 实验：提前加载 `@Patch` 目标类，尝试规避 duplicate class 崩溃 |

---

## 附录 D：本文怎么维护

- 本文是**产品 + 工程全景**，不替代 [docs/architecture.md](docs/architecture.md) 的契约索引；
- 改动以下任一项时请同步本文：新增页面或设置项（§3）、命令/事件（附录 B/C）、跨进程文件
  （§4.7、§6）、外部服务（§7）、测试策略（§9）、构建方式（§10）、成熟度（§2.2、§5.4）；
- 数字（行数、测试数）每次都重新数，不沿用旧值；
- 实验性功能必须同时写「当前状态」与「逃生方式」，不能只写目标。
