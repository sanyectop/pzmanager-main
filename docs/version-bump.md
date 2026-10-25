# 版本号修改清单（发版必读）

> 最近一次适用：`1.0.261025 → 1.1.261025`（2026-10-25）。
> 以后每次改版本号，按本文档逐项过一遍，最后用文末的校验命令确认没有漏网之鱼。

## 版本命名规则

- 格式：`{主版本}.{次版本}.{YYMMDD}`，例如 `1.1.261025`（2026-10-25 构建）。
- **配置文件里不带 `v` 前缀**（package.json / Cargo.toml / tauri.conf.json 都是裸 semver）；
  **`v` 前缀只出现在展示层**：关于页徽标（`v{appManifest.version}`）、发布页文案、DLL 日志首行。

## 一、必须手改的 6 处

| # | 文件 | 位置 | 说明 / 谁消费它 |
| --- | --- | --- | --- |
| 1 | `package.json` | `"version"` | **关于页显示 `v{appManifest.version}`**（`src/App.tsx` 顶部 `import appManifest from "../package.json"`，徽标在设置 › 关于）；也是 npm 包版本 |
| 2 | `src-tauri/tauri.conf.json` | `"version"` | **Tauri 打包版本**：NSIS 安装包的产品版本、`PZManager_{version}_x64-setup.exe` 产物文件名 |
| 3 | `src-tauri/Cargo.toml` | `version` | `pzmanager` Rust 包版本（应与 tauri.conf.json 保持一致） |
| 4 | `overlay-dll/Cargo.toml` | `version` | `pz_overlay` DLL 版本。**运行时可见**：DLL 加载时用 `env!("CARGO_PKG_VERSION")` 往 `%TEMP%\overlay.log` 写首行 `pz_overlay vX.Y.Z 已加载（pid=…）`——排查用户跑的是哪版 DLL 就看这行 |
| 5 | `docs/promo/release-page/index.html` | 「当前版本：vX.Y.Z」文案 + 下载链接里的文件名 | 发布页展示；下载链接文件名要和 `release/` 里实际产物一致 |
| 6 | `docs/overlay-injection.md` §7 | 排查表里的示例日志行 `pz_overlay vX.Y.Z 已加载` | 示例要跟着当前版本走，避免排查时对不上 |

## 二、不要手改、随构建自动更新的 3 处

| 文件 | 更新方式 |
| --- | --- |
| `package-lock.json` | `npm install --package-lock-only`（或任意一次 `npm install`）。注意：它曾经停在 `0.1.0` 很久没人发现——改完 package.json 后立刻同步它 |
| `src-tauri/Cargo.lock` | `cargo check` / `cargo build`（src-tauri 目录） |
| `overlay-dll/Cargo.lock` | `cargo check` / `cargo build`（overlay-dll 目录） |

## 三、改完版本号后需要重新生成的产物

| 产物 | 说明 |
| --- | --- |
| `release/PZManager_{version}_x64-setup.exe` | NSIS 安装包，`npm run tauri:build` 重新产出。**旧文件名的产物就是旧版本的二进制，不要手工改名冒充新版本**，发版前把新产物放进 `release/` 并提交 |
| `release/PZManager_{version}_x64_portable.zip` | 便携版，需要重新压缩（内含新 DLL + 新 exe） |
| GitHub Release 的 tag / 标题 / 附件 | 仓库外，发布时手动对齐 |

## 四、标准发版步骤

1. 改上面「一」里的 6 处（全局搜索旧版本号逐一替换）。
2. `npm install --package-lock-only` 同步 package-lock.json。
3. `npm run build:all`：先构建正式 DLL（`overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll`），再构建前端（同时刷新两个 Cargo.lock —— 注意 build:overlay 用的是 `--release`，Cargo.lock 在 `cargo build` 时同样会更新）。
4. `npm run tauri:build` 产出 NSIS 安装包，把它与重新压缩的便携版放进 `release/`。
5. 校验（见下节）。

## 五、校验命令

```bash
# 1) 源码里不应再有旧版本号（应无输出；release/ 里的旧产物除外）
grep -rn "1\.0\.261025" --include="*.json" --include="*.toml" --include="*.md" \
  --include="*.html" --include="*.ts" --include="*.tsx" --include="*.rs" . \
  | grep -v node_modules | grep -v "\.zcode" | grep -v "release/"

# 2) lockfile 都已同步（三处都应输出新版本号）
grep -m1 '"version": "1.1.261025"' package-lock.json
grep -A1 'name = "pzmanager"'  src-tauri/Cargo.lock    | grep 1.1.261025
grep -A1 'name = "pz_overlay"' overlay-dll/Cargo.lock  | grep 1.1.261025

# 3) 新 DLL 内含版本字符串（Windows/Git Bash）
grep -a -o "pz_overlay v1.1.261025" overlay-dll/target/x86_64-pc-windows-msvc/release/pz_overlay.dll

# 4) 安装包 / 便携包文件名带新版本号
ls release/
```

运行期抽查：启动器「设置 › 关于」徽标显示 `v1.1.261025`；启动游戏后 `%TEMP%\overlay.log` 首行是
`pz_overlay v1.1.261025 已加载（pid=…）`。

## 六、备注

- 截至本文档撰写时，`tests/` 里没有断言版本号的测试；若以后加了版本相关守卫测试，记得同步本清单。
- `tauri.conf.json` 的 `version` 与 `src-tauri/Cargo.toml` 的 `version` 没有机制强制一致，发版前肉眼核对一遍。
