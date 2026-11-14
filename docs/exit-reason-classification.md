# 游戏进程非正常结束：错误抓取与停止原因

> 状态：**一期已实现**（`overlay_session.rs` 的 `StopReason` / `ExitWatch` / `classify_game_exit`；
> 前端 `src/sessionEnd.ts` + App.tsx 监听弹**模态框**——正文分「结果 / 停止原因与退出码 / 诊断」
> 三段，单按钮「知道了」，不自动消失）。WER 事件日志为二期，见文末「不做的事」。
> 原始设计草案，目标是在游戏异常退出后给出**可归类的停止原因**，而不是一句笼统的
> 「游戏进程已退出」。

## 背景

游戏经「Steam 启动请求」拉起，**不是启动器的子进程**。这决定了两件事：

1. 不能用父子进程机制（job object、继承句柄）被动感知退出，只能主动监视。
2. 拿退出码必须自己 `OpenProcess` + `GetExitCodeProcess`，且要在进程对象消亡前拿到。

当前实现（`overlay_session.rs`）：

- 注入成功后即用 `ExitWatch` 持有**常驻句柄**
  （`OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, pid)`），主循环每轮
  `ExitWatch::check()`：`WaitForSingleObject` 返回 `WAIT_OBJECT_0` 即已退出，
  **在退出瞬间**用句柄 `GetExitCodeProcess` 取退出码（常驻句柄顺带防 PID 复用）。
  旧设计「每 40ms 轮询 `process_alive` 且丢弃退出码」已废弃；`process_alive` 只是
  常驻句柄打开失败（权限/时序）时的回退，此时拿不到退出码。
- 退出后 `diagnose_game_exit_from_console` 扫 console.txt 尾部 64KB 已知签名
  （NoClassDefFoundError / OutOfMemoryError / 笼统 uncaughtException）。
- 12:54 那次会话（ZB 行数=8、阶段=转换游戏类）没有任何签名命中，用户只能看到
  「游戏进程已退出」——分类缺失的直接体现。

## 停止原因枚举

| 枚举值 | 含义 | 典型证据 |
|---|---|---|
| `normalExit` | 游戏正常退出（玩家主动退出、WM_CLOSE 优雅关闭） | 退出码 0 |
| `javaCrash` | JVM 内 Java 层异常退出（uncaughtException 等） | console.txt 崩溃签名 |
| `nativeCrash` | JVM/游戏原生崩溃 | 退出码为 NT 异常码、`hs_err_pid<pid>.log`、WER 事件 |
| `externalTerminate` | 被外部终止（任务管理器结束进程、Steam 强退更新、关机） | 退出码 1 且无任何崩溃证据 |
| `launcherStop` | 启动器「停止游戏进程」强杀 | `stop_game_process` 在 taskkill 前写 pid 标记，退出监视消费后**跳过全部停止分析**（不读 console / hs_err、不弹报告） |
| `unknown` | 证据不足，无法归类 | 兜底 |

（serde 序列化即上表的 camelCase 字符串；事件载荷 `OverlaySessionUpdate.stopReason` /
`exitCode`，会话详情行附 `停止原因=…，退出码=…(0x…)`。）

## 信号采集

### 1. 退出码（已实现，`ExitWatch`）

- 注入成功后**一次性持有常驻句柄**：`OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, pid)`。
- 主循环每轮 `WaitForSingleObject(handle, 0)`（非阻塞）检测退出，返回 `WAIT_OBJECT_0`
  即已退出，**立刻** `GetExitCodeProcess`——此时进程对象必然还在（常驻句柄顺带防 PID 复用）。
- 永远不把 `STILL_ACTIVE`（259）当退出码；`GetExitCodeProcess` 失败则退出码记 `None`。
- 句柄持有失败（权限/时序）时退回 `process_alive` 轮询路径，退出码记 `None`，不阻断会话收尾。

### 2. hs_err_pid<pid>.log（JVM 原生崩溃，最强信号）

JVM 原生崩溃（访问违例、栈溢出等）会写 `hs_err_pid<pid>.log`，文件名自带 pid。
游戏目录已验证存在大量历史 dump，是现成的免费信号。

- 搜索位置：游戏工作目录、`%USERPROFILE%\Zomboid\`、`%TEMP%`。
- 匹配条件（防误报，见「边界」）：**文件名 pid == 本会话 pid** 且文件 mtime 落在
  会话窗口内。
- 解析（`parse_hs_err_summary`）：只取 dump 头部 `# ` 开头、同时含 `EXCEPTION` 与 `0x`
  的那一行异常摘要（如 `#  EXCEPTION_ACCESS_VIOLATION (0xc0000005) at pc=…`）——
  不解析 `Internal Exception` 与线程栈；这一行已足够生成一句可操作的诊断。

### 3. Windows 事件日志（崩溃的第三方公证）

- 来源 `Application Error` / `Windows Error Reporting`，EventID 1000 / 1001，
  读取 Application 日志中会话结束时刻 ±30s 的条目，取 faulting module 与异常代码。
- **用户用任务管理器结束进程不会产生任何 WER 事件**——这是崩溃与外部终止最干净的
  判别器。
- 实现代价较高（EVT API 或解析），列为二期；一期先靠「退出码 + hs_err + console」。

### 4. console.txt 尾部（已有机制，表驱动化）

`diagnose_game_exit_from_console` 保留，签名表从硬编码 if 链改为数据驱动的
`&[(匹配串, 停止原因, 诊断文案)]`，便于新增：

按优先级（最具体 → 兜底）匹配，命中即返回；命中行会压成一行摘要（去掉
`ERROR: General f:0 at Xxx > ` 前缀、折叠空白、截断 140 字）：

| 规则 | 触发签名 | 诊断方向 |
|---|---|---|
| 服务器字典缺失 | `Missing dictionary script on client:` | 客户端与服务端模组不一致 |
| 脚本冲突 | `Tile duplicate: <贴图格>` | 统计游戏本体 `media/scripts` 里该 `entity` 的声明文件数：≥2 本体重复 / =1 模组覆盖 / 其余模组间冲突 |
| 脚本加载 | `Script load errors` | 某个模组的 scripts 定义没通过校验 |
| 缺失类 | `NoClassDefFoundError` / `ClassNotFoundException` | 游戏版本与模组不匹配 |
| 类签名不匹配 | `NoSuchMethodError` / `NoSuchFieldError` / `AbstractMethodError` / `IncompatibleClassChangeError` / `IllegalAccessError` / `VerifyError` | 同上（链接失败家族） |
| 内存 | `OutOfMemoryError` / `GC overhead limit exceeded` / `Metaspace` / `unable to create new native thread` | 调高内存上限 / 减少模组 |
| 本地库 | `UnsatisfiedLinkError` / `java.library.path` / `Native Library` | 模组自带 DLL 缺失 / 位数不符 / 被杀软拦截 |
| Steam | `SteamAPI_Init` / `steam_api64` / `Could not initialize Steam` | Steam 未启动；离线启动下依赖 Steam 的模组可能不可用 |
| 窗口/OpenGL | `Failed to create window` / `OpenGL context` / `GLFW error` / `Pixel format not accelerated` | 显卡驱动 / 远程桌面 / 无 GPU |
| 存档 | `Error loading save` / `save file is corrupted` / `Failed to load world` 等 | 存档损坏或版本/模组不匹配 |
| Lua（**需未捕获签名**） | `attempt(ed) to call nil value` / `stack traceback:` / `LuaManager` | 模组 Lua 脚本错误 |
| 模组加载（需未捕获签名） | `failed to load mod` / `Error loading mod` 等 | 模组不兼容 / 依赖缺失 |
| 着色器（需未捕获签名） | `shader compile error` / `Failed to compile shader` 等 | 光影包 / 材质模组 / 驱动 |
| 贴图缺失（需未捕获签名） | `TEXTURE NOT FOUND` / `Couldn't find texture` / `Couldn't find sprite` | 资源包或材质模组未适配 |
| 兜底 | `uncaughtException` 等未捕获签名 | 模组与版本不匹配，先禁用最近新增的模组 |

> 带「需未捕获签名」的规则只在 console 里确实出现 `uncaughtException` / `Exception in thread` /
> `FATAL EXCEPTION` / `ExceptionInInitializerError` / `StackOverflowError` 时才采纳——正常情况下
> 模组 Lua 报错、贴图缺失只是警告，游戏照常运行（`<方法>> Exception thrown` 是"被捕获后打印"，
> 从来不算崩溃证据）。

**签名必须属于本次会话**：现有「console.txt 被本次启动重写后才读取」的 fresh 逻辑
已保证尾部是本次内容；二次确认取「签名行 mtime ≥ session_start」或依赖 fresh 判定，
实现取其一并写注释。

## 判定顺序与边界（核心）

裁决顺序（强信号优先，退出码只做输入、不做裁判）：

```text
hs_err（pid+mtime 匹配）      → native_crash（顺带解析异常码）
WER 事件（二期，时间窗匹配）   → native_crash
console.txt 崩溃签名          → java_crash（签名自带具体诊断）
退出码 ∈ NT 异常码集合         → native_crash
退出码 == 0                   → normal_exit
退出码 == 1 且无任何崩溃证据   → external_terminate
以上全部未命中                → unknown
```

必须明确的边界：

- **任务管理器的两种操作性质不同**：
  - 「应用」页/关窗口的 **结束任务**：先发 `WM_CLOSE`，游戏优雅退出，退出码 0 ——
    与玩家点游戏内退出**原理上无法区分**，统一归 `normal_exit`，不做猜测。
  - 「详细信息」页的 **结束进程**：`TerminateProcess(handle, 1)`，退出码 1、无
    WER、无 hs_err、console 无签名 —— 归 `external_terminate`，文案
    「游戏被外部结束（可能是任务管理器或 Steam）」。
- **退出码 1 的歧义**：JVM `System.exit(1)`（如部分 Java 异常路径）与
  `TerminateProcess(1)` 无法靠退出码区分。因此退出码 1 **只在** hs_err / WER /
  console 三处都无崩溃证据时才判 `external_terminate`；有任何一处命中则崩溃优先。
- **NT 异常码集合**（判 `native_crash` 的退出码白名单）：
  `0xC0000005`（访问违例）、`0xC0000409`（栈缓冲溢出/security check）、
  `0xC00000FD`（栈溢出）、`0x80000003`（断点）、`0xE0434352` / `0xE06D7363`
  （.NET / C++ EH，理论不该出现在 PZ 但一并兜住）。
  注意任务管理器某些场景也会用到个别非常规码（如 `0x40010004`），遇到拿不准的
  优先 `unknown`，不要硬判崩溃。
- **Steam 强退**（更新接管）、系统关机/注销/蓝屏/断电：对启动器而言全是
  「无崩溃证据的外部终止」，与任务管理器混在 `external_terminate` 里；文案用
  「被外部结束」而不是点名任务管理器，避免冤枉用户操作。
- **不能实时拦截**：启动器不附加调试器、不注入终止钩子，一切抓取都是
  **事后取证**。被杀瞬间无法保存现场，能依赖的只有被杀前游戏自己落盘的三类证据
  （console.txt、hs_err、覆盖层心跳）。
- **覆盖层 DLL 心跳的用途**：心跳文件最后时间戳只能证明「被杀前进程还活着」，
  对分类贡献有限；不参与裁决。launcher.log 的会话详情行
  （`覆盖层会话详情：ZB 行数=…，阶段=…，advice 分母=…，渲染≈…，耗时=…，结果=…`）
  会带上渲染采样供排查（读不到采样时写 `渲染≈无采样`）。

## 数据落点

- `SessionOutcome` 增加 `exit_code: Option<u32>` 与 `stop_reason: StopReason` 字段。
- launcher.log 会话详情行追加（诊断拼在结果后、逗号分隔；停止原因是 camelCase 标识）：
  `… 结果=游戏进程已退出。 诊断：游戏以异常码 3221225477(0xC0000005) 结束（原生崩溃），建议提交反馈排查。，停止原因=nativeCrash，退出码=3221225477(0xC0000005)`
- 任务中心（`task_store.rs`）持久化同一枚举，前端按枚举展示文案与图标；
  `unknown` 也返回诊断文案（「游戏退出原因未知（退出码 …），可提交反馈附启动器日志排查」），
  前端 `sessionEnd.ts` 原样展示（把「结果 / 停止原因与退出码 / 诊断」分成三段）。

## 会话收尾后继续监视（game-exited）

覆盖层会话线程原本随「加载完成」一起结束，之后游戏再崩溃就无人监视（2026-11-07
实测踩坑：崩溃报告只在覆盖层存续期内生效）。现在 `run_session` 在收尾 emit 之后，
若 outcome 没有 stop_reason（= 覆盖层已结束、游戏仍在跑），进入
`watch_game_after_overlay`：重新 `ExitWatch::open(pid)` 持常驻句柄，1 秒间隔轮询，
上限 24 小时（对齐光影包还原监听）。退出后走同一套 `classify_and_diagnose_exit`
归类，launcher.log 记「覆盖层会话后游戏退出：停止原因=…，退出码=…」，console 记录
追加 `[PZManager] 游戏已退出：…`，并以 **`game-exited` 阶段**发 `overlay-session`
事件（携带 stopReason/exitCode），前端同一套停止原因模态框弹报。

## 静默兜底的 console 停更守卫

`[ZB]` 静默兜底（`ZB_IDLE_TIMEOUT_ANY_PHASE` = 60 秒）曾把「加载中」阶段的正常
空档误判成「加载完成」提前收尾——实测游戏加载模组时 `[ZB]` 可以静默 >60 秒，而
console.txt 一直在持续输出。现在 launcher 侧的兜底收尾
（`idle_completion_with_console`）额外要求 **console.txt 已停更 ≥ 30 秒**
（`CONSOLE_QUIET_GUARD`）；console 还在长 = 还在加载，继续等。里程碑阶段
（注册 Lua 接口 / 初始化完成回调）的 10 秒静默是强信号，不加固。兜底收尾的
reason 明确写「不判定加载完成」，且不再把兜底时长记成启动用时。

边界：该守卫只在 launcher 侧调用点生效；DLL fallback 路径
（`overlay-dll/src/console_reader.rs` 共用 `overlay_core::idle_completion`）未动，
避免 DLL 行为回归——launcher 已死的场景本就没有弹报通道。

## 不做的事

- 不采用 job object 挂靠（对 Steam 中转启动的进程注入 job 有兼容风险，收益仅是
  提前几十毫秒感知）。
- 不实时拦截/阻断进程终止（需要调试器或驱动，越界）。
- 一期不接 WER 事件日志（API 成本高；hs_err + console 已覆盖绝大多数场景）。

## 案例：2026-11-07 PZArrayList 闪退（验证本设计）

- `hs_err`：无（Java 层异常，JVM 不写 dump）→ 走 console 签名，现有机制命中，
  判 `java_crash`。
- 12:54 那次 ZB 行数=8、无签名：若退出码为 1 → `external_terminate`（更符合
  「转换游戏类阶段 6 秒即退」的疑似手动终止）；若拿不到退出码 → `unknown`。
  现状两者都只显示「游戏进程已退出」，改造后可区分。
**2026-11-11 最终定位**：42.22.2 移除了 `zombie.util.list.PZArrayList`，而旧版
Viewpoint 插件（DGVViewpointEyes 等）的类常量/字段描述符仍引用它；ZombieBuddy
在 `LuaManager.init → Exposer.exposeClassNow` 反射暴露这些类时才触发类加载，
所以 `hs_err` 依旧没有、console 签名是唯一证据。现在由**启动前兼容性预检**
（`mod_compat.rs`）在拉起游戏前扫出这批模组并支持一键禁用，不必再等崩溃后反查。

### 案例：2026-11-12 官方 42.22 脚本重复（MetalSmallPoleFence）

- 现象：进游戏（不连服务器）在脚本加载阶段退出，诊断弹窗点名
  `RuntimeException: [MetalSmallPoleFence] Tile duplicate: constructedobjects_01_61 at
  SpriteConfigScript...` + `IOException: Script load errors`。
- 定位：官方 42.22 的 `media/scripts/generated/entities/walls/` 里有两份声明**同名实体**
  `MetalSmallPoleFence` 的脚本——`entity_metal_smallpolefence.txt`（带
  `OnCreate = BuildRecipeCode.fenceOrWall.OnCreate`，11-11 随更新新写入）与
  `entity_metal_smallpolelfence.txt`（旧拼写、无 OnCreate，10-18 旧文件，Steam 更新未清理）。
  两份都把 face N 指到 `constructedobjects_01_61`，触发 Tile duplicate。
- 修复（已在本机执行）：删除旧拼写一份（先备份到
  `.pzmanager-backups/20261112-manual-dupfix/`），保留带 OnCreate 的新版。
  注意 `pzopt` 目录曾放过生成的散装 class，`generated/` 下同时存在 11-11 新文件与
  10-18 旧文件是 pzopt 生成目录未被 Steam 清理的迹象。
- 教训：脚本类错误要按「实体名 → 声明文件数」区分"游戏本体重复"与"模组覆盖本体"，
  诊断已实现（`count_entity_script_definitions`）。
