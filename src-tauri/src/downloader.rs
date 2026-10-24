use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{stream::FuturesUnordered, StreamExt};
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

static NEXT_DOWNLOAD_ID: AtomicU64 = AtomicU64::new(1);
const DOWNLOAD_EVENT: &str = "shader-download-progress";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    download_id: String,
    file_name: String,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

fn safe_file_name(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.contains('/') || trimmed.contains('\\') {
        return Err("下载文件名无效".to_string());
    }
    let name = Path::new(trimmed)
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or_default();
    if name.is_empty() || name == "." || name == ".." {
        return Err("下载文件名无效".to_string());
    }
    Ok(name.to_string())
}

fn file_name_from_url(url: &reqwest::Url) -> String {
    url.path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .and_then(|segment| safe_file_name(segment).ok())
        .filter(|name| name != "." && name != "..")
        .unwrap_or_else(|| "download.zip".to_string())
}

fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|part| part.to_str());
    let mut candidate = directory.join(file_name);
    let mut index = 2;
    while candidate.exists() {
        let name = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        candidate = directory.join(name);
        index += 1;
    }
    candidate
}

fn parse_total_length(content_range: Option<&reqwest::header::HeaderValue>) -> Option<u64> {
    let value = content_range?.to_str().ok()?;
    value.rsplit_once('/')?.1.parse().ok()
}

fn report_progress(
    app: &AppHandle,
    download_id: &str,
    file_name: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
) {
    let _ = app.emit(
        DOWNLOAD_EVENT,
        DownloadProgress {
            download_id: download_id.to_string(),
            file_name: file_name.to_string(),
            downloaded_bytes,
            total_bytes,
        },
    );
}

pub(crate) async fn download_file(
    app: AppHandle,
    download_id: String,
    url: String,
    file_name: String,
    directory: String,
    thread_count: usize,
) -> Result<String, String> {
    if !(1..=16).contains(&thread_count) {
        return Err("下载线程数必须在 1 到 16 之间".to_string());
    }
    let parsed_url =
        reqwest::Url::parse(url.trim()).map_err(|error| format!("下载链接无效: {error}"))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err("下载链接只支持 HTTP 或 HTTPS".to_string());
    }
    let file_name = if file_name.trim().is_empty() {
        file_name_from_url(&parsed_url)
    } else {
        safe_file_name(&file_name)?
    };
    let directory = PathBuf::from(directory.trim());
    if directory.as_os_str().is_empty() {
        return Err("下载目录不能为空".to_string());
    }
    fs::create_dir_all(&directory).map_err(|error| format!("创建下载目录失败: {error}"))?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let internal_id = NEXT_DOWNLOAD_ID.fetch_add(1, Ordering::Relaxed);
    let work_dir = directory.join(format!(".pzmanager-download-{timestamp}-{internal_id}"));
    fs::create_dir(&work_dir).map_err(|error| format!("创建下载临时目录失败: {error}"))?;
    let client = reqwest::Client::new();
    let result = async {
        let probe = client
            .get(parsed_url.clone())
            .header(RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(|error| format!("连接下载服务器失败: {error}"))?;
        if probe.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let total = parse_total_length(probe.headers().get(CONTENT_RANGE))
                .ok_or_else(|| "下载服务器未返回有效的文件大小".to_string())?;
            let _ = probe.bytes().await;
            if total == 0 {
                return Err("下载文件为空".to_string());
            }
            let workers = thread_count.min(total as usize).max(1);
            let chunk_size = total.div_ceil(workers as u64);
            let downloaded = Arc::new(AtomicU64::new(0));
            let mut pending = FuturesUnordered::new();
            for index in 0..workers {
                let start = index as u64 * chunk_size;
                if start >= total {
                    break;
                }
                let end = (start + chunk_size - 1).min(total - 1);
                let client = client.clone();
                let url = parsed_url.clone();
                let part_path = work_dir.join(format!("part-{index:02}"));
                let app = app.clone();
                let download_id = download_id.clone();
                let file_name = file_name.clone();
                let downloaded = Arc::clone(&downloaded);
                pending.push(async move {
                    let response = client
                        .get(url)
                        .header(RANGE, format!("bytes={start}-{end}"))
                        .send()
                        .await
                        .map_err(|error| format!("下载分段失败: {error}"))?
                        .error_for_status()
                        .map_err(|error| format!("下载分段失败: {error}"))?;
                    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                        return Err("服务器中断了分段下载".to_string());
                    }
                    let mut output = File::create(&part_path)
                        .map_err(|error| format!("创建下载分段失败: {error}"))?;
                    let mut response = response;
                    while let Some(bytes) = response
                        .chunk()
                        .await
                        .map_err(|error| format!("接收下载数据失败: {error}"))?
                    {
                        output
                            .write_all(&bytes)
                            .map_err(|error| format!("写入下载分段失败: {error}"))?;
                        let count = downloaded.fetch_add(bytes.len() as u64, Ordering::Relaxed)
                            + bytes.len() as u64;
                        report_progress(&app, &download_id, &file_name, count, Some(total));
                    }
                    output
                        .flush()
                        .map_err(|error| format!("刷新下载分段失败: {error}"))?;
                    if fs::metadata(&part_path)
                        .map_err(|error| format!("检查下载分段失败: {error}"))?
                        .len()
                        != end - start + 1
                    {
                        return Err("下载分段大小校验失败".to_string());
                    }
                    Ok::<_, String>(part_path)
                });
            }
            let mut parts = Vec::new();
            while let Some(part) = pending.next().await {
                parts.push(part?);
            }
            parts.sort();
            let temp_file = work_dir.join("complete.download");
            let mut output = File::create(&temp_file)
                .map_err(|error| format!("创建临时下载文件失败: {error}"))?;
            for part in parts {
                let mut input =
                    File::open(part).map_err(|error| format!("读取下载分段失败: {error}"))?;
                io::copy(&mut input, &mut output)
                    .map_err(|error| format!("合并下载分段失败: {error}"))?;
            }
            output
                .flush()
                .map_err(|error| format!("刷新下载文件失败: {error}"))?;
            let destination = unique_destination(&directory, &file_name);
            fs::rename(&temp_file, &destination)
                .map_err(|error| format!("保存下载文件失败: {error}"))?;
            if fs::metadata(&destination)
                .map_err(|error| format!("检查下载文件失败: {error}"))?
                .len()
                != total
            {
                return Err("下载文件大小校验失败".to_string());
            }
            Ok::<_, String>(destination.to_string_lossy().into_owned())
        } else {
            let response = probe
                .error_for_status()
                .map_err(|error| format!("下载请求失败: {error}"))?;
            let total = response.content_length();
            let temp_file = work_dir.join("complete.download");
            let mut output = File::create(&temp_file)
                .map_err(|error| format!("创建临时下载文件失败: {error}"))?;
            let mut downloaded = 0u64;
            let mut response = response;
            while let Some(bytes) = response
                .chunk()
                .await
                .map_err(|error| format!("接收下载数据失败: {error}"))?
            {
                output
                    .write_all(&bytes)
                    .map_err(|error| format!("写入下载文件失败: {error}"))?;
                downloaded += bytes.len() as u64;
                report_progress(&app, &download_id, &file_name, downloaded, total);
            }
            output
                .flush()
                .map_err(|error| format!("刷新下载文件失败: {error}"))?;
            let destination = unique_destination(&directory, &file_name);
            fs::rename(&temp_file, &destination)
                .map_err(|error| format!("保存下载文件失败: {error}"))?;
            Ok(destination.to_string_lossy().into_owned())
        }
    }
    .await;
    let _ = fs::remove_dir_all(&work_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::{file_name_from_url, safe_file_name, unique_destination};
    use std::path::Path;
    #[test]
    fn rejects_download_paths_in_file_names() {
        assert!(safe_file_name("../outside.zip").is_err());
        assert_eq!(safe_file_name("shader.zip").unwrap(), "shader.zip");
    }
    #[test]
    fn derives_a_file_name_from_a_custom_url() {
        let url =
            reqwest::Url::parse("https://example.test/files/MyShader.zip?download=1").unwrap();
        assert_eq!(file_name_from_url(&url), "MyShader.zip");
    }
    #[test]
    fn creates_a_non_overwriting_destination() {
        let directory = Path::new("downloads");
        assert_eq!(
            unique_destination(directory, "shader.zip"),
            directory.join("shader.zip")
        );
    }
}
