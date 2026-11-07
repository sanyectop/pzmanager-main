//! 任务中心的落盘：`%USERPROFILE%\Zomboid\.pzmanager\tasks.json`。
//!
//! 任务列表的模型与更新都由前端（`App.tsx` 的 `tasks` state）持有，
//! 后端只做不透明 JSON 文档的持久化——不镜像 `TaskItem` 结构，
//! 前端字段演进时不用同步 Rust。写坏 / 缺文件都不能影响启动：
//! 读取失败一律按「无历史任务」处理并记一条日志。

use crate::*;
use std::fs;
use std::path::PathBuf;

/// 历史任务上限：防止长期使用后 tasks.json 无限增长。
/// 只裁剪落盘内容，不动前端 state（界面上仍有「清除已完成」）。
pub(crate) const TASK_HISTORY_CAP: usize = 200;

fn tasks_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("tasks.json"))
}

/// 启动时读历史任务。文件不存在 / 解析失败都返回空列表——
/// tasks.json 属于可丢弃缓存，绝不能让它变成启动阻塞项。
#[tauri::command]
pub(crate) fn load_saved_tasks() -> Result<Vec<serde_json::Value>, String> {
    load_saved_tasks_at(&tasks_path()?)
}

fn load_saved_tasks_at(path: &std::path::Path) -> Result<Vec<serde_json::Value>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取任务历史失败: {error}"))?;
    match serde_json::from_str::<Vec<serde_json::Value>>(&content) {
        Ok(tasks) => Ok(tasks),
        Err(error) => {
            launcher_log::warn(&format!(
                "任务历史 tasks.json 解析失败（{error}），已按无历史任务处理。"
            ));
            Ok(Vec::new())
        }
    }
}

/// 保存任务列表（前端在任务变化后防抖调用）。非对象的条目直接丢弃，
/// 超出上限的旧任务裁掉；先写临时文件再改名，写一半崩溃不会留坏文件。
#[tauri::command]
pub(crate) fn save_tasks(tasks: Vec<serde_json::Value>) -> Result<(), String> {
    save_tasks_at(&tasks_path()?, tasks)
}

fn save_tasks_at(path: &std::path::Path, tasks: Vec<serde_json::Value>) -> Result<(), String> {
    let kept: Vec<serde_json::Value> = tasks
        .into_iter()
        .filter(|task| task.is_object())
        .take(TASK_HISTORY_CAP)
        .collect();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建任务历史目录失败: {error}"))?;
    }
    let content = serde_json::to_string_pretty(&kept)
        .map_err(|error| format!("序列化任务历史失败: {error}"))?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, format!("{content}\n"))
        .map_err(|error| format!("保存任务历史失败: {error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("保存任务历史失败: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tasks_at(path: &PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn roundtrips_tasks_through_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.json");
        write_tasks_at(
            &path,
            r#"[{"id":"t1","kind":"shader-download","title":"下载光影","status":"error"}]"#,
        );
        let loaded = load_saved_tasks_at(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0]["id"], "t1");
        assert_eq!(loaded[0]["kind"], "shader-download");
    }

    #[test]
    fn missing_or_corrupted_file_reads_as_empty() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("none").join("tasks.json");
        assert!(load_saved_tasks_at(&missing).unwrap().is_empty());

        let path = temp.path().join("tasks.json");
        write_tasks_at(&path, "{not json");
        assert!(load_saved_tasks_at(&path).unwrap().is_empty());
    }

    #[test]
    fn save_keeps_objects_only_and_trims_to_cap() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("tasks.json");
        let mut tasks: Vec<serde_json::Value> = Vec::new();
        for index in 0..(TASK_HISTORY_CAP + 30) {
            tasks.push(serde_json::json!({ "id": format!("t{index}"), "kind": "memory-clean" }));
        }
        // 非对象条目（字符串 / null）应当被丢弃。
        tasks.push(serde_json::json!("not a task"));
        tasks.push(serde_json::Value::Null);

        save_tasks_at(&path, tasks).unwrap();
        let loaded = load_saved_tasks_at(&path).unwrap();
        assert_eq!(loaded.len(), TASK_HISTORY_CAP);
        // 保留的是最新的前 CAP 条（调用方负责把新任务排前面）。
        assert_eq!(loaded[0]["id"], "t0");
        assert_eq!(loaded.last().unwrap()["id"], format!("t{}", TASK_HISTORY_CAP - 1));
        // 落盘是合法 JSON 且以换行结尾（与 settings.json 一致）。
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.ends_with("]\n"));
    }
}
