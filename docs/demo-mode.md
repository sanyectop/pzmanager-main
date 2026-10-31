# 演示模式（方案暂存，未实现）

设置 › 通用 新增「演示模式」开关。开启后，所有**会增删/修改文件**的操作一律走假数据，
不产生真实修改，但耗时与真实操作基本一致——用于演示与录屏。用户自身的配置修改
（启动器 settings.json、预设等）不受影响，仍然真实保存。

## 状态存储

- `AppSettings.demo_mode: bool`（settings.json 持久化）+ `save_demo_mode` 命令；
  前端设置页开关读写它。
- Rust 侧全局 `DEMO_MODE: AtomicBool`：前端在启动加载设置后与切换开关时调用
  `set_demo_mode(bool)` 同步；所有敏感命令在入口自行检查该标志，前端调用方不用传参。

## 覆盖范围（重点：版本管理页面）

| 操作 | 演示行为 |
| --- | --- |
| 完整保存 / 保存变化（游戏版本） | 合成 ProgressEvent（preparing → scanning → hashing → copying/cleaning），无文件读写；往内存演示版本链追加一条记录 |
| 还原版本 | 合成 restoring → pzopt → cleaning 进度；跳过 PZ Optimization 卸载/重装；不还原模组版本 |
| 检查当前版本 | 合成 hashing 进度，返回「完全一致」匹配 |
| 删除版本 | 演示版本链中移除记录，合成 deleting 进度 |
| 模组版本 保存/检查/还原/删除、整合包保存/还原 | 同上，走演示模组版本链与演示整合包列表 |
| get_version_status / get_mod_version_status / list_mod_version_packs / list_mod_version_targets | 版本与整合包数据来自演示内存状态；游戏路径与模组清单仍走真实探测（只读） |
| Lua 启用/停用/批量、保存顺序、自动排序 | 计算逻辑照跑（只读扫描），跳过 default.txt 写入与备份 |
| 光影包导入/删除 | 返回成功消息但不复制/不删除文件 |
| 启动器设置类（模糊度、字号、预设、版本存储目录草稿等） | **真实保存**（用户配置除外条款） |

## 耗时仿真

真实操作的分段耗时（实测数量级）：

- 完整保存 8–15s、保存变化 3–6s、还原 6–12s、检查 8–15s；
- 模组单个小版本 1–2s、一键保存全部 10–20s；
- 进度事件保持 ~120ms 间隔（与真实节流一致），各阶段按真实占比分配时长。

## 实现要点

- 演示版本链 / 演示模组版本链 / 演示整合包存 Rust 侧 `Mutex` 内存状态（本次运行有效，
  关闭应用即清空）；`get_version_status` 等读取命令在演示模式下改由演示状态拼装
  `VersionStatus`（结构体照抄，`currentMatch` 恒为与最新版本一致）。
- 合成进度复用 `spawn_version_operation_with` 的节流与事件通道（version-progress /
  mod-version-progress），前端任务中心 / 进度面板零改动。
- 演示模式下 `VERSION_OPERATION_ACTIVE` 互斥照常生效；进度回调忽略取消请求
  （与「还原前卸载 PZ Optimization」同理由：演示时长是固定的，中断没有意义）。
- 删除版本 / 删除模组版本也计入演示范围（见「覆盖范围」）。

## 状态

方案暂存，未实现。实现时从「设置 › 通用」开关开始，按覆盖范围表逐条接 demo 分支。
