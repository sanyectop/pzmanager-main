# pzmanager

Project Zomboid（僵尸毁灭工程）Windows 桌面启动器 + 模组管理器。基于 Tauri 2 + Rust + React 19 + TypeScript，负责 Steam 环境探测、启动链改写（内存上限 / ZombieBuddy agent 策略）、Lua 与 JAR 模组管理、Viewpoint 截图与光影包管理、创意工坊浏览与资源下载、开发者新闻翻译，以及游戏内加载覆盖层（注入 DLL + Hook OpenGL）。

## 文档

| 文档 | 内容 |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | **从这里开始**：分层结构、前后端模块职责、Tauri 命令清单、磁盘与跨进程状态、测试与构建、已知技术债 |
| [docs/overlay-injection.md](docs/overlay-injection.md) | 覆盖层注入：注入链路、Hook 原理、GDI/GL 界面、停止条件、实测数据与排错表 |
| [docs/README-legacy.md](docs/README-legacy.md) | 早期 README 存档（内容已并入本文件） |

## 快速开始

```powershell
npm install
npm run tauri:dev      # 开发（前端固定端口 10002）
npm run tauri:build    # 打包（自动先构建覆盖层 DLL，再产出安装包）
```

只检查 Rust 后端：

```powershell
cd src-tauri; cargo check
```

## 发布打包

```powershell
npm run tauri:build
```

该命令会依次执行：

1. `npm run build:overlay` —— 构建 `pz_overlay.dll`（`cargo build --release --target x86_64-pc-windows-msvc`，需要 MSVC 的 `cl.exe` 编译 MinHook 的 C 源码）；
2. `npm run build` —— TypeScript 编译 + Vite 前端构建；
3. `tauri build` —— 编译 Rust 后端并产出安装包。

`pz_overlay.dll` 已通过 `tauri.conf.json` 的 `bundle.resources` 打进安装包（与 exe 同目录）。安装版中前端传出的开发期相对路径不存在，后端会按「exe 目录 + 文件名」兜底定位到打包的 DLL，覆盖层功能开箱即用。

安装包输出位置：`src-tauri/target/release/bundle/`（NSIS `.exe` 安装器）。便携版取 `src-tauri/target/release/` 下的 `pzmanager.exe` 与 `pz_overlay.dll` 打包 zip 即可。

## 测试

```powershell
cd src-tauri; cargo test      # Rust：解析、配置改写、事务回滚、注解扫描、覆盖层路径解析等
node --test tests/            # 前端：源码/配置契约断言（Node ≥ 22）
```

## 环境要求

* Windows x64（游戏、启动器、覆盖层 DLL 必须同为 64 位）
* Node.js ≥ 22、Rust stable（默认注入后端即可；`dll-syringe` 后端需 nightly，见覆盖层文档）
* MSVC 工具链（Rust 后端与覆盖层 DLL 均需要；C++ 组件提供 `cl.exe`）
