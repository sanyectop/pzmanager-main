//! 启动前模组兼容性预检：Java 模组字节码引用了当前游戏 jar 里**已不存在**的类时
//! 提前报告，避免进游戏后才发现闪退。
//!
//! 背景（2026-11-11）：42.22.2 移除了 `zombie.util.list.PZArrayList`，旧版
//! Viewpoint 插件 DGVViewpointEyes 的字段描述符仍引用它；ZombieBuddy 在
//! `LuaManager.init` 阶段反射暴露该类时抛 `NoClassDefFoundError`，游戏在进入
//! 主菜单前秒退。这类"模组与游戏版本不匹配"无法靠版本号判断（模组自己声明的
//! `versionMin` 可能早已满足），但可以**直接比对字节码引用与游戏类表**。
//!
//! 数据来源全部复用现有能力：
//! * 模组清单：`jar_scan_inputs()` / `jar_scan_candidates()`（只扫磁盘上启用的
//!   `mod.info`，被禁用（改名 `mod.info.disabled`）的模组天然不参与）；
//! * 类引用：`class_references()`（常量池里的 `zombie.*` / `fmod.*`）；
//! * 游戏类表：直接读 `projectzomboid.jar` 的中央目录。
//!
//! 结果按「jar 路径 + size + mtime」缓存到 `.pzmanager/mod-compat-cache.json`，
//! 热启动只做 stat + 读缓存；游戏更新（jar size/mtime 变化）会让类表缓存失效。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::*;

const CACHE_VERSION: u32 = 2;
const CACHE_FILE: &str = "mod-compat-cache.json";
/// 每个模组最多展示多少个缺失类（完整列表在日志里）。
const MAX_MISSING_PER_MOD: usize = 8;

/// 一个「引用了当前游戏不存在类」的已启用 Java 模组。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModCompatIssue {
    pub(crate) mod_id: String,
    pub(crate) name: String,
    pub(crate) workshop_id: Option<u64>,
    pub(crate) mod_info_path: String,
    pub(crate) jar_path: String,
    pub(crate) missing_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModCompatReport {
    pub(crate) issues: Vec<ModCompatIssue>,
    /// 本次检查的已启用 Java 模组数。
    pub(crate) scanned: usize,
    pub(crate) duration_ms: u64,
    /// 类表与模组引用是否全部命中缓存（首次/游戏更新后为 false）。
    pub(crate) cache_hit: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatCache {
    version: u32,
    game: Option<CachedGameClasses>,
    jars: HashMap<String, CachedJarRefs>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedGameClasses {
    fingerprint: String,
    classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedJarRefs {
    size: u64,
    mtime: u64,
    refs: Vec<String>,
}

fn cache_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join(CACHE_FILE))
}

fn load_cache() -> CompatCache {
    let Ok(path) = cache_path() else {
        return CompatCache::default();
    };
    let Ok(content) = fs::read_to_string(path) else {
        return CompatCache::default();
    };
    let Ok(cache) = serde_json::from_str::<CompatCache>(&content) else {
        return CompatCache::default();
    };
    if cache.version != CACHE_VERSION {
        return CompatCache::default();
    }
    cache
}

fn save_cache(cache: &CompatCache) {
    let Ok(path) = cache_path() else {
        return;
    };
    if let Ok(content) = serde_json::to_string(cache) {
        let _ = fs::write(path, content);
    }
}

fn mtime_seconds(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// `projectzomboid.jar` 的类名集合（点分形式）。
///
/// 读 zip 中央目录即可，不解压；按 `size-mtime` 缓存，命中时直接返回。
fn game_classes(
    jar_path: &Path,
    cache: &mut CompatCache,
) -> Result<(Vec<String>, bool), String> {
    let metadata = fs::metadata(jar_path)
        .map_err(|error| format!("读取游戏 jar 失败（{}）：{error}", jar_path.display()))?;
    let fingerprint = format!("{}-{}", metadata.len(), mtime_seconds(&metadata));
    if let Some(cached) = cache
        .game
        .as_ref()
        .filter(|cached| cached.fingerprint == fingerprint)
    {
        return Ok((cached.classes.clone(), true));
    }

    let file = fs::File::open(jar_path)
        .map_err(|error| format!("打开游戏 jar 失败（{}）：{error}", jar_path.display()))?;
    let archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("解析游戏 jar 失败（{}）：{error}", jar_path.display()))?;
    let mut classes = BTreeSet::new();
    for name in archive.file_names() {
        let Some(stem) = name.strip_suffix(".class") else {
            continue;
        };
        if stem.is_empty() || stem.starts_with("META-INF/") || stem.starts_with("module-info") {
            continue;
        }
        classes.insert(stem.replace('/', "."));
    }
    let classes: Vec<String> = classes.into_iter().collect();
    cache.game = Some(CachedGameClasses {
        fingerprint,
        classes: classes.clone(),
    });
    Ok((classes, false))
}

/// 某个模组 jar 引用到的游戏类（`zombie.*` / `fmod.*`），带缓存。
fn jar_references(
    jar_path: &Path,
    cache: &mut CompatCache,
    seen: &mut HashSet<String>,
    cache_hits: &mut usize,
    cache_misses: &mut usize,
) -> Vec<String> {
    let Ok(metadata) = fs::metadata(jar_path) else {
        return Vec::new();
    };
    let size = metadata.len();
    let mtime = mtime_seconds(&metadata);
    let key = jar_path.to_string_lossy().to_ascii_lowercase();
    seen.insert(key.clone());
    if let Some(cached) = cache.jars.get(&key) {
        if cached.size == size && cached.mtime == mtime {
            *cache_hits += 1;
            return cached.refs.clone();
        }
    }

    *cache_misses += 1;
    let refs = read_jar_class_references(jar_path);
    cache.jars.insert(
        key,
        CachedJarRefs {
            size,
            mtime,
            refs: refs.clone(),
        },
    );
    refs
}

/// 读一个 jar 里所有 class 的**类级**引用（并集）。
///
/// 只认两种精确来源，避免把常量池里的方法名/日志字符串误报成类：
/// * `CONSTANT_Class` 条目（二进制名，如 `zombie/util/list/PZArrayList`）；
/// * 字段/方法描述符里的 `Lzombie/...;` 类型（如
///   `Lzombie/util/list/PZArrayList;`——DGVViewpointEyes 的崩溃就是这种）。
fn read_jar_class_references(jar_path: &Path) -> Vec<String> {
    let Ok(file) = fs::File::open(jar_path) else {
        return Vec::new();
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return Vec::new();
    };
    let mut references = BTreeSet::new();
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        if !entry.name().ends_with(".class") {
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size().min(4 * 1024 * 1024) as usize);
        if std::io::Read::read_to_end(&mut entry, &mut bytes).is_err() {
            continue;
        }
        for reference in strict_class_references(&bytes) {
            references.insert(reference);
        }
    }
    references.into_iter().collect()
}

/// 描述符里的 `Lzombie/...;` 类型引用（含内部类 `$`）。
fn descriptor_class_pattern() -> &'static regex::Regex {
    static PATTERN: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"L((?:zombie|fmod)/[A-Za-z0-9_$]+(?:/[A-Za-z0-9_$]+)*)\s*;")
            .expect("描述符类引用正则必须能编译")
    })
}

/// 一个 class 文件里**确实会触发类加载**的游戏类引用。
fn strict_class_references(class_bytes: &[u8]) -> Vec<String> {
    use java_class_rs::ConstantPoolEntry;

    let Ok((_, class_file)) = java_class_rs::parse_classfile(class_bytes) else {
        return Vec::new();
    };
    let pool = &class_file.constant_pool;
    let mut references = BTreeSet::new();
    for entry in pool {
        match entry {
            ConstantPoolEntry::Class(info) => {
                if let Some(name) = java_class_rs::get_utf8(pool, info.name_index) {
                    let dotted = name.trim_start_matches('[').replace('/', ".");
                    if is_game_class_name(&dotted) {
                        references.insert(dotted);
                    }
                }
            }
            ConstantPoolEntry::Utf8(info) => {
                for capture in descriptor_class_pattern().captures_iter(&info.value) {
                    references.insert(capture[1].replace('/', "."));
                }
            }
            _ => {}
        }
    }
    references.into_iter().collect()
}

/// 是否是游戏类（`zombie.*` / `fmod.*`），排除数组前缀与空名。
fn is_game_class_name(name: &str) -> bool {
    let name = name.trim_start_matches('[');
    (name.starts_with("zombie.") || name.starts_with("fmod."))
        && !name.ends_with('.')
        && !name.contains(';')
}

/// 引用名是否是"包名"而不是类名：游戏里存在以 `name.` 开头的类时视为包。
///
/// 常量池里也会出现 `zombie.Lua` 这类包名字符串；直接当缺失类会误报。
fn looks_like_package(name: &str, classes: &HashSet<String>) -> bool {
    let prefix = format!("{name}.");
    classes.iter().any(|class| class.starts_with(&prefix))
}

/// 启动前兼容性预检（前端启动按钮点击后、真正拉起游戏前调用）。
#[tauri::command(async)]
pub(crate) fn check_mod_compatibility() -> Result<ModCompatReport, String> {
    let started = Instant::now();
    let game_path = project_zomboid_install_path()
        .ok_or_else(|| "找不到游戏安装目录，无法检查模组兼容性。".to_string())?;
    let jar_path = game_path.join("projectzomboid.jar");
    if !jar_path.is_file() {
        return Err(format!(
            "找不到游戏 jar（{}），无法检查模组兼容性。",
            jar_path.display()
        ));
    }

    let mut cache = load_cache();
    let (classes, game_cache_hit) = game_classes(&jar_path, &mut cache)?;
    let class_index: HashSet<String> = classes.into_iter().collect();

    let roots = jar_scan_inputs();
    let candidates = jar_scan_candidates(&roots);
    let mut issues = Vec::new();
    let mut scanned = 0usize;
    let mut cache_hits = 0usize;
    let mut cache_misses = 0usize;
    let mut seen_jars: HashSet<String> = HashSet::new();

    for (info_path, info, workshop_id) in candidates {
        // 只算磁盘上启用的模组：禁用的 mod.info 被改名成 mod.info.disabled，
        // 候选里虽然会保留（用于列表展示），但启动前预检不关心它们。
        if info_path.file_name().and_then(|name| name.to_str()) != Some("mod.info") {
            continue;
        }
        let Some(mod_jar) = mod_jar_path(&info_path, &info) else {
            continue;
        };
        scanned += 1;
        let refs = jar_references(&mod_jar, &mut cache, &mut seen_jars, &mut cache_hits, &mut cache_misses);
        let missing: Vec<String> = refs
            .into_iter()
            .filter(|name| !class_index.contains(name))
            .filter(|name| !looks_like_package(name, &class_index))
            .take(MAX_MISSING_PER_MOD)
            .collect();
        if missing.is_empty() {
            continue;
        }
        issues.push(ModCompatIssue {
            mod_id: info.id.clone(),
            name: info.name.clone(),
            workshop_id,
            mod_info_path: info_path.to_string_lossy().into_owned(),
            jar_path: mod_jar.to_string_lossy().into_owned(),
            missing_classes: missing,
        });
    }

    // 缓存只保留本次仍在用的 jar，避免卸载模组后无限增长。
    cache.jars.retain(|key, _| seen_jars.contains(key));
    cache.version = CACHE_VERSION;
    save_cache(&cache);

    let duration_ms = started.elapsed().as_millis() as u64;
    let cache_hit = game_cache_hit && cache_misses == 0;
    if issues.is_empty() {
        launcher_log::info(&format!(
            "模组兼容性预检：{scanned} 个已启用 Java 模组未发现缺失类引用\
             （耗时 {duration_ms} ms，缓存命中 {cache_hits}，失效 {cache_misses}）。"
        ));
    } else {
        let detail: Vec<String> = issues
            .iter()
            .map(|issue| format!("{}（{}）", issue.name, issue.missing_classes.join("、")))
            .collect();
        launcher_log::warn(&format!(
            "模组兼容性预检：{} 个模组引用了当前游戏已不存在的类：{}。\
             （扫描 {scanned} 个，耗时 {duration_ms} ms）",
            issues.len(),
            detail.join("；")
        ));
    }
    Ok(ModCompatReport {
        issues,
        scanned,
        duration_ms,
        cache_hit,
    })
}

/// 启动前一键禁用不兼容模组（沿用现有 `mod.info` → `mod.info.disabled` 机制，
/// 可随时在模组页重新启用）。
#[tauri::command(async)]
pub(crate) fn disable_mods_for_launch(mod_info_paths: Vec<String>) -> Result<usize, String> {
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    let mut disabled = 0usize;
    for raw in mod_info_paths {
        let path = PathBuf::from(raw.trim());
        if !path.is_file() {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap_or_default();
        let info = parse_java_mod_info(&content);
        let mod_dir = path
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mod_id = info
            .as_ref()
            .map(|info| info.id.clone())
            .filter(|id| !id.trim().is_empty())
            .or_else(|| {
                path.parent()
                    .and_then(|parent| parent.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| mod_dir.clone());
        let package_name = info
            .as_ref()
            .map(|info| info.package_name.clone())
            .unwrap_or_default();
        let workshop_id = workshop_id_from_info_path(&path);
        set_jar_mod_info_enabled(&path.to_string_lossy(), false)?;
        upsert_jar_mod_state_entry(
            &mut state,
            JarModStateEntry {
                mod_dir,
                mod_id,
                workshop_id,
                package_name,
                disabled: true,
            },
        );
        disabled += 1;
    }
    if disabled > 0 {
        write_jar_mod_state(&state_path, &state)?;
        launcher_log::info(&format!("启动前已禁用 {disabled} 个不兼容模组。"));
    }
    Ok(disabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_zip_entry(zip: &mut zip::ZipWriter<std::fs::File>, name: &str, bytes: &[u8]) {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }

    #[test]
    fn reads_game_class_index_from_jar() {
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("projectzomboid.jar");
        let file = fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        write_zip_entry(&mut zip, "zombie/Lua/LuaManager.class", b"x");
        write_zip_entry(&mut zip, "zombie/util/list/PZArrayUtil.class", b"x");
        write_zip_entry(&mut zip, "META-INF/PROJECTZ.SF", b"x");
        write_zip_entry(&mut zip, "media/lua/shared/a.lua", b"x");
        zip.finish().unwrap();

        let mut cache = CompatCache::default();
        let (classes, cache_hit) = game_classes(&jar, &mut cache).unwrap();
        assert!(!cache_hit);
        assert!(classes.contains(&"zombie.Lua.LuaManager".to_string()));
        assert!(classes.contains(&"zombie.util.list.PZArrayUtil".to_string()));
        // META-INF / 非 class 条目不算类。
        assert!(!classes.iter().any(|class| class.contains("META-INF")));
        assert!(!classes.iter().any(|class| class.ends_with(".lua")));

        // 同一份 jar 第二次命中缓存。
        let (classes_again, cache_hit_again) = game_classes(&jar, &mut cache).unwrap();
        assert!(cache_hit_again);
        assert_eq!(classes_again, classes);
    }

    #[test]
    fn package_names_are_not_reported_as_missing_classes() {
        // 常量池里会出现包名字符串（如 zombie.Lua）；游戏里有
        // zombie.Lua.LuaManager 时它不能算"缺失类"。
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("projectzomboid.jar");
        let file = fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        write_zip_entry(&mut zip, "zombie/Lua/LuaManager.class", b"x");
        zip.finish().unwrap();
        let mut cache = CompatCache::default();
        let (classes, _) = game_classes(&jar, &mut cache).unwrap();
        let index: HashSet<String> = classes.into_iter().collect();
        assert!(looks_like_package("zombie.Lua", &index));
        // 42.22 移除的类没有同名子类，不会被当成包。
        assert!(!looks_like_package("zombie.util.list.PZArrayList", &index));
    }

    /// 真实环境冒烟：读本机游戏 jar + 已启用 Java 模组，确认预检能跑通并
    /// 报出引用已移除类的模组（2026-11-11：DGVViewpointEyes → PZArrayList）。
    /// 依赖本机 Steam 库，默认忽略；手动运行：
    /// `cargo test --lib real_world_preflight -- --ignored --nocapture`
    #[test]
    #[ignore = "依赖本机游戏目录与已启用模组"]
    fn real_world_preflight_runs_and_reports() {
        let report = check_mod_compatibility().expect("预检应成功");
        eprintln!(
            "scanned={} issues={} duration={}ms cache_hit={}",
            report.scanned, report.issues.len(), report.duration_ms, report.cache_hit
        );
        for issue in &report.issues {
            eprintln!("{} ({}) -> {:?}", issue.name, issue.mod_id, issue.missing_classes);
        }
        assert!(report.scanned > 0, "本机应能扫到已启用的 Java 模组");
    }

    #[test]
    fn jar_reference_cache_invalidates_on_change() {
        let directory = tempfile::tempdir().unwrap();
        let jar = directory.path().join("mod.jar");
        fs::write(&jar, b"not a zip yet").unwrap();
        let mut cache = CompatCache::default();
        let mut seen = HashSet::new();
        let (mut hits, mut misses) = (0usize, 0usize);
        // 非 zip：读不出引用，但缓存条目会记下 size/mtime。
        let first = jar_references(&jar, &mut cache, &mut seen, &mut hits, &mut misses);
        assert!(first.is_empty());
        assert_eq!(misses, 1);
        let cached = cache
            .jars
            .values()
            .next()
            .cloned()
            .expect("应写入缓存");
        // 同尺寸 + 同 mtime（秒级）视为未变化。
        let mut cache2 = CompatCache::default();
        cache2.jars.insert(
            jar.to_string_lossy().to_ascii_lowercase(),
            CachedJarRefs {
                size: cached.size,
                mtime: cached.mtime,
                refs: vec!["zombie.removed.Class".to_string()],
            },
        );
        let (mut hits2, mut misses2) = (0usize, 0usize);
        let refs = jar_references(&jar, &mut cache2, &mut seen, &mut hits2, &mut misses2);
        assert_eq!(refs, vec!["zombie.removed.Class".to_string()]);
        assert_eq!(hits2, 1);
        assert_eq!(misses2, 0);
    }
}
