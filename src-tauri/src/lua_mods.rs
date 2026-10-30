//! Lua 模组：mod.info 解析、启用顺序、启用/禁用与海报读取。
//!
//! 通过 `lib.rs` 的 `pub(crate) use lua_mods::*;` 保持原有调用路径。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;
use crate::*;
pub(crate) fn parse_default_mod_ids(content: &str) -> Vec<String> {
    let Some(mods_start) = content.find("mods") else {
        return Vec::new();
    };
    let Some(open) = content[mods_start..].find('{') else {
        return Vec::new();
    };
    let remainder = &content[mods_start + open + 1..];
    let end = remainder.find('}').unwrap_or(remainder.len());
    remainder[..end]
        .split(',')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("mod")
                .then(|| value.trim().trim_matches(['"', '\'']).to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LuaModInfo {
    pub(crate) id: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) poster: Option<String>,
    pub(crate) requires: Vec<String>,
    pub(crate) load_after: Vec<String>,
    pub(crate) load_before: Vec<String>,
    pub(crate) load_first: bool,
    pub(crate) load_last: bool,
}

pub(crate) fn split_lua_mod_ids(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| {
            part.trim()
                .trim_matches(['"', '\''])
                .trim()
                .trim_start_matches(['/', '\\'])
        })
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn lua_mod_flag_enabled(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches(['"', '\''])
            .to_ascii_lowercase()
            .as_str(),
        "on" | "true" | "yes" | "1" | "0"
    )
}

pub(crate) fn parse_lua_mod_info(content: &str) -> LuaModInfo {
    let mut info = LuaModInfo::default();
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "id" if info.id.is_none() => info.id = Some(value.to_string()),
            "name" if info.name.is_none() => info.name = Some(value.to_string()),
            "poster" if info.poster.is_none() => info.poster = Some(value.to_string()),
            "require" => info.requires.extend(split_lua_mod_ids(value)),
            "loadafter" | "loadmodafter" => info.load_after.extend(split_lua_mod_ids(value)),
            "loadbefore" | "loadmodbefore" => info.load_before.extend(split_lua_mod_ids(value)),
            "loadfirst" => info.load_first = lua_mod_flag_enabled(value),
            "loadlast" => info.load_last = lua_mod_flag_enabled(value),
            _ => {}
        }
    }
    info
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LuaModSortingRule {
    pub(crate) load_after: Vec<String>,
    pub(crate) load_before: Vec<String>,
    pub(crate) load_first: bool,
    pub(crate) load_last: bool,
}

pub(crate) fn parse_lua_sorting_rules(content: &str) -> HashMap<String, LuaModSortingRule> {
    let mut rules = HashMap::<String, LuaModSortingRule>::new();
    let mut current_id: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_id = Some(trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase());
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let Some(id) = current_id.as_ref() else {
            continue;
        };
        let rule = rules.entry(id.clone()).or_default();
        match key.trim().to_ascii_lowercase().as_str() {
            "loadafter" | "loadmodafter" => {
                rule.load_after.extend(split_lua_mod_ids(value));
            }
            "loadbefore" | "loadmodbefore" => {
                rule.load_before.extend(split_lua_mod_ids(value));
            }
            "loadfirst" => rule.load_first = lua_mod_flag_enabled(value),
            "loadlast" => rule.load_last = lua_mod_flag_enabled(value),
            _ => {}
        }
    }
    rules
}

pub(crate) fn lua_mod_sorting_rules() -> HashMap<String, LuaModSortingRule> {
    let path = user_zomboid_directory()
        .map(|directory| directory.join("Lua").join("sorting_rules.txt"))
        .ok();
    path.and_then(|path| fs::read_to_string(path).ok())
        .map(|content| parse_lua_sorting_rules(&content))
        .unwrap_or_default()
}

pub(crate) fn lua_mod_info_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("mod.info"), root.join("common").join("mod.info")];
    if let Ok(entries) = fs::read_dir(root) {
        files.extend(entries.flatten().filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            (entry.file_type().ok()?.is_dir() && (name.starts_with("41") || name.starts_with("42")))
                .then(|| entry.path().join("mod.info"))
        }));
    }
    files.into_iter().filter(|path| path.is_file()).collect()
}

pub(crate) fn lua_mod_poster_path(
    root: &Path,
    info_path: Option<&Path>,
    info: &LuaModInfo,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let (Some(poster), Some(parent)) = (info.poster.as_deref(), info_path.and_then(Path::parent))
    {
        let relative = Path::new(poster.trim());
        if !relative.is_absolute()
            && !relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            candidates.push(parent.join(relative));
        }
    }
    candidates.extend([
        root.join("preview.png"),
        root.join("poster.png"),
        root.join("preview.jpg"),
        root.join("poster.jpg"),
    ]);
    candidates
        .into_iter()
        .find(|path| is_supported_image(path) && is_regular_file(path))
}

pub(crate) fn extend_unique_mod_ids(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for value in values {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            target.push(value);
        }
    }
}

pub(crate) fn lua_mod_metadata(root: &Path) -> (String, String, LuaModInfo, Option<PathBuf>) {
    for info_path in lua_mod_info_files(root) {
        let Ok(content) = fs::read_to_string(&info_path) else {
            continue;
        };
        let info = parse_lua_mod_info(&content);
        if info.id.is_some() || info.name.is_some() {
            let id = info.id.clone().unwrap_or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let name = info.name.clone().unwrap_or_else(|| id.clone());
            return (
                id,
                name,
                info.clone(),
                lua_mod_poster_path(root, Some(&info_path), &info),
            );
        }
    }
    let id = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    (
        id.clone(),
        id,
        LuaModInfo::default(),
        lua_mod_poster_path(root, None, &LuaModInfo::default()),
    )
}

pub(crate) fn merge_lua_mod_entry(existing: &mut LuaModEntry, candidate: LuaModEntry) {
    let prefer_candidate = candidate.has_b42 && !existing.has_b42;
    if prefer_candidate {
        existing.requires = candidate.requires.clone();
        existing.load_after = candidate.load_after.clone();
        existing.load_before = candidate.load_before.clone();
    } else if existing.has_b42 && !candidate.has_b42 {
        // Keep the Build 42 metadata when the same ID also has a legacy copy.
    } else {
        extend_unique_mod_ids(&mut existing.requires, candidate.requires.clone());
        extend_unique_mod_ids(&mut existing.load_after, candidate.load_after.clone());
        extend_unique_mod_ids(&mut existing.load_before, candidate.load_before.clone());
    }
    existing.load_first |= candidate.load_first;
    existing.load_last |= candidate.load_last;
    existing.has_common |= candidate.has_common;
    existing.has_b42 |= candidate.has_b42;
    existing.is_loader |= candidate.is_loader;
    existing.load_order = existing.load_order.or(candidate.load_order);
    existing.enabled = existing.load_order.is_some();
    if prefer_candidate {
        existing.name = candidate.name;
        existing.path = candidate.path;
        existing.source = candidate.source;
        existing.workshop_id = candidate.workshop_id;
        if candidate.poster_path.is_some() {
            existing.poster_path = candidate.poster_path;
        }
    } else if existing.poster_path.is_none() {
        existing.poster_path = candidate.poster_path;
    }
}

pub(crate) fn lua_mod_entries() -> Result<Vec<LuaModEntry>, String> {
    let zomboid = user_zomboid_directory()?;
    let sorting_rules = lua_mod_sorting_rules();
    let default_path = zomboid.join("mods").join("default.txt");
    let enabled_ids = fs::read_to_string(&default_path)
        .map(|content| parse_default_mod_ids(&content))
        .unwrap_or_default();
    let enabled_orders: HashMap<_, _> = enabled_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.to_lowercase(), index + 1))
        .collect();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut add_entry = |path: PathBuf, source: &str, workshop_id: Option<String>| {
        let key = format!("{source}:{}", path.display());
        if !seen.insert(key) {
            return;
        }
        let (id, name, info, poster) = lua_mod_metadata(&path);
        let requires = info.requires.clone();
        let mut load_after = info.load_after.clone();
        let mut load_before = info.load_before.clone();
        let mut load_first = info.load_first;
        let mut load_last = info.load_last;
        if let Some(rule) = sorting_rules.get(&id.to_ascii_lowercase()) {
            extend_unique_mod_ids(&mut load_after, rule.load_after.clone());
            extend_unique_mod_ids(&mut load_before, rule.load_before.clone());
            load_first |= rule.load_first;
            load_last |= rule.load_last;
        }
        let load_order = enabled_orders.get(&id.to_lowercase()).copied();
        let has_common = path.join("common").is_dir();
        let has_42 = fs::read_dir(&path)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .starts_with("42")
            });
        entries.push(LuaModEntry {
            is_loader: id.eq_ignore_ascii_case("ZombieBuddy")
                || workshop_id.as_deref() == Some(ZOMBIE_BUDDY_MOD_ID),
            id,
            name,
            path: path.to_string_lossy().into_owned(),
            source: source.to_string(),
            workshop_id,
            enabled: load_order.is_some(),
            load_order,
            requires,
            load_after,
            load_before,
            load_first,
            load_last,
            has_common: path.join("common").is_dir(),
            has_b42: has_42 || has_common,
            poster_path: poster.map(|path| path.to_string_lossy().into_owned()),
        });
    };
    let local_mods = zomboid.join("mods");
    if let Ok(items) = fs::read_dir(&local_mods) {
        for entry in items
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            add_entry(entry.path(), "local", None);
        }
    }
    for root in workshop_background_roots() {
        let Ok(items) = fs::read_dir(&root) else {
            continue;
        };
        for workshop_item in items
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let workshop_id = workshop_item.file_name().to_string_lossy().into_owned();
            if workshop_id.parse::<u64>().is_err() {
                continue;
            }
            for mod_root in discover_mod_roots(&workshop_item.path()) {
                add_entry(mod_root, "workshop", Some(workshop_id.clone()));
            }
        }
    }
    let mut deduplicated = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(index) = deduplicated
            .iter()
            .position(|existing: &LuaModEntry| existing.id.eq_ignore_ascii_case(&entry.id))
        {
            merge_lua_mod_entry(&mut deduplicated[index], entry);
        } else {
            deduplicated.push(entry);
        }
    }
    entries = deduplicated;
    entries.sort_by(|left, right| {
        left.load_order
            .cmp(&right.load_order)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command(async)]
pub(crate) fn list_lua_mods() -> Result<Vec<LuaModEntry>, String> {
    lua_mod_entries()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaModToggleResult {
    pub(crate) enabled: bool,
    pub(crate) load_order: Option<usize>,
    pub(crate) added: Vec<String>,
    pub(crate) missing_requires: Vec<String>,
    pub(crate) dependents: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LuaModBatchToggleResult {
    pub(crate) enabled: bool,
    pub(crate) changed: Vec<String>,
    pub(crate) added: Vec<String>,
    pub(crate) missing_requires: Vec<String>,
    pub(crate) dependents: Vec<String>,
    pub(crate) skipped: Vec<String>,
}

/// Byte range of the `mods { ... }` block inside default.txt: the `mods` keyword,
/// its opening brace and its closing brace.
pub(crate) fn default_mods_block(content: &str) -> Option<(usize, usize, usize)> {
    let mods_start = content.find("mods")?;
    let open = mods_start + content[mods_start..].find('{')?;
    let close = open + content[open..].find('}')?;
    Some((mods_start, open, close))
}

/// Rebuilds the `mods` block with `ids` in order. Everything outside the block,
/// plus the file's line endings and indentation, is preserved.
pub(crate) fn write_default_mod_ids(default_path: &Path, ids: &[String]) -> Result<(), String> {
    let existing = match fs::read_to_string(default_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("读取 default.txt 失败: {error}")),
    };
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let block = default_mods_block(&existing);
    let indent = block
        .map(|(_, open, close)| {
            existing[open + 1..close]
                .lines()
                .find(|line| line.trim_start().to_ascii_lowercase().starts_with("mod"))
                .map(|line| line[..line.len() - line.trim_start().len()].to_string())
                .unwrap_or_else(|| "    ".to_string())
        })
        .unwrap_or_else(|| "    ".to_string());

    let mut body = String::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        // 注意：PZ 自己的 default.txt 里是**不带引号**的 `mod = 模组ID,`。
        // 写成 `mod = "模组ID",` 会让游戏把引号当成 ID 的一部分，从而识别不出任何模组。
        body.push_str(&format!("{indent}mod = {id},{newline}"));
    }
    let block_text = format!("mods{newline}{{{newline}{body}}}");

    let updated = match block {
        Some((mods_start, _, close)) => {
            let mut content = String::with_capacity(existing.len() + block_text.len());
            content.push_str(&existing[..mods_start]);
            content.push_str(&block_text);
            content.push_str(&existing[close + 1..]);
            content
        }
        None => {
            let mut content = existing.clone();
            if content.trim().is_empty() {
                // 游戏自己的文件第一行是 `VERSION = 1,`，缺了它 PZ 可能不认这份配置。
                content.clear();
                content.push_str(&format!("VERSION = 1,{newline}{newline}"));
            } else if !content.ends_with('\n') {
                content.push_str(newline);
            }
            content.push_str(&block_text);
            content.push_str(newline);
            content
        }
    };

    if let Some(parent) = default_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 mods 目录失败: {error}"))?;
    }
    fs::write(default_path, updated).map_err(|error| format!("写入 default.txt 失败: {error}"))
}

pub(crate) fn read_default_mod_ids(default_path: &Path) -> Result<Vec<String>, String> {
    match fs::read_to_string(default_path) {
        Ok(content) => Ok(parse_default_mod_ids(&content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取 default.txt 失败: {error}")),
    }
}

/// Matches the load-order semantics used by Build 42 mod managers:
/// `require` and `loadAfter` edges run first, `loadBefore` is inverted, and
/// `loadFirst` / `loadLast` only affect the stable fallback order.
pub(crate) fn order_mod_ids_by_requires(entries: &[LuaModEntry], ids: &[String]) -> Vec<String> {
    fn visit(
        id: &str,
        by_id: &HashMap<String, &LuaModEntry>,
        before: &HashMap<String, Vec<String>>,
        ordered: &mut Vec<String>,
        done: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) {
        let key = id.to_ascii_lowercase();
        if done.contains(&key) || !visiting.insert(key.clone()) {
            return;
        }
        if let Some(dependencies) = before.get(&key) {
            for dependency in dependencies {
                if by_id.contains_key(&dependency.to_ascii_lowercase()) {
                    visit(dependency, by_id, before, ordered, done, visiting);
                }
            }
        }
        visiting.remove(&key);
        if done.insert(key) {
            ordered.push(
                by_id
                    .get(&id.to_ascii_lowercase())
                    .map(|entry| entry.id.clone())
                    .unwrap_or_else(|| id.to_string()),
            );
        }
    }

    let active_ids = ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let by_id = entries
        .iter()
        .map(|entry| (entry.id.to_ascii_lowercase(), entry))
        .collect::<HashMap<_, _>>();
    let mut before = HashMap::<String, Vec<String>>::new();
    for id in ids {
        let key = id.to_ascii_lowercase();
        let Some(entry) = by_id.get(&key) else {
            continue;
        };
        let dependencies = before.entry(key).or_default();
        extend_unique_mod_ids(
            dependencies,
            entry
                .requires
                .iter()
                .chain(&entry.load_after)
                .filter(|dependency| active_ids.contains(&dependency.to_ascii_lowercase()))
                .cloned(),
        );
    }
    for entry in entries {
        if !active_ids.contains(&entry.id.to_ascii_lowercase()) {
            continue;
        }
        for target in &entry.load_before {
            let target_key = target.to_ascii_lowercase();
            if active_ids.contains(&target_key) {
                extend_unique_mod_ids(before.entry(target_key).or_default(), [entry.id.clone()]);
            }
        }
    }

    let original_order = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    let mut candidates = ids.to_vec();
    candidates.sort_by(|left, right| {
        let left_entry = by_id.get(&left.to_ascii_lowercase()).copied();
        let right_entry = by_id.get(&right.to_ascii_lowercase()).copied();
        let priority =
            |id: &str, entry: Option<&LuaModEntry>| match id.to_ascii_lowercase().as_str() {
                "modmanager" => 0,
                "modmanagerserver" => 1,
                "modoptions" => 2,
                _ if entry.is_some_and(|entry| entry.load_first) => 100,
                _ if entry.is_some_and(|entry| entry.load_last) => 10_000,
                _ => 1_000,
            };
        priority(left, left_entry)
            .cmp(&priority(right, right_entry))
            .then_with(|| {
                original_order
                    .get(&left.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(usize::MAX)
                    .cmp(
                        &original_order
                            .get(&right.to_ascii_lowercase())
                            .copied()
                            .unwrap_or(usize::MAX),
                    )
            })
    });

    let mut ordered = Vec::new();
    let mut done = HashSet::new();
    let mut visiting = HashSet::new();
    for id in candidates {
        visit(&id, &by_id, &before, &mut ordered, &mut done, &mut visiting);
    }
    ordered
}

/// Adds `id` plus every prerequisite it declares (recursively) to `ids`,
/// appending so prerequisites end up ahead of their dependents.
pub(crate) fn plan_lua_mod_enable(
    entries: &[LuaModEntry],
    id: &str,
    ids: &mut Vec<String>,
    added: &mut Vec<String>,
    missing: &mut Vec<String>,
    active: &mut HashSet<String>,
) {
    let key = id.to_ascii_lowercase();
    if !active.insert(key.clone()) {
        return;
    }
    match entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(id))
    {
        Some(entry) => {
            for require in &entry.requires {
                plan_lua_mod_enable(entries, require, ids, added, missing, active);
            }
            if !ids
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&entry.id))
            {
                ids.push(entry.id.clone());
                added.push(entry.id.clone());
            }
        }
        None => {
            if !ids.iter().any(|value| value.eq_ignore_ascii_case(id))
                && !missing.iter().any(|value| value.eq_ignore_ascii_case(id))
            {
                missing.push(id.to_string());
            }
        }
    }
    active.remove(&key);
}

/// Finds the Lua mod that belongs to a JAR mod, preferring `mod.info` id matches
/// and falling back to the mod's display name.
pub(crate) fn pick_lua_mod_id(
    entries: &[LuaModEntry],
    info_id: Option<&str>,
    fallback_name: &str,
) -> Option<String> {
    if let Some(target) = info_id.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(target))
        {
            return Some(entry.id.clone());
        }
    }
    let name = fallback_name.trim();
    if name.is_empty() {
        return None;
    }
    entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(name) || entry.name.eq_ignore_ascii_case(name))
        .map(|entry| entry.id.clone())
}

#[tauri::command]
pub(crate) fn read_lua_mod_poster(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !is_supported_image(&path) || !is_regular_file(&path) {
        return Err("Lua 模组海报不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取 Lua 模组海报失败: {error}"))
}

#[tauri::command(async)]
pub(crate) fn set_lua_mod_enabled(mod_id: String, enabled: bool) -> Result<LuaModToggleResult, String> {
    let target_id = mod_id.trim();
    if target_id.is_empty() {
        return Err("缺少模组 ID".to_string());
    }
    let entries = lua_mod_entries()?;
    let Some(target) = entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(target_id))
    else {
        return Err(format!("未找到模组 {target_id}"));
    };
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let mut ids = read_default_mod_ids(&default_path)?;
    let default_path_has_file = default_path.is_file();
    let backup_default = |path: &Path| -> Result<(), String> {
        if default_path_has_file {
            let backup_dir = user_zomboid_directory()?
                .join(".pzmanager")
                .join("default-backups")
                .join(current_unix_timestamp()?);
            backup_file(path, &backup_dir, "default.txt")?;
        }
        Ok(())
    };

    if !enabled {
        ids.retain(|id| !id.eq_ignore_ascii_case(&target.id));
        let dependents = entries
            .iter()
            .filter(|entry| ids.iter().any(|id| id.eq_ignore_ascii_case(&entry.id)))
            .filter(|entry| {
                entry
                    .requires
                    .iter()
                    .any(|require| require.eq_ignore_ascii_case(&target.id))
            })
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        backup_default(&default_path)?;
        write_default_mod_ids(&default_path, &ids)?;
        return Ok(LuaModToggleResult {
            enabled: false,
            load_order: None,
            added: Vec::new(),
            missing_requires: Vec::new(),
            dependents,
        });
    }

    if !target.has_b42 {
        return Err(format!("{} 不是 B42 模组，无法启用", target.id));
    }

    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut active = HashSet::new();
    plan_lua_mod_enable(
        &entries,
        &target.id,
        &mut ids,
        &mut added,
        &mut missing_requires,
        &mut active,
    );
    let ordered = order_mod_ids_by_requires(&entries, &ids);
    backup_default(&default_path)?;
    write_default_mod_ids(&default_path, &ordered)?;
    let load_order = ordered
        .iter()
        .position(|id| id.eq_ignore_ascii_case(&target.id))
        .map(|index| index + 1);
    Ok(LuaModToggleResult {
        enabled: true,
        load_order,
        added,
        missing_requires,
        dependents: Vec::new(),
    })
}

#[tauri::command(async)]
pub(crate) fn set_lua_mods_enabled(
    mod_ids: Vec<String>,
    enabled: bool,
) -> Result<LuaModBatchToggleResult, String> {
    let mut requested = Vec::new();
    for id in mod_ids {
        let id = id.trim();
        if !id.is_empty()
            && !requested
                .iter()
                .any(|value: &String| value.eq_ignore_ascii_case(id))
        {
            requested.push(id.to_string());
        }
    }
    if requested.is_empty() {
        return Err("没有选中 Lua 模组".to_string());
    }

    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let original = read_default_mod_ids(&default_path)?;
    let mut ids = original.clone();
    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut dependents = Vec::new();
    let mut skipped = Vec::new();

    let changed = if !enabled {
        let target_ids = requested
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let changed = original
            .iter()
            .filter(|id| target_ids.contains(&id.to_ascii_lowercase()))
            .cloned()
            .collect();
        ids.retain(|id| !target_ids.contains(&id.to_ascii_lowercase()));
        let remaining_enabled = ids
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        dependents = entries
            .iter()
            .filter(|entry| remaining_enabled.contains(&entry.id.to_ascii_lowercase()))
            .filter(|entry| {
                entry
                    .requires
                    .iter()
                    .any(|require| target_ids.contains(&require.to_ascii_lowercase()))
            })
            .map(|entry| entry.id.clone())
            .collect();
        changed
    } else {
        let mut active = HashSet::new();
        for id in requested {
            let Some(target) = entries
                .iter()
                .find(|entry| entry.id.eq_ignore_ascii_case(&id))
            else {
                skipped.push(id);
                continue;
            };
            if !target.has_b42 {
                skipped.push(target.id.clone());
                continue;
            }
            plan_lua_mod_enable(
                &entries,
                &target.id,
                &mut ids,
                &mut added,
                &mut missing_requires,
                &mut active,
            );
        }
        added.clone()
    };

    let ordered = order_mod_ids_by_requires(&entries, &ids);
    let unchanged = original.len() == ordered.len()
        && original
            .iter()
            .zip(&ordered)
            .all(|(left, right)| left.eq_ignore_ascii_case(right));
    if !unchanged {
        if default_path.is_file() {
            let backup_dir = user_zomboid_directory()?
                .join(".pzmanager")
                .join("default-backups")
                .join(current_unix_timestamp()?);
            backup_file(&default_path, &backup_dir, "default.txt")?;
        }
        write_default_mod_ids(&default_path, &ordered)?;
    }

    Ok(LuaModBatchToggleResult {
        enabled,
        changed,
        added,
        missing_requires,
        dependents,
        skipped,
    })
}

#[tauri::command(async)]
pub(crate) fn save_lua_mod_order(mod_ids: Vec<String>) -> Result<Vec<String>, String> {
    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    if !default_path.is_file() {
        return Err("default.txt 不存在，无法排序".to_string());
    }
    let existing = read_default_mod_ids(&default_path)?;
    let mut merged: Vec<String> = Vec::new();
    for id in &mod_ids {
        let id = id.trim();
        if id.is_empty()
            || !existing.iter().any(|value| value.eq_ignore_ascii_case(id))
            || merged.iter().any(|value| value.eq_ignore_ascii_case(id))
        {
            continue;
        }
        merged.push(id.to_string());
    }
    for id in existing {
        if !merged.iter().any(|value| value.eq_ignore_ascii_case(&id)) {
            merged.push(id);
        }
    }
    let ordered = order_mod_ids_by_requires(&entries, &merged);
    let backup_dir = user_zomboid_directory()?
        .join(".pzmanager")
        .join("default-backups")
        .join(current_unix_timestamp()?);
    backup_file(&default_path, &backup_dir, "default.txt")?;
    write_default_mod_ids(&default_path, &ordered)?;
    Ok(ordered)
}
