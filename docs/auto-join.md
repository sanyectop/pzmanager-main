# 自动加入服务器（设置 › 实验性功能）

启动游戏后自动连入配置的服务器：游戏进主菜单的瞬间，替用户点一次直连弹窗的「连接」，
全程零点击（密码、Steam 中继开关都自动带上）。B42 42.22.2 实测可用。

## 原理（逆向依据，本机 42.22.2）

B42 主菜单本来就支持「启动时直连」——Steam 邀请走的 `+connect` 命令行链路：

- `zombie/gameStates/MainScreenState` 把 `+connect <ip:端口>` / `+password <密码>` 写进系统属性
  `args.server.connect` / `args.server.password`；
- `LuaManager$GlobalObject` 暴露成一次性 Lua API `getServerAddressFromArgs()` /
  `getServerPasswordFromArgs()`；
- `MainScreen.lua:1783` 在主菜单初始化时消费，经 `BootstrapConnectPopup:connect(ip, port, 密码)`
  → `ServerConnectPopup`（密码预填）→ **等用户点「连接」** → `ConnectToServer:connect(...)`。

原生链路差最后一击（要手动点连接，用户名空时按钮还被禁用）。本功能不抢 `+connect`
参数，改为**直接调用与「连接」按钮完全相同的 `ConnectToServer:connect(...)`**
（`ServerConnectPopup.lua:210` 的调用形态：`doHash=true, authType=1`），因此：

- Steam 模式下用户名无关，密码从游戏收藏夹（`getServerList()`，存 `db/ServerListSteam.db`）
  按 ip:端口 匹配取出；收藏里没存密码就用启动器配置里的；
- Steam 中继开关以收藏条目（`getUseSteamRelay()`）为准，收藏里没有按直连处理；
- 模组同步确认、密码错误、Google 二次验证等弹窗照常出现（不该静默的部分没绕过）。

## 文件与生命周期

| 文件 | 谁写 | 谁删 |
| --- | --- | --- |
| `<游戏>/media/lua/client/pzm_autojoin.lua`（钩子，源码 `src-tauri/lua/`） | 启动流程：功能开启时每次启动重写（内容一致跳过）；游戏更新清掉 media 后自愈 | 功能关闭且游戏未运行时，启动流程顺手删 |
| `%USERPROFILE%\Zomboid\Lua\pzm-autojoin.ini`（一次性配置：`target=` / `password=` / `requested=`；必须放 `Lua\` 子目录——B42 的 `cacheFileExists`/`getFileReader` 把相对路径解析到 `Zomboid\Lua\`，写 `Zomboid\` 根目录钩子读不到，只会静默不连） | 启动流程：点「启动游戏」时写入 | ① 后台线程等游戏退出后删；② 钩子读到后立刻改写成 `consumed=1`（自消费，启动器中途被杀也不会反复触发） |

钩子是散装 game-dir Lua（游戏自动加载，无需启用模组）。`pzopt/` 目录是 PZ_Optimization
Workshop 工具的地盘，不要把钩子放进去——它的卸载脚本会连锅端。

## 启动器侧

- `auto_join.rs`：钩子安装/卸载（幂等）、配置写入/清理、`normalize_auto_join_target`
  （`主机:端口`，与钩子 Lua 的解析**严格对齐**——host 段不允许任何空白，端口 1–65535
  纯数字；启动器多接受一个格式，游戏里就多一次静默拒绝）、`spawn_auto_join_cleanup`
  （先等游戏起来（5 分钟超时=启动失败，清配置防以后手动开游戏被抢连）再等退出；只走
  `game_running_quick` 进程快照，启动路径纪律见 shaderpack_config.rs 注释）。
- `settings.rs`：`save_auto_join_settings(enabled, target, password)`——开启时校验地址，
  关闭时**清空目标与密码**（settings.json 不留明文）并顺手清残留配置。
- `lib.rs` 启动流程：设置读取后算 `auto_join_target`，光影覆写之后装钩子 + 写配置 +
  起清理线程；日志只记密码位数（`describe_auto_join_password`），不落明文。
- 前端：设置 › 实验性功能 新分页（`experimentalSettingsContent`），开关即时保存、
  地址/密码走「保存设置」；关闭成功后前端同步清空输入框。

## 边界

- 只支持 `主机:端口`（与游戏主菜单直连一致），IPv6 不支持。
- `-nosteam` 服务器可用：收藏夹里存了账号密码就用（`getUserName()` / 服务端密码同上）。
- demo 模式下钩子主动放弃（与游戏原生 args-join 的 `isDemo()` 门槛对齐）。
- 游戏大版本更新后：media 里的钩子被清掉，下一次从启动器启动自动重装；期间手动开游戏
  只是失去自动连入，无副作用。
- 版本隔离/还原版本等操作还原 game 文件后同理，自愈。
