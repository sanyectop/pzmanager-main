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
| `src-tauri/src/overlay.rs` | 注入实现：路径校验、进程轮询、重复注入检测、手动 `inject_overlay` 命令 |
| `src-tauri/src/overlay_session.rs` | 自动会话：等窗口 → 注入 → 实时解析 `[ZB]` 加载进度 → 完成后发出停止信号 |
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
# 或开发模式：
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
> 其它阶段的静默要等 **120 秒**才兜底结束。

1. 控制台出现主菜单标记：`STATE: exit zombie.gameStates.TermsOfServiceState`、
   `added to the main menu`、`ZombieBuddy.getVersion()`；
2. `[ZB]` 输出静默：收尾阶段（`注册 Lua 接口` / `初始化完成回调`）10 秒；其它阶段 120 秒兜底；
3. 完全没有 `[ZB]` 输出时，注入后 3 分钟兜底；
4. 注入后 9 分钟会话上限；
5. DLL 内部还有 10 分钟安全上限，避免启动器异常时一直保持红色。

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
2. 观察游戏窗口：窗口一出现就变红，面板上实时显示 `[ZB]` 加载阶段、行数与最近一行日志。
3. 预期结果：
   - 游戏窗口整体变纯红（RGBA = 1,0,0,1），并**一直保持**到 ZombieBuddy 加载完成；
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
  [pid=8644 t=0.043s] 首次进入 wglSwapBuffers Hook，注入链路已打通。
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
[pid=12345 t=  0.118s] 首次进入 wglSwapBuffers Hook，注入链路已打通。
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
| 加载完成判定 | 启动器读取 `%USERPROFILE%\Zomboid\console.txt`：解析 `[ZB]` 行给出阶段与计数，命中主菜单标记或 `[ZB]` 静默 10 秒即结束覆盖 |
| GL 状态恢复 | `glPushAttrib(GL_ALL_ATTRIB_BITS)` → 关 scissor → 必要时切回默认 FBO → `glClearColor` + `glClear` → 还原 FBO → `glPopAttrib()`，clear color 等属性由 `glPopAttrib` 完整还原 |
| 失败静默 | Hook 任何一步失败只写 `%TEMP%\overlay.log`，不弹窗、不打断游戏；加载失败不影响游戏运行 |
| 不 panic | 回调里只用原子操作与时间比较，无分配、无 `unwrap`；DLL 的 release profile 显式使用 `panic = "unwind"`，即使初始化线程 panic 也只死一个线程，不会 abort 游戏进程 |
| 不改动游戏数据 | 全程只调用 `glClearColor` / `glClear` / 属性栈，不触碰投影矩阵、纹理、着色器与游戏内存 |
| 换帧前的红色 | GL 的 `glClear` 必须等到第一次 `wglSwapBuffers` 才有意义，而游戏窗口出现后常要 10~40 秒才第一次换帧。DLL 启动时会另起一个线程，用 GDI（`GetDC` + `FillRect`）每 200ms 把游戏窗口客户区刷成红色，直到首次换帧（`FIRST_CALL_LOGGED`）或收到停止信号；首次换帧后自动交给 OpenGL 覆盖层，颜色无缝衔接 |

### 6.1 换帧前的预填充与后续加载界面

#### 加载界面（已实现）

`src-tauri/src/overlay_ui.rs` 会在游戏窗口上盖一个 **topmost + 点击穿透 + 不抢焦点** 的分层窗口，
画三样东西（实测截图确认）：

* 红色底：铺满游戏窗口客户区；
* **左下角：最新 6 行控制台输出**（去掉 `LOG : General f:0>` 前缀、单行截断到 120 字、连续重复行去重）；
* **底部居中：进度条 + 阶段文字/百分比**（例：`注入游戏类  55%`）。

进度来源就是控制台：`overlay_session` 实时 tail `%USERPROFILE%\Zomboid\console.txt`，
并叠加两条曲线（`OverlayUi::set_progress` 只前进不回退）：

1. **阶段估算（兜底）**：把 `[ZB]` 行映射成阶段里程碑（`扫描模组列表` → … → `初始化完成回调`），再按行数缓慢增长；
2. **真实注入进度（主）**：分母 = 运行期日志里 `[ZB] java mod list to load:` 列出的 jar
   （因此自动排除被禁用的模组）+ ZombieBuddy 自带 bundled patches，用启动器自己的 class 注解解析器
   （`count_jar_advice_methods`）统计声明的注入方法数；分子 = 日志里
   `[ZB] patching <目标> with N advice(s)` 的 N 之和。

   实测吻合度（同一次启动）：静态解析 **611** 条 advice（22 个 jar，含 ZB 自带 50），
   运行期 ΣN = **614**；609 个 patching 事件落在 361 个不同目标上（同一目标可能被重复转换）。
   分母在日志出现 jar 列表后由后台线程扫描得到，之后进度条直接显示
   `注入游戏类 · 注入 350/611` 这类真实计数，收尾前封顶 97%，加载完成时置 100%。

控制台行通过 `OverlayUi::push_line` 实时进入界面队列。

进度条以**真实计数**为准：分母就绪后只用 `已注入 advice / 总数` 直接驱动（`set_precise_progress`，
允许回落），阶段估算只作为分母就绪前的兜底——否则会被偏高的阶段值顶住，看起来像卡在 82%/88% 这类固定数字。
实测回放同一次启动：`1/360 → 101/360 → 202/360 → 302/360 → … → 345/360 ≈ 96%`，曲线连续。

另外，覆盖层**只在游戏窗口处于前台且未最小化时显示**（`GetForegroundWindow` + `IsIconic` 判断，
否则 `SW_HIDE`），切到其它窗口会自动隐藏，不会遮挡别的程序。

用独立分层窗口而不是注入绘制的原因：GL 的 `glClear` 只能作用在游戏自己的帧缓冲上，
而游戏"窗口已出现但还没换帧"阶段没有任何可绘制目标；分层窗口由 DWM 合成，**换帧前后都能显示**。
窗口位置每 100ms 跟随游戏窗口，并已设置 per-monitor DPI 感知（125%/150% 缩放下也能精确贴合）。
限制：游戏若使用**独占全屏**，分层窗口可能被挡住——此时 DLL 的 GL 红色仍然生效，只是不带文字/进度条。

实测（测试宿主刻意延迟 12 秒才第一次换帧）：

| 时刻 | 窗口采样 | GL 已换帧 | 说明 |
| --- | --- | --- | --- |
| 注入后 0.6s / 1.6s / 3.1s / 6s / 9.1s | 255,0,0 | false | **换帧前就是纯红**，GDI 每 200ms 刷新一次 |
| 注入后 11.1s | 255,0,0 | true | 首次换帧后由 GL 覆盖层接管，无闪烁 |
| 收到停止信号后 | 13,38,102 | — | 恢复游戏自身画面 |

`%TEMP%\overlay.log` 对应输出：

```
[pid=27364 t=0.001s] 窗口已出现但尚未换帧，先用 GDI 填充为红色。
[pid=27364 t=0.031s] wglSwapBuffers Hook 已启用: target=0x... original=0x...
[pid=27364 t=10.667s] 首次进入 wglSwapBuffers Hook，注入链路已打通。
```

要做"加载界面"，可以直接在这条 GDI 线程里画更多东西（`CreateFontW` + `DrawTextW` 写字、`CreateSolidBrush` + `FillRect` 画进度条），
不需要引入任何 UI 框架，也不会和游戏的 GL 渲染抢状态。进度数据可以由启动器传入（它已经在实时解析 `[ZB]` 输出），
最简单的做法是启动器把进度写进一个命名共享内存或 `%TEMP%\pz_overlay_status.txt`，DLL 的这条线程定期读取并重绘。
需要注意：一旦游戏开始换帧，窗口内容改由 GL 呈现，GDI 画的内容会被覆盖——所以"换帧前"是最容易做加载界面的窗口期；
若要在 GL 阶段继续显示文字/进度，需要走 GL 绘制或顶层分层窗口（下一阶段再定）。

关于 FBO：`glBindFramebuffer` 属于 GL 3.0，`opengl32.dll` 并不导出，因此代码在**首次回调时**（此刻一定有当前 GL 上下文）用 `wglGetProcAddress` 解析一次并缓存；解析不到就永久降级为「不动 FBO 绑定」，其余逻辑照常。

关于上下文类型：`glPushAttrib/glPopAttrib` 属于 GL 2.1 兼容接口。Project Zomboid（LWJGL 2）在 Windows 上使用的是兼容模式上下文，因此可用；
如果将来游戏切到 **core profile**，这两个调用会被驱动忽略（只产生 GL 错误，画面仍会变红，但状态栈不再生效），届时需要改成显式保存/还原 `GL_COLOR_CLEAR_VALUE`、`GL_SCISSOR_TEST`、`GL_SCISSOR_BOX`。

## 7. 常见失败原因排查

| 现象 | 可能原因 | 处理 |
| --- | --- | --- |
| 提示「等待游戏进程超时（60 秒）」 | 游戏还没启动 / 进程名不在候选列表 | 确认任务管理器里能看到 `ProjectZomboid64.exe` 或 `javaw.exe`；若为其它名字，把它加进 `src-tauri/src/overlay.rs` 的 `TARGET_PROCESS_NAMES` |
| 提示「远程 LoadLibraryW 返回空句柄」 | DLL 位数不符（32 位游戏）、路径含特殊字符、杀软拦截 | 确认用 `--target x86_64-pc-windows-msvc` 构建；把 DLL 放到普通目录（如 `D:\tools\pz_overlay.dll`）；给杀软加白名单 |
| 提示「打开游戏进程失败 / 拒绝访问」 | 游戏以管理员运行、启动器没有；或多用户/不同完整性级别 | 用相同权限级别运行两者（都普通身份或都管理员）；Steam 启动的游戏通常是普通身份 |
| 提示「枚举 PID x 的模块列表失败」 | 目标进程是 32 位（WOW64），或进程刚退出 | 该错误不会中断注入（代码会继续尝试注入并给出更准确的报错）；确认使用 64 位游戏 |
| 日志里没有「Hook 已启用」 | `opengl32.dll` 未加载（游戏还在 Java 启动阶段）或已在 60 秒超时 | 等到游戏主菜单/存档界面再注入；`WAIT_TIMEOUT` 可在 `overlay-dll/src/lib.rs` 调整 |
| 有「Hook 已启用」但没有「首次进入」 | 游戏换帧没有走 `opengl32!wglSwapBuffers`（例如直接走其它 WGL/扩展路径） | 少见的兜底方案：把 Hook 目标改为 `gdi32!SwapBuffers`（GDI 的 `SwapBuffers` 在 Windows 上会转发到 `wglSwapBuffers`，两者通常等价）；若确需改动，改 `wait_for_swap_buffers_export` 里的模块与导出名即可 |
| 进入回调但窗口没变红 | GL 状态挡住了 clear（scissor 已处理，剩余可能是多重采样/自定义 FBO），或换帧后画面被再次覆盖 | 看日志确认回调频率；确认游戏用的是默认帧缓冲；必要时在回调里临时 `glDisable(GL_MULTISAMPLE)` |
| 红色一直不消失 | 启动器没发出停止信号（会话线程异常/被杀） | 正常流程最多 9 分钟会由会话上限结束，DLL 自身还有 10 分钟安全上限；检查 `%TEMP%\overlay.log` 是否出现「收到启动器停止信号」 |
| 红色消失得太早 | 命中了主菜单标记或 `[ZB]` 静默 10 秒 | 见 `docs` 中 4 节的停止条件，可调整 `overlay_session.rs` 里的 `ZB_IDLE_TIMEOUT` / `COMPLETION_MARKERS` |
| 变红后花屏/渲染异常 | GL 状态或 FBO 绑定未还原 | 本实现用 `glPushAttrib/glPopAttrib` + 还原 FBO 绑定；若仍有问题，把 `%TEMP%\overlay.log` 与复现步骤一起记录，并临时把 `paint_red_frame` 里的 FBO 段落注释掉做对照实验 |
| 红屏且完全没有文字/界面 | 红色本身就是覆盖层底色（每帧 `glClear` 刷红），"无字"说明界面这一环断了。按可能性：① 进程里跑的是旧版"纯红探针"DLL（游戏目录残留 `-agentpath` 指向旧路径，宿主发现同名模块已加载就跳过注入）；② GL 残留状态把界面四边形整体丢弃（旧版未关深度测试/面剔除，或活动纹理单元非 0）；③ 游戏重建 WGL 上下文后旧纹理名失效；④ 界面线程 panic/锁中毒死亡、或找不到游戏窗口 | ① 问用户：红屏出现前有没有几秒能看到文字界面（有 = GL 环节问题，没有 = DLL/线程问题）；红屏是否超过 3 分钟不消失（是 = DLL 不响应停止事件，旧 DLL 嫌疑大）。② 收集：`%TEMP%\overlay.log`（新版会先写「pz_overlay v1.2.261026 已加载」，旧 DLL 没有这行）、`%USERPROFILE%\Zomboid\console.txt` 里 `[PZManager]` 行记录的 DLL 完整路径、游戏目录 `ProjectZomboid64.json` 的 `vmArgs[0]` 与启动 `.bat` 的 `_JAVA_OPTIONS` 里 `-agentpath` 指向。③ 校验安装目录 `pz_overlay.dll` 是否新版：`findstr /m "pz_overlay_status.txt" pz_overlay.dll` 有输出 = 新版；新版宿主注入旧 DLL 会直接报「覆盖层 DLL 版本过旧」，进程内预载路径不一致时界面会显示警告 |
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

本次改动前的代码已备份到 `backup/20261009-235243-overlay-inject/`（含 `src/`、`src-tauri/` 等，已排除 `target`、`node_modules`、`dist`）。
需要回滚时，用该目录覆盖回对应文件即可；删除 `overlay-dll/`、`src/overlayInjection.ts` 与本文档即可回到改动前状态。

## 附：强制跳过 ZombieBuddy 的模组审批界面

ZombieBuddy 用 `-agentlib:zbNative=<参数>` 的第一个参数决定策略（`Loader.setPolicy` 是 write-once，
重复 agent 会被 `Agent.premain` 忽略），可选值见 `doc/CommandLine.md`：

| 值 | 行为 |
| --- | --- |
| `prompt` | 默认：未知/变更过的 JAR 弹出确认界面，未确认前卡在 `Waiting for Java mods approval…` |
| `deny-new` | 静默跳过未批准的 JAR，不弹窗 |
| `allow-all` | 全部加载，跳过审批弹窗与 ZBS 校验 |

启动器现在会在**三个位置**都强制写 `-agentlib:zbNative=policy=allow-all`：

1. Steam `LaunchOptions`（`launch_project_zomboid` / `save_memory_limit` 写入，带 `--` 结束符）；
2. 游戏目录 `ProjectZomboid64.json` 的 `vmArgs`；
3. 游戏目录 `ProjectZomboid64.bat` 的 `SET _JAVA_OPTIONS`。

其中 2、3 由 `force_zombie_buddy_policy()` 在每次点击「启动游戏」时检查并改写（带 `.pzmanager-backups/<时间戳>/` 备份，内容已正确时不重复写）。
之所以三处都要写：JVM 只会加载第一个 agent 并锁定策略，而游戏目录里的 JSON/BAT 往往先于 Steam 启动参数生效。

> ⚠️ `policy=allow-all` 是 ZombieBuddy 官方标注为"不推荐"的模式：所有 Java 模组（含未签名/来源不明的）都会被直接加载，
> 也不再做 ZBS 签名校验。Java 模组可以执行本机代码，请自行确认订阅内容可信。
> 若想保留校验、只是不想看到弹窗，可改用 `policy=deny-new`（静默跳过未批准的模组）。
