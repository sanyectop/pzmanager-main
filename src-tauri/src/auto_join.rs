//! 自动加入服务器（设置 › 实验性功能）。
//!
//! 原理：B42 主菜单本来就能「启动时直接连入服务器」（Steam 邀请走的那条
//! `+connect` / `MainScreen.lua` 读 `getServerAddressFromArgs()` 的链路），最终都收束到
//! `ConnectToServer:connect(...)`——与直连弹窗「连接」按钮是同一个调用。启动器把一个钩子
//! Lua 装进 `<游戏>/media/lua/client/`（散装 Lua 游戏自动加载，无需启用模组），并在每次
//! 启动游戏前把一次性配置写进 Zomboid 主目录；钩子在主菜单就绪后按配置发起连接。
//!
//! 文件生命周期：
//! - `media/lua/client/pzm_autojoin.lua`：功能开启时随每次启动重写（游戏大版本更新会清掉
//!   media 里的改动，靠启动时重装自愈）；关闭时启动前删除。
//! - `%USERPROFILE%\Zomboid\Lua\pzm-autojoin.ini`：启动前写入、游戏退出后由后台线程删除；
//!   钩子读到后也会立刻把它改写成 `consumed=1`，启动器中途被杀也不会反复触发。
//!
//! 密码不进日志（`describe_auto_join_password` 只报有无和位数）；配置文件里的密码是
//! 明文，但只落在本机 Zomboid 目录——与游戏自己在收藏夹里存密码同级。

use crate::*;

/// 一次性连接配置文件名（钩子读这个文件）。
///
/// 必须放在 `Zomboid\Lua\` 子目录：B42 的 `cacheFileExists` / `getFileReader` /
/// `getFileWriter` 把相对路径解析到 `ZomboidFileSystem.getCacheDir()` 下的 `Lua\`
/// （字节码实锤 + 磁盘佐证：`Zomboid\Lua\ModOptions.ini`、`keysB42.ini`），写进
/// `Zomboid\` 根目录钩子永远读不到，只会静默不连。B41 时代的语义才是 Zomboid 根目录。
pub(crate) const AUTO_JOIN_CONFIG_FILE: &str = "pzm-autojoin.ini";
/// 装进游戏目录 `media/lua/client/` 的钩子文件名。pzm 前缀是启动器自己的命名空间，
/// 不要放进 pzopt/（那是 PZ_Optimization Workshop 工具的目录，卸载时会被一起清掉）。
pub(crate) const AUTO_JOIN_HOOK_FILE: &str = "pzm_autojoin.lua";

pub(crate) const AUTO_JOIN_HOOK_SOURCE: &str = include_str!("../lua/pzm_autojoin.lua");

/// 「实验性 › 自动加入服务器」的地址：`主机:端口`。
/// 与钩子 Lua 的解析严格对齐：主机段不做 trim 且不允许任何空白字符，端口必须是
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

fn auto_join_hook_path(game_path: &Path) -> PathBuf {
    game_path
        .join("media")
        .join("lua")
        .join("client")
        .join(AUTO_JOIN_HOOK_FILE)
}

pub(crate) fn auto_join_config_path() -> Result<PathBuf, String> {
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

/// 写一次性连接配置到指定路径。密码为空 / None 时不写 password 行
/// （收藏夹里保存过密码的服务器优先用收藏的，见钩子 Lua）。
/// 父目录（`Zomboid\Lua`）缺失时顺手创建——全新用户首次启动前该目录可能还不存在。
pub(crate) fn write_auto_join_config_at(
    path: &Path,
    target: &str,
    password: Option<&str>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建自动加入配置目录失败：{error}"))?;
    }
    let mut content = format!("target={target}\r\n");
    if let Some(password) = password.map(str::trim).filter(|value| !value.is_empty()) {
        content.push_str(&format!("password={password}\r\n"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    content.push_str(&format!("requested={stamp}\r\n"));
    fs::write(path, content).map_err(|error| format!("写入自动加入配置失败：{error}"))
}

/// 写一次性连接配置到当前用户的 Zomboid 主目录。
pub(crate) fn write_auto_join_config(
    target: &str,
    password: Option<&str>,
) -> Result<(), String> {
    let path = auto_join_config_path()?;
    write_auto_join_config_at(&path, target, password)
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

/// 日志里展示密码状态：只报有无和位数，绝不落明文。
pub(crate) fn describe_auto_join_password(password: Option<&str>) -> String {
    match password.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => format!("已配置（{} 位）", value.chars().count()),
        None => "未配置（用收藏夹保存的密码或进服后手动输入）".to_string(),
    }
}

/// 游戏退出后清理一次性配置的后台线程。
///
/// 先等游戏起来（最多 5 分钟——超时说明启动失败，把配置清掉，避免用户之后手动开
/// 游戏被抢连一次），再等游戏退出。线程随启动器退出而消亡；残留配置由钩子的
/// `consumed=1` 自消费兜底，最坏情况是下次手动启动游戏多触发一次自动连接。
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
