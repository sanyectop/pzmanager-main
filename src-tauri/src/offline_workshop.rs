//! 离线启动的创意工坊模组映射（目录联接 / junction）。
//!
//! 背景：游戏只扫描 `%USERPROFILE%\Zomboid\mods` 里的本地模组；创意工坊模组平时
//! 由 Steam 运行时（Steamworks API）提供给游戏。「从游戏文件启动」的离线启动没有
//! Steam 运行时，游戏会扫不到创意工坊模组。这里的做法：启动前把每个已下载的
//! 创意工坊模组目录用一个**目录联接（junction，无需管理员权限）**映射到
//! `Zomboid\mods\pzmanager-ws-<workshopId>-<模组名>`，游戏按本地模组扫描即可；
//! Steam 模式（正常启动 / 启动器重启）时全部拆除，避免与 Steam 自带的工坊扫描
//! 重复加载同一批模组（模组页 / 版本快照也会因此出现重复条目）。
//!
//! 生命周期：
//! * 离线启动（Steam 未运行）：先清残留，再按当前创意工坊目录重建映射；
//! * Steam 启动 / 启动器重启：全部拆除（个别被占用拆不掉的记日志跳过）。
//!
//! 实现用原生 `FSCTL_SET_REPARSE_POINT`（MOUNT_POINT reparse）而不是 `mklink /J`：
//! 不用起 cmd 子进程（几百个模组时快一个数量级），也不用担心模组名里的 cmd
//! 元字符（`&`、`%`）被拆坏。拆除只用 `fs::remove_dir`（它只移除联接点本身，
//! 不会递归进目标目录）。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::mod_versioning;

/// 本启动器创建的联接名前缀。清理时只动带这个前缀且确为联接点的目录项，
/// 用户自己放在 `Zomboid\mods` 里的东西绝不碰。
pub(crate) const OFFLINE_LINK_PREFIX: &str = "pzmanager-ws-";

/// 联接名里模组名的长度上限：workshopId(10) + 前缀(13) + 40 ≈ 65 字符，
/// 加上 Zomboid 路径和模组内部结构仍远低于 MAX_PATH。
const LINK_NAME_MAX_CHARS: usize = 40;

fn offline_mods_directory() -> Result<PathBuf, String> {
    Ok(crate::user_zomboid_directory()?.join("mods"))
}

/// 把模组名整理成联接名里可用的形式：替换 Windows 文件名非法字符，
/// 去掉结尾的点和空格，截短到 [`LINK_NAME_MAX_CHARS`]。
fn sanitize_offline_name(name: &str) -> String {
    let mut result = String::new();
    for ch in name.trim().chars() {
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control() {
            result.push('_');
        } else {
            result.push(ch);
        }
    }
    let trimmed = result.trim().trim_end_matches('.');
    let truncated: String = trimmed.chars().take(LINK_NAME_MAX_CHARS).collect();
    if truncated.is_empty() {
        "mod".to_string()
    } else {
        truncated
    }
}

/// 在 `mods_dir` 里拆除所有本启动器创建的离线映射，返回个数。
///
/// 只拆「名字带前缀 **且** 是联接点」的目录项：Rust 在 Windows 上把 junction
/// 也报告为 symlink，普通目录不会满足条件，所以用户的同名真目录绝不误删。
#[cfg(windows)]
pub(crate) fn remove_offline_links_in(mods_dir: &Path) -> Result<usize, String> {
    let items = fs::read_dir(mods_dir)
        .map_err(|error| format!("读取 {} 失败：{error}", mods_dir.display()))?;
    let mut removed = 0usize;
    for entry in items.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.starts_with(OFFLINE_LINK_PREFIX) {
            continue;
        }
        if !entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            continue;
        }
        if let Err(error) = fs::remove_dir(entry.path()) {
            // 被占用（游戏正在运行）时拆不掉：记日志跳过，下次再清。
            crate::launcher_log::warn(&format!(
                "移除离线模组联接 {} 失败：{error}",
                entry.path().display()
            ));
        } else {
            removed += 1;
        }
    }
    Ok(removed)
}

/// 拆除默认位置（`Zomboid\mods`）的所有离线映射。
pub(crate) fn remove_offline_workshop_links() -> Result<usize, String> {
    #[cfg(windows)]
    {
        remove_offline_links_in(&offline_mods_directory()?)
    }
    #[cfg(not(windows))]
    {
        Ok(0)
    }
}

/// 为本次离线启动重建创意工坊映射，返回给人看的摘要（进日志与成功提示）。
///
/// Steam 正在运行时不做任何事：游戏能自己通过 Steamworks 枚举创意工坊，
/// 本地映射只会造成重复。否则清掉旧映射后，把当前所有已下载的创意工坊模组
/// （[`crate::collect_mod_roots`] 的 workshop 部分）逐个联接进 `Zomboid\mods`。
pub(crate) fn sync_offline_workshop_links() -> Result<String, String> {
    #[cfg(windows)]
    if crate::steam_is_running().unwrap_or(false) {
        return Ok("Steam 正在运行，游戏可自行读取创意工坊，未创建离线映射。".to_string());
    }

    let removed = remove_offline_workshop_links().unwrap_or_else(|error| {
        crate::launcher_log::warn(&format!("清理旧离线映射失败：{error}"));
        0
    });

    let roots: Vec<mod_versioning::ModRoot> = crate::collect_mod_roots()
        .into_iter()
        .filter(|root| root.source == "workshop" && root.workshop_id.is_some())
        .collect();
    if roots.is_empty() {
        return Ok(format!(
            "未发现已下载的创意工坊模组，游戏里只能用本地模组（清理了 {removed} 个旧映射）。"
        ));
    }

    let mods_dir = offline_mods_directory()?;
    fs::create_dir_all(&mods_dir)
        .map_err(|error| format!("创建 {} 失败：{error}", mods_dir.display()))?;

    let mut used_names: HashSet<String> = HashSet::new();
    let mut created = 0usize;
    let mut failed: Vec<String> = Vec::new();
    for root in &roots {
        let workshop_id = root.workshop_id.as_deref().unwrap_or_default();
        let base = sanitize_offline_name(&root.name);
        let mut link_name = format!("{OFFLINE_LINK_PREFIX}{workshop_id}-{base}");
        let mut suffix = 2;
        while !used_names.insert(link_name.clone()) {
            // 同一个创意工坊物品里有两个 mod.info 同名的模组时靠序号区分。
            link_name = format!("{OFFLINE_LINK_PREFIX}{workshop_id}-{base}-{suffix}");
            suffix += 1;
        }
        match create_junction(&mods_dir.join(&link_name), Path::new(&root.path)) {
            Ok(()) => created += 1,
            Err(error) => {
                crate::launcher_log::warn(&format!("离线映射「{}」失败：{error}", root.name));
                failed.push(root.name.clone());
            }
        }
    }

    if failed.is_empty() {
        Ok(format!(
            "已把 {created} 个创意工坊模组映射到 Zomboid\\mods（Steam 启动或重启启动器时自动清理）。"
        ))
    } else {
        Ok(format!(
            "已映射 {created} 个创意工坊模组，{} 个失败（如「{}」），详见日志。",
            failed.len(),
            failed.first().take().unwrap_or(&String::new())
        ))
    }
}

/// 创建一个指向 `target` 的目录联接（junction）。联接位置必须不存在。
///
/// 用 [`junction`] crate（rustup 在 Windows 上同款）：它负责建空目录、以
/// reparse-point 方式打开并写 `FSCTL_SET_REPARSE_POINT` 的 MOUNT_POINT 数据，
/// 无需管理员权限。这里只负责前置校验与错误消息本地化。
#[cfg(windows)]
pub(crate) fn create_junction(link: &Path, target: &Path) -> Result<(), String> {
    if !target.is_dir() {
        return Err(format!("联接目标不存在：{}", target.display()));
    }
    if let Ok(meta) = link.symlink_metadata() {
        // 上次联接没拆掉（如被占用后残留）：联接点是本次要重建的产物，先拆再建；
        // 不是联接点的真目录绝不覆盖。
        if meta.file_type().is_symlink() && fs::remove_dir(link).is_ok() {
            // 拆掉了旧联接，继续创建。
        } else {
            return Err(format!("链接位置已存在：{}", link.display()));
        }
    }
    junction::create(target, link).map_err(|error| {
        format!(
            "创建目录联接 {} → {} 失败：{error}",
            link.display(),
            target.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn junction_links_target_and_removes_cleanly() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("workshop-mod");
        fs::create_dir_all(target.join("42/media")).unwrap();
        fs::write(target.join("mod.info"), b"name = ProbeMod\n").unwrap();

        let link = temp.path().join("pzmanager-ws-42-ProbeMod");
        create_junction(&link, &target).unwrap();

        // 穿过联接能看到目标内容；联接本身按 symlink 报告（清理判定的依据）。
        assert!(link.join("mod.info").is_file());
        assert!(fs::read_to_string(link.join("mod.info")).unwrap().contains("ProbeMod"));
        let meta = link.symlink_metadata().unwrap();
        assert!(meta.file_type().is_symlink());

        // 用与 remove_offline_links_in 相同的判定拆除：目标内容必须完好。
        assert!(fs::remove_dir(&link).is_ok());
        assert!(!link.exists());
        assert!(target.join("mod.info").is_file());
        assert!(target.join("42").is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn remove_only_touches_prefixed_junctions() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir_all(&target).unwrap();

        // 带前缀的联接：应被拆掉。
        let link = temp.path().join(format!("{OFFLINE_LINK_PREFIX}123-Mod"));
        create_junction(&link, &target).unwrap();
        // 带前缀的真目录 / 无前缀的联接：都绝不能动。
        let real_dir = temp.path().join(format!("{OFFLINE_LINK_PREFIX}real"));
        fs::create_dir_all(&real_dir).unwrap();
        let unprefixed = temp.path().join("other-mod");
        create_junction(&unprefixed, &target).unwrap();

        let removed = remove_offline_links_in(temp.path()).unwrap();
        assert_eq!(removed, 1);
        assert!(!link.exists());
        assert!(real_dir.is_dir(), "用户自己的同名真目录不许误删");
        assert!(unprefixed.exists(), "无前缀的联接不许误删");
        assert!(target.is_dir());
    }

    #[test]
    fn sanitize_replaces_illegal_characters_and_trims() {
        assert_eq!(sanitize_offline_name("Hats & Glasses"), "Hats & Glasses");
        assert_eq!(sanitize_offline_name("a:b*c?d"), "a_b_c_d");
        assert_eq!(sanitize_offline_name("  trailer... "), "trailer");
        assert_eq!(sanitize_offline_name("/\\:*?\"<>|"), "_________");
        assert_eq!(sanitize_offline_name("   "), "mod");
        // 截断到上限，避免联接名把路径推过 MAX_PATH。
        let long = "很长的模组名称".repeat(20);
        assert_eq!(sanitize_offline_name(&long).chars().count(), LINK_NAME_MAX_CHARS);
    }
}
