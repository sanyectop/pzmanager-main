//! 模组版本隔离：把 `versioning.rs` 的引擎按「每个模组一条独立版本链」复用。
//!
//! 存储布局（`<版本存储目录>/mods/` 下，每个模组一个独立版本库）：
//!
//! ```text
//! mods/<key>/
//! ├── base/<id>/                第一个完整快照
//! ├── checkpoints/<id>/         后续完整快照
//! ├── deltas/<from>__<to>/      增量包
//! ├── manifests/<id>.json       文件清单
//! ├── trash/restore-*/         还原时移出的多余文件
//! ├── tmp/
//! └── meta.json                 版本链元数据
//! ```
//!
//! `<key>`：创意工坊 `workshop-<workshopId>-<modId>`，本地 `local-<modId>`；
//! 拿不到可用 ID 时用路径哈希兜底。键不随 Steam 库搬家变化，同一个模组在
//! 不同库里都能找回自己的历史版本。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::versioning;

/// 一个物理模组根目录。和 `list_lua_mods` 的列表项不同：这里**不按模组 ID 合并**，
/// 本地副本与创意工坊副本各自是一条链。
#[derive(Debug, Clone)]
pub struct ModRoot {
    pub path: PathBuf,
    /// `local` / `workshop`
    pub source: String,
    pub workshop_id: Option<String>,
    pub mod_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionTarget {
    pub key: String,
    pub root_path: String,
    pub name: String,
    pub mod_id: String,
    pub source: String,
    pub workshop_id: Option<String>,
    pub has_store: bool,
    pub version_count: usize,
    pub latest_version_id: Option<String>,
    pub latest_label: Option<String>,
    pub latest_created_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionStatus {
    pub target: ModVersionTarget,
    pub store_path: String,
    pub versions: Vec<versioning::VersionRecord>,
    pub current_match: versioning::VersionCurrentMatch,
    pub pending_changes: Option<versioning::VersionPendingChanges>,
    pub operation_running: bool,
}

/// 记录种类：「整合包 N」是一键保存全部模组（所有模组），
/// 「变更 N」是一键保存模组变更（只含当次有变化的模组）。还原范围就是记录里列出的模组。
pub const PACK_KIND_ALL: &str = "all";
pub const PACK_KIND_CHANGES: &str = "changes";

/// 一个「整合包 / 变更记录」＝某一时刻若干模组各自版本的集合。
/// 每条模组版本仍存在自己的链里，这里只保存引用。
/// `kind` 是后加的字段：旧 `modpacks.json` 里没有，读回时按整合包处理。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPackRecord {
    pub id: String,
    pub label: String,
    pub created_at: u64,
    #[serde(default = "mod_pack_kind_default")]
    pub kind: String,
    pub mods: Vec<versioning::ModVersionRef>,
}

fn mod_pack_kind_default() -> String {
    PACK_KIND_ALL.to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModPackFile {
    #[serde(default = "mod_pack_format")]
    format: u32,
    #[serde(default)]
    packs: Vec<ModPackRecord>,
}

fn mod_pack_format() -> u32 {
    1
}

fn mod_pack_path(store_base: &Path) -> PathBuf {
    store_base.join("modpacks.json")
}

pub fn read_packs(store_base: &Path) -> Result<Vec<ModPackRecord>, String> {
    let path = mod_pack_path(store_base);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(&path).map_err(|error| format!("读取整合包记录失败：{error}"))?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    let file: ModPackFile =
        serde_json::from_str(&content).map_err(|error| format!("解析整合包记录失败：{error}"))?;
    Ok(file.packs)
}

pub fn write_packs(store_base: &Path, packs: &[ModPackRecord]) -> Result<(), String> {
    fs::create_dir_all(store_base).map_err(|error| format!("创建整合包目录失败：{error}"))?;
    let file = ModPackFile {
        format: mod_pack_format(),
        packs: packs.to_vec(),
    };
    let content = serde_json::to_string_pretty(&file)
        .map_err(|error| format!("序列化整合包记录失败：{error}"))?;
    let path = mod_pack_path(store_base);
    let temp = store_base.join("modpacks.json.tmp");
    fs::write(&temp, format!("{content}\n"))
        .map_err(|error| format!("写入整合包记录失败：{error}"))?;
    fs::rename(&temp, &path).map_err(|error| format!("保存整合包记录失败：{error}"))
}

/// 模组 ID / 工坊 ID 只保留文件系统安全的字符，其余折成 `_`。
fn sanitize_id(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }
    let sanitized = sanitized.trim_matches(|character| character == '_' || character == '.');
    sanitized.chars().take(48).collect()
}

/// 路径兜底哈希：规范化分隔符 + 小写，取 SHA-256 前 8 位。
fn path_hash(path: &Path) -> String {
    let normalized = path
        .to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    digest.iter().take(4).map(|byte| format!("{byte:02x}")).collect()
}

/// 模组版本库的目录名。
pub fn store_key(root: &ModRoot) -> String {
    let mod_id = sanitize_id(&root.mod_id);
    if root.source == "workshop" {
        let workshop_id = root
            .workshop_id
            .as_deref()
            .map(sanitize_id)
            .filter(|value| !value.is_empty());
        return match workshop_id {
            Some(workshop_id) => {
                let suffix = if mod_id.is_empty() {
                    path_hash(&root.path)
                } else {
                    mod_id
                };
                format!("workshop-{workshop_id}-{suffix}")
            }
            None => format!("workshop-{}", path_hash(&root.path)),
        };
    }
    if mod_id.is_empty() {
        format!("local-{}", path_hash(&root.path))
    } else {
        format!("local-{mod_id}")
    }
}

/// 某个模组的独立版本库目录。
pub fn store_path(store_base: &Path, root: &ModRoot) -> PathBuf {
    store_base.join("mods").join(store_key(root))
}

fn target_for(store_base: &Path, root: &ModRoot) -> (ModVersionTarget, Vec<versioning::VersionRecord>) {
    let store = store_path(store_base, root);
    let versions = versioning::read_versions(&store).unwrap_or_default();
    let latest = versions.last();
    let target = ModVersionTarget {
        key: store_key(root),
        root_path: root.path.to_string_lossy().into_owned(),
        name: root.name.clone(),
        mod_id: root.mod_id.clone(),
        source: root.source.clone(),
        workshop_id: root.workshop_id.clone(),
        has_store: !versions.is_empty(),
        version_count: versions.len(),
        latest_version_id: latest.map(|record| record.id.clone()),
        latest_label: latest.map(|record| record.label.clone()),
        latest_created_at: latest.map(|record| record.created_at),
    };
    (target, versions)
}

/// 列表只读各版本库的 `meta.json`，不扫描模组目录本身。
pub fn list_targets(store_base: &Path, roots: &[ModRoot]) -> Vec<ModVersionTarget> {
    let mut targets: Vec<ModVersionTarget> = roots
        .iter()
        .map(|root| target_for(store_base, root).0)
        .collect();
    targets.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.key.cmp(&right.key))
    });
    targets
}

/// 单个模组的完整状态：会做一次「路径 + 大小」扫描来识别当前目录属于哪个版本。
pub fn status(
    store_base: &Path,
    root: &ModRoot,
    operation_running: bool,
) -> Result<ModVersionStatus, String> {
    let store = store_path(store_base, root);
    let (target, versions) = target_for(store_base, root);
    let status = versioning::get_status(
        Some(&root.path),
        &store,
        None,
        "mods",
        operation_running,
    )?;
    Ok(ModVersionStatus {
        target,
        store_path: store.to_string_lossy().into_owned(),
        versions,
        current_match: status.current_match,
        pending_changes: status.pending_changes,
        operation_running,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root(path: &str, source: &str, workshop_id: Option<&str>, mod_id: &str) -> ModRoot {
        ModRoot {
            path: PathBuf::from(path),
            source: source.to_string(),
            workshop_id: workshop_id.map(str::to_string),
            mod_id: mod_id.to_string(),
            name: mod_id.to_string(),
        }
    }

    #[test]
    fn store_keys_are_stable_and_source_specific() {
        let workshop = root("H:\\Steam\\content\\108600\\3809306528\\mods\\Viewpoint", "workshop", Some("3809306528"), "Viewpoint");
        assert_eq!(store_key(&workshop), "workshop-3809306528-viewpoint");

        let local = root("C:\\Users\\x\\Zomboid\\mods\\MyMod", "local", None, "MyMod");
        assert_eq!(store_key(&local), "local-mymod");

        // 大小写与空白不会改变键
        let loud = root("C:\\Users\\x\\Zomboid\\mods\\MyMod", "local", None, "  MY MOD  ");
        assert_eq!(store_key(&loud), "local-my_mod");

        // 同一个模组换 Steam 库，键不变
        let moved = root("D:\\SteamLibrary\\steamapps\\workshop\\content\\108600\\3809306528\\mods\\Viewpoint", "workshop", Some("3809306528"), "viewpoint");
        assert_eq!(store_key(&moved), store_key(&workshop));
    }

    #[test]
    fn non_ascii_or_missing_ids_fall_back_to_a_path_hash() {
        let chinese = root("C:\\Users\\x\\Zomboid\\mods\\我的模组", "local", None, "");
        let first = store_key(&chinese);
        let second = store_key(&chinese);
        assert_eq!(first, second);
        assert!(first.starts_with("local-"));
        assert_eq!(first.len(), "local-".len() + 8);

        let other = root("C:\\Users\\x\\Zomboid\\mods\\另一个", "local", None, "");
        assert_ne!(first, store_key(&other));
    }

    #[test]
    fn list_targets_reads_meta_without_scanning_mod_directories() {
        let directory = tempfile::tempdir().unwrap();
        let store_base = directory.path().join("versions");
        let mod_root = root("C:\\mods\\Alpha", "local", None, "Alpha");
        let store = store_path(&store_base, &mod_root);
        fs::create_dir_all(&store).unwrap();
        fs::write(
            store.join("meta.json"),
            r#"{
  "format": 1,
  "versions": [
    {
      "id": "v1",
      "parentId": null,
      "steamBuildId": null,
      "branch": "mods",
      "baseType": "full",
      "createdAt": 1700000000,
      "rootHash": "sha256:aa",
      "metadataHash": "sha256:bb",
      "fileCount": 3,
      "totalBytes": 42,
      "label": "基准版本",
      "changeLog": null
    }
  ],
  "lastSeenBuildId": null,
  "activeVersionId": "v1"
}"#,
        )
        .unwrap();

        // 模组目录根本不存在，列表仍然能读出元数据。
        let targets = list_targets(&store_base, &[mod_root.clone()]);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].has_store);
        assert_eq!(targets[0].version_count, 1);
        assert_eq!(targets[0].latest_version_id.as_deref(), Some("v1"));
        assert_eq!(targets[0].latest_created_at, Some(1_700_000_000));
    }

    #[test]
    fn mod_status_scans_a_small_directory() {
        let directory = tempfile::tempdir().unwrap();
        let store_base = directory.path().join("versions");
        let mod_root = directory.path().join("mods").join("Alpha");
        fs::create_dir_all(&mod_root).unwrap();
        fs::write(mod_root.join("mod.info"), b"name=Alpha\nid=Alpha\n").unwrap();

        let root = root(&mod_root.to_string_lossy(), "local", None, "Alpha");
        let before = status(&store_base, &root, false).unwrap();
        assert!(!before.target.has_store);
        assert_eq!(before.current_match.kind, "none");

        versioning::create_full_snapshot(
            &root.path,
            &store_path(&store_base, &root),
            None,
            "mods",
            Some("测试"),
            true,
            &mut |_| Ok(()),
        )
        .unwrap();

        let after = status(&store_base, &root, false).unwrap();
        assert_eq!(after.target.version_count, 1);
        assert_eq!(after.current_match.version_id, after.target.latest_version_id);
    }

    #[test]
    fn pack_records_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        let store_base = directory.path().join("versions");
        assert!(read_packs(&store_base).unwrap().is_empty());

        let reference = versioning::ModVersionRef {
            key: "local-alpha".to_string(),
            mod_id: "Alpha".to_string(),
            name: "Alpha".to_string(),
            source: "local".to_string(),
            workshop_id: None,
            version_id: "v1".to_string(),
            root_path: "C:\\mods\\Alpha".to_string(),
        };
        let record = ModPackRecord {
            id: "pack-1".to_string(),
            label: "整合包 1".to_string(),
            created_at: 1_700_000_000,
            kind: PACK_KIND_ALL.to_string(),
            mods: vec![reference.clone()],
        };
        write_packs(&store_base, std::slice::from_ref(&record)).unwrap();

        let loaded = read_packs(&store_base).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "pack-1");
        assert_eq!(loaded[0].label, "整合包 1");
        assert_eq!(loaded[0].kind, PACK_KIND_ALL);
        assert_eq!(loaded[0].mods.len(), 1);
        assert_eq!(loaded[0].mods[0].version_id, "v1");

        // 变更记录（一键保存模组变更写的）也要原样读回。
        let changes = ModPackRecord {
            id: "pack-2".to_string(),
            label: "变更 1".to_string(),
            created_at: 1_700_000_100,
            kind: PACK_KIND_CHANGES.to_string(),
            mods: vec![reference],
        };
        write_packs(&store_base, &[changes]).unwrap();
        let loaded = read_packs(&store_base).unwrap();
        assert_eq!(loaded[0].kind, PACK_KIND_CHANGES);
        assert_eq!(loaded[0].label, "变更 1");

        // 旧格式（加 kind 之前的 modpacks.json）没有 kind 字段：读回按整合包处理。
        fs::write(
            store_base.join("modpacks.json"),
            r#"{
  "format": 1,
  "packs": [
    {
      "id": "pack-old",
      "label": "整合包 1",
      "createdAt": 1700000000,
      "mods": []
    }
  ]
}"#,
        )
        .unwrap();
        let legacy = read_packs(&store_base).unwrap();
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].id, "pack-old");
        assert_eq!(legacy[0].kind, PACK_KIND_ALL);

        write_packs(&store_base, &[]).unwrap();
        assert!(read_packs(&store_base).unwrap().is_empty());
    }
}
