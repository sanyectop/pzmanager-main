//! Steam 请求的中转（镜像）源管理：设置页「通用 › Steam 网络加速」。
//!
//! 三个 Steam 端点（工坊浏览 HTML、工坊详情、开发者新闻）统一走
//! [`send_with_relay`]：按设置的模式给出「直连 / 中转」候选地址，顺序尝试，
//! 全部失败才报错（资源中心的镜像 + 官方兜底同一套路）。自动模式下直连
//! 失败会熔断一段时间（照 `news_translation` 的 Google 熔断思路），大陆
//! 用户不用每次都先白等一次直连超时；中转故障时对称地临时切回直连。
//!
//! 中转服务本体是 `update-api/functions/steam/[[path]].js`（Pages Function，
//! 与更新源同项目部署在 `pzmapi.sanyec.top/steam/*`）。workers.dev 域名在
//! 大陆被墙，自建中转必须用自有域名。

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 默认中转地址。与 `update-api/functions/steam/[[path]].js` 的路由前缀（`/steam`）
/// 对应；换地址 = 改这里 + 重新部署 Pages 项目 + 更新 README。
pub(crate) const DEFAULT_RELAY_BASE: &str = "https://pzmapi.sanyec.top/steam";
/// 设置页「Steam 网络加速」的三种模式。
pub(crate) const RELAY_MODES: [&str; 3] = ["auto", "relay", "direct"];
pub(crate) const DEFAULT_RELAY_MODE: &str = "auto";
/// 直连熔断时长：自动模式下直连失败后，这段时间内中转优先。
const LINK_BACKOFF: Duration = Duration::from_secs(600);

/// Steam 图片 CDN 主机白名单。与 `update-api/functions/steam/[[path]].js` 的
/// `IMAGE_HOSTS`、`src/steamRelay.ts` 的 `STEAM_IMAGE_HOSTS` 保持一致；
/// 改这里必须同步另外两处。
pub(crate) const STEAM_IMAGE_HOSTS: [&str; 6] = [
    "shared.cloudflare.steamstatic.com",
    "clan.cloudflare.steamstatic.com",
    "steamuserimages-a.akamaihd.net",
    "cdn.akamai.steamstatic.com",
    "avatars.steamstatic.com",
    "avatars.akamai.steamstatic.com",
];

/// 简单熔断器：某条链路失败后一段时间内不再优先尝试它。
/// 直连 / 中转各一个，互不影响。
struct LinkBreaker {
    disabled_until: Mutex<Option<Instant>>,
}

impl LinkBreaker {
    const fn new() -> Self {
        Self {
            disabled_until: Mutex::new(None),
        }
    }

    fn enabled(&self) -> bool {
        match self.disabled_until.lock() {
            Ok(until) => until.map_or(true, |deadline| Instant::now() >= deadline),
            // 锁中毒按可用处理：宁可多等一次超时，也不能永久放弃这条链路。
            Err(_) => true,
        }
    }

    fn disable(&self) {
        if let Ok(mut until) = self.disabled_until.lock() {
            *until = Some(Instant::now() + LINK_BACKOFF);
        }
    }

    fn enable(&self) {
        if let Ok(mut until) = self.disabled_until.lock() {
            *until = None;
        }
    }
}

static DIRECT_BREAKER: LinkBreaker = LinkBreaker::new();
static RELAY_BREAKER: LinkBreaker = LinkBreaker::new();

/// 设置值归一化：大小写不敏感地匹配三种模式，不认识返回 None。
pub(crate) fn normalize_relay_mode(mode: &str) -> Option<String> {
    let trimmed = mode.trim();
    RELAY_MODES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}

/// 中转地址归一化：去首尾空白和尾斜杠；空值回落内置默认地址。
pub(crate) fn normalize_relay_base(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_RELAY_BASE.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 当前生效的中转配置（模式 + 地址）：读 settings.json，读不到 / 没写过
/// 都用默认值。请求不频繁（工坊列表带防抖、新闻页手动刷新），每次现读即可。
pub(crate) fn relay_config() -> (String, String) {
    let settings = crate::app_settings_path()
        .ok()
        .and_then(|path| crate::load_app_settings_at(&path).ok());
    let mode = settings
        .as_ref()
        .and_then(|settings| settings.steam_relay_mode.as_deref())
        .and_then(normalize_relay_mode)
        .unwrap_or_else(|| DEFAULT_RELAY_MODE.to_string());
    let base = normalize_relay_base(
        settings
            .as_ref()
            .and_then(|settings| settings.steam_relay_base.as_deref())
            .unwrap_or(""),
    );
    (mode, base)
}

/// 把官方 URL 拆成 (主机, 含查询的路径)；不是 https:// 开头返回 None。
fn split_host_and_rest(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://")?;
    match rest.find('/') {
        Some(index) if index > 0 => Some((
            rest[..index].to_string(),
            rest[index..].to_string(),
        )),
        // 没有路径部分（裸主机名）按根路径处理。
        None if !rest.is_empty() => Some((rest.to_string(), "/".to_string())),
        _ => None,
    }
}

/// 把官方 Steam URL 改写成中转地址。只放行工坊社区 / Web API / 图片 CDN
/// 三类主机（与 Worker 端白名单一致），其它主机返回 None。
pub(crate) fn relay_url_for(base: &str, official_url: &str) -> Option<String> {
    let (host, path_and_query) = split_host_and_rest(official_url)?;
    if host == "steamcommunity.com" {
        Some(format!("{base}/community{path_and_query}"))
    } else if host == "api.steampowered.com" {
        Some(format!("{base}/api{path_and_query}"))
    } else if STEAM_IMAGE_HOSTS.contains(&host.as_str()) {
        Some(format!("{base}/images/{host}{path_and_query}"))
    } else {
        None
    }
}

/// 一个候选源：`url` 是完整请求地址，`relay` 标记走的是中转还是官方直连。
pub(crate) struct SteamSource {
    pub(crate) url: String,
    pub(crate) relay: bool,
}

/// 按模式排列候选源（熔断状态由参数传入，方便纯函数测试）：
/// - direct：只直连，不给中转机会；
/// - relay：中转优先、直连兜底（中转熔断期间直连顶上）；
/// - auto：直连优先（海外用户低延迟），直连熔断期间中转优先。
/// 官方主机不在白名单里（比如换端点了）时只剩直连一条。
pub(crate) fn steam_sources_with(
    official_url: &str,
    mode: &str,
    relay_base: &str,
    direct_enabled: bool,
    relay_enabled: bool,
) -> Vec<SteamSource> {
    let official = SteamSource {
        url: official_url.to_string(),
        relay: false,
    };
    let Some(relay_url) = relay_url_for(relay_base, official_url) else {
        return vec![official];
    };
    if mode == "direct" {
        return vec![official];
    }
    let relay = SteamSource {
        url: relay_url,
        relay: true,
    };
    let prefer_relay = if mode == "relay" {
        relay_enabled
    } else {
        !direct_enabled
    };
    if prefer_relay {
        vec![relay, official]
    } else {
        vec![official, relay]
    }
}

fn steam_sources(official_url: &str, mode: &str, relay_base: &str) -> Vec<SteamSource> {
    steam_sources_with(
        official_url,
        mode,
        relay_base,
        DIRECT_BREAKER.enabled(),
        RELAY_BREAKER.enabled(),
    )
}

/// 依次尝试候选源。`build` 负责给每个地址补各自的请求头 / 方法 / 表单
/// 参数（和中转无关的部分），这里统一处理超时、状态码检查、错误聚合和
/// 熔断状态翻转。返回第一个成功（2xx）的响应。
pub(crate) async fn send_with_relay(
    official_url: &str,
    timeout: Duration,
    build: impl Fn(reqwest::Client, &str) -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    let (mode, base) = relay_config();
    let mut errors = Vec::new();
    for source in steam_sources(official_url, &mode, &base) {
        let request = build(reqwest::Client::new(), &source.url).timeout(timeout);
        let response = request
            .send()
            .await
            .map_err(|error| format!("请求 Steam 失败: {error}"))
            .and_then(|response| {
                response
                    .error_for_status()
                    .map_err(|error| format!("请求 Steam 失败: {error}"))
            });
        match response {
            Ok(response) => {
                if source.relay {
                    RELAY_BREAKER.enable();
                } else {
                    DIRECT_BREAKER.enable();
                }
                return Ok(response);
            }
            Err(error) => {
                if source.relay {
                    RELAY_BREAKER.disable();
                } else {
                    DIRECT_BREAKER.disable();
                }
                errors.push(error);
            }
        }
    }
    Err(errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_relay_base, normalize_relay_mode, relay_url_for, steam_sources_with,
        DEFAULT_RELAY_BASE, DEFAULT_RELAY_MODE,
    };

    #[test]
    fn normalizes_relay_mode_case_insensitively() {
        assert_eq!(
            normalize_relay_mode(" Auto "),
            Some(DEFAULT_RELAY_MODE.to_string())
        );
        assert_eq!(normalize_relay_mode("relay").as_deref(), Some("relay"));
        assert_eq!(normalize_relay_mode("direct").as_deref(), Some("direct"));
        assert_eq!(normalize_relay_mode("mirror"), None);
        assert_eq!(normalize_relay_mode(""), None);
    }

    #[test]
    fn normalizes_relay_base_with_trailing_slash_and_empty() {
        assert_eq!(normalize_relay_base("  "), DEFAULT_RELAY_BASE);
        assert_eq!(
            normalize_relay_base("https://example.com/steam/"),
            "https://example.com/steam"
        );
        assert_eq!(
            normalize_relay_base("https://example.com/steam//"),
            "https://example.com/steam"
        );
        assert_eq!(
            normalize_relay_base("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn rewrites_community_api_and_image_urls() {
        assert_eq!(
            relay_url_for(
                "https://mirror.example/steam",
                "https://steamcommunity.com/workshop/browse/?appid=108600"
            )
            .as_deref(),
            Some("https://mirror.example/steam/community/workshop/browse/?appid=108600")
        );
        assert_eq!(
            relay_url_for(
                "https://mirror.example/steam",
                "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=108600"
            )
            .as_deref(),
            Some("https://mirror.example/steam/api/ISteamNews/GetNewsForApp/v2/?appid=108600")
        );
        assert_eq!(
            relay_url_for(
                "https://mirror.example/steam",
                "https://shared.cloudflare.steamstatic.com/ugc/123/preview.jpg"
            )
            .as_deref(),
            Some("https://mirror.example/steam/images/shared.cloudflare.steamstatic.com/ugc/123/preview.jpg")
        );
    }

    #[test]
    fn refuses_to_relay_unknown_hosts_and_http() {
        assert!(relay_url_for(
            "https://mirror.example/steam",
            "https://store.steampowered.com/app/108600"
        )
        .is_none());
        assert!(relay_url_for(
            "https://mirror.example/steam",
            "http://steamcommunity.com/workshop/browse/"
        )
        .is_none());
        assert!(relay_url_for("https://mirror.example/steam", "steamcommunity.com").is_none());
    }

    #[test]
    fn orders_sources_by_mode_and_breakers() {
        let official = "https://steamcommunity.com/workshop/browse/?p=1";

        // 仅直连：不构造中转候选。
        assert_eq!(
            steam_sources_with(official, "direct", "https://m/steam", true, true).len(),
            1
        );

        // 自动 + 直连可用：直连优先。
        let auto = steam_sources_with(official, "auto", "https://m/steam", true, true);
        assert_eq!(auto.len(), 2);
        assert_eq!(auto[0].url, official);
        assert!(!auto[0].relay);

        // 自动 + 直连熔断：中转优先。
        let auto_broken = steam_sources_with(official, "auto", "https://m/steam", false, true);
        assert!(auto_broken[0].relay);
        assert_eq!(auto_broken[0].url, "https://m/steam/community/workshop/browse/?p=1");

        // 中转优先；中转熔断时直连顶上。
        let relay_first = steam_sources_with(official, "relay", "https://m/steam", true, true);
        assert!(relay_first[0].relay);
        let relay_broken = steam_sources_with(official, "relay", "https://m/steam", true, false);
        assert!(!relay_broken[0].relay);
        // 中转优先时官方直连仍是兜底。
        assert_eq!(relay_first[1].url, official);
    }

    #[test]
    fn falls_back_to_direct_only_for_unknown_official_host() {
        // 白名单外的官方端点（未来新增端点忘了改 Worker）至少还能直连。
        let sources = steam_sources_with(
            "https://example.com/api",
            "relay",
            "https://m/steam",
            true,
            true,
        );
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].url, "https://example.com/api");
    }
}
