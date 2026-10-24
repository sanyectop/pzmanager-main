# pzmanager

Project Zomboid（僵尸毁灭工程）Windows 桌面启动器 + 模组管理器。基于 Tauri 2 + Rust + React 19 + TypeScript，负责 Steam 环境探测、启动链改写（内存上限 / ZombieBuddy agent 策略）、Lua 与 JAR 模组管理、Viewpoint 截图与光影包管理、创意工坊浏览与资源下载、开发者新闻翻译，以及游戏内加载覆盖层（注入 DLL + Hook OpenGL）。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | **从这里开始**：分层结构、前后端模块职责、Tauri 命令清单、磁盘与跨进程状态、测试与构建、已知技术债 |
| [docs/overlay-injection.md](docs/overlay-injection.md) | 覆盖层注入：注入链路、Hook 原理、GDI/GL 界面、停止条件、实测数据与排错表 |
| [docs/superpowers/](docs/superpowers) | 最初"路径探测"阶段的计划与设计（历史归档，已过时） |

## 快速开始

```powershell
npm install
npm run tauri:dev      # 开发（前端固定端口 10002）
npm run tauri:build    # 打包
```

只检查 Rust 后端：

```powershell
cd src-tauri; cargo check
```

构建覆盖层 DLL（需要 MSVC 的 `cl.exe`，MinHook 的 C 源码要编译）：

```powershell
cd overlay-dll; cargo build --release --target x86_64-pc-windows-msvc
```

## 测试

```powershell
cd src-tauri; cargo test      # Rust：解析、配置改写、事务回滚、注解扫描等
node --test tests/            # 前端：源码/配置契约断言（Node ≥ 22）
```

## 环境要求

* Windows x64（游戏、启动器、覆盖层 DLL 必须同为 64 位）
* Rust stable（默认注入后端即可；`dll-syringe` 后端需 nightly，见覆盖层文档）
* MSVC 工具链（仅构建覆盖层 DLL 时需要 C++ 组件提供 `cl.exe`）
