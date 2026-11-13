# pzmanager 架构文档

> **快照时间：2026-10-11 01:35**。写下这份文档时，工作区里仍有 Agent 在改
> `src-tauri/src/lib.rs`、`src-tauri/src/overlay_session.rs`、`src/App.tsx`、`src/App.css`
> 与 `src/gameProcess.ts`。因此：
>
> * **结构与契约**（分层、模块职责、磁盘布局、跨进程协议）是稳定的，按本文理解不会错；
> * **行数、命令清单、页面细节**是会漂移的，凡带 ⚠️ 的表格都请回代码确认；
> * 若本文与代码冲突，**以代码为准**，并顺手改本文（见附录 B）。

## 0. 文档地图

| 想看什么 | 看哪里 |
| --- | --- |
| 全局地图：分层、模块、命令、磁盘、测试、构建 | **本文** |
| 覆盖层注入（注入/Hook/GDI+GL 绘制/停止条件/排错） | [overlay-injection.md](overlay-injection.md) —— 覆盖层领域的唯一权威，含实测数据 |
| 界面风格、布局、组件、动效与无障碍规范 | [ui-style-guide.md](ui-style-guide.md) —— 前端视觉与交互的统一约束 |
| 最初"路径探测"阶段的计划与设计 | [superpowers/plans/](superpowers/plans)、[superpowers/specs/](superpowers/specs) —— **已过时**，只作考古 |
| 快速开始与常用命令 | [../README.md](../README.md) |

## 1. 这是什么

`pzmanager` 是一个 **Windows 桌面端的 Project Zomboid（僵尸毁灭工程）启动器 + 模组管理器**。它不修改游戏本身，而是通过三类手段替玩家完成周边工作：

1. **环境与启动链管理**：从注册表/VDF 里找出 Steam、游戏目录、创意工坊目录与当前账户；改写 Steam 启动项、`ProjectZomboid64.json`、`ProjectZomboid64.bat` 里的内存上限与 JVM agent 参数，然后 `steam://rungameid/108600` 拉起游戏。
2. **模组管理**：Lua 模组（`mods/default.txt` 的启停与加载顺序，含依赖前置排序）与 JAR 模组（ZombieBuddy 的注入式模组：审批决策、`Mod.info`/`Mod.info.disabled` 文件态、注解冲突分析、后台扫描）。
3. **覆盖层与加载进度**：等游戏窗口出现 → 注入自研 DLL → Hook `opengl32!wglSwapBuffers`，在游戏窗口**内部**绘制红底 + 控制台尾部 + 进度条，实时解析游戏控制台 `[ZB]` 输出估算 ZombieBuddy 的注入进度，加载完成后自动撤除。

此外还附带：Viewpoint 光影模组的截图/光影包管理、创意工坊浏览器、Modrinth/CurseForge 光影下载器、Steam 开发者新闻 + 批量翻译、自定义启动背景图。

## 2. 技术栈与硬约束

| 层 | 选型 |
| --- | --- |
| 壳 | Tauri 2（`src-tauri/`），窗口无边框自绘标题栏，插件：`opener`、`dialog`、`clipboard-manager` |
| 前端 | React 19 + TypeScript ~6 + Chakra UI 3 + Emotion + FontAwesome 7，Vite 8 构建 |
| 后端 | Rust 2021，`reqwest` 0.13 + `futures-util`（下载）、`serde`/`serde_json`、`regex`、`sha2`、`zip` 8、`image` 0.25、`java-class-rs` 0.1（解析 class 注解）、`winreg` 0.55、`windows` 0.62.2 |
| 覆盖层 DLL | 独立 crate `pz_overlay`（cdylib），`minhook` 0.9.0 + `windows` 0.62.2（GDI/OpenGL），**需要 MSVC `cl.exe`**（MinHook 的 C 源码由 `cc` 编译） |

硬约束（写代码时别踩）：

* **仅 Windows / 仅 x64**。游戏、启动器、DLL 必须同为 64 位；`overlay.rs` 里对非 Windows 目标直接 `compile_error!`。
* **注入默认后端是 stable 可编译的原生 Win32**（`CreateRemoteThread` + `LoadLibraryW`）。`dll-syringe` 所有已发布版本都带 `#![feature(...)]`，只有在 `--features dll-syringe-inject` + nightly 工具链下才参与编译。
* **覆盖层 DLL 的 release profile 故意用 `panic = "unwind"`**：注入进游戏进程后，初始化线程 panic 只死一个线程，绝不能 `abort` 掉游戏。

## 3. 目录地图

```
pzm/
├─ src/                        前端（React/TS）
│  ├─ App.tsx                  ⚠️ ~5.3k 行，单文件承载全部页面
│  ├─ App.css                  ⚠️ ~2.7k 行，手写样式，类名统一 pz- 前缀
│  ├─ main.tsx                 React 挂载 + Chakra Provider
│  └─ <功能>.ts                抽取出来的纯函数模块（见 5.3）
├─ src-tauri/                  启动器（Rust）
│  ├─ src/lib.rs               ⚠️ ~6.6k 行，主后端 + 内联 Rust 测试
│  ├─ src/main.rs              仅调用 pzmanager_lib::run()
│  ├─ src/{workshop,resource,downloader,news,news_translation}.rs
│  ├─ src/overlay*.rs          注入 + 会话 + 两套注入后端
│  ├─ capabilities/default.json  Tauri 权限白名单
│  ├─ tauri.conf.json          正式构建；tauri.dev.conf.json 开发覆盖
│  └─ tests/fixtures/          Rust 测试夹具（news_split_cases.json）
├─ overlay-dll/                覆盖层 DLL（独立 crate，被注入进游戏进程）
│  ├─ src/lib.rs               DllMain / 等模块与导出 / MinHook / GL 绘制 / 停止事件
│  ├─ src/ui.rs                窗口内加载界面：GDI DIB → (换帧后) GL 纹理
│  └─ target-verify/           一次性验证构建产物（非正式产物路径）
├─ tests/                      前端侧 node:test 断言（17 个文件，见 10.2）
├─ docs/                       本文 + 覆盖层文档 + superpowers 历史文档
├─ backup/<时间戳>-<slug>/      人工快照，当前唯一的"版本历史"（见 12.7）
├─ pzmodsample/                参考源码：ZombieBuddy / Viewpoint / PCL2-Language / SJMCL
├─ python/                     早期 Flask 工坊浏览器原型（已被 Rust 版取代）
├─ public/                     字体、图标、默认背景
└─ dist/                       Vite 构建产物（未提交）
```

## 4. 运行时拓扑

```
┌─────────────────────────────┐        invoke/emit (Tauri IPC)
│ pzmanager.exe（WebView + Rust）│◄──────────────────────────────► 前端 React
│  · Steam/VDF 解析、配置改写    │
│  · 工坊/资源/新闻 HTTP 客户端  │
│  · overlay_session 会话线程    │
└───────┬─────────────┬───────┘
        │ 1. 写配置    │ 2. CreateRemoteThread+LoadLibraryW
        ▼             ▼
  Steam 配置 /      ┌──────────────────────────────┐
  游戏目录 JSON/BAT │ ProjectZomboid64.exe (JVM)    │
        │           │  └─ pz_overlay.dll (注入)     │
        │           │      · Hook wglSwapBuffers    │
        │           │      · 读 %TEMP% 状态文件      │
        │           │      · 心跳失效自读 console.txt │
        │           │      · 等 Local\…Stop_<pid>   │
        │           └──────────────────────────────┘
        │ 3. tail %USERPROFILE%\Zomboid\console.txt（解析 [ZB] 进度）
        └──────────────────────────────────────────────► overlay_session 线程
```

三条**跨进程契约**（改任何一边都要同步另一边，详见第 8 节）：

1. 启动器 → DLL：状态文件 `%TEMP%\pz_overlay_status.txt`（阶段文字 / 进度 / 控制台尾部 / 覆盖层背景色 `bgcolor=#RRGGBB` / 心跳 `beat=` / `console=` 路径 / `session=` 起点）；
2. 启动器 → DLL：命名事件 `Local\PZManagerOverlayStop_<游戏PID>`（`SetEvent` 即停止覆盖）；
3. DLL → 启动器：命名事件 `Local\PZManagerOverlayStarted_<PID>`（首帧已画，会话在此之前不做"加载完成"判定）+ `%TEMP%\overlay.log`（人读日志）。

> 状态文件带 1 秒心跳。DLL 3 秒收不到心跳（或状态文件缺失）时，会切换为自己的
> `console.txt` 读取器，用同一套阶段/完成规则继续更新界面并自行摘 Hook；启动器关闭
> 不再让覆盖层停在最后一帧。启动器恢复心跳后 DLL 切回启动器数据。

## 5. 前端（`src/`）

### 5.1 页面结构

一级页面枚举 `ActivePage = launch | overview | shots | shaderpacks | workshop | mods | settings`，其中两个是"容器页"：

| 一级页 | 子页 | 内容 |
| --- | --- | --- |
| 启动 `launch` | — | 账户/内存上限/启动参数、启动与停止游戏、覆盖层会话进度面板 |
| 支持库 `overview` | — | 探测到的本机路径、ZombieBuddy Viewpoint/PZ_Optimization 状态与一键配置；三项安装内容收在同一个「支持组件」面板里，每行沿用模组行的卡片外观（`.pz-mod-row`），默认折叠（折叠态：名称 + 一句话说明 + 状态标签 + ⋯ 详情按钮）。加载器那行只报状态并指路「设置 › 模组加载器」，不在本页做安装 |
| 截图管理 `shots` | — | Viewpoint 截图列表、预览、复制到剪贴板 |
| 光影管理 `shaderpacks` | — | 光影包导入（zip）/删除、图标与搜索词推导；每包「配置」弹窗解析 shaders.properties 并保存参数覆写（启动时写入 viewpoint-live.properties，游戏退出后整文件还原） |
| 资源中心 `workshop` | `workshop \| shaderDownload \| developerNews` | 工坊筛选浏览 / Modrinth·CurseForge 光影下载 / Steam 新闻 + 翻译 |
| 模组管理 `mods` | `lua \| jar` | Lua 加载顺序与启停（启用行标题旁显示当前位次 `#N`，即 `default.txt` 的 mods 块顺序）、JAR 审批决策与冲突分析 |
| 设置 `settings` | `about \| general \| gameOptions \| toolbox \| logs \| modLoader \| startup \| experimental \| developer` | 关于、通用、游戏设置（`options.ini` 行级编辑）、百宝箱、日志、模组加载器（加载器方案 + 该方案加载器的状态与安装）、启动、实验性、开发者选项 |

* 翻页条统一用 `src/components/pagination.tsx` 的 `Pagination`：上一页 / 数字页码（首末页 + 当前页及前后各一页，规则在 `src/paginationUi.ts`）/ 下一页，右对齐放在列表右下角（创意工坊列表与光影包下载列表共用同一套外观）。

### 5.2 `App.tsx` 的组织方式

* 全部状态集中在单个 `App()` 组件里的 ~110 个 `useState`/`useRef`（游戏进程、覆盖层、内存、模组列表与多选集合、工坊筛选与请求序号、新闻翻译进度、背景图、扫描进度……）。
* 大块 UI 先在各分支里构造为变量（`launchContent`、`overviewContent`、`shotsContent`、`shaderpacksContent`、`resourceCenterContent`、`modsContent`、`settingsContent`），最后按 `activePage` 条件渲染。
* 组件内定义了一批局部子组件：`ToastViewport`、`MediaPanel`、`ShotThumbnail`、`WorkshopTagPicker`、`WorkshopDropdown`、`ResourceDownloadBrowser`、`BackgroundThumbnail`、`JarModPoster`、`LuaModPoster`、`ModBatchBar`、`ZombieBuddyLoaderPanel`、`BackgroundMenu`、`WindowTitlebar`。
* 竞态处理习惯：长请求用 `useRef` 存**请求序号**（`workshopRequestId`、`developerNewsRequestId`）或扫描 id（`jarScanIdRef`、`workshopBackgroundScanIdRef`），回包时比对后丢弃过期结果；后端扫描通过事件流式推进度。
* 过渡/动画：`pageTransitionPhase` 控制页面进出，`launchCooldown`/`gameWindowReady` 驱动启动按钮左滑与停止按钮划入。

### 5.3 抽出来的纯函数模块

这些模块不依赖 React/DOM，因此可以被 `tests/` 直接断言：

| 文件 | 职责 |
| --- | --- |
| [modManagement.ts](../src/modManagement.ts) | Lua 模组排序/过滤（启用优先、`loadOrder`、`hasB42`）、多选集合运算（增删/批量/反选）、保序遍历合并 |
| [jarModUi.ts](../src/jarModUi.ts) | JAR 冲突分析：按 `(注解, 方法名)` 求交集，`@Overwrite`=high、`@Redirect`/`@ModifyConstant`=mediumHigh、`@Inject`/`@ModifyVariable`=mediumLow，并稳定排序 |
| [developerMode.ts](../src/developerMode.ts) | 通知策略：非开发模式只显示 `error`，且消息裁到首行 140 字符 |
| [gameProcess.ts](../src/gameProcess.ts) | `shouldResetGameWindow()`：窗口就绪标记与后端进程状态的对账 ⚠️（新增，仍在演进） |
| [launchArgs.ts](../src/launchArgs.ts) | 默认启动参数 `["-agentlib:zbNative", "--"]` 与拼接 |
| [externalLinks.ts](../src/externalLinks.ts) | 工坊链接：优先 `steam://openurl/…`，回退 https |
| [overlayInjection.ts](../src/overlayInjection.ts) | `inject_overlay` 的封装 + 开发期默认 DLL 路径 `../overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll` |

### 5.4 前端调用后端的方式

统一 `invoke<T>("命令名", { camelCase 参数 })`，事件用 `listen`。前端一共用到 ~45 个命令（与注册清单一致）。约定：

* 后端 `Err(String)` 会被 Tauri 原样抛出，前端统一转成 `Error` 后交给 Toast；Toast 全部自动退场（普通 3 秒、错误 8 秒，底部进度条即倒计时），历史进通知中心可回看；`success` / `info` 只在开发者模式显示，`error` 与 `notice`（如背景音乐自动播放提示）两种始终显示；
* 二进制媒体（海报/截图/背景）走 `invoke<ArrayBuffer>` 拿字节，再在组件里转 Blob URL；
* 后台扫描类命令返回 `scanId: number`，进度通过事件推送。

## 6. 后端（`src-tauri/src/`）

### 6.1 模块职责

| 模块 | 职责 | 备注 |
| --- | --- | --- |
| [lib.rs](../src-tauri/src/lib.rs) | Steam 发现、配置改写与备份事务、模组管理、背景/截图/光影、class 注解扫描、全部命令注册 | 单一巨石文件，~90 个内联测试 |
| [workshop.rs](../src-tauri/src/workshop.rs) | Steam 创意工坊爬虫（`APP_ID=108600`，无 API Key，HTML/JSON 解析，**300s 内存缓存**） | 与 `python/app.py` 原型同源 |
| [resource.rs](../src-tauri/src/resource.rs) | Modrinth + CurseForge 光影资源检索 | 注释明确对齐 PCL2 的 `DlSourceModGet` |
| [downloader.rs](../src-tauri/src/downloader.rs) | 多线程分块 HTTP 下载（`RANGE`/`CONTENT-RANGE`）+ 进度事件 | 供光影下载器使用 |
| [news.rs](../src-tauri/src/news.rs) | Steam 新闻 API + 正文清洗，切分成可翻译片段 | 夹具 `tests/fixtures/news_split_cases.json` |
| [news_translation.rs](../src-tauri/src/news_translation.rs) | 批量翻译（命令上限与前端均为 **20 句/批**，`TRANSLATION_BATCH_SIZE` = `BATCH_SIZE` = `JOIN_BATCH` = 20，三处必须同步；单条 ≤2000 字；超 2000 字的批次会先按 20 句 / 2000 字重切再拼），默认引擎链：Google `translate_a/single` → 必应（手机版 `cn.bing.com` 页取 IG/IID/token，再 POST `ttranslatev3`，单段 ≤1000 字自动切分）→ MyMemory（按文字系统猜源语言）；**每 20 句拼成一段发一次引擎请求**，拼接译文按 `\n` 拆回，**行数对不上（必应 LLM 后端偶尔合并换行）或引擎全败时整组作废，退回逐句并发（4 句一波）补翻**——曾因"15 句拼接失败后逐句串行重试"慢到像"没在翻译"；「通用 › 翻译引擎」可手动指定优先引擎（`translationEngine`：auto/bing/google/mymemory/relay，默认 google），指定引擎排链首、失败后按默认链回退；relay（`steam-relay` 的 `/translate` → 本机 Argos，只接 zh/en/ru）仅在手动指定时在批级先试。必应走**独立的 no-proxy 客户端**（系统代理/境外出口会被必应判成机房 IP 直接 401，参考实现也是直连），失败只静默 60 秒（切换引擎会立即解除）；Google / relay 失败熔断 10 分钟。译文缓存键 = (原文, 语言, 引擎偏好)，条目 **10 分钟过期**：切换引擎后会重新翻译，不会拿旧引擎结果冒充；故障期间回退引擎的结果也不会永久顶替已恢复的引擎（否则必应恢复后仍永远命中 MyMemory 的旧缓存）。`engine == "none"`（引擎全败）的结果不进缓存。任一引擎从可用转入熔断时向 launcher.log 写一条带原因的日志（如「必应」：引导失败 / HTTP 401 / 429 / 响应异常），熔断期内的重复失败不重复记录；网络链路另有手动冒烟测试 `cargo test bing_live -- --ignored --nocapture`（必应 bootstrap → ttranslatev3）与 `cargo test translate_batch_live -- --ignored --nocapture`（整条命令管线）
| [overlay.rs](../src-tauri/src/overlay.rs) | 注入入口：路径校验、进程轮询（优先 `ProjectZomboid64.exe`，其次 `javaw.exe`，都找不到时按窗口标题 `Project Zomboid` **精确匹配**兜底——无 Xbox 服务的机器上游戏进程显示为 wsappx，按进程名找不到）、重复注入检测 | 编译期二选一注入后端 |
| `overlay_inject_win32.rs` / `overlay_inject_dll_syringe.rs` | 两套等价后端，对外都是 `inject_dll(pid, dll_path)` | 后者需 nightly |
| [overlay_session.rs](../src-tauri/src/overlay_session.rs) | 会话编排：等窗口 → 注入 → 解析控制台 → 写状态文件 → 判定完成 → 发停止事件；并提供游戏进程查询/停止命令；顺带记录「检测到窗口 → 移除覆盖层」的启动时长（落盘 `startup-timing.json`，发 `startup-timing` 事件，`startup_timing` 命令读回） | ⚠️ 正在大改 |
| [memclean.rs](../src-tauri/src/memclean.rs) | 系统内存状态（`GlobalMemoryStatusEx`）与工作集清理（`SetProcessWorkingSetSizeEx(-1,-1)`，等价 `EmptyWorkingSet`）；激进模式追加系统级四步（`MemoryEmptyWorkingSets` / `SetSystemFileCacheSize(-1,-1,0)` / `MemoryFlushModifiedList` / `MemoryPurgeStandbyList`）；静默清理后台线程；`is_elevated` / `restart_as_admin` 管提权 | 形态对齐 PCL2 的「内存优化」；清理**跳过游戏进程与自身** |
| [task_store.rs](../src-tauri/src/task_store.rs) | 任务中心落盘：tasks.json 的 `load_saved_tasks` / `save_tasks`（不透明 JSON 文档，模型由前端持有）；非对象条目丢弃、裁剪到 200 条、临时文件 + 改名原子写；读失败按无历史处理并写 WARN | 前端清洗在 `src/taskHistory.ts` 的 `sanitizeSavedTasks` |
| `main.rs` | 仅 `pzmanager_lib::run()` | 5 行 |

### 6.2 Steam 发现链

`steam_roots()`（注册表 + 常见路径回退）→ `library_paths_from_config()` 解析 `libraryfolders.vdf` → `game_path_from_library()` / `workshop_path_from_library()`（读 `steamapps/appmanifest_108600.acf`）→ `steam_profile_from_root()` 读 `userdata/*/config/loginusers.vdf` 取最近账户（SteamID64 → account id 用于定位 `localconfig.vdf`）。缺失任何一环都按"正常的未找到"处理，不报错。

### 6.3 启动链与模组加载器方案（安全相关，改动前必读）

启动时挂哪个加载器由设置里的「模组加载器方案」（`modLoaderScheme`：`zombieBuddy` / `none`，默认 `zombieBuddy`）决定，`launch_project_zomboid` 读它再调 `apply_launch_loader(memory, mod_loader_policy_agent(&scheme), fast_launch_agent)`：

- **内存上限始终写**（三处都写；JSON 里的旧 `-Xmx` 会覆盖启动器设置）；
- **加载器 agent 只在方案启用加载器时归一化**：`policy_agent = None`（方案 `none`）时 `patch_json_launcher_with_agent` / `patch_batch_with_agent` 整段跳过 agent 逻辑，绝不往用户的启动配置里塞 agent；
- **极速启动的 `-agentpath:` 只在三件事同时成立时才写**：设置里 `fastLaunch = true`、方案是 `zombieBuddy`、且那份覆盖层 DLL **确实存在**（`resolve_fast_launch_agent_path()` 走 `overlay::imp::normalize_dll_path()` 做存在性校验）。任一条不成立都传 `None`，而 `None` 会把启动文件里已有的 `-agentpath:` **全部摘掉**——因为 `-agentpath` 指向的文件找不到会让 **JVM 直接启动失败**，那违反"任何失败都回退原生扫描"的红线。`.`bat` 的 `SET _JAVA_OPTIONS` 是按空白切分的，所以路径含空格时只写 JSON、不写 .bat（`fast_launch_agent_for_batch()`）。

ZombieBuddy 的 agent 策略是 **write-once**（`Loader.setPolicy`），JVM 只加载第一个 agent，因此方案是 `zombieBuddy` 时启动器在**三个位置**都把 agent 归一化成 `-agentlib:zbNative=policy=allow-all`：

1. Steam `LaunchOptions`（`patch_steam_launch_options_content_with_memory`，agent 归并到 `policy=allow-all`；**裸 `--` 一律剥掉**——B42 的 Zulu 25 JRE 直接报 "Unrecognized option: --" 拒绝创建 JVM，游戏秒退，2026-11-07 实测）；状态判定（`steam_config_has_both`）同步只验 agent 存在、且看 108600 块内**最后一条** LaunchOptions（VDF 同名键后者生效，真实文件出现过补丁与用户自设参数两条并存）；
2. 游戏目录 `ProjectZomboid64.json` 的 `vmArgs`；
3. 游戏目录 `ProjectZomboid64.bat` 的 `SET _JAVA_OPTIONS`。

**游戏退出诊断的脚本冲突规则**（2026-11-12 实测）：console 出现 `Tile duplicate: <贴图格>` 时，从同一行提取脚本名（`Tile duplicate:` 前最近的 `[..]`）与贴图格，再统计游戏本体 `media/scripts` 里声明该 `entity` 的文件数：**≥2 = 游戏本体自己重复打包**（提示验证游戏文件完整性 / 回滚版本），**1 = 模组覆盖了本体脚本**，**0 / 目录未知 = 模组间冲突**；没有 Tile duplicate 但有 `Script load errors` 时给通用脚本加载错误提示。诊断函数拆成纯文本版 `diagnose_console_text(text, install)`，测试可注入安装目录，不受本机游戏文件影响。
**启动熔断（15 秒）**：点下「启动游戏」**那一刻**开始计时（`launchClickedAtRef`，不等命令返回——后端若卡住，
invoke 永远不回来，等待状态和超时都不会被触发；2026-11-08 实测过这种"点了没反应也不报超时"）。前端每 0.3s
轮询 `check_game_process`，本轮始终没见过游戏进程时 15 秒（`LAUNCH_START_TIMEOUT_MS`）即熔断——停轮询、清
PID/计时、弹错误提示（Steam 启动用 `steam_process_running` 区分「Steam 没起来」和「Steam 在但游戏没拉起」两种文案；从游戏文件启动则指向游戏本体与 `ProjectZomboid64.exe`）、
解除按钮的"启动中"状态，可立刻重试；晚到的成功回包只补挂覆盖层、不再重复提示。覆盖层会话线程仍会继续等
（自身上限 60 秒），游戏若晚到，`overlay-session` 的 injecting/loading 事件仍会把窗口就绪与计时补上。

**从游戏文件启动（不经 Steam）**：`launch_project_zomboid` 的 `launchSource = "files"`（开关在
启动页 › 启动卡 › 启动设置弹窗里的「从游戏文件启动（离线）」，存 `settings.json` 的 `launchFromGameFiles`，
命令 `save_launch_from_game_files`；原来的工具栏硬盘图标按钮已并入该开关）。与 Steam 启动共用同一套前置配置（内存校验 / 加载器 agent / 极速启动 /
`apply_launch_loader` 写 JSON+BAT / 光影覆写 / 启动时清理内存），差异有三：**不读写 Steam localconfig.vdf**
（Steam 没装没登录也能玩，`steam_configs` 为空跳过补丁）；**不走 `ProjectZomboid64.exe`**——它是原生启动器，
Steam 相关初始化在无 Steam 环境时直接弹 fatal error（2026-11-10 实测：Steam 客户端未运行，进程秒退退出码 1；
bat 的 java 命令行不经过那一步，Steam 未运行也能进游戏，游戏侧自行降级无 Steam 模式）；最后用
`Command::new(javaw).current_dir(game_path)` 按 **bat 的方式**启动：`parse_bat_launch_command` 运行时解析
`ProjectZomboid64.bat` 的第一条 java 行（重试分支忽略），拿固定 JVM 参数 / `PZ_CLASSPATH` / 主类，
`build_file_java_args` 按顺序组装「bat 固定参数 → `-Xmx`（覆盖 bat 自带值）→ 加载器 agent → 极速启动 agent →
用户 JVM 参数（`-X*`/`-XX*`/`-D`/`-agent*`/`--*` 前缀启发式拆分）→ `-cp` → 主类 → 用户游戏参数」；bat 解析失败
回落 `default_bat_launch_command`（与官方脚本当前内容一致）。进程名变成 `javaw.exe`（缺 javaw 回落 java.exe），
覆盖层 / 进程检测 / 停止游戏都按这个名字工作（javaw 本来就在 `TARGET_PROCESS_NAMES` 与 memclean 保护名单里）。
代价：pzopt 的 AOT/GC 调优参数只写在 `ProjectZomboid64.json`（exe 专用），bat 路径不生效；Steam 成就 / 游戏时间
统计等平台功能不可用；模组与存档不受影响。

**42.22 签名 jar 与散装 class（"JNI ERROR"）**：42.22 正式版起官方 `projectzomboid.jar` 带签名
（`META-INF/PROJECTZ.EC`）。classpath 里的 `.`（官方 bat/JSON 都有）会加载游戏目录下遗留的散装
`.class`（旧版 pzopt / 手工替换，实测 445 个），与签名 jar 里的同名类**同一包混载**时 JVM 在加载主类
阶段抛 `SecurityException: signer information does not match`，用户看到的就是启动弹 "JNI ERROR"、
游戏秒退且不写 console。Java 没有关闭 JAR 签名校验的开关（`-Xverify:none` 只管字节码验证），所以
**处理方式是"不喂散装类"**：`apply_launch_loader_at_paths` 在「jar 已签名（读 zip 中央目录的
`META-INF/*.SF/.EC`）+ 游戏目录确有散装 `.class`」时，把 `ProjectZomboid64.json` 的 classpath 里
`.`/`./` 条目去掉（`strip_dot_classpath_entries`）；「从游戏文件启动」的 java 命令行同理由
`strip_dot_classpath_string` 去掉 bat 的 `./`。改动走既有的备份 + 原子写入事务，并在 launcher.log
提示"建议删除散装 class 恢复官方目录结构"。未签名 jar（42.21 及更早）完全不动 classpath。

**启动前模组兼容性预检**（`mod_compat.rs`，2026-11-11）：42.22.2 移除了
`zombie.util.list.PZArrayList`，旧 Viewpoint 插件（DGVViewpointEyes 等 12 个）的类常量/字段描述符仍引用它；
ZombieBuddy 在 `LuaManager.init` 反射暴露这些类时抛 `NoClassDefFoundError`，游戏在进主菜单前秒退。
这类问题**无法靠版本号判断**（模组的 `versionMin` 早已满足），但可以直接比对字节码引用与游戏类表：
`check_mod_compatibility` 读 `projectzomboid.jar` 中央目录得到类名集合，对每个**已启用** Java 模组
（`jar_scan_inputs` + `jar_scan_candidates`，禁用模组的 `mod.info.disabled` 不参与）提取
**类级**引用（`CONSTANT_Class` 条目 + 字段/方法描述符里的 `Lzombie/...;`，不取任意点分字符串，
避免把方法名/日志误报成类），差集即"引用了当前游戏已不存在的类"。结果按「jar 路径 + size + mtime」
与游戏 jar 指纹缓存在 `.pzmanager/mod-compat-cache.json`（实测冷扫描 47 个模组 ≈2.7s，热缓存 ≈1.4s）。
前端在启动按钮里先调预检：有 issue 弹三选一「禁用这些模组并启动 / 仍然启动 / 取消」，
"禁用"走现有 `mod.info` → `mod.info.disabled` 机制（`disable_mods_for_launch`，可随时恢复）；
预检失败只记日志、绝不阻断启动。

**覆盖层不得盖住 fatal error 弹窗**：游戏没起来而弹错误对话框时，那个弹窗是游戏进程的**第一个可见窗口**——
`has_visible_window` 排除类名以 `#` 开头的系统窗口类（`#32770` 标准对话框），不把弹窗当"游戏窗口出现"，
否则覆盖层会立刻注入并压在弹窗上，用户连错误内容都看不到（2026-11-10 用户实测）。排除后注入继续等真正的
GLFW 主窗口；弹窗被点掉、进程退出，会话按"游戏进程已退出"正常收场。

**「被拒绝加载的模组」收尾面板**（`overlay-core` 共享解析 + 启动器/DLL 双路）：游戏按 mod.info 的
`versionMin`/`versionMax` 与 `require` 链拒绝加载模组时（`checkMissingMods` 静默移除，不打日志），
覆盖层在加载完成时把差异列出来。数据源：console 的 `LOG : Mod … loading <id>` 行（每个实际加载的
模组一行，`overlay_core::parse_loading_mod_line`）− `default.txt` 启用列表
（`parse_enabled_mod_ids`，与 lua_mods 共用同一实现）→ 差集（`rejected_mod_ids`）。启动器在
`track_console` 的四个「成功完成」出口（init 回调 / ≥97% advice / 主菜单标记 / 里程碑静默）计算差集并
记 launcher.log；**差集为空立即收尾，正常启动不多等一秒**。**没有观察到任何 `loading` 行时不判定**——
收尾可能发生在 ≥97% advice 那一刻、模组加载行还没出现，否则会把整份 `default.txt` 误报成"全部被拒"
（2026-11-11 实测 118 个模组全被误报）。差集非空时经 `overlay-reject-warning` 事件
把清单交给前端弹确认框（列表 + 「进入游戏」），覆盖层本身**不**为此停留——完成回调之后游戏进入
Bink 过场/主菜单阶段，该阶段游戏不合成覆盖层绘制的内容，压上去只会黑屏（2026-11-11 实测：DLL 每帧
都在画、屏幕全黑），旧设计本就在完成点立即退场，确认交互由启动器窗口承担。

**离线映射的已知限制**：游戏 FS 层把**规范化后的绝对路径**映射回相对路径时只认注册过的根目录
（`getRelativeFile` 的 "Invalid prefix found for" 校验）。联接目标在工坊库里，离线模式下该根未注册，
所以穿过联接需要这种映射的内容会失败——实测是 3 个地砖模组（Take A Bath / EN_Newburbs / CommunityTilePack）
的深度图（TileDepthTextures 捕获后打日志，贴图没有景深但不致崩溃）。Steam 模式下工坊根由 Steamworks 注册，
无此问题。另外：模组"没被识别"与联接无关——游戏按 mod.info 的 `versionMin`/`versionMax` 与 `require` 链
（`ChooseGameInfo$Mod.isAvailable`）逐个拒绝版本不匹配的模组（实测：游戏 42.21.0 时 Viewpoint 全家
versionMin=42.22 → 连锁拒绝 38 个），Steam 模式同样会被拒；启动器的退出分类曾把这类正常退出误判成
javaCrash（被捕获的 "Exception thrown" 行触发），已改为只认未捕获签名。

**离线启动的创意工坊映射**（`offline_workshop.rs`）：游戏只扫描 `%USERPROFILE%\Zomboid\mods`，创意工坊模组
平时靠 Steamworks API 提供——离线启动（Steam 未运行）时游戏扫不到它们。所以在文件启动且 `steam_is_running()`
为假时，把当前所有已下载的创意工坊模组（`collect_mod_roots()` 的 workshop 部分）用**目录联接（junction）**
逐个映射到 `Zomboid\mods\pzmanager-ws-<workshopId>-<模组名>`，游戏按本地模组扫描。实现用 `junction` crate
（rustup 同款，`FSCTL_SET_REPARSE_POINT` 写 MOUNT_POINT reparse 点，无需管理员权限；不用 `mklink /J` 子进程，
几百个模组快得多，也不怕模组名里的 cmd 元字符）。生命周期：**离线启动 = 先清残留再重建**；**游戏退出
即清理**——离线启动持有游戏进程句柄，起 `pz-offline-cleanup` 线程 `wait()` 游戏退出后立刻拆除联接
（启动器中途关闭时线程随进程消失，由下一次启动的清理兜底）；**Steam 启动 / 启动器重启 = 全部拆除**
（游戏在跑时跳过，个别被占用拆不掉的记日志），避免与 Steam 工坊扫描重复加载。
清理只动「名字带 `pzmanager-ws-` 前缀 **且** 确为联接点」的目录项，用户的同名真目录绝不误删；启动器的
本地模组扫描（`collect_mod_roots` / `list_lua_mods`）都会跳过联接，映射存在期间不产生重复条目。映射结果
随 `launch_project_zomboid` 的返回值拼进成功提示。

> ⚠️ **进程检测禁止用 `tasklist`**（2026-11-08 事故）：本机实测 `tasklist /FI "IMAGENAME eq steam.exe" /NH`
> 要 **85 秒**、`ProjectZomboid64.exe` 那条连带窗口枚举要 **113 秒**。它曾出现在启动路径（光影覆写的"游戏是否在运行"
> 判断）上，把整条启动命令卡在「启动配置已写入」之后，游戏根本拉不起来。现在统一走
> `overlay::imp::image_name_running()`（toolhelp 进程快照，~8ms）：`steam_is_running` / `project_zomboid_running`
> / 光影覆写的 `game_running_quick` 三处都已替换，并有 `#[ignore]` 体检测试 `real_world_launch_guard_timings`
> 断言三者都 < 2 秒。

2、3 由 `apply_launch_loader()` 在每次启动前检查，**内容已正确就不重复写**；写入带 `game_path/.pzmanager-backups/<时间戳>/` 备份，多文件写入走 `apply_file_transaction()`，任一步失败回滚前面已写的文件。`force_zombie_buddy_policy*` 保留为"显式按 ZombieBuddy 处理"的包装（Viewpoint 3D 配置等显式动作用）。

> ⚠️ `policy=allow-all` 是 ZombieBuddy 官方标注"不推荐"的模式：所有 Java 模组（含未签名/来源不明）都会被直接加载，且跳过 ZBS 签名校验。改成 `deny-new` 可保留校验但静默跳过未批准的模组。详见 [overlay-injection.md 附录](overlay-injection.md)。

**极速启动（实验性，已跑通）**：开关在 设置 › 实验性功能，且只在「模组加载器方案 = zombieBuddy」时可用。落地形态：

* **缓存产出**：JAR 扫描结束时若开关开启，`build_fast_launch_cache()` 把**已批准且启用**的模组（jar 路径 / basePackage / jarHash / `@Patch` 类 / `@Exposer` 类）连同 FNV-1a 指纹写成 `%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`（`format` + `generatedAt` + `modSetHash` + `mods[]`）；实测一次扫描产出 25 个模组、Viewpoint 92 patches / 8 exposers。
* **投送（JVMTI agent 早期加载，正式路线）**：启动器把覆盖层 DLL 作为 `-agentpath:<DLL>` 写进 `ProjectZomboid64.json` 的 `vmArgs`（必要时也写 `.bat` 的 `_JAVA_OPTIONS`），JVM 在启动最早期自己加载它并调 `Agent_OnLoad`：门禁（读 `settings.json` 的 `fastLaunch` + `modLoaderScheme`，前端禁用只是界面约束，注入器必须自己再判）→ 校验预扫描缓存（格式版本 + 指纹）→ `GetEnv(JVMTI)` → `AddCapabilities` → 注册 `VMInit` 回调 → **立刻打开 `ClassFileLoadHook`** → 之后在 `VMInit` 回调里 `DefineClass` 送进辅助类（onload 阶段拿不到 `JNIEnv`，实测 `GetEnv(vm, JNI_VERSION_1_6)` 返回 `JNI_EDETACHED = -2`）→ ZombieBuddy 自己加载 `PatchEngine` 时就地改成"包装式装配"。
* **两条必须在代码里守住的前提（都是实测踩出来的）**：
  1. **`-agentpath:` 必须排在最前面**（`normalize_fast_launch_agent_tokens` 把它插到 `vmArgs[0]`）。JVMTI agent 按命令行顺序加载，而 ZombieBuddy 的 `-agentlib:zbNative` 会在**它自己的** `Agent_OnLoad` 里就把 `PatchEngine` 加载好；排在它后面就只有 `VMInit` 时机可用，那时类早就定义完了 —— 实测游戏里 hook 一次都没触发。
  2. **类加载 hook 必须在 `Agent_OnLoad` 里就打开**（不能等 `VMInit`），因为 `VMInit` 在所有 agent 的 onload 之后才发。
* **包装体自带 `catch (Throwable)` 兜底**：hook 打开的时机比"辅助类能进 JVM"的时机更早，中间可能已经有 `collectPatches` 调用；兜底让这些调用直接走被改名的原实现（`collectPatches$pzm`），而不是抛 `NoClassDefFoundError` 把加载器弄坏。**注意：不能用 `RetransformClasses` 事后补救** —— JVMTI 规范规定 retransform 不得新增/删除/重命名方法（`docs/fast-launch-status.md` §6.2 有出处），而"包装式装配"正是新增方法 + 改名，所以只能在**类加载那一刻**改写。
* **应急关闭**：环境变量 `PZM_FAST_LAUNCH_DISABLE=1` 让 agent 立刻什么都不做；把设置开关关掉后，启动器会在下次启动前摘掉 `-agentpath:`。
* **远程注入路线**（`JNI_GetCreatedJavaVMs` + attach + `RetransformClasses`）保留为调试路径，仍要求 `PZM_FAST_LAUNCH_INJECT=1`，且 agent 一旦接管就整体让路（`AGENT_LOADED`）。

字节码工具链（`overlay-dll/src/classfile.rs`）已完成并验证：常量池解析、方法表与 `Code` 属性定位、常量池增量编码、短路方法体、**包装式装配**（原方法改名 `collectPatches$pzm` + 追加同名包装方法 + 原实现字节码逐字节不变）。辅助类 `overlay-dll/java/pzm/fastlaunch/FastLaunchBridge.java` 已写好并编译（`javac --release 8 -d java/classes java/pzm/fastlaunch/FastLaunchBridge.java`），字节用 `include_bytes!` 嵌进 DLL。投送链路的调用顺序（下标/能力位/回调槽位/事件值/回调签名全部从本机头文件实证）：

1. `JNIEnv->DefineClass`（**下标 5**）把辅助类字节送进 JVM（在 `VMInit` 回调里做，那里回调直接给 `JNIEnv`）；
2. `AddCapabilities`（**141**）申请 `can_generate_all_class_hook_events`（bit 26）与 `can_retransform_classes`（bit 37；能力结构是位域，字段顺序要从 `jvmti.h` 逐个数，别猜）；
3. `SetEventCallbacks`（**121**）登记 `VMInit`（槽位 0）与 `ClassFileLoadHook`（槽位 4）；然后**在 `Agent_OnLoad` 里立刻** `SetEventNotificationMode`（**1**）打开 `CLASS_FILE_LOAD_HOOK`（**54**），并打开 `VM_INIT`（**50**）用于稍后定义辅助类；
4. 回调里认类名等于 `me/zed_0xff/zombie_buddy/PatchEngine` 时，用 JVMTI 交来的原始字节就地 `wrap_collect_patches()`，再用 `Allocate`（**45**）分配输出缓冲交回；任何失败 = 放行原字节 = 原生实现；
5. （调试路径才用）`RetransformClasses`（**151**）配合 `class_being_redefined != null` 事后改写。
> 📄 极速启动的完整实现状态、已实证常量、验证方法与实测日志：**[fast-launch-status.md](fast-launch-status.md)**（本文档只保留规格与红线）

⚠️ **环境指针层级实测结论（2026-10-14 深夜修正，务必先读）**：`JavaVM` / `JNIEnv` / `jvmtiEnv` **三者形态完全一样**——JVM 交到手上的都是"一个结构的地址，结构的第一个字段才是函数表指针"（`jni.h`/`jvmti.h` 在 C++ 下分别 typedef 成 `JavaVM_` / `JNIEnv_` / `_jvmtiEnv` 这三个带 `functions` 字段的结构体，在 C 下才 typedef 成"指向函数表的指针"）。所以取函数指针一律是 **`*(void**)base` 拿函数表，再按槽位索引**（`overlay-dll/src/fastlaunch.rs` 的统一入口 `table_fn()`），调用时第一个参数仍然传**原始的 base**。

* 早期版本按 C 签名把它当"指针即表"用，于是槽位索引落在结构体后面的无关内存上并**调用那个无关地址**：`GetEnv`（槽位 6）返回垃圾 `status=1376256 (0x150000)`、`AttachCurrentThread`（槽位 4）直接崩 `0xC0000005`、`jvmtiEnv` 的 `GetVersionNumber`（槽位 87）表现为 `Agent_OnLoad` 里卡死。**这三个现象都只是这一处解引用写错，与 JVMTI 能力、JDK 版本限制、agent 身份都无关**（`jni` crate 用同一槽位一直成功，正因为它按两层解引用读表）。
* 教训：核对"下标"只解决了一半问题，**指针层级必须用运行时的内存布局自检确认**（`overlay-dll/src/fastlaunch.rs` 里保留了两行布局自检日志：`*(JavaVM)` 与 `*(jvmtiEnv)` 都应当是 jvm.dll 里的函数表地址、`*(JavaVM+8)` 为 0）。
* 验证一律走一次性 JVM harness（不碰游戏）：`java -agentpath:<DLL> -cp "<ZombieBuddy.jar>;<harness classes>" Harness`，看 `%TEMP%\fast-launch.log`；用 `java` 退出码区分正常/崩溃（`-1073741819` = `0xC0000005`）。**注意 `cargo test` 不会重新链接 cdylib**，验证前必须 `cargo build`，否则会加载旧 DLL 得出假结论。

**预扫描缓存的生命周期（2026-10-15 改：持久化）**：缓存文件 `%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json` 一旦生成就**持久保留**：

* **只有用户显式点「模组管理 › JAR › 刷新 JAR 列表」才会重建**（`start_zombie_buddy_jar_scan { refresh_cache: true }`）；
* 进页面时的**自动扫描**只在"缓存还不存在"时补一份，已有缓存一律不动（`should_write_fast_launch_cache()`）；
* **改开关、改审批、改启用/禁用都不再删缓存**（早期版本把 `invalidate_fast_launch_cache()` 挂在 `write_jar_mod_state` / `set_jar_mod_info_enabled` 上，结果玩家那一局直接没加速、且在游戏里看不出来）。

代价与缓解（**这是本功能唯一残余的正确性风险**）：改了模组却没刷新时，缓存里的 `patchClasses` 可能是旧的。

* 缓解 1（可见性，**不再是自动兜底**）：注入器侧的桥接类 `FastLaunchBridge.collect(package, loader)` 是**按包名**查缓存的，查不到就记一行 `缓存无此包：<包>`。2026-10-15 起默认是**强制模式**：查不到**不回退**原生扫描（见下方 ⚠️），所以"新启用的模组在刷新前没有补丁"是显式后果，而不是被回退掩盖；
* 缓解 2（可见性）：设置页的状态行会比对指纹并在不一致时显式提示"建议点一次刷新 JAR 列表重建"（`fast_launch_cache_status().matches`），不做静默处理。

> ⚠️ 注意：指纹不一致**不会**让注入器自动放弃缓存（那是旧策略的行为，现已取消）——桥接类只检查 `format` 与 `modSetHash` 非空。所以状态行的提示是给用户看的，不是"系统会自动兜底"。

> ⚠️ **2026-10-15 改：默认"强制模式"（不回退）**。桥接类 `FastLaunchBridge` 现在默认**只用启动器预扫描数据**：缓存里没有这个包、或缓存本身不可用，`collect()` 直接返回空列表，**不再**调用 ZombieBuddy 的原生 `collectPatches`；缓存命中时个别类名取不到也只跳过那一个（不会把整包推回 ClassGraph）。要恢复旧行为，把环境变量 `PZM_FAST_LAUNCH_ALLOW_FALLBACK` 设成 `1`（要能传进游戏进程，例如设成系统环境变量后重启 Steam）。
>
> 代价（**显式接受**）：缓存不全时，那些包的补丁会**直接缺失**，而不是悄悄回退——这正是"强制"的目的（回退会让"没加速"看起来像正常）。所以**每次改完模组集合都要点一次「刷新 JAR 列表」**；新增/启用的模组在刷新前不会有补丁。

> ⚠️ **2026-10-15 深夜补充：缓存必须包含 ZombieBuddy 加载器本体，包匹配要递归到子包。**
> 加载器本体被 `jar_scan_candidates` 排除在「JAR 模组」列表外（它不可审批），但它的补丁类
> （`me.zed_0xff.zombie_buddy.patches.*`）必须进缓存，否则强制模式下 ZB 自己的补丁会被丢掉
> （实测表现：进游戏弹"ZombieBuddy 未安装"）。实现：`zombie_buddy_loader_cache_mod_at()` /
> `fast_launch_cache_mods_at()`（写缓存与校验指纹共用，否则指纹永远对不上）；`scan_java_mod_jar()`
> 收声明包**及其子包**；桥接类按**类自己的包**建索引、`lookup(pkg)` 返回 `pkg` 及其子包
> （对齐 ClassGraph 的 `acceptPackages`）。
>
> ⚠️ **2026-10-15 深夜再补：ZombieBuddy 传进来的 `ClassLoader` 是 `null`。**
> 反编译证实 `Loader.loadJar()` 走 `addJarToClasspath()`（=`Instrumentation.appendToSystemClassLoaderSearch`）
> 之后调 `ApplyPatchesFromPackage(package, **null**, phase)`。`Class.forName(name, false, null)`
> 会用 bootstrap 加载器，导致**每个类**都 CNFE（实测 43/43）。桥接类必须按
> `调用方 loader → 系统加载器 → 线程上下文加载器` 依次尝试——ZB 原生就是这么找的
> （`collectPatches` 在 `loader==null` 时用 `overrideClasspath`）。

> ⚠️ **缓存里的类名必须是二进制名（点号）**：扫描（`class_annotations`）产出的是**内部名**（`viewpoint/Patch_AimAxis`），而注入器要用 `Class.forName` 取类 —— 直接喂内部名会**每个模组都抛 `ClassNotFoundException`、全部回退原生扫描**（表现就是"一点没加速"，日志里只有一串 CNFE，见 `docs/fast-launch-status.md` §6.3）。两侧都做转换：启动器 `binary_class_name()` 写盘时就落点号，桥接类 `binaryName()` 再兜一层（也能救回老缓存）。**测试数据要照真实产物的形状写**（用内部名），否则这个坑测不出来。

两张函数表下标都已从本机 JDK 22 头文件实证（不要凭记忆写，这类下标写错会直接把游戏搞崩）：

* `jni.h` 的 `JNINativeInterface_`（**含 reserved 槽位的绝对下标**）：`DefineClass`=5、`FindClass`=6、`ExceptionClear`=17、`DeleteLocalRef`=23、`GetObjectClass`=31、`GetStaticMethodID`=113、`CallStaticObjectMethodA`=116、`CallStaticVoidMethod`=141、`NewStringUTF`=167、`ExceptionCheck`=228；
* `jvmti.h` 的 `jvmtiInterface_1_`（同样**含 reserved 槽位**，务必按这个口径数，否则整体偏移）：`SetEventNotificationMode`=1、`GetAllThreads`=3、`Allocate`=45、`Deallocate`=46、`GetClassSignature`=47、`GetClassStatus`=48、`GetClassMethods`=51、`IsInterface`=54、`GetMethodName`=63、`GetLoadedClasses`=77、`GetClassLoaderClasses`=78、`GetVersionNumber`=87、`SetEventCallbacks`=121、`GetPotentialCapabilities`=139、`AddCapabilities`=141、`RetransformClasses`=151。

> ⚠️ 这两张表踩过一次坑：第一版抽取**跳过了 `reservedN` 槽位**，JVMTI 那组整体偏了 1（`GetVersionNumber` 记成 85，实际 87），而它已经被写进代码并会以错误的函数指针调用——那是能直接崩游戏的。数下标时**必须把 reserved 槽位算进去**，并且改完要复核。

⚠️ **已识别的崩溃陷阱（务必不要踩）**：JNI 没有"只加载不初始化"的入口，`FindClass("me/zed_0xff/zombie_buddy/PatchEngine")` 会触发类初始化。若在 ZombieBuddy 自己的加载流程之前拉起 `PatchEngine`，它的 `<clinit>` 会在依赖未就绪时执行；一旦抛 `ExceptionInInitializerError`，该类被永久标记失败，ZombieBuddy 之后自己再用它就必然报错——那不是"回退原生扫描"，而是把加载器弄坏。安全路径二选一：① JVMTI `GetLoadedClasses` 只查**已加载**的类，等 ZombieBuddy 自己加载完 `PatchEngine` 再介入；② JVMTI `ClassFileLoadHook` 在加载那一刻接管（时序天然正确，但要写字节码，风险最高）。两条路都必须先 `ExceptionCheck`、失败即 `ExceptionClear` 并整体放弃缓存。

**接管扫描的设计结论（已实现，2026-10-14 跑通）**——下面是选型依据，改动前先读：

* **反射路线不可行（已核实）**：文档《僵毁启动速度优化思路》的"策略 B（Hook Loader 类、注入静态方法、把结果赋给 Loader 的私有静态变量）"落地时发现没有可赋值的字段。`javap -p` 显示 `Loader` 的静态字段只有 `g_known_jars` / `_known_package_phases` / `_known_mains` / `g_jarLoadStatus` / `g_prev|curr_modSet*` 这类"模组集合与包阶段"信息，**没有保存 `@Patch` 类清单的字段**——那份清单是 `Loader.loadMods()` 里的局部变量。`PatchEngine.collectPatches` 又是 `static`，反射无法替换。所以"纯反射喂缓存"做不到。
* **动字节码是唯一出路（已实现）**：纯反射喂缓存做不到（见上一条），所以选了 **`ClassFileLoadHook`（事件 54）在 `PatchEngine` 首次加载时接管**，用"包装式装配"改写：原方法改名 `collectPatches$pzm`（只改它自己的 `name_index`，ZombieBuddy 的调用点仍指向 `collectPatches`），再追加一个同名同签名的包装方法、方法计数 +1，**原实现字节一个都不动**；包装体只有 6 字节（`aload_0 / aload_1 / invokestatic pzm/fastlaunch/FastLaunchBridge.collect / areturn`）。辅助类命中缓存就用缓存的类名 + 传入的 `ClassLoader` 取类；**默认强制模式下未命中的包返回空列表、不调用原实现**，只有设了 `PZM_FAST_LAUNCH_ALLOW_FALLBACK=1` 才反射调 `collectPatches$pzm`（**真正的原生实现**，不是复刻逻辑）。`RetransformClasses`（**151**）那条路仍保留在代码里当调试路径，但正式路线是"接管首次加载"（时序天然正确，不需要先把类拉起来）。
* **时序要求**：缓存必须在 ZombieBuddy 调用 `collectPatches` 之前就位；`PatchEngine` 一旦被加载，改写就只能靠 retransform。这也是为什么"接管首次加载"比"事后改"更稳。
* **红线（2026-10-15 修订）**：只注入已批准且启用的模组；默认强制模式**不**回退原生扫描（改前是"任何一步异常都必须回退"），逃生开关是 `PZM_FAST_LAUNCH_ALLOW_FALLBACK=1`；"缓存与当前启用列表一致"从"自动回退"改成"必须靠「刷新 JAR 列表」保证"。**已定位的短路点**（`javap -c -p` 反编译真实 `ZombieBuddy.jar` 得到）：ClassGraph 全量扫描只在 `me.zed_0xff.zombie_buddy.PatchEngine.collectPatches(String package, ClassLoader) -> List<Class<?>>` 里（该类 22 处 classgraph 引用；`Loader`/`Agent`/`Patch` 均为 0 处），流程是 `new ClassGraph()` → `enableAllInfo()` → `acceptPackages()` → `overrideClassLoaders()` → `overrideClasspath()` → `scan()` → `getAllClasses()`；入口链是 `Agent` → `Loader.preloadMods()` → `Loader.loadMods(List<String>)` → `PatchEngine.collectPatches`。缓存里已经存了每个模组的 `patchClasses`（类名），所以短路 = 让 `collectPatches` 用缓存的类名 + 传入的 `ClassLoader` 直接把类取回来，省掉扫描。硬约束：只注入已批准且启用的模组；指纹对不上或任何一步异常都必须放弃缓存、回退 ZombieBuddy 原生扫描。

### 6.4 模组管理

**Lua 模组**（数据源 `%USERPROFILE%\Zomboid\mods\default.txt` + 各 `mods/<id>/mod.info`）：

* 解析 `mod.info` 得到 `id/name/requires/loadAfter/loadBefore`，多份副本按"Build 42 优先"合并（`merge_lua_mod_entry`）；
* 排序规则来自 `parse_lua_sorting_rules` + B42/MLOS 规则，写回时保持文件其余内容不变（`write_default_mod_ids` 只替换 `mods{}` 块）；
* 启用/停用即"是否出现在 `default.txt`"，并做**依赖前置排序**（`order_mod_ids_by_requires`，缺依赖会报告）。

**JAR 模组**（ZombieBuddy 注入式）：

* 扫描工坊与本地模组根，读 `Mod.info`（或 `Mod.info.disabled` 作为回退）定位 jar 与海报；
* 用 `java-class-rs` 解析 class 字节，识别 `@Patch` / `@Exposer$LuaClass` 与具体注入注解，产出 `patchMethods` 供冲突分析；
* 审批决策与文件态持久化在 `%USERPROFILE%\Zomboid\.pzmanager\jar-mod-state.json`：设置「模组加载器 › 启动时保留 JAR 模组启用状态」（`settings.json` 的 `jarModUseDiskStateOnStartup`，**默认开启**）开启时，启动 / 自动扫描不改写 `mod.info`，而是把磁盘当前状态读作基准并回写记录（`sync_jar_mod_state_on_startup`）；关闭后则把记录**重新套用**回文件（`apply_jar_mod_file_state`），工坊重下覆盖后仍能恢复启动器里的选择。显式「启用 / 禁用」与套用模组预设不受该设置影响，仍会写文件；
* 扫描是后台任务：`start_zombie_buddy_jar_scan` 返回 id，逐条 emit 进度与结果；注解统计也用于覆盖层的"真实注入进度"分母。

### 6.5 资源中心、下载器与新闻

* 工坊浏览：`browse_workshop`（排序/必需与排除标签/分页/搜索词），`workshop_options` 返回可选项（含标签表），结果 5 分钟内存缓存。
* 工坊详情（列表项右键 →「在窗口内打开」）：`fetch_workshop_item_detail` 调 `ISteamRemoteStorage/GetPublishedFileDetails`，取 `description`（新版接口叫 `file_description`，两个都认）里的 BBCode 原文走 `bbcode_to_html` + `add_translation_spans` 切段，前端再复用 `translate_developer_news_batch` 分批翻译、按 `sid` 回填 HTML（`replaceNewsTranslation`）。标题和正文共用一套 sid 方案（`w<id>_title` / `w<id>_t<node>_s<idx>`），所以标题会跟着一起翻译；工坊标题是纯文本，**不做** BBCode 剥离，否则 `[B42]` 这类后缀会被吃掉。
* 光影下载：`fetch_shader_resources` / `fetch_shader_resource_files` 走 Modrinth 优先、CurseForge 兜底；`download_shader_file` 多线程下载到 `%USERPROFILE%\Zomboid\viewpoint-shaderpacks`，进度用事件推送。
* 新闻：`fetch_developer_news` 取 Steam 新闻并切成片段，`translate_developer_news_batch` 按批翻译，前端按 `sid` 回填 HTML（`replaceNewsTranslation`）。目标语言来自设置里的全局项「通用 › 翻译语言」（`settings.json` 的 `translationLanguage`，取值只认 `zh-CN`/`zh-TW`/`en`/`ja`/`ko`/`ru`，读设置时会归一化并回落默认），开发者新闻与创意工坊详情共用这一份，页面内不再各自放语言下拉。翻译引擎可在「通用 › 翻译引擎」（`settings.json` 的 `translationEngine`）手动指定，没写过 / 坏值按 `google` 处理；默认链是**Google → 必应 → MyMemory**：Google 不可用（被墙 / 超时，熔断 10 分钟）时落到必应，必应走手机版 `cn.bing.com` 的 `ttranslatev3`（国内直连、质量好于 MyMemory；单次文本按 900 字切段，令牌过期/401 自动刷新重试一次，失败熔断 60 秒），再不行落到 MyMemory。手动指定某个引擎时它排链首，失败后按默认链回退其余引擎；「中转翻译」（relay，`{relay}/translate` → 本机 Argos）已从可选引擎下线：设置页不再提供该选项，旧配置里的 `relay` 在读设置时按坏值回落 `google`；管线里的 relay 代码保留但不再被选中，缓存里旧的中转译文仍显示为「中转翻译」。翻译来源在新闻卡片上按占比显示（`relay` 标为「中转翻译」，`bing` 标为「必应」）。

### 6.6 背景图与截图/光影

* 背景：自定义图存 `%USERPROFILE%\Zomboid\.pzmanager\backgrounds\<kind>\`；工坊模组的 title 图会被拼成四象限合成图（`compose_title_background_at`）；`set_background_path` 把选中项记进 `settings.json`；二进制经 `read_background` 回传。
* 内置壁纸背景音乐「启动游戏自动暂停」：启动页 › 启动按钮 › 自定义背景 › 内置背景 里开关（`settings.json` 的 `pauseMusicOnLaunch`，缺省开，命令 `save_pause_music_on_launch`）。开启时，后端推送 `startup-timing`（界面出现「启动用时 XXXs」）的那一刻，前端给播放器发一次`pauseSignal`，播放器暂停并立刻落盘（`playing:false` + 当时的播放位置），下次启动保持暂停。只认事件推送，不认挂载时读到的"上次记录"，避免每次开启动器都误暂停。
* 内置壁纸背景音乐：播放器拆成常驻的 `LaunchMusicProvider`（挂在 App 根部，`enabled = wallpaperSeasonal`，持有 `<audio>` 与全部播放状态）和启动页左下角的胶囊控件 `LaunchMusicControls`（同在 `src/components/launchMusic.tsx`）——切到其它页面时控件随页面卸载、`<audio>` 不卸载，音乐不中断；换成自定义壁纸时 `enabled` 变 `false`，自动暂停。控件复用创意工坊背景信息的展开按钮 / 展开菜单样式（共用 `.pz-launch-workshop-info*` 类名），面板内容换成播放器（曲名 / 播放暂停 / 可点击进度条 / 时间）。音源固定为壁纸目录 `public/backgrounds/main.mp3`（打包后 `/backgrounds/main.mp3`），`loop` 循环、收起不打断播放；与创意工坊胶囊互斥（选了工坊背景就不显示内置壁纸）。**播放器带音量调节**（静音按钮 + 0–100 滑条，键盘方向键 ±5）与**播放状态持久化**：音量 / 是否在播 / 播放位置 / 面板展开四项写进 `settings.json` 的 `launchMusic`（命令 `set_launch_music_state`，与 `set_wallpaper_effect` 同款「前端规整后原样存取」约定）；播放中**每 300ms** 把进度同步写进 localStorage（独立定时器，不依赖 `timeupdate` 约 250ms 的节奏，实测间隔稳定 300ms），刷新/关窗最多丢 0.3 秒；settings.json 仍每 5 秒一次 IPC，播放/暂停/拖动进度/调音量/开关面板立即落盘，`pagehide`/卸载时再兜一次。**默认自动播放、默认音量 30**：没写过状态（或字段坏掉）时按"播放 + 30"处理，用户手动暂停后存档的 `false` 会被尊重；存档带格式版本 `v`（当前 2），**老存档（无 `v`）按新默认迁移一次**——只保留播放位置与面板展开，否则改版前写进去的"暂停 + 音量 60"会让新默认看起来没生效。WebView2 默认按手势策略拦截有声自动播放，`tauri.conf.json` / `tauri.dev.conf.json` 的 `additionalBrowserArgs` 都加了 `--autoplay-policy=no-user-gesture-required`；即便被拒，也还有"第一次 `pointerdown` / `keydown` 补放"兜底。真正起播时通过 `onMusicStarted` 回调让启动页弹一条右上角提示（`notice` 类型）。下次启动读取并恢复：音量立即生效，位置在 `loadedmetadata` 后定位一次。**落盘两层**：先同步写 `localStorage`（键 `pzmanager.launchMusic`），再走 IPC 写 settings.json；启动时两份按 `savedAt` 取新的（`pickNewerMusicState`）。原因：页面刷新/关窗瞬间 IPC 可能来不及到达后端，实测"暂停 + 进度"就是这样丢的；localStorage 是同步写入，刷新后仍在。另外**读档完成前禁止落盘**（`loadedRef`）——否则组件在读档前被卸载会把默认值（播放/0s）写回去，把上次的暂停与进度覆盖掉。**恢复期间的落盘还必须保护存档位置**（`persistedPosition`）：起播会触发 `onPlay` 立刻落盘，定位在 `loadedmetadata/durationchange/loadeddata/canplay/play/timeupdate` 任一事件上应用一次（Tauri 资源协议下 `duration` 常为 `Infinity`，不能以"duration 有限"作为定位前提）；`flushState` 里的 `playing` 以 `<audio>.paused` 实况为准，避免 play/pause 事件乱序写反；那一刻 `audio.currentTime` 还是 0，直接写就会把存档进度抹掉、导致每次启动都从头播（2026-11-08 实测问题）；恢复目标一直保留到真正应用为止，暂停时也走同一层保护。纯逻辑（状态规整、音量换算、`m:ss` 格式化、进度百分比）在 `src/launchMusicUi.ts`，测试 `tests/launchMusicUi.test.ts`。
* 四季粒子特效：四季壁纸之上叠一层 canvas（`src/seasonalParticles.ts`，移植自参考 HTML）——春=白色光带、夏=细雨、秋=五裂枫叶、冬=雪花光晕；粒子数量按 `WallpaperEffect.weights` 动态平衡，换季时此消彼长；粒子层套用与季节图**同一套滤镜链**（`url(#pz-seasonal-curve-*)` + 亮度 / 对比度 / 饱和度），色调随壁纸一起变化。只在"真壁纸"那份渲染（亚克力副本是模糊克隆，不重复跑），`visibilitychange` 隐藏时停表，尺寸由 `resize` + `ResizeObserver` 跟踪。
* 内置背景：图片随前端打包在 `public/backgrounds/` 下（当前一张 `default.jpg`，来源是用户给的 `IMG_20261013_134024.png`，转 JPEG q92 存原生 1666×944、494KB）。`settings.json` 里存成 `builtin:<id>` 哨兵值——磁盘上没有对应文件，前端直接用打包 URL 渲染，所以 `set_background_path` 对 `builtin:` 前缀跳过 `background_path_allowed` 目录校验；`backgroundPath` 为 `null`（老配置或没选过）时前端回落这张内置图当默认外观，应用不提供"纯色/无背景"选项。
* 截图/光影：只允许操作托管目录内的文件（`managed_file_path` / `background_path_allowed` 做目录穿越防护），截图上限按支持格式过滤。
* 光影包配置覆写（`shaderpack_config.rs`，**默认关闭**）：`settings.json` 的 `shaderpackOverrideEnabled` 为开时才会在启动前写 `viewpoint-live.properties`；默认关闭状态下光影包页点「配置」会先弹确认框说明「接管期间游戏内修改不会保存」，同意后才把开关置真并打开配置面板。「设置 › 通用 › 启动器接管光影管理」可随时关闭，关闭后启动不再覆写任何光影配置文件（若还留着上次未还原的备份，启动时会先恢复原文件）。
光影包配置覆写（`shaderpack_config.rs`）：光影包页每包「配置」弹窗的选项检测**照搬 Viewpoint `IrisOptions` 的方法**——扫描 zip 内源码（.vsh/.fsh/.gsh/.csh/.tcs/.tes/.glsl；`shaders/` 根与子目录都算，包根支持一层顶层目录包裹 `Wrapper/shaders/…`）里的三类声明：`#define NAME // 注释` / `//#define NAME`（注释掉 = 默认关）为开关、`#define NAME 值 // 注释 [允许值...]` 为取值、`const int|float|bool NAME = 值; // [允许值]` 为常量，再合并 `shaders.properties` 的 `profile.*` 预设 / `sliders` 滑块 / 顶层默认值声明（`read_pack_shader_meta`，源码声明的注释作为说明兜底）。`shaders.properties` 的解析语义照 Iris 1.7.6 逆向结果：`OrderBackedProperties extends java.util.Properties`，即完整 Java `Properties.load` 规则（`\` 续行折叠——BSL/Bliss 的 screen 布局全靠它、`#`/`!` 注释、空白/`=`/`:` 分隔符、`\uXXXX` 等转义）。`shaders/lang/*.lang` 把参数名 / 取值 / 预设替换成用户能看懂的描述。弹窗内可**切换描述语言**（`read_shaderpack_config` 的 `lang` 参数，切换只换标签不动已改的值）：默认中文（`zh_CN` → `zh_TW`），没有中文回退 `en_US` → 包里第一个 .lang；键大小写不敏感兜底，数值匹配自动去尾零，`option.*`/`option.*.comment`/`value.*.*`/`profile.*`；取值标签兼容**按值**（BSL：`value.shadowMapResolution.512=…`）与**按序号**（Bliss：`value.RESOURCEPACK_SKY.0=…`，序号 = 允许值列表下标）两种键形态，缺失的键按 MC 语义用 `en_US.lang` 键级回退。lang 标签里可能带 MC `§` 彩字/格式码：前端 `parseMinecraftFormatting`（`shaderpackConfigUi.ts`）按 Java 版语义解析渲染（颜色码 §0-§f 会禁用既有格式、§r 全重置、格式码叠加保留颜色、未知码字面保留），`McText` 组件渲染彩色 span；原生 `<option>` / title 等渲染不了样式的场合用 `stripMinecraftFormatting` 剥码留文字；纯格式码标签按"没有标签"回退原值。**界面为 Iris 同款分页配置**：解析 `screen=` 主页与 `screen.<name>=` 子屏布局（token 解析照 Iris `OptionMenuElement.create`：参数名 / `[子屏]` / `<empty>` / `<profile>`，`screen.<name>.columns` 列数，没写 screen 时单页铺开全部参数），弹窗内网格分页（每页 5 行 × 列数，**分页条常驻**——分页设计强制适用于所有光影包）、子屏进栈导航 + 返回、`‹ x/y ›` 翻页；token 解析不了（引用未检测到的参数、`<profile>` 但无预设）时按 Iris `OptionMenuContainer` 的**逐元素 try/catch 跳过该元素**（记 launcher.log 警告），菜单照常构建；布局里没列出的参数（MC_NORMAL_MAP / FSH / VSH 等内部 define）保持隐藏——Iris 的 GUI 只渲染布局里列出的元素；**分页形态强制**：没有 `shaders.properties` 的纯源码包（EclipseShaders 类）或过滤后所有页为空时，强制合成截图同款的按钮分页——参数 ≤10（2 列 × 5 行）直接单页铺开，超过则主页变成"第 N 页"按钮页（`auto_page_N` + label），点进去才是该页参数网格；`profile.*` 预设支持**继承**（`profile.Medium=profile.Low …` 展开被继承预设的参数值）。覆写值存 `.pzmanager/shaderpack-overrides.json`。`launch_project_zomboid` 在 `apply_launch_loader` 之后、拉起 Steam 之前调 `apply_shaderpack_overrides_for_launch()`：游戏运行中跳过（Viewpoint 退出时会重写整个文件）、先还原上次遗留备份，再对 `viewpoint-live.properties` 做**行级覆写**——只碰 `minecraft.<packId>.*` / `minecraftProfile.<packId>`，保留注释、顺序与换行风格，`camera.*`/`pack.*`/`graphics.*` 等 Viewpoint 自身键一字不动（键分类与拆分规则见模块注释，packId 优先用 live 文件已观测键反查、按文件名 sanitize 兜底）。写入走 备份（`.pzmanager/viewpoint-live-backups/`，整文件 + manifest，保留 5 份）→ `apply_file_transaction` 原子替换 → 回读校验，失败用备份还原。覆写成功后启动 `pz-shaderpack-restore` 监听线程（`overlay_session.rs`）：等游戏进程出现 → 等退出（24h 兜底）→ 整文件还原；启动器中途退出则由下次启动的覆写流程先还原遗留备份。

### 6.7 命令参考（⚠️ 快照自 `lib.rs` 的 `generate_handler!`；实际条数以代码为准）

| 域 | 命令 |
| --- | --- |
| 环境/设置/启动（18） | `detect_project_zomboid`、`get_system_memory_mb`、`get_app_settings`、`save_memory_limit`、`save_developer_mode`、`save_steam_account_hidden`、`save_translation_language`、`save_font_contrast`、`save_mod_loader_scheme`、`save_fast_launch`、`set_background_path`、`save_shader_download_settings`、`launch_project_zomboid`、`save_native_titlebar` |
| 内存清理（7） | `save_silent_memory_cleanup`、`save_clean_memory_on_launch`、`save_aggressive_memory_clean`、`get_memory_status`、`clean_memory_now`、`is_launcher_elevated`、`restart_as_admin` |
| 一键安装/配置（3） | `install_zombie_buddy`、`install_pz_optimization`、`configure_viewpoint_three_d` |
| 模组管理（8） | `start_zombie_buddy_jar_scan`、`list_lua_mods`、`set_lua_mod_enabled`、`set_lua_mods_enabled`、`save_lua_mod_order`、`set_zombie_buddy_mod_decision`、`set_zombie_buddy_mods_decisions`、`apply_jar_mod_file_state` |
| 媒体读取（4） | `read_background`、`read_jar_mod_poster`、`read_lua_mod_poster`、`read_viewpoint_shot` |
| 背景图（4） | `list_custom_backgrounds`、`scan_workshop_backgrounds`、`start_workshop_background_scan`、`import_custom_background` |
| 截图/光影（8） | `list_viewpoint_shots`、`list_viewpoint_shaderpacks`、`import_viewpoint_shaderpack`、`delete_viewpoint_shaderpack`、`read_shaderpack_config`、`get_shaderpack_overrides`、`save_shaderpack_override`、`clear_shaderpack_override` |
| 资源中心（7） | `workshop_options`、`browse_workshop`、`fetch_workshop_item_detail`、`resource_options`、`fetch_shader_resources`、`fetch_shader_resource_files`、`fetch_minecraft_versions` |
| 下载器（1） | `download_shader_file` |
| 新闻（2） | `fetch_developer_news`、`translate_developer_news_batch` |
| 覆盖层（4） | `overlay::inject_overlay`、`overlay_session::start_overlay_session`、`overlay_session::check_game_process`、`overlay_session::stop_game_process` |
| 版本隔离（12） | `get_version_status`、`save_version_store_path`、`save_version_parallel`、`start_version_snapshot`、`start_version_capture`、`start_version_restore`、`start_version_verify`、`start_version_rebase`、`cancel_version_operation`、`delete_version`、`delete_version_through`、`is_project_zomboid_running` |
| 启动器日志（4） | `read_launcher_log`、`clear_launcher_log`、`open_launcher_log_folder`、`log_frontend_message` |
| 启动时长（1） | `startup_timing`（最近一次「窗口 → 主菜单」时长 + 上一次，供右下角对比显示） |
| 任务中心持久化（2） | `load_saved_tasks`、`save_tasks`——tasks.json（`%USERPROFILE%\Zomboid\.pzmanager\`）的不透明 JSON 文档读写：模型由前端持有，后端只裁剪到 200 条并原子写（临时文件 + 改名）；读失败按无历史任务处理并写 WARN 日志。启动时前端经 `sanitizeSavedTasks`（`src/taskHistory.ts`）清洗条目，退出时还在 `running` 的任务标成「失败：应用退出时任务被中断」（光影下载保留 `request`，重启后重试按钮仍可用）；前端在任务变化后 0.6s 防抖落盘，首次装载完成前不写，装载后把任务 id 计数器拨到历史最大号之后避免撞 id |

版本隔离的存储模型、增量格式与还原流程见 [version-isolation.md](version-isolation.md)。

重新生成：在 `src-tauri/src/lib.rs` 里搜 `invoke_handler`，`generate_handler![]` 的内容就是权威清单（命令参数用 `snake_case`，前端 invoke 用 `camelCase`）。

### 6.8 内存清理（静默 / 启动时 / 激进）

设置 › 启动 里的三项：常规清理只做一件事——**把各进程的工作集（working set）还给系统**，不结束任何进程；打开「激进内存清理」后才追加系统级内存列表与文件缓存清理。

| 设置项 | 存储字段 | 触发时机 |
| --- | --- | --- |
| 静默内存清理 | `silentMemoryCleanup` + `silentMemoryCleanupThreshold`（30–95%，默认 80） | 后台线程每 10s 读一次 `GlobalMemoryStatusEx`，占用 ≥ 阈值就清一次；两次之间至少隔 60s（`CLEANUP_COOLDOWN`），激进模式拉长到 300s（`AGGRESSIVE_CLEANUP_COOLDOWN`），避免反复刷盘/清缓存拖慢读盘 |
| 启动时清理内存 | `cleanMemoryOnLaunch` | `launch_project_zomboid` 里、`apply_launch_loader` 之后、拉起 Steam 之前清理一次 |
| 激进内存清理 | `aggressiveMemoryClean` | 不是独立触发点，而是给手动 / 静默 / 启动前三条路径加系统级步骤 |

* **清理口径对齐 PCL2**：PCL2 的 `--memory` 子进程用「清理前后可用物理内存差」回报释放量（`Application.xaml.vb`），`McLaunchMemoryOptimize` 是启动阶段的那一次（`ModLaunch.vb`）——本项目沿用同一口径，`CleanupReport.freed_mb` 只在可用内存**变多**时才算数。
* **「已提交」回收不了**：commit charge 是各进程持有的提交配额，外部工具把驻留页挤出工作集只会把它变成待机页，数值本身不会回落。激进模式真正能改变的是「已缓存」：`MemoryEmptyWorkingSets` 清全系统工作集 → `SetSystemFileCacheSize(-1,-1,0)` 刷新系统文件缓存 → `MemoryFlushModifiedList` 刷已修改页 → `MemoryPurgeStandbyList` 把待机页标成空闲。界面文案不要承诺清「已提交」。
* **系统级步骤要管理员**：两条 NT 指令前置 `AdjustTokenPrivileges` 打开 `SeProfileSingleProcessPrivilege` / `SeIncreaseQuotaPrivilege`；非管理员会拿到 `STATUS_PRIVILEGE_NOT_HELD`，`CleanupReport` 用 `emptied_working_sets` / `flushed_file_cache` / `flushed_modified` / `purged_standby` 如实回报。设置页在开关打开且 `is_launcher_elevated` 为 false 时给「以管理员身份重启」（`restart_as_admin` → `ShellExecuteW(runas)`，取消 UAC 则返回错误、原实例继续运行）；`tauri::is_dev()` 为真时直接返回错误，避免 `tauri dev` 拉起的第二实例脱离 dev server 白屏。
* **绝不清游戏进程**：`PROTECTED_EXE_NAMES`（`java.exe` / `javaw.exe` / `ProjectZomboid64.exe` …）与自身 pid 一律跳过。激进模式的 `MemoryEmptyWorkingSets` 无法排除单个进程，所以 `protected_process_running()` 为真时跳过这一步（`protected_running` 回报给界面）；静默清理在游戏运行时干脆退回常规模式。
* **不是启动前置条件**：读不到内存状态、清理报错都只记日志，绝不阻断启动。
* **设置即时生效**：`save_silent_memory_cleanup` / `save_aggressive_memory_clean` 落盘后立刻更新原子变量，不用重启启动器；启动时 `setup()` 再把持久化值喂一次。
* **前端**：`get_memory_status` 每 5s 轮询一次（设置页显示占用百分比），清理由后端 `memory-cleaned` 事件推给界面刷新；「上次清理」的系统级结果由 `memoryCleanupSystemDetail()` 统一拼文案。
* **测试**：`memclean.rs` 自带阈值收敛、真实清理跑通、静默/激进配置往返、常规模式不报系统级步骤等单元测试；会真清待机列表的 `aggressive_cleanup_runs` 默认 `#[ignore]`。注意 CI/沙箱里 `tempfile` 相关测试会因临时目录被拒而失败，与本模块无关。

## 7. 覆盖层（概要）

细节以 [overlay-injection.md](overlay-injection.md) 为准，这里只留结构：

* **DLL 侧**（[overlay-dll/src/lib.rs](../overlay-dll/src/lib.rs)）：`DllMain` 只记模块句柄 + `DisableThreadLibraryCalls` + 起后台线程；线程里等 `opengl32.dll` 与 `wglSwapBuffers` 导出 → MinHook 安装 → 首次回调时用 `wglGetProcAddress` 解析并缓存 `glBindFramebuffer`（解析不到就永久降级为"不动 FBO"）；回调只用原子操作，不做分配、不 `unwrap`；另有一条 GDI 线程在"窗口已出现但还没换帧"期间直接刷窗口 DC。
* **窗口内界面**（[overlay-dll/src/ui.rs](../overlay-dll/src/ui.rs)）：同一张 GDI DIB 两用——换帧前 `BitBlt` 到窗口 DC，换帧后上传为 GL 纹理并画满屏四边形；40ms 刷新（≈25fps），显示最新 6 行控制台 + 阶段文字 + 进度条；位置/DPI 按游戏窗口与 `GetDpiForWindow` 跟随。
* **历史方案**：更早的实现是启动器里的独立 topmost 分层窗口（`src-tauri/src/overlay_ui.rs`）。该文件**在当前代码里已不存在**（`lib.rs` 的 `mod` 列表中没有它），只在 `backup/20261010-184005-overlay-inwindow/` 留档。文档 6.1 节若还提它，属于漂移。
* **停止条件**（启动器判定，满足任一即停）：命中主菜单标记 / `[ZB]` 静默 10s / 无 `[ZB]` 时注入后 3 分钟兜底 / 会话 9 分钟上限；DLL 内部另有 10 分钟安全上限。**前置条件**：必须先收到 `…OverlayStarted_<pid>`，否则不做完成判定。

## 8. 磁盘与跨进程状态

| 路径 | 谁写 | 谁读 | 说明 |
| --- | --- | --- | --- |
| `%USERPROFILE%\Zomboid\console.txt` | 游戏 | 启动器（tail） | `[ZB]` 进度与完成标记的唯一来源；启动器只**追加**自己的 `[PZManager]` 记录 |
| `%USERPROFILE%\Zomboid\mods\default.txt` | 游戏/启动器 | 启动器 | Lua 模组启用列表与顺序；启动器只替换 `mods{}` 块 |
| `%USERPROFILE%\Zomboid\options.ini` | 游戏（退出时整份重写）/ 启动器 | 游戏 / 启动器 | 游戏设置：启动器「设置 › 游戏设置」行级读取与修改（`game_options.rs`），每项显示粗略中文名并按小标题分组（`gameOptionsLabels.ts` 16 组：画面模式与性能 / 画质与纹理 / 天气与特效 / 缩放与地图 / 字体与语言 / 界面布局 / 准星与高亮 / 聊天显示 / 信息与提示 / 新手引导与一次性标记 / 声音与音乐 / 语音聊天 / 移动与自动操作 / 键位与手柄 / 镜头与战斗 / 单位与杂项，未收录的键落到「其他」），搜索同时匹配中文名与键名，空分组自动隐藏；游戏运行中只读，写前备份到 `.pzmanager\game-options-backups\`（保留 5 份） |
| `%USERPROFILE%\Zomboid\viewpoint-shaderpacks\` | 启动器 | 启动器 | 光影包托管目录（下载与导入都落这里） |
| `%USERPROFILE%\Zomboid\viewpoint-live.properties` | **Viewpoint（游戏）** | 启动器（行级覆写）/ Viewpoint | Viewpoint 的实时设置总表；启动器启动前只覆写 `minecraft.<packId>.*` / `minecraftProfile.<packId>` 键（行级编辑保留注释与顺序），游戏退出后整文件还原为启动前备份 |
| `%USERPROFILE%\Zomboid\.pzmanager\shaderpack-overrides.json` | 启动器 | 启动器 | 每个光影包的参数覆写配置（packName / profile / values） |
| `%USERPROFILE%\Zomboid\.pzmanager\viewpoint-live-backups\` | 启动器 | 启动器 | 覆写前的整文件备份（`viewpoint-live.properties.bak.<纳秒>`，保留最近 5 份）+ `manifest.json`（还原凭据：原文件是否存在 + 备份文件名）；还原线程 / 下次启动用 |
| `%USERPROFILE%\Zomboid\.pzmanager\settings.json` | 启动器 | 启动器 | 内存上限、背景图、开发模式、账户隐藏、下载器设置；设置 › 开发者选项的「清除启动器用户数据」会删除整个 `.pzmanager` 目录后重建 |
| `%USERPROFILE%\Zomboid\.pzmanager\jar-mod-state.json` | 启动器 | 启动器 | JAR 模组审批决策；默认以磁盘当前状态为基准，关闭「启动时保留 JAR 模组启用状态」后改为重新套用记录 |
| `%USERPROFILE%\Zomboid\.pzmanager\backgrounds\<kind>\` | 启动器 | 启动器 | 导入/合成的背景图 |
| `<游戏目录>\.pzmanager-backups\<时间戳>\` | 启动器 | 人 | 改写 `ProjectZomboid64.json`/`.bat` 前的备份 |
| `%TEMP%\pz_overlay_status.txt` | 启动器 | **DLL** | 阶段文字 + 进度 + 控制台尾部 + 背景色 `bgcolor=` + 心跳 `beat=` + `console=`/`session=`（覆盖层界面数据源）；心跳超时后 DLL 自读 `console.txt` |
| `%TEMP%\overlay.log` | **DLL** | 人 | Hook 安装、首次换帧、收到停止信号等 |
| 命名事件 `Local\PZManagerOverlayStop_<pid>` | 启动器 | DLL | 停止覆盖 |
| 命名事件 `Local\PZManagerOverlayStarted_<pid>` | DLL | 启动器 | 首帧已画，允许做完成判定 |
| Steam `localconfig.vdf`（LaunchOptions） | 启动器 | Steam | 启动参数与内存；写入前备份 |
| `ProjectZomboid64.json` / `.bat` | 启动器 | JVM | agent 策略与 `-Xmx` |

## 9. 外部服务依赖

| 服务 | 用途 | 位置 |
| --- | --- | --- |
| Steam 注册表 / `libraryfolders.vdf` / `appmanifest_108600.acf` / `loginusers.vdf` / `localconfig.vdf` | 发现与启动配置 | `lib.rs` |
| `steamcommunity.com/workshop/browse/` | 工坊浏览（HTML 解析，5 分钟缓存） | [workshop.rs](../src-tauri/src/workshop.rs) |
| `api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1` | 工坊项目详情（正文 BBCode 与标签） | [workshop.rs](../src-tauri/src/workshop.rs) |
| `api.steampowered.com/ISteamNews/GetNewsForApp/v2` + `store.steampowered.com` | 开发者新闻 | [news.rs](../src-tauri/src/news.rs) |
| `translate.googleapis.com`、`cn.bing.com`（手机版翻译页 + `/ttranslatev3`）、`translate.yandex.net` | 新闻 / 创意工坊 / 翻译器翻译（中转不可用 / 语种不支持时的回退） | [news_translation.rs](../src-tauri/src/news_translation.rs) |
| 中转地址的 `/translate`（relay → 本机 Argos） | 中 / 英 / 俄离线翻译 | [news_translation.rs](../src-tauri/src/news_translation.rs)、[steam_relay.rs](../src-tauri/src/steam_relay.rs)、`steam-relay/translate/` |
| `api.modrinth.com/v2`、`api.curseforge.com/v1` | 光影资源检索 | [resource.rs](../src-tauri/src/resource.rs) |

## 10. 测试

### 10.1 Rust（`src-tauri`）

`lib.rs` 末尾的 `#[cfg(test)] mod tests`（~90 个测试）覆盖：VDF/转义路径解析、`Mod.info` 大小写不敏感解析、JSON/BAT/Steam 启动项改写与内存替换、agent 归并、写入事务回滚、工坊背景流式回调、class 注解识别、JAR 扫描、`default.txt` 块替换与依赖排序、JAR 状态持久化等。夹具在 `src-tauri/tests/fixtures/`（由 `news_translation.rs` 以 `include_str!` 引入）。

```powershell
cd src-tauri; cargo test
```

### 10.2 前端（`tests/`）

17 个 `*.test.ts`，**全是"源码文本断言"**：把 [App.tsx](../src/App.tsx)、[App.css](../src/App.css)、`tauri.conf.json`、`capabilities/default.json` 读成字符串，再用正则断言实现细节（例如 `.pz-mod-row.is-selected` 的 border-color、`role="checkbox"` 出现 2 次、Vite 端口 10002、窗口尺寸等）。

* 优点：零依赖、能锁死样式/配置契约，不需要 DOM 环境。
* 代价：**重构 CSS 或改 JSX 就会碎**，而且它们验证不了运行时行为；写这类断言时优先锁定"语义契约"（ARIA 角色、必需属性、配置项），少锁具体色值与选择器组合。

运行方式（`package.json` 里**没有 test 脚本**，Node ≥22 自带 TS 类型剥离）：

```powershell
node --test tests/          # 或 node --test tests/*.test.ts
```

## 11. 构建与运行

| 目的 | 命令 |
| --- | --- |
| 前端开发 | `npm run dev`（固定端口 10002，`strictPort`） |
| 完整开发（含 Rust） | `npm run tauri:dev`（用 `tauri.dev.conf.json`；工作目录为 `src-tauri`，故 `DEV_OVERLAY_DLL_PATH` 是 `../overlay-dll/...`） |
| 打包 | `npm run tauri:build` |
| 仅检查后端 | `cd src-tauri; cargo check` |
| 构建覆盖层 DLL | `cd overlay-dll; cargo build --release --target x86_64-pc-windows-msvc` → `overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll` |
| 切注入后端（nightly） | `cargo +nightly build --features dll-syringe-inject` |

前置条件：Windows + MSVC 工具链（覆盖层 DLL 必须有 `cl.exe`）；Rust stable 即可（默认注入后端）；`npm install`。

⚠️ 打包缺口：`tauri.conf.json` 的 `bundle` 没有配置 `resources`，覆盖层 DLL 不会被打进安装包，前端默认路径也是指向开发产物的相对路径。若要发布"开箱即用"的版本，需要把 DLL 作为资源打包并改 `overlayInjection.ts` 的默认路径解析。

## 12. 已知问题与技术债

1. **巨石文件**：`App.tsx`（~5.3k 行 / 单组件 ~110 个 state）与 `lib.rs`（~6.6k 行）都是单人可维护性的上限区。抽取方向已存在（`src/*.ts` 纯函数模块、`src-tauri/src/*.rs` 子模块），继续沿着它拆即可：UI 按页面拆组件、`lib.rs` 按域拆模块（Steam 发现 / 配置改写 / 模组 / 背景 / 命令层）。
2. **源码文本断言**：见 10.2，重构成本被放大。
3. **没有 test 脚本、没有 CI**：`npm test` 不存在，Rust 与 TS 测试都要手敲命令；也没有任何自动化门禁。
4. **文档漂移**：[overlay-injection.md](overlay-injection.md) 的 6.1 节仍描述已删除的 `src-tauri/src/overlay_ui.rs`；[superpowers/](superpowers) 的 plan/spec 停留在"只做路径探测"阶段。本文第 7 节已标注现状。
5. **窗口尺寸自相矛盾**：`tauri.conf.json` 里 `width: 808 < minWidth: 970`，首开窗口会被立刻撑到 970。
6. **磁盘占用**：`src-tauri/target` ~75.8 GB、`backup/` ~2.3 GB、`overlay-dll/target-verify` ~92 MB，都在工作区内且未提交。
7. **版本控制薄弱**：分支 `master`，历史只有 `1 / 2 / 3 / 4` 四条无信息提交，其余全部未提交；`backup/<时间戳>-<slug>/` 实际上承担了版本历史。另外仓库因 ACL 归属问题需要 `git -c safe.directory=F:/pzmanager …` 才能执行 git 命令。
8. **`policy=allow-all` 的既定取舍**：见 6.3 的警告，属于"有意为之但必须知情"的行为。
9. **命令注册是单点**：`run()` 里 49 条命令集中在一个 `generate_handler!`，新增命令必须同时改这里和前端类型，容易漏。
10. **覆盖层依赖前端相对路径与 `%TEMP%` 文件**：调试方便（可以直接看/改状态文件），但状态文件协议由两边各自实现，改一边必须同步另一边（第 8 节是当前契约清单）。

## 13. 常见改动入口（速查）

| 我想改… | 先看 |
| --- | --- |
| 启动参数 / 内存 / 模组加载器方案 | `lib.rs` 的 `patch_*`、`apply_launch_loader*`、`mod_loader_policy_agent`、[launchArgs.ts](../src/launchArgs.ts)、设置 › 模组加载器 |
| Lua 模组排序或启停规则 | `parse_lua_sorting_rules`、`order_mod_ids_by_requires`、`write_default_mod_ids`、[modManagement.ts](../src/modManagement.ts) |
| JAR 模组扫描 / 冲突风险分级 | `scan_java_mod_jar`、`jar_mod_state_*`、[jarModUi.ts](../src/jarModUi.ts) |
| 覆盖层绘制内容或进度来源 | [overlay-injection.md](overlay-injection.md) → `overlay-dll/src/ui.rs` + `overlay_session.rs` |
| 新增一个后端命令 | `lib.rs` 写 `#[tauri::command]` → 加进 `run()` 的 `generate_handler!` → 前端 `invoke` → 在 `tests/` 补一条断言（如适用） |
| 新增一个页面 | `ActivePage` 联合类型 + `navigationItems` + `App()` 里的 `<page>Content` 分支 + `App.css` 的 `pz-` 样式 |
| 外部服务地址/超时 | 对应模块顶部的 `const`（`workshop.rs` / `resource.rs` / `news.rs` / `news_translation.rs`） |

## 附录 A：常量与 ID

| 常量 | 值 | 含义 |
| --- | --- | --- |
| `PROJECT_ZOMBOID_APP_ID` | `108600` | Steam AppID |
| `ZOMBIE_BUDDY_MOD_ID` / `ZOMBIE_BUDDY_WORKSHOP_ID` | `3619862853` | ZombieBuddy 加载器 |
| `VIEWPOINT_MOD_ID` | `3809306528` | Viewpoint 光影模组 |
| `ZOMBIE_BUDDY_AGENT` | `-agentlib:zbNative` | agent 前缀 |
| `ZOMBIE_BUDDY_POLICY_AGENT` | `-agentlib:zbNative=policy=allow-all` | 强制策略（见 6.3 警告） |
| `MOD_LOADER_SCHEMES` / `DEFAULT_MOD_LOADER_SCHEME` | `["zombieBuddy", "none"]` / 默认 `zombieBuddy` | 「模组加载器方案」（设置 › 模组加载器），决定启动时挂哪个加载器 |
| 开发端口 | `10002` | Vite `strictPort`，与 `tauri.conf.json` 的 `devUrl` 一致 |

## 附录 B：本文档怎么维护

* 本文是**导航 + 契约**文档，不要往里塞实现细节，细节留在各模块头部的 `//!` 注释与 [overlay-injection.md](overlay-injection.md)。
* 改动以下任一项时请同步本文：新增/删除后端命令（6.7）、新的跨进程文件或事件（第 8 节）、新增外部服务（第 9 节）、新增页面（5.1）、测试策略变化（第 10 节）。
* 标注方式：稳定的结构直接写；易漂移的具体值（行数、清单、配置项）加 ⚠️ 并说明以代码为准；已知的文档漂移写进 12.4 而不是把旧描述留在正文。
