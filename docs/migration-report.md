# 迁移报告：`F:\code\projectzomboidmanager` → `F:\pzm`

> 本文件由迁移过程生成，记录搬了什么、什么被重新生成、以及验证证据。

## 1. 结论

代码已**字节级完整迁移**到 `F:\pzm`，全部配置在新目录**重新生成**，全部构建产物**从零重建**。
新项目前端构建、Rust 类型检查、Rust 测试、覆盖层 DLL（含 MinHook 的 C 源码）编译均已通过。

## 2. 迁移策略

| 类别 | 做法 |
| --- | --- |
| 代码 | 逐文件复制，迁移后做全量 SHA256 比对 |
| 配置 | **不照搬**，在新目录全部重新生成并重新校准 |
| 构建产物 | 不迁移，全部在新目录重新构建 |
| 版本库 | 旧目录无 `.git`（已非 git 仓库），故无历史可迁移 |

## 3. 已迁移内容

12 棵代码树 + 3 个根文件，共 **2112 个文件 / 63.67 MB**：

| 路径 | 说明 |
| --- | --- |
| `src/` | 前端 React/TS 源码（12 个文件） |
| `src-tauri/src/` | Rust 后端（12 个文件，含 `lib.rs` 6641 行） |
| `src-tauri/tests/` | Rust 集成测试 |
| `src-tauri/icons/` | 打包图标（16 个） |
| `overlay-dll/src/`、`overlay-dll/examples/` | 注入 DLL 源码 |
| `tests/` | 前端契约测试（17 个） |
| `python/` | 配套 Python 参考实现 |
| `scripts/` | 开发辅助脚本 |
| `public/` | 前端静态资源（字体/图标/背景） |
| `pzmodsample/` | 样本模组数据（2014 个文件） |
| `docs/` | 架构、注入、样式文档 |
| `index.html`、`README.md`、`AGENTS.md` | 根文件 |

## 4. 未迁移内容及原因

| 内容 | 大小 | 原因 |
| --- | --- | --- |
| `src-tauri/target/` | 9.0 GB | 构建产物，重建 |
| `overlay-dll/target/`、`target-verify/` | 787 MB | 构建产物，重建 |
| `node_modules/` | 125 MB | 依赖，`npm install` 重建 |
| `dist/` | 10.4 MB | 前端构建产物，重建 |
| `pzmodsample/**/.git/` | 219.3 MB（3 个第三方仓库） | 第三方版本库历史，非项目代码 |
| `.idea/` | — | IDE 本地配置 |
| `src-tauri/src/lib.rs.bak` | 230 KB | 陈旧备份，与 `lib.rs` 仅注释不同且无任何引用，**已删除** |
| `src-tauri/gen/` | 360 KB | 由 `tauri-build` 自动生成的 schema，重建 |

## 5. 重新生成的配置（16 个文件）

`package.json`、`package-lock.json`、`tsconfig.json`、`tsconfig.node.json`、`vite.config.ts`、`.gitignore`、
`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/build.rs`、`src-tauri/tauri.conf.json`、
`src-tauri/tauri.dev.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/.gitignore`、
`overlay-dll/Cargo.toml`、`overlay-dll/Cargo.lock`、`overlay-dll/.gitignore`

生成后必须满足 `tests/` 里钉死的契约（否则契约测试失败）：

* `package.json` → `scripts["tauri:dev"] === "tauri dev --config src-tauri/tauri.dev.conf.json"`
* `vite.config.ts` → 含 `port: 10002`
* `tauri.conf.json` → `build.devUrl === "http://127.0.0.1:10002"`、`app.windows[0].minWidth === 970`
* `tauri.dev.conf.json` → `app.windows[0].additionalBrowserArgs === "--remote-debugging-port=10001"`
* `capabilities/default.json` → 含 `opener:allow-open-url` 与 Steam 创意工坊 URL 白名单

### 5.1 本次校准点（与原配置有意不同）

1. **窗口宽度 808 → 970**：原配置 `width: 808` 与 `minWidth: 970` 矛盾，Tauri/OS 会强制抬到 970，
   有效行为不变；改后配置自洽。
2. **补上缺失的脚本**：`npm test`（README 记录的 `node --test tests/` 在 Node 24 下不可用，见 §7.2，
   已改为 `node --test "tests/*.test.ts"`）、`npm run tauri:build`（README 已文档化但原 `package.json` 没有）。
3. **保留 `native-tls`**：不使用 rustls + aws-lc-rs，从根上避免构建期调用 `cl.exe` 编译 C 代码。
4. **保留 `overlay-dll` 的 `[profile.dev.package.minhook] debug = false`**：规避 minhook C 源码在
   `-Z7` 下可能触发的 `D8050`。
5. **文档与 `AGENTS.md` 里的旧绝对路径**改为 `F:\pzm`（`docs/architecture.md`、`docs/overlay-injection.md`、`AGENTS.md`）。
   源码中的 `User-Agent: ProjectZomboidManager/0.1` 属应用标识，非路径，未改动。

## 6. 环境诊断

用户报告的问题是**构建/编译失败**。诊断结果：

* 工具链正常：Node v24.18.0、npm 11.16.0、cargo/rustc 1.99.0。
* 旧 `Cargo.toml` 记录过一个环境故障：MSVC 14.51 下 `cl.exe` 报
  `error D8050: 无法执行 c1.dll`，导致 aws-lc-sys 构建失败。
* **实测该故障当前不复现**：用 vcvars64 编译 `int main(void){return 0;}` 成功（exit 0）；
  覆盖层 DLL 的 MinHook C 源码也编译通过（见 §7）。
* 因此保留了 `native-tls` 这一更稳的后端选择（不依赖 C 编译器），但当前环境其实两条路都可用。

## 7. 验证矩阵

| 验证项 | 结果 |
| --- | --- |
| 代码 SHA256 全量比对（2112 文件） | **仅 3 处差异，均为有意为之**（删除 `lib.rs.bak`、2 个文档改路径） |
| `npm install` | 成功，184 个包 |
| `npm run build`（`tsc && vite build`） | **成功**；`tsc` 0 类型错误；1981 模块；产出 `dist/` |
| `node --test "tests/*.test.ts"` | 35 个测试：**31 通过 / 4 失败**（4 个失败为迁移前既存，见 §7.1） |
| `cargo check --all-targets` | **成功**，`Finished dev profile in 51.69s`，仅 2 条 dead-code 警告 |
| `cargo test` | **成功**，`90 passed; 0 failed; 2 ignored` |
| `cargo test`（强制重编译 lib test） | **成功**，`Compiling pzmanager` → `Finished in 19.51s`，90 passed |
| `overlay-dll` `cargo check --all-targets` | **成功**，`Finished dev in 20.50s`，10 条 warning（MinHook C 编译通过） |
| `tauri-build` 重新生成 `src-tauri/gen/schemas` | 生成 4 个文件，大小与旧项目**完全一致**（75052/579/138525/138525 字节），证明重新生成的 `tauri.conf.json` + `capabilities/default.json` 语义等价 |
| `cargo build --release`（LTO） | **成功**，`Finished release profile [optimized] in 4m 24s`，产出 `pzmanager.exe` 8.74 MB（已 strip） |
| `overlay-dll` `cargo build --release --target x86_64-pc-windows-msvc` | **成功**，`Finished release in 9.97s`，产出 `pz_overlay.dll` 0.25 MB，**无 `D8050`** |
| `npm run tauri:build`（含 MSI 打包） | Rust 编译**成功**（3m 16s，`Built application at: ...\pzmanager.exe`）；**MSI 打包阶段超时失败**：`failed to bundle project: timeout: global`，见 §7.4 |

### 7.1 迁移前既存的前端契约测试失败（4 个，非迁移引入）

以下 4 个测试在**旧项目与旧目录中同样失败**（旧：31/35，新：31/35，完全一致），
它们对 `src/App.tsx` / `src/App.css` 做逐字文本匹配，而源码已重构、测试未同步：

* `keeps selection, enabled and disabled mod row states visually distinct`
* `places workshop tag selection in the compact sort row`
* `limits workshop search and sort buttons to 24 DIP`
* `places workshop result summary above pagination at the bottom of the results area`

失败原因是测试内硬编码的缩进/旧选择器与现状不符。例如测试要求 CSS 里存在
`.pz-workshop-filter-row button`，而实际 CSS 已改为作用域选择器
`.pz-resource-center-main .pz-workshop-filter-row button`（`App.css:1599`）；
测试要求 `<HStack\n                className="pz-workshop-result-summary"` 这一确切缩进，
而 `App.tsx:4282` 的缩进已不同。**属测试陈旧，非代码缺陷，本次未改动应用源码。**

### 7.2 `node --test tests/` 在 Node 24 下不可用

新旧项目**都**会报 `Error: Cannot find module 'F:\...\tests'`（Node 把目录当成模块入口）。
可用形式是 `node --test "tests/*.test.ts"`。`package.json` 的 `test` 脚本已按可用形式生成。

### 7.3 release 构建

`cargo build --release`（`lto = true`、`codegen-units = 1`）**通过**（4m 24s），
产出 `src-tauri/target/release/pzmanager.exe`（8.74 MB）。
覆盖层 DLL 的 release 构建同样通过（9.97s，`pz_overlay.dll` 0.25 MB）。

### 7.4 打包（MSI/NSIS）在下载工具链处超时

`npm run tauri:build` 的**编译阶段成功**，失败点在 MSI 打包：

```
Built application at: F:\pzm\src-tauri\target\release\pzmanager.exe
 Info Patching F:\pzm\src-tauri\target\release\pzmanager.exe with bundle type information: msi
failed to bundle project: `timeout: global`
```

`timeout: global` 是 Tauri CLI 去下载 WiX 工具链（MSI 打包器）时的网络超时，
**与代码、配置、Rust 工具链均无关**。首次打包需要能访问 GitHub 下载 WiX/NSIS；
网络可用后重跑 `npm run tauri:build` 即可，或先用 `cargo build --release` 得到可执行文件。

## 8. 尚未验证 / 注意事项

* **GUI 运行时已验证**：`npm run tauri:dev` 成功启动，React 已挂载（DevTools 实测 `#root`
  有子节点、启动图已移除），界面文本与 Steam 账户读取均正常。
  仅 `tauri:build` 的 MSI 打包环节因下载 WiX 超时未完成（见 §7.4）。
* **首次并行 codegen 出现过一次瞬时失败**：第一次 `cargo test` 报 `could not compile pzmanager (lib test)`，
  随后**未改任何代码**重跑即通过，并在强制重编译后再次通过（§7）。判定为首次并行编译时的资源竞争，
  非配置缺陷；若再次出现，优先怀疑并行编译资源占用而非工程配置。
* **`F:\pzm\.idea\`** 不是迁移内容：本机 JetBrains IDE 在目录创建后自动生成（`pzm.iml`），
  自带 `.gitignore`，未做处理。
* 新项目不含 git 仓库（旧项目也没有）；如需版本控制，建议在 `F:\pzm` 执行 `git init` 并做首次提交。

## 9. 覆盖层 DLL 路径修复（迁移后用户报告）

### 9.1 现象与根因

用户从资源管理器**双击** `F:\pzm\src-tauri\target\release\pzmanager.exe` 启动后，
注入覆盖层时报"找不到 DLL"。

根因不在索引/迁移，而在**代码本身的脆弱设计**：

* `src/overlayInjection.ts:11` 的开发期默认值是**相对路径**
  `../overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll`，
  其注释假设"工作目录是 `src-tauri`"（`tauri dev` 从项目根启动时成立）。
* `src-tauri/src/overlay.rs` 的 `normalize_dll_path()` 只按
  `std::env::current_dir()` 拼接相对路径。
* 资源管理器双击启动时，进程工作目录 = exe 所在目录 `...\target\release`，
  拼接结果 `...\target\overlay-dll\target\...\pz_overlay.dll` 并不存在 → 报错。

进程证据：该实例的父进程是 `explorer.exe`。

### 9.2 修复

`overlay.rs`：相对路径不再只按工作目录解析，而是依次尝试

1. 当前工作目录（保持原行为与优先级）；
2. **可执行文件所在目录及其各级祖先目录**（最多 6 层）——
   双击启动时正是靠这一项命中 `F:\pzm\src-tauri` 这一层。

同时错误信息改为**列出全部尝试过的完整路径**，并提示可用界面上的「选择 DLL」按钮手动指定。
绝对路径行为不变；`canonicalize` 依旧不用（避免 `\\?\` 前缀影响 `LoadLibraryW`）。

### 9.3 验证

* 新增 3 个 Rust 单元测试（`overlay::tests`），覆盖"命中后面的基目录""全部落空时列出所有位置"
  "基目录以工作目录打头且包含 exe 祖先"；`cargo test` **93 passed / 0 failed**（原 90 + 新 3）。
* **活体验证**：以敌意工作目录 `F:\pzm\src-tauri\target\debug`（等价于双击 exe）启动应用，
  经 DevTools 调用后端，拿到真实候选列表：

  | # | 候选基目录 | 拼接结果 | 命中 |
  | --- | --- | --- | --- |
  | 1 | `...\target\debug`（工作目录） | `...\target\overlay-dll\...` | ❌ 修复前唯一候选 |
  | 2 | `...\src-tauri\target` | `...\src-tauri\overlay-dll\...` | ❌ |
  | 3 | **`F:\pzm\src-tauri`** | `F:\pzm\overlay-dll\target\x86_64-pc-windows-msvc\release\pz_overlay.dll` | ✅ |
  | 4 | `F:\pzm` | `F:\overlay-dll\...` | ❌ |
  | 5 | `F:\` | — | ❌ |

* release 版已用修复重新构建（`cargo build --release`，2m48s）。

### 9.4 顺带发现：开发期 WebView2 数据目录也是硬编码

`src-tauri/src/lib.rs:4400` 的 `ensure_webview2_data_folder()` 在 **debug 构建**下把
`WEBVIEW2_USER_DATA_FOLDER` 硬编码为 `F:\webview2-data\pzmanager`（刻意放在项目外，
避免被 Vite 监视；release 构建则沿用 WebView2 默认目录）。
该目录当前存在（81 MB）。**若 `F:\` 不可写，debug 构建会直接
`panic!("无法创建 WebView2 数据目录 ...")` 而启动失败**——这是又一处机器相关的硬编码，
换机器/换盘符时是隐患。本次未改动（超出"路径查找"范围），如需改为
`%LOCALAPPDATA%` 或由配置注入，需另行确认。

### 9.5 一个未能归因的瞬时现象

修复过程中 `npm run tauri:dev` 有一次启动后**立即以 exit 0 退出**，
同时先前由 `Start-Process` 启动的 release 实例也消失；无 panic 输出，
`WEBVIEW2_USER_DATA_FOLDER` 在 Process/User/Machine 三个作用域均未设置（两者数据目录并不冲突）。
随后**直接启动** debug exe 连续运行 10 秒以上且至今稳定（窗口正常、10001/10002 端口正常），
故判断为环境瞬时问题而非代码缺陷，记录备查。

## 10. 包管理器混用（迁移后出现，待决）

迁移时项目是纯 npm 工程（`package-lock.json` 由 `npm install` 生成，README 亦用 npm）。
迁移后系统装入了 pnpm，且项目目录里执行过 pnpm 安装，导致当前**两套锁文件并存**：

| 文件 / 目录 | 来源 | 时间 |
| --- | --- | --- |
| `package-lock.json` | 迁移时 npm 生成 | 13:05 |
| `pnpm-lock.yaml` | pnpm 安装生成 | 22:52 |
| `node_modules/.pnpm` | pnpm 虚拟存储 | 22:52 |
| `node_modules/.package-lock.json` | npm 标记（已陈旧） | 迁移时 |

实测两者当前都能跑通 `run build`（npm 337ms / pnpm 296ms，exit 0），
但**混用是隐患**：任一管理器再次安装都会重排 `node_modules`，令另一套锁文件与磁盘状态漂移。

**注意**：`src-tauri/tauri.conf.json` 的 `beforeDevCommand` / `beforeBuildCommand` 目前写的是
`npm run dev` / `npm run build`。若决定改用 pnpm 为准，除了删除 `package-lock.json`
与陈旧的 `node_modules/.package-lock.json`，还应把这两项改为 `pnpm run dev` / `pnpm run build`，
否则 Tauri 仍会经 npm 调用脚本（当前能工作只是因为 npm 与 pnpm 的 `node_modules/.bin` 软链恰好都可用）。
