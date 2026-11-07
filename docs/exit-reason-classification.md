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

- 主循环每 40ms（`POLL_INTERVAL`）调 `process_alive(pid)`；内部
  `OpenProcess` → `GetExitCodeProcess` → 与 `STILL_ACTIVE` 比对后**把退出码丢弃**，
  然后立刻 `CloseHandle`。
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
| `unknown` | 证据不足，无法归类 | 兜底 |

（serde 序列化即上表的 camelCase 字符串；事件载荷 `OverlaySessionUpdate.stopReason` /
`exitCode`，会话详情行附 `停止原因=…，退出码=…(0x…)`。）

## 信号采集

### 1. 退出码（消灭竞态）

现状是「轮询发现已死 → 才去开句柄」，进程对象可能已经消亡，`OpenProcess` 失败，
退出码永久丢失。改法：

- 注入成功后**一次性持有常驻句柄**：`OpenProcess(SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, pid)`。
- 主循环用 `WaitForSingleObject(handle, POLL_INTERVAL)` 代替「纯 sleep + 查存活」，
  返回 `WAIT_OBJECT_0` 即已退出，**立刻** `GetExitCodeProcess`——此时进程对象必然还在。
- 永远不把 `STILL_ACTIVE`（259）当退出码；`GetExitCodeProcess` 失败则记 `unknown` 并降级。
- 句柄持有失败（权限/时序）时退回现有轮询路径，退出码记 `None`，不阻断会话收尾。

### 2. hs_err_pid<pid>.log（JVM 原生崩溃，最强信号）

JVM 原生崩溃（访问违例、栈溢出等）会写 `hs_err_pid<pid>.log`，文件名自带 pid。
游戏目录已验证存在大量历史 dump，是现成的免费信号。

- 搜索位置：游戏工作目录、`%USERPROFILE%\Zomboid\`、`%TEMP%`。
- 匹配条件（防误报，见「边界」）：**文件名 pid == 本会话 pid** 且文件 mtime 落在
  会话窗口内。
- 解析字段：`Exception Code`（0xC0000005 等）、`Internal Exception`、崩溃线程栈头部
  一两行——足够生成一句可操作的诊断。

### 3. Windows 事件日志（崩溃的第三方公证）

- 来源 `Application Error` / `Windows Error Reporting`，EventID 1000 / 1001，
  读取 Application 日志中会话结束时刻 ±30s 的条目，取 faulting module 与异常代码。
- **用户用任务管理器结束进程不会产生任何 WER 事件**——这是崩溃与外部终止最干净的
  判别器。
- 实现代价较高（EVT API 或解析），列为二期；一期先靠「退出码 + hs_err + console」。

### 4. console.txt 尾部（已有机制，表驱动化）

`diagnose_game_exit_from_console` 保留，签名表从硬编码 if 链改为数据驱动的
`&[(匹配串, 停止原因, 诊断文案)]`，便于新增：

- `NoClassDefFoundError` / `ClassNotFoundException` → `java_crash`（类名提取进诊断，
  现状已做）。
- `OutOfMemoryError` → `java_crash`（内存诊断，现状已做）。
- `uncaughtException` / `Exception thrown` → `java_crash`。
- `EXCEPTION_ACCESS_VIOLATION` / `siginfo:` / `hs_err` 引用行 → `native_crash`。

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
  对分类贡献有限；不参与裁决，仅在 launcher.log 里留 `最后心跳=…秒前` 供排查。

## 数据落点

- `SessionOutcome` 增加 `exit_code: Option<u32>` 与 `stop_reason: StopReason` 字段。
- launcher.log 会话详情行追加：
  `… 结果=游戏进程已退出。 退出码=3221225477(0xC0000005) 停止原因=native_crash。 诊断：…`
- 任务中心（`task_store.rs`）持久化同一枚举，前端按枚举展示文案与图标；
  `unknown` 不展示诊断建议，避免误导。

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
