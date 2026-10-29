# 覆盖层自读 console.txt（混合模式）

## 目标

启动器存活时保持现状：启动器解析 `%USERPROFILE%\Zomboid\console.txt`，把精确 advice 进度写进 `%TEMP%\pz_overlay_status.txt`。

启动器关闭、崩溃或状态文件超过心跳窗口后：DLL 不再停在最后一帧，而是自己读 `console.txt`，更新阶段/日志/估算进度，并在加载完成时自行摘 Hook、停止绘制。

不改变现有「绝不呈现裸背景帧」「时间窗换帧门控」等不变量。

## 当前问题（已确认）

- DLL 只消费状态文件：`overlay-dll/src/ui.rs` 的 `poll_status_file()`；文件不更新就永久显示最后一帧。
- 启动器关闭后 `SetEvent(Local\PZManagerOverlayStop_<pid>)` 永远不会发出；DLL 只能等 `overlay-dll/src/lib.rs` 的 600 秒安全上限。
- 状态文件没有心跳、console 路径或会话标识；启动器在 `[ZB]` 长时间静默时也可能不写文件，所以不能只靠 mtime 判断启动器是否存活。
- DLL 没有 jar 注解扫描依赖，无法独立计算精确 advice 分母；回退路径使用阶段估算进度。

## 实施步骤

### 0. 备份

先建 `backup/pre-overlay-selfread` 分支/提交当前工作区，出问题时可以整段回退。

### 1. 抽出共享的纯解析模块

新增无依赖 crate `overlay-core/`：

- `overlay-core/Cargo.toml`、`overlay-core/src/lib.rs`。
- 从 `src-tauri/src/overlay_session.rs` 移入/共享纯函数与常量：
  - `zb_phase`、`on_game_init_complete`、`completion_marker`、`COMPLETION_MARKERS`
  - `parse_advice_count`、`extract_jar_path`
  - `progress_for`（当前是死代码，回退路径会用）
  - `idle_completion`、`SILENCE_COMPLETION_PHASES`、超时常量
  - console 行规范化（去 `> ` 前缀、120 字符截断、最多 6 行、连续去重）
- 在 `src-tauri/Cargo.toml` 和 `overlay-dll/Cargo.toml` 加 `overlay-core = { path = "../overlay-core" }`。
- `overlay_session.rs` 改为调用共享函数；Rust 测试迁到 `overlay-core` 或改导入。
- 好处：launcher 精确解析和 DLL 回退解析使用同一套阶段/完成标记，避免两份规则漂移。

### 2. 启动器状态文件加心跳与 console 路径

修改 `src-tauri/src/overlay_session.rs` 的 `StatusFile`：

- 新字段：`console_path`、`launcher_pid`、`session_start_ms`、`beat`。
- `flush()` 在现有 `progress/phase/memmax/bgcolor/line` 之外写：
  - `beat=<unix_ms>`（每次 flush 更新）
  - `console=<绝对路径>`
  - `launcherpid=<进程 PID>`
  - `session=<unix_ms>`（本次启动的 session_start）
- 新增 `heartbeat()`：即使没有新 `[ZB]` 行/进度变化，也至少每 1 秒强制 flush 一次，避免长时间静默（Authors list fetch 等）被误判为启动器已死。
- 在 `track_console` 的 40ms 轮询里调用心跳；等待 console 文件刷新期间也保持心跳。
- `StatusFile::new` 改为接收 console 路径；`run_session_inner` 把 `console_path` 的计算提前到创建状态文件之前。
- 保留现有 `bgcolor=` 的精确格式，`tests/overlayBackgroundColor.test.ts` 继续通过；新增断言覆盖 `beat=`/`console=`/`launcherpid=`/`session=`。

### 3. DLL 自读 console.txt

新增 `overlay-dll/src/console_reader.rs`：

- `ConsoleReader` 保存 console 路径、文件 offset、pending 行、phase、zb_lines、applied_advices、last_zb_at、overlay_started、session_start。
- 增量读取：8KB/次，`String::from_utf8_lossy`；文件变短时重置 offset/pending；文件 mtime 早于 `session_start` 时不读，避免回放上一局日志。
- 复用 `overlay-core`：阶段映射、advice 计数、完成标记、idle 判定、console 行规范化、阶段估算进度。
- 完成条件：
  - `Running callbacks for onGameInitComplete` 立即完成（42.22 主菜单 Bink 过场之前）；
  - 主菜单标记兜底；
  - 启动后 `[ZB]` 静默 10 秒（收尾阶段）/120 秒（其它阶段）；
  - 180 秒没有 `[ZB]`、540 秒会话上限兜底。
- 返回 `ConsolePoll { phase, progress, lines, has_progress, complete: Option<String> }`。
- 回退进度用 `progress_for` 估算，并做单调钳制，避免启动器关闭瞬间进度倒退；阶段文本可显示 `注入 N 条 advice`。
- 单元测试：用 tempdir 写模拟 console，覆盖截断重置、`onGameInitComplete`、主菜单标记、idle、UTF-8、6 行窗口。

### 4. DLL UI 线程接入回退

修改 `overlay-dll/src/ui.rs`：

- `poll_status_file()` 返回状态快照，并判断启动器是否存活：
  - 有 `beat=`：`now - beat <= 3s` 为 fresh；
  - 老版本没有 `beat`：退回 mtime 新鲜度；
  - 缺失/超过 2 秒宽限：进入 fallback。
- fresh：完全保持现有行为（用启动器的 phase/progress/lines）；不启动自读，避免双写。
- stale/缺失：
  - 仍然应用状态文件里的 `bgcolor`/`memmax`/`console`/`session`（配置可保留）；
  - 用 `console=` 路径创建/轮询 `ConsoleReader`；没有该字段时退回 `%USERPROFILE%\Zomboid\console.txt`；
  - 在 UI 锁外完成文件 I/O/解析，短暂持锁应用 phase/progress/lines；
  - 首次切换写 `overlay.log`：「启动器状态陈旧，改为自读 console.txt」；完成时写原因。
- 回退只在没有 fresh 启动器状态时驱动 UI/停止；启动器恢复写心跳后重新以启动器为准。
- 增加 `STARTED`/first-frame 原子标志（hook 首次进入时置位），供回退的 idle 判定使用。

### 5. DLL 自行停止

修改 `overlay-dll/src/lib.rs`：

- 新增 `request_shutdown(reason)`：写日志后启动独立线程执行 `shutdown_overlay()`，不要在 UI 线程持 UI 锁时直接调用（`shutdown_overlay` 会 `ui::stop()` 再锁 UI，且会 sleep 300ms/摘 Hook）。
- 在 `initialize_overlay` 的启用 Hook 前检查 `SHUTTING_DOWN`，避免「自读提前完成 → 初始化随后又启用 Hook」的竞态。
- 保留现有 stop event watcher 和 600 秒安全上限；`SHUTTING_DOWN` 已保证两条停止路径幂等。
- UI 线程在发出 shutdown 请求后不再绘制；下一轮看到 `running=false` 退出。

### 6. 文档

- `docs/overlay-injection.md`：状态文件新增心跳/console/launcherpid/session；DLL 回退自读与自停；启动器关闭后的行为；排错看 `overlay.log` 的自读日志。
- `docs/architecture.md` 跨进程契约、`PROJECT_OVERVIEW.md` 相关段落同步。

## 测试与验收

- `cargo test`：`overlay-core`、`overlay-dll`、`src-tauri` 全量；
- `npm test`：现有 overlay 不变量、背景色、启动时长测试；新增状态文件心跳/DLL 自读源码契约测试；
- `npm run build:overlay` 重建 dev DLL；`npm run build` 构建前端；必要时 `npm run tauri:build`；
- 手工验证：
  1. 启动器打开时，覆盖层仍显示精确 advice 百分比；
  2. 注入后关闭启动器，覆盖层继续更新 phase/日志/估算进度，并在 `onGameInitComplete` 或主菜单前自行停止；
  3. `%TEMP%\overlay.log` 出现自读/自停日志，游戏不受影响；
  4. 启动器在 `[ZB]` 长时间静默时不会被误判为已死（心跳生效）。
- 新 DLL 需要重启游戏才会被加载；当前运行中的游戏仍使用旧 DLL。

## 风险与回滚

- 主要风险：共享 crate/两个 Cargo.toml 的构建、DLL 自停竞态、UI 锁顺序、心跳误判、console 路径自定义。对应措施：心跳独立于内容、I/O 在 UI 锁外、shutdown 独立线程、初始化前检查 SHUTTING_DOWN、状态文件保留 console/session。
- 出问题可回退 `backup/pre-overlay-selfread`；DLL 和 launcher 需成对回退/重建。

## 不做的事

- 不把 jar 注解扫描搬进 DLL；回退路径使用阶段估算，启动器存活时仍是精确 advice 进度。
- 不改现有「无裸背景帧」和换帧时间窗策略。
- 不删除状态文件协议；它现在同时承担配置/心跳/启动器数据通道。