# 覆盖层注入（第一阶段：注入 → Hook OpenGL → 纯红覆盖 → 自动恢复）

本阶段只验证一条链路是否打通：

```
游戏窗口出现 → 启动器自动注入 DLL → DLL Hook opengl32!wglSwapBuffers → 在游戏窗口内清屏为纯红
→ 启动器实时读取游戏控制台的 `[ZB]` 输出显示加载进度 → ZombieBuddy 加载完成后停止绘制 → 游戏画面恢复
```

不读取游戏进度、不绘制 UI 面板、不依赖 ZombieBuddy 或任何模组、不引入 hudhook/imgui/egui。

## 1. 代码位置

| 位置 | 作用 |
| --- | --- |
| `overlay-dll/` | 覆盖层 DLL 的独立 Cargo 工程（cdylib，Rust + MinHook） |
| `overlay-dll/src/lib.rs` | 全部 Hook 逻辑：`DllMain`、等待模块/导出、MinHook 安装、GL 绘制、停止事件与安全上限 |
| `overlay-dll/src/console_reader.rs` | 启动器心跳失效后的 `console.txt` 自读器（阶段/进度/自停） |
| `overlay-dll/src/ui.rs` | 游戏窗口内部的加载界面绘制：GDI DIB 渲染（换帧前 BitBlt 到游戏窗口 DC，换帧后上传 GL 纹理），状态来自 `%TEMP%\pz_overlay_status.txt` |
| `overlay-core/src/lib.rs` | 启动器与 DLL 共用的纯解析：阶段、完成标记、advice、进度估算、控制台行规范化 |
| `src-tauri/src/overlay.rs` | 注入实现：路径校验、进程轮询、重复注入检测、手动 `inject_overlay` 命令 |
| `src-tauri/src/overlay_session.rs` | 自动会话：等窗口 → 注入 → 实时解析 `[ZB]` 加载进度 → 完成后发出停止信号 |
| `src-tauri/src/fast_launch.rs` | 极速启动：预扫描缓存与启动配置备份 |
| `src-tauri/src/jar_mods.rs` | JAR 模组的 Java class 注解解析（`class_annotations`，配合 `lib.rs` 的 `count_jar_advice_methods` 统计注入方法数，作为进度分母） |
| `src-tauri/src/overlay_inject_win32.rs` | 默认注入后端（stable）：`CreateRemoteThread` + `LoadLibraryW` |
| `src-tauri/src/overlay_inject_dll_syringe.rs` | 可选注入后端（nightly）：dll-syringe |
| `src/overlayInjection.ts` | 前端调用封装 + React 按钮示例 |

## 2. 依赖与工具链要求

### DLL 工程（`overlay-dll`）

| 项 | 要求 |
| --- | --- |
| 目标 | `x86_64-pc-windows-msvc`（只支持 x64） |
| Rust | stable 即可（本机验证：1.99.0） |
| C 编译器 | **必须有** `cl.exe`：`minhook` 依赖通过 `cc` crate 编译 MinHook 的 C 源码。安装 Visual Studio 或 Build Tools 时勾选「使用 C++ 的桌面开发」 |
| 链接库 | `opengl32.lib`（Windows SDK 自带，`#[link(name = "opengl32")]` 由 MSVC 自动解析）；`kernel32` 等系统库同样由默认链接配置提供；无需手工配置 `LINK` 环境变量 |
| 版本锁定 | `minhook = "0.9.0"`、`windows = "0.62.2"`（feature 见 `overlay-dll/Cargo.toml`） |

### 启动器（`src-tauri`）

| 项 | 要求 |
| --- | --- |
| 目标 | `x86_64-pc-windows-msvc`（必须与游戏、DLL 位数一致） |
| Rust | stable（默认注入后端） |
| 依赖 | `windows = "0.62.2"`（默认后端）；`dll-syringe = "0.18.2"`（可选 feature，需要 nightly） |

## 3. 编译

### 3.1 编译覆盖层 DLL

```powershell
cd F:\pzmanager\overlay-dll
cargo build --release --target x86_64-pc-windows-msvc
```

产物路径：

```
F:\pzmanager\overlay-dll\target\x86_64-pc-windows-msvc\release\pz_overlay.dll
```

自检（可选，确认导出与导入正确）。`dumpbin` 来自 Visual Studio，请在「Developer PowerShell for VS」里执行，
或先找到它的完整路径（示例：`...\VC\Tools\MSVC\<版本>\bin\Hostx64\x64\dumpbin.exe`）：

```powershell
dumpbin /exports .\target\x86_64-pc-windows-msvc\release\pz_overlay.dll
# 期望：DllMain
dumpbin /imports .\target\x86_64-pc-windows-msvc\release\pz_overlay.dll
# 期望：OPENGL32.dll 里的 glClear / glClearColor / glPushAttrib / glPopAttrib / glDisable / glGetIntegerv / wglGetProcAddress
```

### 3.2 编译启动器

```powershell
cd F:\pzmanager
npm install            # 首次
npm run tauri:build    # 打包构建
# 或开发模式（tauri:dev 会先跑 npm run build:overlay 重建 DLL，
# 避免 dev 启动把旧副本复制到 exe 旁、改动"看起来没生效"）：
npm run tauri:dev
```

只想检查后端能否编译：

```powershell
cd F:\pzmanager\src-tauri
cargo check
```

### 3.3 切换注入后端（可选，需要 nightly）

`dll-syringe` 所有已发布版本（0.9 ~ 0.18.2）都带 `#![feature(...)]`
（`linked_list_cursors`、`once_cell_try`、`min_specialization` 等），**在 stable 工具链上直接编译失败**：

```
error[E0554]: `#![feature]` may not be used on the stable release channel
 --> dll-syringe-0.18.2\src\lib.rs:2:34
```

因此默认后端是等价的「原生 Win32 注入」（`CreateRemoteThread` + `LoadLibraryW`，与 dll-syringe 内部做法相同），
同时保留了 dll-syringe 后端供需要时启用：

```powershell
rustup toolchain install nightly
cd F:\pzmanager\src-tauri
cargo +nightly build --features dll-syringe-inject
```

两套后端对外接口一致（`inject_dll(pid, dll_path)`），切换后 `inject_overlay` 命令的行为、错误文案与前端调用方式都不变。
仓库内已验证：`rustc 1.101.0-nightly` + `dll-syringe 0.18.2` 下 `cargo +nightly check --features dll-syringe-inject` 整棵依赖树编译通过。

## 4. 前端调用

启动器内置了「覆盖层与 ZombieBuddy 加载进度」面板（启动页"启动游戏"卡片下方）：

* 点「启动游戏」后会自动调用 `start_overlay_session`：等游戏窗口出现 → 注入覆盖层 → 实时显示 `[ZB]` 加载进度；
* 面板上还有「立即开始覆盖层会话」（游戏不是从启动器启动时用）和「选择 DLL…」按钮；
* 默认 DLL 路径为开发构建产物 `../overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll`。

自动会话的停止条件（满足任一即通知 DLL 停止覆盖）：

> 前置条件：DLL 画出第一帧后会把 `Local\PZManagerOverlayStarted_<pid>` 事件置位，启动器**在收到该事件之前不会做任何"加载完成"判定**。
> 这是必须的：实测游戏窗口出现后要过 9~40 秒才出现第一次换帧，而 ZombieBuddy 在早期类加载阶段会有超过 30 秒的日志空档，不加这个前提就会在红色还没画出来时就把覆盖层关掉。
>
> ⚠️ **第二个前提（2026-10-16 修）**：`[ZB]` 静默**只对收尾阶段**（`注册 Lua 接口` / `初始化完成回调`）按 10 秒判定。
> ZB 在加载**中途**联网（`Authors list fetch`，实测静默 10~41 秒）和跑 ClassGraph 全量扫描期间同样没有 `[ZB]` 输出；
> 早先一律按 10 秒判定，会让覆盖层在游戏还没进主菜单时就被摘掉 —— 表现就是"没有加载过程，直接跳到加载完成"
> （实测：00:31 那次在 `checking mods ban status` 后静默 10.4 秒就被判定完成，覆盖层 27 秒就撤了，而游戏一直加载到 00:32:49）。
> 其它阶段的静默要等 **60 秒**才兜底结束（联网静默实测最长 41 秒，60 秒留余量）。

1. 控制台出现 `[ZB] Running callbacks for onGameInitComplete`：ZombieBuddy 的最后一个里程碑，
   在这里**立即**停止覆盖。42.22 的主菜单紧接着会播放 Bink 过场（console 里的 `binkId:`），
   等到主菜单标记再停会把覆盖层压在过场上，表现为加载完成时闪屏；
2. 控制台出现主菜单标记：`STATE: exit zombie.gameStates.TermsOfServiceState`、
   `added to the main menu`、`ZombieBuddy.getVersion()`（没有捕获到上一条时的兜底）；
3. `[ZB]` 输出静默：收尾阶段（`注册 Lua 接口` / `初始化完成回调`）10 秒（强信号，直接判定完成）；
   其它阶段 60 秒兜底，且额外要求 `console.txt` 已停更 ≥ 30 秒（`CONSOLE_QUIET_GUARD`，
   console 还在长 = 还在加载）；非里程碑阶段的兜底收尾 reason 明确写「不判定加载完成」；
4. 完全没有 `[ZB]` 输出时，注入后 3 分钟兜底；
5. **渲染看门狗**：DLL 每 500ms 把换帧速率回写到 `%TEMP%\pz_overlay_render.txt`。
   游戏持续渲染 ≥40 次/秒（hook 命中率，`RENDER_READY_RATE`，约等于 20fps 呈现）满 10 秒
   （`RENDER_READY_HOLD`）、而 `console.txt` 仍不更新（读不到 `[ZB]` 进度）时立即结束覆盖。
   有些机器游戏根本不写 `console.txt`，旧逻辑会一直盖到 3 分钟上限；
6. 注入后 9 分钟会话上限；
7. DLL 内部还有 10 分钟安全上限，避免启动器异常时一直保持红色。

### 4.1 启动器关闭后的自读回退

状态文件现在带 **1 秒心跳**（`beat=`），并写入 `console=<console.txt 路径>` 和
`session=<会话起点 ms>`。DLL 的行为：

* 启动器存活（heartbeat 在 3 秒内）时继续用启动器的阶段/精确 advice 进度；
* 启动器关闭、崩溃或心跳超时后，DLL 自己增量读取 `console.txt`，复用
  `overlay-core` 的阶段/完成/估算进度规则，不依赖启动器；
* 自读判定 `onGameInitComplete` / 主菜单标记 / `[ZB]` 静默 / 180 秒无输出后，
  从独立线程走正常 `shutdown_overlay()`：停界面线程、摘 Hook，游戏不受影响；
* 启动器恢复写心跳后，DLL 切回启动器数据；启动器的命名事件停止仍然有效；
* 回退进度是阶段估算（没有 jar advice 分母），启动器存活时仍是精确百分比。

```text
启动器存活：status file（beat/phase/progress/lines） ──► DLL
启动器关闭：console.txt ──► DLL 自读 ──► 自停
```

日志里会看到：

```text
[pid=…] 启动器状态陈旧/缺失，改为自读 console.txt。
[pid=…] 自读完成：ZombieBuddy 初始化完成回调结束，覆盖结束。
```

日志：

* DLL 自身写 `%TEMP%\overlay.log`（Hook 安装、停止信号、异常等）；
* 启动器把注入/停止记录以 `[PZManager]` 前缀追加到游戏控制台输出文件
  `%USERPROFILE%\Zomboid\console.txt`（追加写入，不覆盖游戏自身与 `[ZB]` 的输出）。

```ts
import { invoke } from "@tauri-apps/api/core";

// 启动器开发模式（npm run tauri:dev）的工作目录是 src-tauri，所以用相对路径指向 DLL 产物
const dllPath = "../overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll";

try {
  const message = await invoke<string>("inject_overlay", { dllPath });
  console.log(message); // 已向 ProjectZomboid64.exe（PID 12345）注入覆盖层 DLL：...
} catch (error) {
  // 后端返回的 Err(String) 会在这里变成异常信息，直接弹窗即可
  alert(String(error));
}
```

仓库里已封装好可直接使用的版本（`src/overlayInjection.ts`）：

```ts
import { DEV_OVERLAY_DLL_PATH, injectOverlay } from "./overlayInjection";

async function handleInjectOverlay() {
  try {
    const message = await injectOverlay(DEV_OVERLAY_DLL_PATH);
    showToast(message, "success");
  } catch (error: unknown) {
    showToast(error instanceof Error ? error.message : String(error), "error");
  }
}
```

对应按钮：

```tsx
<button type="button" onClick={handleInjectOverlay}>测试覆盖层</button>
```

## 5. 验证步骤

1. 在启动器启动页点击「启动游戏」（会自动开始覆盖层会话；也可以先启动游戏再点「立即开始覆盖层会话」）。
2. 观察游戏窗口：注入后加载界面在游戏窗口内部出现（背景色默认纯红，可由启动器设置自定义），
   面板上实时显示 `[ZB]` 加载阶段、行数与最近一行日志。
3. 预期结果：
   - 游戏窗口内出现加载界面，并**一直保持**到 ZombieBuddy 加载完成；
   - ZombieBuddy 加载完成后自动恢复正常画面，可继续操作；
   - 游戏不崩溃、无花屏、无残留红色。

### 5.0 本机已完成的实测结论（2026-10-10）

* **渲染路径已确认**：Project Zomboid B42 使用 LWJGL3 + GLFW；GLFW 的 WGL 后端调用的是 GDI 的
  `SwapBuffers`，实测该调用**会**进入 `opengl32!wglSwapBuffers`（120 帧 → Hook 命中 120 次），
  所以 Hook `wglSwapBuffers` 是正确目标，不需要额外 Hook `gdi32!SwapBuffers`。
* **端到端已确认**：对一个使用 `gdi32!SwapBuffers` 换帧的最小 OpenGL 宿主程序注入本 DLL：

  | 阶段 | 窗口采样平均 RGB | 结论 |
  | --- | --- | --- |
  | 注入前 | 59,79,128（深蓝） | 正常画面 |
  | 注入后 2 秒 | 229,52,56 | 红色覆盖生效 |
  | 注入后 11 秒 | 228,52,56 | 仍然保持红色（5 秒自动关闭已取消） |
  | 发出停止信号后 2 秒 | 57,79,128 | 收到停止信号后恢复、无残留 |
  | 二次注入 | 保持深蓝、进程继续渲染 | 未重复 Hook、未崩溃 |

  `%TEMP%\overlay.log` 对应输出：

  ```
  [pid=8644 t=0.001s] wglSwapBuffers Hook 已启用: target=0x... original=0x...
  [pid=8644 t=0.001s] 已监听停止事件 Local\PZManagerOverlayStop_8644
  [pid=8644 t=0.043s] 首次进入 wglSwapBuffers Hook（opengl32 路径，hdc=0x... ctx=0x...），注入链路已打通。
  [pid=8644 t=10.971s] 收到启动器停止信号，覆盖绘制结束。
  ```

### 5.1 确认 DLL 已经注入

* Process Explorer 打开游戏进程 → 切到 `View` → `Lower Pane` → `DLLs`，列表里应出现 `pz_overlay.dll`（右键 `Properties` 可看到完整路径）。
* 命令行等价检查（列出游戏进程里所有名字带 `overlay` 的模块）：

```powershell
Get-Process ProjectZomboid64 -ErrorAction SilentlyContinue |
  ForEach-Object { $_.Modules } |
  Where-Object { $_.ModuleName -like '*overlay*' } |
  Select-Object ModuleName, FileName
```

### 5.2 确认 Hook 已生效

DLL 会把关键节点写进 `%TEMP%\overlay.log`（例如 `C:\Users\<你>\AppData\Local\Temp\overlay.log`）：

```
[pid=12345 t=  0.002s] wglSwapBuffers Hook 已启用: target=0x... original=0x...
[pid=12345 t=  0.118s] 首次进入 wglSwapBuffers Hook（opengl32 路径，hdc=0x... ctx=0x...），注入链路已打通。
```

* 只有第一条：Hook 装上了，但游戏没有调用 `wglSwapBuffers`（见排查表）。
* 两条都有但仍不变红：说明绘制被 GL 状态挡住（scissor / FBO），或换帧时机被游戏覆盖。

### 5.3 验证重复注入

同一进程再次触发注入：

* 启动器侧检测到同名模块已加载 → 直接返回「跳过重复注入」；
* 即使绕过该检查（例如手动再次 `LoadLibrary`），DLL 内部还有两道护栏：`DLL_PROCESS_ATTACH` 一次性开关 + 同名模块计数，都会直接跳过再次 Hook。

预期：不崩溃、不叠加 Hook、不出现两次红色。

## 6. 实现要点（稳定性相关）

| 要求 | 实现 |
| --- | --- |
| Hook `wglSwapBuffers` | `GetModuleHandleA("opengl32.dll")` 循环等待 → `GetProcAddress("wglSwapBuffers")` 循环等待 → `MinHook::create_hook` + `enable_hook`，原始指针存入 `AtomicU64` |
| 不阻塞加载器 | `DllMain(DLL_PROCESS_ATTACH)` 只记录模块句柄、调用 `DisableThreadLibraryCalls`、然后起后台线程；所有等待都在线程里 |
| 停止机制 | 启动器创建命名事件 `Local\PZManagerOverlayStop_<游戏PID>`；DLL 用一个后台线程 `WaitForSingleObject`，收到信号后把 `AtomicBool` 置 false，回调只剩一次原子读；另有 10 分钟安全上限兜底 |
| 加载完成判定 | 启动器读取 `%USERPROFILE%\Zomboid\console.txt`：解析 `[ZB]` 行给出阶段与计数，命中 `onGameInitComplete` 立即结束；主菜单标记兜底；收尾阶段（`注册 Lua 接口` / `初始化完成回调`）静默 10 秒，其它阶段 60 秒兜底且要求 `console.txt` 停更 ≥30 秒（详见 §4 停止条件 3） |
| GL 状态恢复 | `glPushAttrib(GL_ALL_ATTRIB_BITS)` → 关 scissor → 必要时切回默认 FBO → `glClearColor` + `glClear` → 还原 FBO → `glPopAttrib()`，clear color 等属性由 `glPopAttrib` 完整还原 |
| 失败静默 | Hook 任何一步失败只写 `%TEMP%\overlay.log`，不弹窗、不打断游戏；加载失败不影响游戏运行 |
| 不 panic | 回调里只用原子操作与时间比较，无分配、无 `unwrap`；DLL 的 release profile 显式使用 `panic = "unwind"`，即使初始化线程 panic 也只死一个线程，不会 abort 游戏进程 |
| 不改动游戏数据 | 全程只调用 `glClearColor` / `glClear` / 属性栈，不触碰投影矩阵、纹理、着色器与游戏内存 |
| 加载界面绘制 | 界面全部画在游戏窗口内部（`overlay-dll/src/ui.rs`）：UI 线程每 40ms（`REFRESH_INTERVAL`）画一幅完整 DIB。换帧前把 DIB `BitBlt` 到游戏窗口 DC——仅在最近 500ms 没换帧且非真全屏时；换帧后同一张 DIB 上传为 GL 纹理随每帧绘制。铁律（`overlay-dll/src/lib.rs`）：界面图没就绪就一帧都不绘制，宁可放行游戏自己的首帧。背景色由启动器 `bgcolor=#RRGGBB` 自定义（默认纯红） |

### 6.1 游戏窗口内部的加载界面

加载界面**不再创建任何独立窗口**（早先 `src-tauri/src/overlay_ui.rs` 的独立分层窗口方案已废弃），
全部画面都画在游戏窗口内部，由 `overlay-dll/src/ui.rs` 实现：

* UI 线程每 40ms（`REFRESH_INTERVAL`）把整幅界面画进一张 GDI DIB：
  背景色（默认纯红，可由启动器经状态文件的 `bgcolor=#RRGGBB` 覆盖）+
  **左下角最新 6 行控制台输出**（去掉 `LOG : General f:0>` 前缀、连续重复行去重）+
  **底部居中的进度条与阶段文字/百分比**（例：`注入游戏类  55%`）；
* **游戏还没开始换帧**：窗口内容归 GDI 管，把 DIB `BitBlt` 到游戏窗口 DC。
  只在**最近 500ms 没有换帧**且非真全屏窗口时执行——游戏持续换帧时 GL 覆盖随每一帧出现，
  BitBlt 退场；真全屏窗口（options.ini `fullScreen=true` 且未开无边框）走翻转模型，
  GDI 写入不可靠，只让 GL 画（换帧前显示游戏自己的画面）；无边框窗口走 DWM 合成路径
  （与窗口模式相同），BitBlt 照常兜底；
* **游戏开始换帧后**：同一张 DIB 上传成 GL 纹理，在 `wglSwapBuffers` 回调里画满屏四边形；
* **铁律（`overlay-dll/src/lib.rs`）**：界面图没就绪就**一帧都不绘制**——裸背景帧一旦呈现，
  而游戏此后长时间不再换帧（实测有机器整个加载期只有 1 次换帧），屏幕上就是一块持续到主菜单的纯色，
  宁可放行游戏自己的首帧。

界面状态由启动器写入 `%TEMP%\pz_overlay_status.txt`（`overlay_session.rs` 的 `StatusFile`，
带 1 秒心跳），DLL 的 UI 线程定期读取；启动器关闭/心跳超时后，DLL 会自己读 `console.txt`
继续绘制并自停（见 §4.1）。进度来源就是控制台：`overlay_session` 实时 tail
`%USERPROFILE%\Zomboid\console.txt`，并叠加两条曲线：

1. **阶段估算（兜底）**：`set_phase_only` 把 `[ZB]` 行映射成阶段里程碑
   （`扫描模组列表` → … → `初始化完成回调`），作为阶段文字显示（不给出真实百分比）；
2. **真实注入进度（主）**：分母 = 运行期日志里 `[ZB] java mod list to load:` 列出的 jar
   （因此自动排除被禁用的模组）+ ZombieBuddy 自带 bundled patches，用启动器自己的 class 注解解析器
   （`count_jar_advice_methods`）统计声明的注入方法数；分子 = 日志里
   `[ZB] patching <目标> with N advice(s)` 的 N 之和。

   实测吻合度（同一次启动）：静态解析 **611** 条 advice（22 个 jar，含 ZB 自带 50），
   运行期 ΣN = **614**；609 个 patching 事件落在 361 个不同目标上（同一目标可能被重复转换）。
   分母在日志出现 jar 列表后由后台线程扫描得到，之后进度条直接显示
   `注入游戏类 · 注入 350/611` 这类真实计数，收尾前封顶 97%，加载完成时置 100%。

控制台行通过 `StatusFile::push_line` 实时进入界面队列。

进度条以**真实计数**为准：分母就绪后用 `set_precise_progress` 直接驱动（**允许回落**，
直接覆盖当前值），阶段估算只作为分母就绪前的兜底——否则会被偏高的阶段值顶住，
看起来像卡在 82%/88% 这类固定数字。
实测回放同一次启动：`1/360 → 101/360 → 202/360 → 302/360 → … → 345/360 ≈ 96%`，曲线连续。

关于 FBO：`glBindFramebuffer` 属于 GL 3.0，`opengl32.dll` 并不导出，因此代码在**首次回调时**（此刻一定有当前 GL 上下文）用 `wglGetProcAddress` 解析一次并缓存；解析不到就永久降级为「不动 FBO 绑定」，其余逻辑照常。

关于上下文类型：`glPushAttrib/glPopAttrib` 属于 GL 2.1 兼容接口。Project Zomboid（LWJGL 2）在 Windows 上使用的是兼容模式上下文，因此可用；
如果将来游戏切到 **core profile**，这两个调用会被驱动忽略（只产生 GL 错误，画面仍会变红，但状态栈不再生效），届时需要改成显式保存/还原 `GL_COLOR_CLEAR_VALUE`、`GL_SCISSOR_TEST`、`GL_SCISSOR_BOX`。

## 7. 常见失败原因排查

| 现象 | 可能原因 | 处理 |
| --- | --- | --- |
| 提示「等待游戏进程超时（60 秒）」 | 游戏还没启动 / 进程名不在候选列表 | 先看任务管理器：`ProjectZomboid64.exe` / `javaw.exe` 之外，**没有 Xbox 服务的机器上游戏可能显示为 wsappx（pz 是它的子进程）**——这种情况无需改动，启动器会按窗口标题 `Project Zomboid`（精确匹配）兜底识别进程并注入；真正的其它名字才需要加进 `src-tauri/src/overlay.rs` 的 `TARGET_PROCESS_NAMES` |
| 提示「远程 LoadLibraryW 返回空句柄」 | DLL 位数不符（32 位游戏）、路径含特殊字符、杀软拦截 | 确认用 `--target x86_64-pc-windows-msvc` 构建；把 DLL 放到普通目录（如 `D:\tools\pz_overlay.dll`）；给杀软加白名单 |
| 提示「打开游戏进程失败 / 拒绝访问」 | 游戏以管理员运行、启动器没有；或多用户/不同完整性级别 | 用相同权限级别运行两者（都普通身份或都管理员）；Steam 启动的游戏通常是普通身份 |
| 提示「枚举 PID x 的模块列表失败」 | 目标进程是 32 位（WOW64），或进程刚退出 | 该错误不会中断注入（代码会继续尝试注入并给出更准确的报错）；确认使用 64 位游戏 |
| 日志里没有「Hook 已启用」 | `opengl32.dll` 未加载（游戏还在 Java 启动阶段）或已在 60 秒超时 | 等到游戏主菜单/存档界面再注入；`WAIT_TIMEOUT` 可在 `overlay-dll/src/lib.rs` 调整 |
| 有「Hook 已启用」但没有「首次进入」 | 两条换帧路径都没命中（例如走其它 WGL/扩展路径） | 现在默认就会**同时** Hook `gdi32!SwapBuffers`（`install_gdi_swap_hook`，在 wgl Hook 启用后无条件安装）：部分 LWJGL 胶水直接调 GDI 的 SwapBuffers，这类机器上 opengl32 侧换帧数为 0。先确认 overlay.log 里有没有「首次进入 SwapBuffers Hook（gdi32 路径，…）」；两条路径都没有才需要排查其它呈现方式 |
| 进入回调但窗口没变红 | GL 状态挡住了 clear（scissor 已处理，剩余可能是多重采样/自定义 FBO），或换帧后画面被再次覆盖 | 看日志确认回调频率；确认游戏用的是默认帧缓冲；必要时在回调里临时 `glDisable(GL_MULTISAMPLE)` |
| 红色一直不消失 | 启动器没发出停止信号（会话线程异常/被杀） | 正常流程最多 9 分钟会由会话上限结束，DLL 自身还有 10 分钟安全上限；检查 `%TEMP%\overlay.log` 是否出现「收到启动器停止信号」 |
| 加载界面消失得太早 | 命中了主菜单标记或收尾阶段 `[ZB]` 静默 10 秒 | 见 `docs` 中 4 节的停止条件；阈值常量在 `overlay-core/src/lib.rs`（`COMPLETION_MARKERS` / `ZB_IDLE_TIMEOUT` / `ZB_IDLE_TIMEOUT_ANY_PHASE`），`overlay_session.rs` 只是引用 |
| 加载中段闪屏/左下角黑块（42.22 常见） | 加载屏开始连续换帧时帧间隔可能短到几毫秒。旧实现按「5ms 内同 HDC+上下文」去重，把**真正呈现到窗口**的独立帧当成重复帧跳过，游戏画面（含左下角黑块）漏出 | 现在**每一次 swap 都绘制**，只有「外层 wglSwapBuffers 调用的原始实现又走到 gdi32!SwapBuffers」这种同一次呈现的嵌套才跳过。心跳里的「换帧=」与「绘制=」应基本相等（差异只来自嵌套路径） |
| 红屏 + 一侧黑块（全屏） | 全屏窗口通常走 DXGI 翻转模型，GDI 的 `BitBlt` 写入不可靠（会被丢弃或只贴上一部分，表现为红底 + 黑块）。2026-11-11 用户反馈：42.21 全屏、加载期 GL 只换帧 6 次/41 秒，UI 纹理到 31 秒才首次上传，期间全靠 BitBlt | 真全屏窗口（options.ini `fullScreen=true` 且 `borderless!=true`）跳过 GDI BitBlt，只让 GL 覆盖绘制；换帧前显示游戏自己的加载画面，不再贴可能残缺的 GDI 图。overlay.log 会记一条「检测到真全屏窗口…」。**无边框窗口不跳过**：它虽也铺满显示器，但走 DWM 合成路径（与窗口模式相同），BitBlt 可靠——曾把无边框一并跳过，导致无边框下 GL 整个加载期只换帧 2-3 次、覆盖层完全不显示（2026-11-14 用户反馈，`blit_skip_for_fullscreen` + 每会话读一次 options.ini 修复）。 |
| 红色一直不消失（游戏已经能玩） | ① 游戏没有写 `console.txt`，所有 `[ZB]` 判定都失效；② `[ZB]` 静默兜底过长 | 新版有两条兜底：渲染看门狗（hook 命中持续 ≥40 次/秒满 10 秒但 `console.txt` 不更新 → 结束）和静默 60 秒兜底；启动器日志里有「覆盖层会话详情：ZB 行数/阶段/advice 分母/渲染速率」，先看 ZB 行数是否为 0 |
| 变红后花屏/渲染异常 | GL 状态或 FBO 绑定未还原 | 本实现用 `glPushAttrib/glPopAttrib` + 还原 FBO 绑定；若仍有问题，把 `%TEMP%\overlay.log` 与复现步骤一起记录，并临时把 `paint_red_frame` 里的 FBO 段落注释掉做对照实验 |
| 红屏且完全没有文字/界面 | 红色本身就是覆盖层底色（每帧 `glClear` 刷红），"无字"说明界面这一环断了。按可能性：① 进程里跑的是旧版"纯红探针"DLL（游戏目录残留 `-agentpath` 指向旧路径，宿主发现同名模块已加载就跳过注入）；② GL 残留状态把界面四边形整体丢弃（旧版未关深度测试/面剔除，或活动纹理单元非 0）；③ 游戏重建 WGL 上下文后旧纹理名失效；④ 界面线程 panic/锁中毒死亡、或找不到游戏窗口 | ① 问用户：红屏出现前有没有几秒能看到文字界面（有 = GL 环节问题，没有 = DLL/线程问题）；红屏是否超过 3 分钟不消失（是 = DLL 不响应停止事件，旧 DLL 嫌疑大）。② 收集：`%TEMP%\overlay.log`（新版会先写「pz_overlay v1.7.261114 已加载」，旧 DLL 没有这行）、`%USERPROFILE%\Zomboid\console.txt` 里 `[PZManager]` 行记录的 DLL 完整路径、游戏目录 `ProjectZomboid64.json` 的 `vmArgs[0]` 与启动 `.bat` 的 `_JAVA_OPTIONS` 里 `-agentpath` 指向。③ 校验安装目录 `pz_overlay.dll` 是否新版：`findstr /m "pz_overlay_status.txt" pz_overlay.dll` 有输出 = 新版；新版宿主注入旧 DLL 会直接报「覆盖层 DLL 版本过旧」，进程内预载路径不一致时界面会显示警告 |
| `error[E0554]: #![feature] ... stable release channel` | 启用了 `dll-syringe-inject` feature 但用的是 stable | 用 `cargo +nightly build --features dll-syringe-inject`，或去掉该 feature 使用默认后端 |
| `cargo build` 报找不到 C 编译器 | 缺少 MSVC C++ 工具链 | 安装 Visual Studio Build Tools 的「使用 C++ 的桌面开发」组件（提供 `cl.exe`） |

## 8. Crate 版本与签名适配说明

* `minhook = 0.9.0`
  * `MinHook::create_hook(target: *mut c_void, detour: *mut c_void) -> Result<*mut c_void, MH_STATUS>`
  * 内部自动完成 `MH_Initialize`（`Once`），失败会 `panic`，因此初始化线程里调用；回调中不触碰 MinHook API。
  * 若升级到把 `initialize()` 暴露为公开 API 或改成返回 `Result` 的版本，需要在 `initialize_overlay()` 中显式初始化并处理错误。
* `windows = 0.62.2`
  * `HMODULE` / `HANDLE` 是 newtype（`HMODULE(pub *mut c_void)`），`GetModuleHandleA` 返回 `Result<HMODULE>`，`GetProcAddress` 返回 `Option<fn() -> isize>`（`FARPROC`）。
  * `GetProcAddress` 是「失败返回 None」而不是 `Result`，代码里已按 `Option` 处理；若未来版本改为 `Result`，`wait_for_swap_buffers_export` / `inject_into_process` 需同步调整。
  * `CreateRemoteThread` 需要同时启用 `Win32_System_Threading` 与 `Win32_Security`（参数里引用 `SECURITY_ATTRIBUTES`）。
  * `WriteProcessMemory` 位于 `Win32::System::Diagnostics::Debug`，需要 `Win32_System_Diagnostics_Debug` feature。
* `dll-syringe = 0.18.2`（可选后端）
  * `Syringe::inject(&self, path) -> Result<BorrowedProcessModule, InjectError>`；`OwnedProcess::from_pid(u32)`、`Process::pid()` 已按 0.18 签名使用。
  * 需要 nightly（见 3.3）。

## 9. 回滚

覆盖层相关代码都在 git 历史里，需要回滚时用 `git log` 找到改动前的提交恢复对应文件；
删除 `overlay-dll/`、`src/overlayInjection.ts` 与本文档即可回到改动前状态。

## 附：强制跳过 ZombieBuddy 的模组审批界面

ZombieBuddy 用 `-agentlib:zbNative=<参数>` 的第一个参数决定策略（`Loader.setPolicy` 是 write-once，
重复 agent 会被 `Agent.premain` 忽略），可选值见 `doc/CommandLine.md`：

| 值 | 行为 |
| --- | --- |
| `prompt` | 默认：未知/变更过的 JAR 弹出确认界面，未确认前卡在 `Waiting for Java mods approval…` |
| `deny-new` | 静默跳过未批准的 JAR，不弹窗 |
| `allow-all` | 全部加载，跳过审批弹窗与 ZBS 校验 |

启动器按「模组加载器方案」决定是否写 agent（`lib.rs` 的 `mod_loader_policy_agent(scheme)`）：

* **方案 = `zombieBuddy`**：启动路径（`launch_project_zomboid`）把
  `-agentlib:zbNative=policy=allow-all` 写进**三个位置**：
  1. Steam `LaunchOptions`（`patch_steam_launch_options_at_paths`，带 `--` 结束符）；
  2. 游戏目录 `ProjectZomboid64.json` 的 `vmArgs`；
  3. 游戏目录 `ProjectZomboid64.bat` 的 `SET _JAVA_OPTIONS`。
* **方案 = `none`**：`mod_loader_policy_agent` 返回 `None`，启动路径**不写** agent，
  且会把启动参数里残留的加载器 agent 摘掉（`strip_loader_agent_tokens`）。

另有 `force_zombie_buddy_policy()`（无条件往 JSON/BAT 写 ZB agent），只在**保存内存上限**
（`settings.rs` 的 `save_memory_limit`）时调用——保证改内存时 JSON/BAT 里旧的 `-Xmx`
被覆盖、不残留旧 agent。因此只有 `zombieBuddy` 方案下，三处才会同时写上 agent。

之所以三处都要写：JVM 只会加载第一个 agent 并锁定策略，而游戏目录里的 JSON/BAT 往往先于 Steam 启动参数生效。

> ⚠️ `policy=allow-all` 是 ZombieBuddy 官方标注为"不推荐"的模式：所有 Java 模组（含未签名/来源不明的）都会被直接加载，
> 也不再做 ZBS 签名校验。Java 模组可以执行本机代码，请自行确认订阅内容可信。
> 若想保留校验、只是不想看到弹窗，可改用 `policy=deny-new`（静默跳过未批准的模组）。
