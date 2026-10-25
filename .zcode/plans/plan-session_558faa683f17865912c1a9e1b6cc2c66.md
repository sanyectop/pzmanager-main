## 执行顺序(按你的要求:先备份,不对就回退)

### 第 0 步:git 备份回退点
- `git add -A && git commit`:把当前工作区(1.0.261025 版本号、release 产物、promo 页面等全部未提交改动)提交为备份提交,提交信息如「chore: 备份 1.0.261025 发布准备状态(覆盖层修复前回退点)」;
- 建分支 `backup/pre-overlay-fix` 指向该提交;之后任何时候发现问题,`git reset --hard backup/pre-overlay-fix` 即可完整回退(仓库无远程,备份在本地)。

### 第 1 步:中和 GL 残留状态(overlay-dll/src/lib.rs draw_ui_texture ~684 行)
画满屏四边形前增加 `glDisable(GL_DEPTH_TEST / GL_CULL_FACE / GL_STENCIL_TEST / GL_BLEND)`、`glActiveTexture(GL_TEXTURE0)`。外层已有 glPushAttrib/glPopAttrib,状态会自动还原。

### 第 2 步:GL 上下文重建时失效纹理缓存(lib.rs:654-682)
记录 `wglGetCurrentContext()`,发现变化则丢弃旧纹理名、重新 glGenTextures 并强制 glTexImage2D 重传(修复 LWJGL 重建上下文后纹理失效)。

### 第 3 步:UI 链路补日志(overlay-dll/src/ui.rs + lib.rs)
- UI 线程包 `catch_unwind`,panic 写入 overlay.log;
- 互斥锁中毒时记录一次再退出(替换 ui.rs:195 的静默 `unwrap_or(false)`);
- 首次记录「窗口未找到 / DIB 未创建」等 image() 为 None 的原因、首次纹理上传成功;
- DLL 加载时把版本号 1.0.261025 写入 overlay.log(便于确认用户跑的是哪版)。

### 第 4 步:旧 DLL 检测提示(宿主侧)
- 注入前检查目标 pz_overlay.dll 是否含新版标记(如状态文件名字符串),旧探针 DLL 返回明确中文错误提示「DLL 版本过旧,请更新/重装」;
- 检测到进程内已加载同名 DLL 时,枚举其完整路径,若与本次要注入的路径不同,通过 overlay-session 事件提示「检测到其他位置的旧版覆盖层 DLL(极速启动残留 -agentpath),请在设置中修复」。

### 第 5 步:清理与文档
- 删除死代码 draw_animation_overlay(lib.rs:565-635)、animation_quad_for(ui.rs:871-889)及其静态量;
- 更新 overlay-dll/Cargo.toml 过时描述(现仍写 'tints the game window red for 5 seconds');
- docs/overlay-injection.md §7 排查表增加「红屏无字」条目:判别信号(红屏前有无 GDI 文字阶段、红屏是否超 3 分钟)、需收集的日志(overlay.log、console.txt、游戏目录 JSON/.bat 的 -agentpath、findstr /m "JVM Memory" pz_overlay.dll)。

### 第 6 步:验证与收尾
- `cargo check` overlay-dll 与 src-tauri 两个 crate,通过后单独提交修复 commit;
- 本机若有 PZ 环境可构建 DLL 实测注入;不行则止步于编译验证。

### 回退方式
任意环节不对:`git reset --hard backup/pre-overlay-fix`(或 `git revert` 修复提交),工作区回到备份前状态。