# 发布前检查 + 创建 F:\pzmanager-main GitHub 发布仓库

## 1. 修复 pz_overlay.dll 打包缺口(改 F:\pzmanager 源码)

这是唯一的代码修复(你选择了"修复"),其余垃圾文件按你的要求**不动主仓库**,只在 pzmanager-main 中排除。

- 读取 `src-tauri/tauri.conf.json`、`src/overlayInjection.ts`、`src-tauri/src/overlay.rs`,确认当前路径解析逻辑。
- 在 `tauri.conf.json` 增加 `bundle.resources`,把 `overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll` 打进安装包(映射为资源根目录下的 `pz_overlay.dll`)。
- 调整运行时 DLL 路径解析:发布版优先从 exe 同目录/资源目录读取,保留开发期相对路径作为 fallback;保持现有测试通过。

## 2. 完整发布前检查(在 F:\pzmanager)

- 前端:`npm install` → `npm run build`(tsc + vite)。
- 后端:`src-tauri` 内 `cargo test`(131 条)。
- 前端测试:`node --test`(36 条)。
- 打包:`npm run tauri:build`,确认产出安装包,并检查包内是否包含 `pz_overlay.dll`。
- 顺带核查:敏感信息扫描(API key 走环境变量,已确认无硬编码)、版本号(package.json / tauri.conf.json 均为 0.1.0,保持不变)。

## 3. 创建 F:\pzmanager-main 干净源码仓库

**包含**(白名单复制):`src/`、`src-tauri/`(不含 target)、`overlay-dll/`(不含 target、不含 `java/classes/*.class` 编译产物)、`public/`、`index.html`、`package.json`、单一锁文件(按实际使用的包管理器保留一个,解决双锁文件问题)、Vite/TS/样式配置、测试文件、`.gitignore`(补齐新增排除项)。

**排除**:`%SystemDrive%/`、`undefined/`、`node_modules/`、`dist/`、`src-tauri/target/`、`overlay-dll/target/`、`.claude/`、`AGENTS.md`、`docs/`(开发文档含 copy-backups)、`python/`(开发期辅助脚本)、`scripts/`、`pzmodsample/`、`UI_AUDIT_REPORT.md`。

**README**:保留现有文案不删除(遵循"改文案前先备份"的约定,原 README 备份到仓库内 `docs/` 下),在其基础上新增面向发布者/贡献者的构建与打包说明。

**git**:在 F:\pzmanager-main 中 `git init -b main` 并完成首次 commit(不配置远程,推送 GitHub 时你提供仓库地址即可;或届时用 gh CLI 创建)。

**校验**:在 pzmanager-main 中运行 `npm install` + `npm run build` 确认复制出的仓库可独立构建。

## 4. 交付物

- 发布检查报告(构建/测试结果、安装包产物路径、DLL 是否已打入、遗留问题清单——如主仓库未清理的垃圾文件、无 LICENSE 文件、无 CI 等)。
- F:\pzmanager-main:可直接推 GitHub 的干净源码仓库。

主仓库 F:\pzmanager 除第 1 步的 DLL 修复外不做任何改动(垃圾文件、暂存区、零提交状态均保持原样)。