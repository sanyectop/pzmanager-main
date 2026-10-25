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
        │           │      · 等 Local\…Stop_<pid>   │
        │           └──────────────────────────────┘
        │ 3. tail %USERPROFILE%\Zomboid\console.txt（解析 [ZB] 进度）
        └──────────────────────────────────────────────► overlay_session 线程
```

三条**跨进程契约**（改任何一边都要同步另一边，详见第 8 节）：

1. 启动器 → DLL：状态文件 `%TEMP%\pz_overlay_status.txt`（阶段文字 / 进度 / 控制台尾部）；
2. 启动器 → DLL：命名事件 `Local\PZManagerOverlayStop_<游戏PID>`（`SetEvent` 即停止覆盖）；
3. DLL → 启动器：命名事件 `Local\PZManagerOverlayStarted_<PID>`（首帧已画，会话在此之前不做"加载完成"判定）+ `%TEMP%\overlay.log`（人读日志）。

## 5. 前端（`src/`）

### 5.1 页面结构

一级页面枚举 `ActivePage = launch | overview | shots | shaderpacks | workshop | mods | settings`，其中两个是"容器页"：

| 一级页 | 子页 | 内容 |
| --- | --- | --- |
| 启动 `launch` | — | 账户/内存上限/启动参数、启动与停止游戏、覆盖层会话进度面板 |
| 支持库 `overview` | — | 探测到的本机路径、ZombieBuddy Viewpoint/PZ_Optimization 状态与一键配置；三项安装内容收在同一个「支持组件」面板里，每行沿用模组行的卡片外观（`.pz-mod-row`），默认折叠（折叠态：名称 + 一句话说明 + 状态标签 + ⋯ 详情按钮）。加载器那行只报状态并指路「设置 › 模组加载器」，不在本页做安装 |
| 截图管理 `shots` | — | Viewpoint 截图列表、预览、复制到剪贴板 |
| 光影管理 `shaderpacks` | — | 光影包导入（zip）/删除、图标与搜索词推导 |
| 资源中心 `workshop` | `workshop \| shaderDownload \| developerNews` | 工坊筛选浏览 / Modrinth·CurseForge 光影下载 / Steam 新闻 + 翻译 |
| 模组管理 `mods` | `lua \| jar` | Lua 加载顺序与启停、JAR 审批决策与冲突分析 |
| 设置 `settings` | `general \| console \| downloader \| modLoader` | 通用、加载控制台、下载器、模组加载器（加载器方案 + 该方案加载器的状态与安装） |

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

* 后端 `Err(String)` 会被 Tauri 原样抛出，前端统一转成 `Error` 后交给 Toast；
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
| [news_translation.rs](../src-tauri/src/news_translation.rs) | 批量翻译（15 条/批，单条 ≤2000 字），引擎 Google `translate_a/single` → 失败回退 MyMemory（按文字系统猜源语言），Google 失败后熔断 10 分钟直接走 MyMemory；`engine == "none"` 视为失败触发逐句重试，`none` 结果不进缓存 | 带内存缓存 |
| [overlay.rs](../src-tauri/src/overlay.rs) | 注入入口：路径校验、进程轮询（优先 `ProjectZomboid64.exe`，其次 `javaw.exe`）、重复注入检测 | 编译期二选一注入后端 |
| `overlay_inject_win32.rs` / `overlay_inject_dll_syringe.rs` | 两套等价后端，对外都是 `inject_dll(pid, dll_path)` | 后者需 nightly |
| [overlay_session.rs](../src-tauri/src/overlay_session.rs) | 会话编排：等窗口 → 注入 → 解析控制台 → 写状态文件 → 判定完成 → 发停止事件；并提供游戏进程查询/停止命令；顺带记录「检测到窗口 → 移除覆盖层」的启动时长（落盘 `startup-timing.json`，发 `startup-timing` 事件，`startup_timing` 命令读回） | ⚠️ 正在大改 |
| [memclean.rs](../src-tauri/src/memclean.rs) | 系统内存状态（`GlobalMemoryStatusEx`）与工作集清理（`SetProcessWorkingSetSizeEx(-1,-1)`，等价 `EmptyWorkingSet`）+ 静默清理后台线程 | 形态对齐 PCL2 的「内存优化」；清理**跳过游戏进程与自身** |
| `main.rs` | 仅 `pzmanager_lib::run()` | 5 行 |

### 6.2 Steam 发现链

`steam_roots()`（注册表 + 常见路径回退）→ `library_paths_from_config()` 解析 `libraryfolders.vdf` → `game_path_from_library()` / `workshop_path_from_library()`（读 `steamapps/appmanifest_108600.acf`）→ `steam_profile_from_root()` 读 `userdata/*/config/loginusers.vdf` 取最近账户（SteamID64 → account id 用于定位 `localconfig.vdf`）。缺失任何一环都按"正常的未找到"处理，不报错。

### 6.3 启动链与模组加载器方案（安全相关，改动前必读）

启动时挂哪个加载器由设置里的「模组加载器方案」（`modLoaderScheme`：`zombieBuddy` / `none`，默认 `zombieBuddy`）决定，`launch_project_zomboid` 读它再调 `apply_launch_loader(memory, mod_loader_policy_agent(&scheme), fast_launch_agent)`：

- **内存上限始终写**（三处都写；JSON 里的旧 `-Xmx` 会覆盖启动器设置）；
- **加载器 agent 只在方案启用加载器时归一化**：`policy_agent = None`（方案 `none`）时 `patch_json_launcher_with_agent` / `patch_batch_with_agent` 整段跳过 agent 逻辑，绝不往用户的启动配置里塞 agent；
- **极速启动的 `-agentpath:` 只在三件事同时成立时才写**：设置里 `fastLaunch = true`、方案是 `zombieBuddy`、且那份覆盖层 DLL **确实存在**（`resolve_fast_launch_agent_path()` 走 `overlay::imp::normalize_dll_path()` 做存在性校验）。任一条不成立都传 `None`，而 `None` 会把启动文件里已有的 `-agentpath:` **全部摘掉**——因为 `-agentpath` 指向的文件找不到会让 **JVM 直接启动失败**，那违反"任何失败都回退原生扫描"的红线。`.`bat` 的 `SET _JAVA_OPTIONS` 是按空白切分的，所以路径含空格时只写 JSON、不写 .bat（`fast_launch_agent_for_batch()`）。

ZombieBuddy 的 agent 策略是 **write-once**（`Loader.setPolicy`），JVM 只加载第一个 agent，因此方案是 `zombieBuddy` 时启动器在**三个位置**都把 agent 归一化成 `-agentlib:zbNative=policy=allow-all`：

1. Steam `LaunchOptions`（`patch_steam_launch_options_content_with_memory`，注意 `--` 结束符与已有 agent 的归并）；
2. 游戏目录 `ProjectZomboid64.json` 的 `vmArgs`；
3. 游戏目录 `ProjectZomboid64.bat` 的 `SET _JAVA_OPTIONS`。

2、3 由 `apply_launch_loader()` 在每次启动前检查，**内容已正确就不重复写**；写入带 `game_path/.pzmanager-backups/<时间戳>/` 备份，多文件写入走 `apply_file_transaction()`，任一步失败回滚前面已写的文件。`force_zombie_buddy_policy*` 保留为"显式按 ZombieBuddy 处理"的包装（Viewpoint 3D 配置等显式动作用）。

> ⚠️ `policy=allow-all` 是 ZombieBuddy 官方标注"不推荐"的模式：所有 Java 模组（含未签名/来源不明）都会被直接加载，且跳过 ZBS 签名校验。改成 `deny-new` 可保留校验但静默跳过未批准的模组。详见 [overlay-injection.md 附录](overlay-injection.md)。

**极速启动（实验性，已跑通）**：开关在 设置 › 启动，且只在「模组加载器方案 = zombieBuddy」时可用。落地形态：

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
* 审批决策与文件态持久化在 `%USERPROFILE%\Zomboid\.pzmanager\jar-mod-state.json`：工坊重下覆盖了 `Mod.info` 后，启动器仍能把用户选择**重新套用**回文件（`apply_jar_mod_file_state`）；
* 扫描是后台任务：`start_zombie_buddy_jar_scan` 返回 id，逐条 emit 进度与结果；注解统计也用于覆盖层的"真实注入进度"分母。

### 6.5 资源中心、下载器与新闻

* 工坊浏览：`browse_workshop`（排序/必需与排除标签/分页/搜索词），`workshop_options` 返回可选项（含标签表），结果 5 分钟内存缓存。
* 工坊详情（列表项右键 →「在窗口内打开」）：`fetch_workshop_item_detail` 调 `ISteamRemoteStorage/GetPublishedFileDetails`，取 `description`（新版接口叫 `file_description`，两个都认）里的 BBCode 原文走 `bbcode_to_html` + `add_translation_spans` 切段，前端再复用 `translate_developer_news_batch` 分批翻译、按 `sid` 回填 HTML（`replaceNewsTranslation`）。标题和正文共用一套 sid 方案（`w<id>_title` / `w<id>_t<node>_s<idx>`），所以标题会跟着一起翻译；工坊标题是纯文本，**不做** BBCode 剥离，否则 `[B42]` 这类后缀会被吃掉。
* 光影下载：`fetch_shader_resources` / `fetch_shader_resource_files` 走 Modrinth 优先、CurseForge 兜底；`download_shader_file` 多线程下载到 `%USERPROFILE%\Zomboid\viewpoint-shaderpacks`，进度用事件推送。
* 新闻：`fetch_developer_news` 取 Steam 新闻并切成片段，`translate_developer_news_batch` 按批翻译，前端按 `sid` 回填 HTML（`replaceNewsTranslation`）。目标语言来自设置里的全局项「通用 › 翻译语言」（`settings.json` 的 `translationLanguage`，取值只认 `zh-CN`/`zh-TW`/`en`/`ja`/`ko`/`ru`，读设置时会归一化并回落默认），开发者新闻与创意工坊详情共用这一份，页面内不再各自放语言下拉。

### 6.6 背景图与截图/光影

* 背景：自定义图存 `%USERPROFILE%\Zomboid\.pzmanager\backgrounds\<kind>\`；工坊模组的 title 图会被拼成四象限合成图（`compose_title_background_at`）；`set_background_path` 把选中项记进 `settings.json`；二进制经 `read_background` 回传。
* 内置背景：图片随前端打包在 `public/backgrounds/` 下（当前一张 `default.jpg`，来源是用户给的 `IMG_20261013_134024.png`，转 JPEG q92 存原生 1666×944、494KB）。`settings.json` 里存成 `builtin:<id>` 哨兵值——磁盘上没有对应文件，前端直接用打包 URL 渲染，所以 `set_background_path` 对 `builtin:` 前缀跳过 `background_path_allowed` 目录校验；`backgroundPath` 为 `null`（老配置或没选过）时前端回落这张内置图当默认外观，应用不提供"纯色/无背景"选项。
* 截图/光影：只允许操作托管目录内的文件（`managed_file_path` / `background_path_allowed` 做目录穿越防护），截图上限按支持格式过滤。

### 6.7 命令参考（⚠️ 54 条，快照自 `lib.rs` 的 `generate_handler!`）

| 域 | 命令 |
| --- | --- |
| 环境/设置/启动（17） | `detect_project_zomboid`、`get_system_memory_mb`、`get_app_settings`、`save_memory_limit`、`save_developer_mode`、`save_steam_account_hidden`、`save_translation_language`、`save_font_contrast`、`save_mod_loader_scheme`、`save_fast_launch`、`set_background_path`、`save_shader_download_settings`、`launch_project_zomboid` |
| 内存清理（4） | `save_silent_memory_cleanup`、`save_clean_memory_on_launch`、`get_memory_status`、`clean_memory_now` |
| 一键安装/配置（3） | `install_zombie_buddy`、`install_pz_optimization`、`configure_viewpoint_three_d` |
| 模组管理（8） | `start_zombie_buddy_jar_scan`、`list_lua_mods`、`set_lua_mod_enabled`、`set_lua_mods_enabled`、`save_lua_mod_order`、`set_zombie_buddy_mod_decision`、`set_zombie_buddy_mods_decisions`、`apply_jar_mod_file_state` |
| 媒体读取（4） | `read_background`、`read_jar_mod_poster`、`read_lua_mod_poster`、`read_viewpoint_shot` |
| 背景图（4） | `list_custom_backgrounds`、`scan_workshop_backgrounds`、`start_workshop_background_scan`、`import_custom_background` |
| 截图/光影（4） | `list_viewpoint_shots`、`list_viewpoint_shaderpacks`、`import_viewpoint_shaderpack`、`delete_viewpoint_shaderpack` |
| 资源中心（7） | `workshop_options`、`browse_workshop`、`fetch_workshop_item_detail`、`resource_options`、`fetch_shader_resources`、`fetch_shader_resource_files`、`fetch_minecraft_versions` |
| 下载器（1） | `download_shader_file` |
| 新闻（2） | `fetch_developer_news`、`translate_developer_news_batch` |
| 覆盖层（4） | `overlay::inject_overlay`、`overlay_session::start_overlay_session`、`overlay_session::check_game_process`、`overlay_session::stop_game_process` |
| 启动时长（1） | `startup_timing`（最近一次「窗口 → 主菜单」时长 + 上一次，供右下角对比显示） |

重新生成：在 `src-tauri/src/lib.rs` 里搜 `invoke_handler`，`generate_handler![]` 的内容就是权威清单（命令参数用 `snake_case`，前端 invoke 用 `camelCase`）。

### 6.8 内存清理（静默 / 启动时）

设置 › 启动 里的两项，都只做同一件事：**把各进程的工作集（working set）还给系统**，不结束任何进程、不碰页面文件。

| 设置项 | 存储字段 | 触发时机 |
| --- | --- | --- |
| 静默内存清理 | `silentMemoryCleanup` + `silentMemoryCleanupThreshold`（30–95%，默认 80） | 后台线程每 10s 读一次 `GlobalMemoryStatusEx`，占用 ≥ 阈值就清一次；两次之间至少隔 60s（`CLEANUP_COOLDOWN`） |
| 启动时清理内存 | `cleanMemoryOnLaunch` | `launch_project_zomboid` 里、`apply_launch_loader` 之后、拉起 Steam 之前清理一次 |

* **清理口径对齐 PCL2**：PCL2 的 `--memory` 子进程用「清理前后可用物理内存差」回报释放量（`Application.xaml.vb`），`McLaunchMemoryOptimize` 是启动阶段的那一次（`ModLaunch.vb`）——本项目沿用同一口径，`CleanupReport.freed_mb` 只在可用内存**变多**时才算数。
* **绝不清游戏进程**：`PROTECTED_EXE_NAMES`（`java.exe` / `javaw.exe` / `ProjectZomboid64.exe` …）与自身 pid 一律跳过。把正在跑的游戏工作集压掉，后面每帧都要缺页重读，玩家只会感到卡顿。
* **不是启动前置条件**：读不到内存状态、清理报错都只记日志，绝不阻断启动。
* **设置即时生效**：`save_silent_memory_cleanup` 落盘后立刻 `set_silent_cleanup()` 改原子变量，不用重启启动器；启动时 `setup()` 再把持久化值喂一次。
* **前端**：`get_memory_status` 每 5s 轮询一次（仅在弹窗里显示占用百分比），清理由后端 `memory-cleaned` 事件推给界面刷新。
* **测试**：`memclean.rs` 自带 3 条（阈值收敛、真实清理跑通、静默配置往返）；注意 CI/沙箱里 `tempfile` 相关测试会因临时目录被拒而失败，与本模块无关。

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
| `%USERPROFILE%\Zomboid\viewpoint-shaderpacks\` | 启动器 | 启动器 | 光影包托管目录（下载与导入都落这里） |
| `%USERPROFILE%\Zomboid\.pzmanager\settings.json` | 启动器 | 启动器 | 内存上限、背景图、开发模式、账户隐藏、下载器设置 |
| `%USERPROFILE%\Zomboid\.pzmanager\jar-mod-state.json` | 启动器 | 启动器 | JAR 模组审批决策，工坊重下后可重新套用 |
| `%USERPROFILE%\Zomboid\.pzmanager\backgrounds\<kind>\` | 启动器 | 启动器 | 导入/合成的背景图 |
| `<游戏目录>\.pzmanager-backups\<时间戳>\` | 启动器 | 人 | 改写 `ProjectZomboid64.json`/`.bat` 前的备份 |
| `%TEMP%\pz_overlay_status.txt` | 启动器 | **DLL** | 阶段文字 + 进度 + 控制台尾部（覆盖层界面数据源） |
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
| `translate.googleapis.com`、`translate.yandex.net` | 新闻翻译 | [news_translation.rs](../src-tauri/src/news_translation.rs) |
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
