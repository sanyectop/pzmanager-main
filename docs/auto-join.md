# 自动加入服务器（设置 › 实验性功能）

启动游戏后自动连入配置的服务器：游戏进主菜单后由**游戏自己的原生 `+connect` 链路**
弹出连接（地址/密码已预填），启动器的钩子 Lua 把「连接」自动点掉，全程零点击。
2026-11-14 重构：原生链路负责时机与健壮性，钩子只做"补一下点击"，失败时可降级为手点。

## 原理（逆向依据，本机 42.22.2）

B42 主菜单支持「启动时直连」——Steam 邀请走的 `+connect` 命令行链路：

- `zombie/gameStates/MainScreenState.enter` 把命令行参数 `+connect <ip:端口>` /
  `+password <密码>` 写进系统属性 `args.server.connect` / `args.server.password`；
- 主菜单 UI 就绪后游戏触发 `OnMainMenuEnter` → `LoadMainScreenPanelInt`
  （MainScreen.lua:1666）读属性（`getServerAddressFromArgs()` 读后即清，天然一次性）→
  `BootstrapConnectPopup:connect(ip, port, 密码)` 查询服务器 → 非 coop 服弹出
  `ServerConnectPopup`（地址、服务器密码、用户名都按收藏夹预填）——**等用户点「连接」**；
  coop 服则直接 `connectCoop`，无需弹窗。

原生链路差最后一下点击（用户名为空时按钮还被禁用）。启动器的处理分两半：

1. **启动器**（`auto_join.rs` + `lib.rs` 启动流程）：java 命令末尾追加
   `+connect 主机:端口`（密码非空时再追加 `+password 密码`）程序参数；
   Steam 方式则把它们写进 Steam 启动项。同时往 `Zomboid\Lua\` 写一次性标记
   `pzm-autojoin.ini`（**只含 `target=`，密码不落盘**——它经 `+password` 参数传给游戏）。
2. **钩子 Lua**（`media/lua/client/pzm_autojoin.lua`）：包一层
   `ServerConnectPopup:setServer`——弹窗预填完之后，若目标与本次启动的 ini 一致，
   补齐用户名（Steam 模式用 `getCurrentUserProfileName()` 兜底）并自动点一次「连接」
   （`onOptionMouseDown({internal="CONNECT"})`，与按钮点击同一分支），随即把 ini 改写成
   `consumed=1`（一次性，防反复触发）。

**降级保证**：钩子失败（游戏版本改动、Lua API 变化）只损失自动点击——原生弹窗照常
弹出、地址密码已预填，用户手点「连接」即可。连接失败、密码错误、Google 二次验证等
弹窗全是游戏原生逻辑，不绕过。

## 历史（为什么不是旧的钩子直调方案）

旧实现（≤2026-11-14）：钩子在 `OnGameBoot` 读配置武装，`OnFETick` 轮询主菜单条件后直接调
`ConnectToServer:connect(...)`。实测 B42.22 上武装成功后轮询永远不推进——连接不发起、
无任何输出（连诊断都不打），两处坑：

- 配置文件路径：B42 的 `cacheFileExists`/`getFileReader` 相对路径解析到 `Zomboid\Lua\`
  （字节码 + 磁盘双重验证），写 `Zomboid\` 根目录钩子读不到，静默不连（已修一版）；
- 武装后的轮询不知为何永远不满足条件——菜单时机、Lua 环境（前端/引导两个上下文）
  全靠猜，无法观测。

因此整体退役：现在原生链路负责时机，钩子只在一个**确定就绪**的时刻
（`setServer` 预填完成）做一件事，且每一步都有日志。文件名仍是
`media/lua/client/pzm_autojoin.lua` + `Zomboid\Lua\pzm-autojoin.ini`（target-only）。

## 文件与生命周期

| 文件 | 谁写 | 谁删 |
| --- | --- | --- |
| `<游戏>/media/lua/client/pzm_autojoin.lua`（钩子，源码 `src-tauri/lua/`） | 启动流程：功能开启时每次启动重写（内容一致跳过）；游戏更新清掉 media 后自愈 | 功能关闭且游戏未运行时，启动流程顺手删 |
| `%USERPROFILE%\Zomboid\Lua\pzm-autojoin.ini`（一次性标记：只含 `target=`；必须放 `Lua\` 子目录，见上文） | 启动流程：点「启动游戏」时写入 | ① 后台线程等游戏退出后删；② 钩子自动点击后立刻改写 `consumed=1` |

钩子是散装 game-dir Lua（游戏自动加载，无需启用模组）。`pzopt/` 目录是 PZ_Optimization
Workshop 工具的地盘，不要把钩子放进去——它的卸载脚本会连锅端。

## 启动器侧

- `auto_join.rs`：`normalize_auto_join_target`（`主机:端口`，host 段不允许任何空白，
  端口 1–65535 纯数字；启动器多接受一个格式，游戏里就多一次静默拒绝）、
  `auto_join_program_args` / `auto_join_redact_password`（+password 值在日志里掩码）、
  钩子安装/卸载（幂等）、标记写入/清理、`spawn_auto_join_cleanup`（先等游戏起来
  （5 分钟超时=启动失败，清标记防以后手动开游戏被抢连）再等退出；只走
  `game_running_quick` 进程快照，启动路径纪律见 shaderpack_config.rs 注释）。
- `settings.rs`：`save_auto_join_settings(enabled, target, password)`——开启时校验地址，
  关闭时**清空目标与密码**（settings.json 不留明文）并顺手清残留标记。
- `lib.rs` 启动流程：设置读取后算 `auto_join_target` → 拼 `+connect`/`+password` 参数
  （Steam 路径写启动项，游戏文件路径追加到主类之后，日志里 +password 值掩码）→
  装钩子 + 写标记 + 起清理线程；日志只记密码位数，不落明文。
- 前端：设置 › 实验性功能（`experimentalSettingsContent`），开关保存分两种情况：
  **关闭**开关即时保存；**打开**时若地址为空则**不落盘**，只提示"请填写服务器地址后点「保存设置」完成开启"，
  地址非空才即时保存。地址/密码走「保存设置」；关闭成功后前端同步清空输入框。

## 边界

- 只支持 `主机:端口`（与游戏主菜单直连一致），IPv6 不支持。
- 密码：优先级 = 收藏夹保存的 > 启动器配置的（原生 `setServer` 预填收藏的，
  `+password` 参数只在收藏没存时生效——参数写入发生在预填之后）。
- `-nosteam` 服务器：用户名取收藏夹条目（`setServer` 预填）；收藏里没有且非 Steam 模式
  时不自动点击（无法凭空补用户名），弹窗等用户补用户名手点——不会用错误身份硬连。
- 游戏大版本更新后：media 里的钩子被清掉，下一次从启动器启动自动重装；期间手动开游戏
  时弹窗照常出现（原生链路不依赖钩子），只是要手点一下。
- 版本隔离/还原版本等操作还原 game 文件后同理，自愈。
- 启动器中途被杀：残留标记由钩子 `consumed=1` 自消费 + 清理线程兜底，最坏情况是下次
  手动开游戏自动点一次连接。
