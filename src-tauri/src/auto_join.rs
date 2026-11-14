//! 自动加入服务器（设置 › 实验性功能）。
//!
//! 架构（2026-11-14 重构，"原生链路 + 最小钩子辅助"）：
//!
//! 1. **启动器**把游戏原生 `+connect 主机:端口`（可选 `+password 密码`）程序参数追加到
//!    java 命令末尾（Steam 方式则写进启动项）。`MainScreenState.enter` 解析成系统属性
//!    `args.server.connect` / `args.server.password`，主菜单就绪时游戏触发
//!    `OnMainMenuEnter` → `LoadMainScreenPanelInt` 读属性（读后即清，天然一次性）→
//!    `BootstrapConnectPopup:connect` 查询服务器 → 弹出 `ServerConnectPopup`
//!    （地址/密码已预填）。**这条链路是游戏自己的 Steam 邀请代码，时机与健壮性都有保证；
//!    它只差"点一下连接"这一下。**
//!
//! 2. **钩子 Lua**（`media/lua/client/pzm_autojoin.lua`，散装 game-dir Lua，无需启用模组）
//!    只做一件事：给 `ServerConnectPopup.setServer` 包一层——弹窗预填完之后，若目标与
//!    本次启动的 `Zomboid\Lua\pzm-autojoin.ini` 一致，补齐用户名（Steam 模式下用
//!    `getCurrentUserProfileName()` 兜底）并自动点一次「连接」，把原生流程升级为零点击。
//!    钩子失败（游戏版本改动等）只损失自动点击，弹窗仍会弹出，用户手点即可——优雅降级。
//!
//! 历史：更早的实现是钩子在 OnGameBoot 武装 + OnFETick 轮询主菜单直接调
//! `ConnectToServer:connect`。实测 B42.22 上武装后轮询永远不推进（连接不发起、无任何
//! 输出），且配置路径、菜单时机、Lua 环境处处是坑。改成上面的架构后，启动器不再依赖
//! 钩子的时机，钩子失败也不阻断连接。
//!
//! 密码不进日志（`describe_auto_join_password` 只报有无和位数）：密码经 `+password`
//! 程序参数传给游戏，**不再写进 ini**（旧方案会明文落盘）。

use crate::*;

/// 一次性目标标记（钩子判断"这个弹窗是不是启动器发起的"），放 `Zomboid\Lua\`——
/// B42 的 `cacheFileExists`/`getFileReader` 相对路径解析到该目录（字节码与磁盘双重验证：
/// `ModOptions.ini`、`keysB42.ini` 都在 Lua\ 下）。只写 target=，不写密码。
pub(crate) const AUTO_JOIN_CONFIG_FILE: &str = "pzm-autojoin.ini";
/// 装进游戏目录 `media/lua/client/` 的钩子文件名。pzm 前缀是启动器自己的命名空间，
/// 不要放进 pzopt/（那是 PZ_Optimization Workshop 工具的目录，卸载时会被一起清掉）。
pub(crate) const AUTO_JOIN_HOOK_FILE: &str = "pzm_autojoin.lua";

pub(crate) const AUTO_JOIN_HOOK_SOURCE: &str = include_str!("../lua/pzm_autojoin.lua");

/// 「实验性 › 自动加入服务器」的地址：`主机:端口`。
/// 与游戏主菜单的直连解析严格对齐：主机段不做 trim 且不允许任何空白字符，端口必须是
/// 1–65535 的纯数字（前导零可以）。这里多接受一个格式，游戏里就会多一次静默拒绝，
/// 用户只会看到"没自动连入"。IPv6 与游戏主菜单直连一样不支持。
pub(crate) fn normalize_auto_join_target(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let (host, port) = trimmed.rsplit_once(':')?;
    if host.is_empty() || host.chars().any(|ch| ch.is_whitespace()) {
        return None;
    }
    let port: u16 = port.parse().ok()?;
    if port == 0 {
        return None;
    }
    Some(format!("{host}:{port}"))
}

/// 日志里展示密码状态：只报有无和位数，绝不落明文。
pub(crate) fn describe_auto_join_password(password: Option<&str>) -> String {
    match password.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => format!("已配置（{} 位）", value.chars().count()),
        None => "未配置（用收藏夹保存的密码或进服后手动输入）".to_string(),
    }
}

/// `+connect` / `+password` 程序参数（追加到 java 命令的 `MainScreenState` 之后）。
/// 密码为空 / None 时不生成 `+password` 段（收藏夹里保存过密码的服务器优先用收藏的，
/// 见游戏内 ServerConnectPopup:setServer 的预填逻辑）。
pub(crate) fn auto_join_program_args(target: &str, password: Option<&str>) -> Vec<String> {
    let mut args = vec!["+connect".to_string(), target.to_string()];
    if let Some(password) = password.map(str::trim).filter(|value| !value.is_empty()) {
        args.push("+password".to_string());
        args.push(password.to_string());
    }
    args
}

/// 日志展示用的参数掩码：`+password` 的下一个 token（密码本身）替换成 `***`。
/// 其余参数原样保留——java 命令日志不能因此泄明文。
pub(crate) fn auto_join_redact_password(args: &[String]) -> Vec<String> {
    let mut display = Vec::with_capacity(args.len());
    let mut mask_next = false;
    for token in args {
        if mask_next {
            display.push("***".to_string());
            mask_next = false;
            continue;
        }
        mask_next = token == "+password";
        display.push(token.clone());
    }
    display
}

fn auto_join_hook_path(game_path: &Path) -> PathBuf {
    game_path
        .join("media")
        .join("lua")
        .join("client")
        .join(AUTO_JOIN_HOOK_FILE)
}

fn auto_join_config_path() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?
        .join("Lua")
        .join(AUTO_JOIN_CONFIG_FILE))
}

/// 把钩子写进游戏目录；返回是否发生了写入。幂等：内容一致就跳过，
/// 避免每次启动都白写一遍磁盘（内容不一致时覆盖——游戏更新后自愈）。
pub(crate) fn install_auto_join_hook_at(game_path: &Path) -> Result<bool, String> {
    let path = auto_join_hook_path(game_path);
    if let Ok(existing) = fs::read_to_string(&path) {
        if existing == AUTO_JOIN_HOOK_SOURCE {
            return Ok(false);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建自动加入钩子目录失败：{error}"))?;
    }
    fs::write(&path, AUTO_JOIN_HOOK_SOURCE)
        .map_err(|error| format!("写入自动加入钩子失败：{error}"))?;
    Ok(true)
}

/// 从游戏目录移除钩子；返回是否真的删了文件。
pub(crate) fn remove_auto_join_hook_at(game_path: &Path) -> Result<bool, String> {
    let path = auto_join_hook_path(game_path);
    if !path.is_file() {
        return Ok(false);
    }
    fs::remove_file(&path).map_err(|error| format!("删除自动加入钩子失败：{error}"))?;
    Ok(true)
}

/// 写一次性目标标记（只写 target=，密码经 `+password` 参数传给游戏，不落盘）。
pub(crate) fn write_auto_join_config_at(path: &Path, target: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建自动加入配置目录失败：{error}"))?;
    }
    fs::write(path, format!("target={target}\r\n"))
        .map_err(|error| format!("写入自动加入配置失败：{error}"))
}

pub(crate) fn write_auto_join_config(target: &str) -> Result<(), String> {
    let path = auto_join_config_path()?;
    write_auto_join_config_at(&path, target)
}

/// 删除一次性配置；返回是否真的删了文件。
pub(crate) fn clear_auto_join_config_at(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    fs::remove_file(path).is_ok()
}

pub(crate) fn clear_auto_join_config() -> bool {
    auto_join_config_path()
        .map(|path| clear_auto_join_config_at(&path))
        .unwrap_or(false)
}

/// 游戏退出后清理一次性配置的后台线程。
///
/// 先等游戏起来（最多 5 分钟——超时说明启动失败，把配置清掉，避免用户之后手动开
/// 游戏被抢连一次），再等游戏退出。线程随启动器退出而消亡；残留配置由钩子的
/// `consumed=1` 自消费兜底，最坏情况是下次手动启动游戏多触发一次自动点击。
/// 游戏检测只用 game_running_quick（纯进程快照，有界）——不碰枚举窗口的
/// `project_zomboid_running()`，无响应窗口会把它拖住。
#[cfg(windows)]
pub(crate) fn spawn_auto_join_cleanup() {
    let spawned = std::thread::Builder::new()
        .name("pzm-autojoin-cleanup".to_string())
        .spawn(|| {
            let mut started = false;
            for _ in 0..100 {
                if game_running_quick() {
                    started = true;
                    break;
                }
                thread::sleep(Duration::from_secs(3));
            }
            if !started {
                launcher_log::warn("自动加入：5 分钟内未检测到游戏进程，已清理一次性配置。");
                let _ = clear_auto_join_config();
                return;
            }
            while game_running_quick() {
                thread::sleep(Duration::from_secs(5));
            }
            if clear_auto_join_config() {
                launcher_log::info("自动加入：游戏已退出，已清理一次性配置。");
            }
        });
    if spawned.is_err() {
        // 创建失败只影响清理时机：配置被钩子消费后即失效，下次启动也会再覆盖/清理。
        launcher_log::warn("自动加入：清理线程创建失败，残留配置将在下次启动时清理。");
    }
}

#[cfg(not(windows))]
pub(crate) fn spawn_auto_join_cleanup() {}
