//! Steam 请求的中转（镜像）源管理：设置页「通用 › Steam 网络加速」。
//!
//! 三个 Steam 端点（工坊浏览 HTML、工坊详情、开发者新闻）统一走
//! [`send_with_relay`]：按设置的模式给出「直连 / 中转」候选地址，顺序尝试，
//! 全部失败才报错（资源中心的镜像 + 官方兜底同一套路）。自动模式下直连
//! 失败会熔断一段时间（照 `news_translation` 的 Google 熔断思路），大陆
//! 用户不用每次都先白等一次直连超时；中转故障时对称地临时切回直连。
//!
//! 中转服务本体是 `steam-relay/server.js`（零依赖 Node 服务器，部署在
//! 自己的海外服务器上，见 steam-relay/README.md）。workers.dev 等域名
//! 在大陆被墙，中转域名必须大陆可达。

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 默认中转地址：自建中转服务器（steam-relay/ 项目，部署在 pzmlan.sanyec.top）。
/// 换服务器 = 改这里 + 同步 `src/steamRelay.ts` 的 DEFAULT_STEAM_RELAY_BASE；
/// 用户也能在设置页覆盖。
pub(crate) const DEFAULT_RELAY_BASE: &str = "https://pzmlan.sanyec.top";
/// 设置页「Steam 网络加速」的三种模式。
pub(crate) const RELAY_MODES: [&str; 3] = ["auto", "relay", "direct"];
pub(crate) const DEFAULT_RELAY_MODE: &str = "auto";
/// 直连熔断时长：自动模式下直连失败后，这段时间内中转优先。
const LINK_BACKOFF: Duration = Duration::from_secs(600);

/// Steam 图片 CDN 主机白名单。Steam 会按出口地区返回不同 CDN 域名
/// （2026-10 实测工坊预览图走 images.steamusercontent.com / fastly 系，
/// cloudflare / akamai 系是其它地区的变体），所以全都放行。
/// 与 `steam-relay/server.js` 的 `IMAGE_HOSTS`、`src/steamRelay.ts` 的
/// `STEAM_IMAGE_HOSTS` 保持一致；改这里必须同步另外两处。
pub(crate) const STEAM_IMAGE_HOSTS: [&str; 10] = [
    "images.steamusercontent.com",
    "cdn.fastly.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "store.steamstatic.com",
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

/// 本地代理归一化：接受 `127.0.0.1:7897`（无 scheme 时补 http://）、
/// http/https/socks5 三种 scheme；其它值视为无效返回 None。
pub(crate) fn normalize_steam_proxy(proxy: &str) -> Option<String> {
    let trimmed = proxy.trim();
    if trimmed.is_empty() {
        return None;
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let (scheme, rest) = with_scheme.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "socks5" | "socks5h") {
        return None;
    }
    let rest = rest.trim_end_matches('/');
    // 必须带端口：本地代理几乎都是 127.0.0.1:port 形态，缺端口基本是填错了。
    let (host, port) = rest.rsplit_once(':')?;
    if host.is_empty() || port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
        return None;
    }
    Some(format!("{scheme}://{rest}"))
}

/// 读取设置页「Steam 网络加速 › 本地代理」；空 / 无效 = 不用显式代理
/// （退回 reqwest 默认的 Windows 系统代理 / 环境变量）。
pub(crate) fn configured_proxy() -> Option<String> {
    crate::app_settings_path()
        .ok()
        .and_then(|path| crate::load_app_settings_at(&path).ok())
        .and_then(|settings| settings.steam_proxy)
        .and_then(|proxy| normalize_steam_proxy(&proxy))
}

/// Steam 请求统一从这里取客户端：
/// - 官方直连（`relay = false`）：配了本地代理就用它，否则保持默认行为
///   （reqwest 自动读系统代理 / 环境变量，开着系统代理型加速器时直接受益）；
/// - 中转 / 健康检查（`relay = true`）：始终 `.no_proxy()`。中转服务器在大陆
///   直连可达，不能被失效的残留系统代理（如加速器异常退出留下的死端口）拖死。
pub(crate) fn steam_client(relay: bool) -> reqwest::Client {
    let builder = reqwest::Client::builder();
    if relay {
        return builder.no_proxy().build().unwrap_or_else(|_| reqwest::Client::new());
    }
    match configured_proxy().and_then(|proxy| reqwest::Proxy::all(&proxy).ok()) {
        Some(proxy) => builder
            .proxy(proxy)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new()),
        None => builder.build().unwrap_or_else(|_| reqwest::Client::new()),
    }
}

/// 本地代理是否可用（设置页「自动检测」）：并行探常见 HTTP/SOCKS 端口，
/// 只有真能访问 Steam 的才算通过。返回可用代理地址 + 耗时，按耗时升序。
const COMMON_PROXY_PORTS: [u16; 7] = [7897, 7890, 1080, 10808, 10809, 2080, 7891];

pub(crate) async fn detect_local_proxies() -> Vec<ProxyProbe> {
    // 探测目标用新闻 API（体积小、国内直连通常通但被墙时必失败）。
    const PROBE_URL: &str =
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid=108600&count=1&maxlength=1&format=json";
    let mut tasks = Vec::new();
    for port in COMMON_PROXY_PORTS {
        tasks.push(tokio::spawn(async move {
            let proxy_url = format!("http://127.0.0.1:{port}");
            let Ok(proxy) = reqwest::Proxy::all(&proxy_url) else {
                return None;
            };
            let client = reqwest::Client::builder().proxy(proxy).build().ok()?;
            let started = Instant::now();
            let response = client
                .get(PROBE_URL)
                .timeout(Duration::from_secs(3))
                .send()
                .await;
            let latency_ms = started.elapsed().as_millis() as u64;
            match response {
                Ok(response) if response.status().is_success() => Some(ProxyProbe {
                    proxy: proxy_url,
                    latency_ms,
                }),
                _ => None,
            }
        }));
    }
    let mut found = Vec::new();
    for task in tasks {
        if let Ok(Some(probe)) = task.await {
            found.push(probe);
        }
    }
    found.sort_by_key(|probe| probe.latency_ms);
    found
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProxyProbe {
    pub(crate) proxy: String,
    pub(crate) latency_ms: u64,
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

/// 供需要「响应级校验」的调用方（工坊列表要确认 HTML 真能解析出 SSR 数据，
/// 解析失败同样该换源）自己循环候选源用；每个源跑完调 `note_source_result`
/// 上报成败，熔断语义与 [`send_with_relay`] 完全一致。
pub(crate) fn sources_for(official_url: &str) -> Vec<SteamSource> {
    let (mode, base) = relay_config();
    steam_sources(official_url, &mode, &base)
}

pub(crate) fn note_source_result(source: &SteamSource, ok: bool) {
    let breaker = if source.relay {
        &RELAY_BREAKER
    } else {
        &DIRECT_BREAKER
    };
    if ok {
        breaker.enable();
    } else {
        breaker.disable();
    }
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
        let request = build(steam_client(source.relay), &source.url).timeout(timeout);
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
                note_source_result(&source, true);
                return Ok(response);
            }
            Err(error) => {
                note_source_result(&source, false);
                errors.push(error);
            }
        }
    }
    Err(errors.join("; "))
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_relay_base, normalize_relay_mode, normalize_steam_proxy, relay_url_for,
        steam_sources_with, DEFAULT_RELAY_BASE, DEFAULT_RELAY_MODE,
    };

    #[test]
    fn normalizes_local_proxy_input() {
        // 只有地址没写 scheme 时补 http://（最常见的填法）。
        assert_eq!(
            normalize_steam_proxy("127.0.0.1:7897").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            normalize_steam_proxy("  HTTP://127.0.0.1:7897/ ").as_deref(),
            Some("http://127.0.0.1:7897")
        );
        assert_eq!(
            normalize_steam_proxy("socks5://127.0.0.1:1080").as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        // 无效值：空、缺端口、不支持的 scheme、端口不是数字。
        assert_eq!(normalize_steam_proxy(""), None);
        assert_eq!(normalize_steam_proxy("127.0.0.1"), None);
        assert_eq!(normalize_steam_proxy("ftp://127.0.0.1:21"), None);
        assert_eq!(normalize_steam_proxy("http://127.0.0.1:abc"), None);
    }

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
        // 工坊预览图实际返回的主机（2026-10 实测），必须放行。
        assert_eq!(
            relay_url_for(
                "https://pzmlan.sanyec.top",
                "https://images.steamusercontent.com/ugc/123/ABC/?imw=320"
            )
            .as_deref(),
            Some("https://pzmlan.sanyec.top/images/images.steamusercontent.com/ugc/123/ABC/?imw=320")
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
