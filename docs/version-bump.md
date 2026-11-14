# 版本号修改清单（发版必读）

> 最近一次适用：`1.7.261114-devbuild.1794501007 → 1.7.261114`（2026-11-14，正式版）。
> 以后每次改版本号，按本文档逐项过一遍，最后用文末的校验命令确认没有漏网之鱼。

## 版本命名规则

- 稳定版格式：`{主版本}.{次版本}.{YYMMDD}`，例如 `1.6.261108`（2026-11-08 构建）。
- 开发期内测格式：`{主版本}.{次版本}.{YYMMDD}-devbuild.{buildId}`，例如
  `1.7.261114-devbuild.1794501007`（buildId 是时间戳类数字，标识「这一次编译」）。
  devbuild 只用于开发期分发，发布正式版时去掉 `-devbuild.{buildId}` 后缀即可。
  四个版本文件（package.json / tauri.conf.json / 两个 Cargo.toml）始终写同一个完整版本号
  （含 devbuild 后缀）。
- **配置文件里不带 `v` 前缀**（package.json / Cargo.toml / tauri.conf.json 都是裸 semver）；
  **`v` 前缀只出现在展示层**：关于页徽标用 `parseDevBuildVersion(appManifest.version)`
  （`src/versioning.ts`）拆出 base，显示 `v{base}`，内测版再附一个独立的
  `DevBuild {buildId}` 徽标（`src/App.tsx` 的 `APP_DEV_BUILD`）；发布页文案、DLL 日志首行
  则写完整版本号。

## 一、必须手改的 7 处

| # | 文件 | 位置 | 说明 / 谁消费它 |
| --- | --- | --- | --- |
| 1 | `package.json` | `"version"` | **关于页显示 `v{base}`**（内测版另有紫色 `DevBuild {buildId}` 徽标）：`src/App.tsx` 顶部 `import appManifest from "../package.json"`，经 `parseDevBuildVersion` 拆分后在设置 › 关于渲染。也是 npm 包版本 |
| 2 | `src-tauri/tauri.conf.json` | `"version"` | **Tauri 打包版本**：NSIS 安装包的产品版本、`PZManager_{version}_x64-setup.exe` 产物文件名 |
| 3 | `src-tauri/Cargo.toml` | `version` | `pzmanager` Rust 包版本（应与 tauri.conf.json 保持一致）。**启动器「检查更新」用它做本地版本号**（`src-tauri/src/update.rs` 的 `env!("CARGO_PKG_VERSION")`，内测 devbuild 时经 `parse_dev_build_version` 只比 base，构建号不参与比较），比它新才会提示更新 |
| 4 | `overlay-dll/Cargo.toml` | `version` | `pz_overlay` DLL 版本。**运行时可见**：DLL 加载时用 `env!("CARGO_PKG_VERSION")` 往 `%TEMP%\overlay.log` 写首行 `pz_overlay vX.Y.Z 已加载（pid=…）`——排查用户跑的是哪版 DLL 就看这行 |
| 5 | `docs/promo/release-page/index.html` | `.dl-note` 的「当前版本：v…」文案 | 发布页展示，文案要和 package.json 同步（当前 `v1.7.261114`）。下载按钮固定指向 GitHub Releases latest 页和 123 云盘分享链接，页面没有带版本号的文件名链接，不需要改 |
| 6 | `docs/overlay-injection.md` §7 | 排查表里的示例日志行 `pz_overlay vX.Y.Z 已加载` | 示例要跟着当前版本走（当前 `pz_overlay v1.7.261114 已加载`），避免排查时对不上 |
| 7 | `update-api(服务端请勿上传)/changelog.md`（该目录不随仓库推送） | H1 版本号、`- 键：值`（发布日期 / 123 云盘 / Windows 安装包 / Windows 便携版 / 发布页）、`## 更新内容` 逐条 | **启动器更新源的编辑入口**：改完跑 `python "pytools(开发人员工具请勿上传)/build_update_api.py"` 生成 `api/latest.json`（字段映射见 update-api 目录内 README.md）。启动器只比更新源晚才提示更新——所以它必须和本次发版一起更新并重新部署，否则老用户收不到提示 |

## 二、不要手改、随构建自动更新的 3 处

| 文件 | 更新方式 |
| --- | --- |
| `package-lock.json` | `npm install --package-lock-only`（或任意一次 `npm install`）。注意：它曾经停在 `0.1.0` 很久没人发现——改完 package.json 后立刻同步它 |
| `src-tauri/Cargo.lock` | `cargo check` / `cargo build`（src-tauri 目录） |
| `overlay-dll/Cargo.lock` | `cargo check` / `cargo build`（overlay-dll 目录） |

## 三、改完版本号后需要重新生成的产物

| 产物 | 说明 |
| --- | --- |
| `release/PZManager_{version}_x64-setup.exe` | NSIS 安装包，`npm run tauri:build` 重新产出。**旧文件名的产物就是旧版本的二进制，不要手工改名冒充新版本**，发版前把新产物放进 `release/`（该目录被 .gitignore 忽略，不随仓库提交，仅作为发版产物的本地归档） |
| `release/PZManager_{version}_x64_portable.zip` | 便携版，需要重新压缩（内含新 DLL + 新 exe） |
| GitHub Release 的 tag / 标题 / 附件 | 仓库外，发布时手动对齐 |

## 四、标准发版步骤

1. 改上面「一」里的 7 处（全局搜索旧版本号逐一替换）。
2. `npm install --package-lock-only` 同步 package-lock.json。
3. `npm run build:all`（= `build:overlay` + `build`）：先构建正式 DLL（`overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll`，同时刷新 `overlay-dll/Cargo.lock`），再 `tsc && vite build` 构建前端。**这一步不碰 `src-tauri/Cargo.lock`**——它要等下一步 `tauri build` 编译 Rust 时才更新。
4. `npm run tauri:build`（裸 `tauri build`，`beforeBuildCommand` 会先跑一遍 `build:all`）：编译 Rust 时同步 `src-tauri/Cargo.lock` 并产出 NSIS 安装包，把它与重新压缩的便携版放进 `release/`。
5. GitHub Release 建 tag / 传附件后，把 `update-api(服务端请勿上传)/api/latest.json`（该目录不随仓库推送） 的 `downloads` 直链指向新附件，重新部署更新源（Cloudflare Pages，见该目录内的 README.md 发版流程），并 `curl` 验证返回新版本号。
6. 校验（见下节）。

## 五、校验命令

```bash
# 1) 源码里不应再有旧版本号（应无输出；release/ 里的旧产物除外）
grep -rn "1\.6\.261108" --include="*.json" --include="*.toml" --include="*.md" \
  --include="*.html" --include="*.ts" --include="*.tsx" --include="*.rs" . \
  | grep -v node_modules | grep -v "\.zcode" | grep -v "release/"

# 2) lockfile 都已同步（三处都应输出新版本号）
grep -m1 '"version": "1.7.261114"' package-lock.json
grep -A1 'name = "pzmanager"'  src-tauri/Cargo.lock    | grep 1.7.261114
grep -A1 'name = "pz_overlay"' overlay-dll/Cargo.lock  | grep 1.7.261114

# 3) 新 DLL 内含版本字符串（Windows/Git Bash，注意带 devbuild 后缀的完整版本号）
grep -a -o "pz_overlay v1.7.261114" overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll

# 4) 安装包 / 便携包文件名带新版本号
ls release/
```

运行期抽查：启动器「设置 › 关于」徽标显示 `v1.7.261114`（正式版无 DevBuild 徽标）；
启动游戏后 `%TEMP%\overlay.log` 首行是 `pz_overlay v1.7.261114 已加载（pid=…）`；
`curl -sS https://pzmapi.sanyec.top/api/latest.json` 返回
`"version": "1.7.261114"`，且「设置 › 关于 › 检查更新」提示已是最新。

## 六、备注

- 截至本文档撰写时，`tests/` 里没有断言版本号的测试；若以后加了版本相关守卫测试，记得同步本清单。
- `tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version` 没有机制强制一致，发版前肉眼核对一遍。
