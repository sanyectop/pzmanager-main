# 模块拆分说明

本次拆分把 `App.tsx` 的顶层代码和 `lib.rs` 的领域逻辑移出单文件，行为保持不变。
拆分前完整快照在 `backup/pre-modularization` 分支，拆分工作在 `refactor/modularize` 分支。

## 前端

| 位置 | 职责 |
| --- | --- |
| `src/App.tsx` | 仍然保留 `App()`：状态、effects、IPC 处理器和页面内容编排 |
| `src/app/types.ts` | 前后端 DTO、页面状态、设置类型 |
| `src/app/constants.ts` | 选项、默认值、阈值、导航项、文案常量 |
| `src/app/utils.ts` | 纯格式化 / 解析 / 小工具函数 |
| `src/components/common.tsx` | `HintDot`、`EmptyState`、`PathValue`、`MediaPanel`、`ShotThumbnail`、`DeveloperNewsCard` |
| `src/components/tasks.tsx` | Toast、通知控制台、任务中心 |
| `src/components/workshop.tsx` | 工坊筛选、下拉、资源下载浏览器 |
| `src/components/backgrounds.tsx` | 背景缩略图与背景菜单 |
| `src/components/mods.tsx` | JAR/Lua 海报、右键菜单、折叠行、批量条、支持库行、ZombieBuddy 面板 |
| `src/components/version.tsx` | 版本时间线、版本记录弹窗、换盘弹窗 |
| `src/components/fastLaunch.tsx` | 极速启动缓存状态行与开启警告 |
| `src/components/shell.tsx` | 自绘标题栏与 `useDialogAutoHeight` |
| `src/components/confirm.tsx` | 应用内确认对话框（替代原生 `window.confirm`） |
| `src/components/crashReport.tsx` | 游戏异常停止报告：原因 + 诊断 + 底部可疑 JAR 模组列表 |
| `src/components/icons.tsx` | 通用小图标（外链箭头 ↗ 等） |
| `src/components/launchMusic.tsx` | 内置壁纸背景音乐播放器（`LaunchMusicProvider` + 启动页胶囊控件） |
| `src/components/pagination.tsx` | 统一翻页条 `Pagination`（页码规则在 `src/paginationUi.ts`） |
| `src/components/settingsCards.tsx` | 设置页分区卡片（标题在卡片外、行放白卡片里） |
| `src/components/shaderpackConfig.tsx` | Viewpoint 光影包配置弹窗（解析 `shaders.properties`，Iris 同款分页配置） |
| `src/components/snapSlider.tsx` | 拖动时视觉连续、松开后就近吸附的滑动条 `SnapSlider` |

规则：

- 新的纯类型 / 常量 / 工具函数直接放进 `src/app/`，不要再堆回 `App.tsx`。
- 新的独立展示组件放进 `src/components/`，通过 props 接收数据，不直接读 `App()` 的局部状态。
- 测试不再只读 `App.tsx`：统一从 [tests/sourceText.ts](../tests/sourceText.ts) 导入
  `appSource` / `rustSource`（拼接后的源码文本）。移动代码不需要再改一堆测试路径。

## Rust

`lib.rs` 现在只保留命令注册、启动流程、工坊/资源命令和应用初始化（约 3.7k 行）；
内联测试已移到 `tests.rs`（约 4.3k 行）。拆出的领域模块通过 `pub(crate) use module::*;`
回到 crate 根，因此：

- `tauri::generate_handler!` 里的命令名不用改；
- 测试里的 `use super::{...}` 不用改；
- 原有未限定调用不用改。

| 模块 | 职责 |
| --- | --- |
| `launch_options.rs` | Steam / JSON / BAT 启动参数解析与改写（纯函数） |
| `settings.rs` | 设置归一化、`settings.json` 读写、设置命令 |
| `steam.rs` | Steam 库、账号、Project Zomboid 路径探测 |
| `backgrounds.rs` | 自定义 / 工坊背景扫描、导入、读取 |
| `lua_mods.rs` | Lua 模组解析、启用顺序、启用/禁用 |
| `jar_mods.rs` | JAR 扫描、状态文件、ZombieBuddy 决策、模组预设 |
| `support.rs` | ZombieBuddy / PZ Optimization / Viewpoint 3D 检测与安装 |
| `fast_launch.rs` | 极速启动预扫描缓存与启动配置备份 |
| `viewpoint_media.rs` | Viewpoint 截图与光影包管理 |
| `fs_utils.rs` | 文件写入、事务式替换、备份 |
| `tests.rs` | 原 `lib.rs` 内联测试；仍在 crate 根作用域下运行 |

规则：

- 新命令优先放进对应领域模块，再在 `lib.rs` 的 `generate_handler!` 里注册。
- 模块内访问 crate 根的类型/常量/函数用 `use crate::*;`；跨模块不要写 `super::super::`。
- 模块里的 DTO 若被 `lib.rs` 或测试直接读字段，字段标 `pub(crate)`。
- 拆分是行为保持重构：先 `cargo check`，再 `cargo test --lib`，再 `npm test` / `npm run build`。

## 下一步

`App.tsx` 的 `App()` 仍有约 1.1 万行（11,645 行），主要是 257 个 `useState`（另有 62 处 `useRef`）、effects 和页面 JSX。
继续拆分时建议按功能抽 hook，而不是继续搬 JSX：

1. `useTaskCenter`：toasts / notifications / tasks。
2. `useDetection`：Steam 探测与安装状态。
3. `useWorkshop`：筛选、分页、详情、翻译任务。
4. `useModManagement`：Lua/JAR 列表、选择、拖拽、预设。
5. `useVersionIsolation`：设置 › 版本的保存/还原/历史状态。
6. `useSettings`：通用/启动/模组加载器设置。

每个 hook 自带状态、effects 和返回的操作函数；页面组件只接收它需要的那一块。
这一步需要逐块迁移并保留现有测试，不要一次性重写。
