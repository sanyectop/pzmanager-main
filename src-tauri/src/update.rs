use reqwest::Client;
use serde::{Deserialize, Serialize};

/// 启动器更新源:Cloudflare Pages 上的静态 api/latest.json(见仓库 update-api/ 目录)。
/// 换域名/换项目后,这里必须同步成实际地址,否则启动器永远查不到新版本。
pub(crate) const UPDATE_FEED_URL: &str = "https://pzmapi.sanyec.top/api/latest.json";

const UPDATE_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// update-api/api/latest.json 的字段镜像。除 version 外全部可缺省:
/// 旧版启动器遇到新增字段会整份解析失败,所以这里的解析必须向后兼容。
/// JSON 里的 `name` 字段只给更新页展示用,启动器不消费,serde 会直接忽略。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherUpdateFeed {
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) pub_date: Option<String>,
    #[serde(default)]
    pub(crate) notes: Option<String>,
    #[serde(default)]
    pub(crate) downloads: Option<LauncherUpdateDownloads>,
    #[serde(default)]
    pub(crate) release_page: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherUpdateDownloads {
    /// 123 云盘分享链接(国内直连),启动器「下载更新」按钮的默认跳转目标。
    #[serde(default)]
    pub(crate) cloud_drive: Option<String>,
    #[serde(default)]
    pub(crate) windows_x64_installer: Option<String>,
    #[serde(default)]
    pub(crate) windows_x64_portable: Option<String>,
}

/// 前端拿到的检查结果。updateAvailable=false 表示已是最新。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherUpdateStatus {
    pub(crate) current_version: String,
    pub(crate) latest_version: String,
    pub(crate) update_available: bool,
    pub(crate) pub_date: Option<String>,
    pub(crate) notes: Option<String>,
    /// 「下载更新」按钮的跳转地址:优先 123 云盘,缺省时回落 GitHub 安装包、便携版。
    pub(crate) download_url: Option<String>,
    pub(crate) release_page_url: Option<String>,
}

pub(crate) async fn fetch_launcher_update(feed_url: &str) -> Result<LauncherUpdateStatus, String> {
    let client = Client::builder()
        .user_agent(concat!("ProjectZomboidManager/", env!("CARGO_PKG_VERSION")))
        .timeout(UPDATE_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let feed = client
        .get(feed_url)
        .send()
        .await
        .map_err(|e| format!("更新源访问失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("更新源响应异常: {e}"))?
        .json::<LauncherUpdateFeed>()
        .await
        .map_err(|e| format!("更新信息解析失败: {e}"))?;
    build_update_status(feed)
}

fn build_update_status(feed: LauncherUpdateFeed) -> Result<LauncherUpdateStatus, String> {
    let latest_version = normalize_version(&feed.version);
    if latest_version.is_empty() {
        return Err("更新信息里缺少有效版本号。".to_string());
    }
    let current_version = normalize_version(env!("CARGO_PKG_VERSION"));
    let downloads = feed.downloads.unwrap_or_default();
    Ok(LauncherUpdateStatus {
        update_available: version_is_newer(&latest_version, &current_version),
        current_version,
        latest_version,
        pub_date: feed.pub_date,
        notes: feed.notes,
        download_url: downloads
            .cloud_drive
            .or(downloads.windows_x64_installer)
            .or(downloads.windows_x64_portable),
        release_page_url: feed.release_page,
    })
}

/// 去掉展示层的 v/V 前缀与首尾空白。
fn normalize_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .trim()
        .to_string()
}

/// candidate 比 current 严格更新才算「有更新」;相等(含同版本重发)不算。
pub(crate) fn version_is_newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == std::cmp::Ordering::Greater
}

/// 按 `.` 分段逐位比较数字;容忍 v 前缀、段内非数字后缀、段数不齐(缺段按 0)。
/// 版本号规则是 `{主版本}.{次版本}.{YYMMDD}`(见 docs/version-bump.md),纯数字段。
pub(crate) fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = version_components(left);
    let right_parts = version_components(right);
    for index in 0..left_parts.len().max(right_parts.len()) {
        let compared = left_parts
            .get(index)
            .copied()
            .unwrap_or(0)
            .cmp(&right_parts.get(index).copied().unwrap_or(0));
        if compared != std::cmp::Ordering::Equal {
            return compared;
        }
    }
    std::cmp::Ordering::Equal
}

fn version_components(version: &str) -> Vec<u64> {
    normalize_version(version)
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_date_style_versions() {
        assert!(version_is_newer("1.1.261101", "1.1.261025"));
        assert!(version_is_newer("1.2.261001", "1.1.261025"));
        assert!(version_is_newer("2.0.0", "1.9.261230"));
        assert!(!version_is_newer("1.1.261025", "1.1.261025"));
        assert!(!version_is_newer("1.0.261025", "1.1.261025"));
    }

    #[test]
    fn tolerates_prefixes_and_uneven_parts() {
        assert!(version_is_newer("v1.1.261101", "1.1.261025"));
        assert!(version_is_newer("1.2", "1.1.9"));
        assert!(!version_is_newer("1.2", "1.2.0"));
        // 段内带后缀时只取前导数字,便于灰度同一构建号。
        assert!(!version_is_newer("1.1.261025-beta", "1.1.261025"));
        assert!(version_is_newer("1.1.261101-rc1", "1.1.261025"));
    }

    #[test]
    fn builds_status_from_feed_json() {
        let feed = serde_json::from_str::<LauncherUpdateFeed>(
            r#"{
                "name": "PZManager",
                "version": "9.9.999999",
                "pubDate": "2026-10-25",
                "notes": "测试更新说明",
                "downloads": {
                    "cloudDrive": "https://pan.example.com/s/abc?pwd=PZMa",
                    "windowsX64Installer": "https://example.com/setup.exe",
                    "windowsX64Portable": "https://example.com/portable.zip"
                },
                "releasePage": "https://example.com/releases"
            }"#,
        )
        .unwrap();
        let status = build_update_status(feed).unwrap();
        assert!(status.update_available);
        assert_eq!(status.latest_version, "9.9.999999");
        // 有 123 云盘链接时,「下载更新」优先打开它,而不是 GitHub 直链。
        assert_eq!(status.download_url.as_deref(), Some("https://pan.example.com/s/abc?pwd=PZMa"));
        assert_eq!(status.release_page_url.as_deref(), Some("https://example.com/releases"));
        assert_eq!(status.pub_date.as_deref(), Some("2026-10-25"));
        assert_eq!(status.notes.as_deref(), Some("测试更新说明"));
    }

    #[test]
    fn download_url_falls_back_to_installer_without_cloud_drive() {
        let feed = serde_json::from_str::<LauncherUpdateFeed>(
            r#"{
                "version": "9.9.999999",
                "downloads": {
                    "windowsX64Installer": "https://example.com/setup.exe",
                    "windowsX64Portable": "https://example.com/portable.zip"
                }
            }"#,
        )
        .unwrap();
        let status = build_update_status(feed).unwrap();
        // 更新源还没部署 cloudDrive 字段时,回落到 GitHub 安装包直链,按钮仍可用。
        assert_eq!(status.download_url.as_deref(), Some("https://example.com/setup.exe"));

        let portable_only = serde_json::from_str::<LauncherUpdateFeed>(
            r#"{ "version": "9.9.999999", "downloads": { "windowsX64Portable": "https://example.com/portable.zip" } }"#,
        )
        .unwrap();
        let status = build_update_status(portable_only).unwrap();
        assert_eq!(status.download_url.as_deref(), Some("https://example.com/portable.zip"));
    }

    #[test]
    fn accepts_minimal_feed_and_rejects_blank_version() {
        let minimal = serde_json::from_str::<LauncherUpdateFeed>(r#"{ "version": "0.0.1" }"#).unwrap();
        let status = build_update_status(minimal).unwrap();
        assert!(!status.update_available);
        assert_eq!(status.download_url, None);

        let blank = serde_json::from_str::<LauncherUpdateFeed>(r#"{ "version": "  " }"#).unwrap();
        assert!(build_update_status(blank).is_err());
    }
}
