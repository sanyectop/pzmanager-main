use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::news::TranslationJob;

const APP_ID: u64 = 108600;
const CACHE_TTL: Duration = Duration::from_secs(300);
const STEAM_BASE_URL: &str = "https://steamcommunity.com/workshop/browse/";
const STEAM_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";
const STEAM_ACCEPT_LANGUAGE: &str = "zh-CN,zh;q=0.9";
const STEAM_REFERER: &str = "https://steamcommunity.com/";
/// 项目详情走官方 Web API：一次 POST 就能拿到 file_description 全文，
/// 不必像列表那样去解析详情页的 SSR 数据。
const STEAM_FILE_DETAILS_API: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
const STEAM_DETAIL_TIMEOUT: Duration = Duration::from_secs(25);
const STEAM_HTML_TIMEOUT: Duration = Duration::from_secs(25);

static HTML_CACHE: OnceLock<Mutex<HashMap<String, CachedHtml>>> = OnceLock::new();

#[derive(Debug)]
struct CachedHtml {
    created_at: Instant,
    html: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopFilters {
    pub(crate) browsesort: String,
    pub(crate) days: u32,
    pub(crate) search_text: String,
    pub(crate) page: u32,
    pub(crate) num_per_page: u32,
    pub(crate) required_tags: Vec<String>,
    pub(crate) excluded_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopItem {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) short_description: String,
    pub(crate) preview_url: String,
    pub(crate) subscriptions: u64,
    pub(crate) favorited: u64,
    pub(crate) views: u64,
    pub(crate) star_rating: f64,
    pub(crate) total_votes: u64,
    pub(crate) file_size: String,
    pub(crate) time_created: u64,
    pub(crate) time_updated: u64,
    pub(crate) num_comments: u64,
    pub(crate) tags: Vec<String>,
    pub(crate) creator_id: String,
    pub(crate) creator_name: String,
    pub(crate) file_url: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopMeta {
    pub(crate) current_page: u64,
    pub(crate) total_pages: u64,
    pub(crate) total_count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopResponse {
    pub(crate) items: Vec<WorkshopItem>,
    pub(crate) meta: WorkshopMeta,
    pub(crate) steam_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopSortOption {
    pub(crate) value: String,
    pub(crate) label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopOptions {
    pub(crate) sort_options: Vec<WorkshopSortOption>,
    pub(crate) tags: Vec<String>,
}

pub(crate) fn workshop_options() -> WorkshopOptions {
    WorkshopOptions {
        sort_options: vec![
            ("trend", "最热门（趋势）"),
            ("mostrecent", "最新发布"),
            ("lastupdated", "最近更新"),
            ("totaluniquesubscribers", "订阅最多"),
            ("toprated", "评分最高"),
        ]
        .into_iter()
        .map(|(value, label)| WorkshopSortOption {
            value: value.to_string(),
            label: label.to_string(),
        })
        .collect(),
        tags: [
            "Build 40",
            "Build 41",
            "Build 42",
            "Animals",
            "Audio",
            "Balance",
            "Building",
            "Clothing/Armor",
            "Farming",
            "Food",
            "Framework",
            "Hardmode",
            "Interface",
            "Items",
            "Language/Translation",
            "Literature",
            "Map",
            "Military",
            "Misc",
            "Models",
            "Multiplayer",
            "Pop Culture",
            "QoL",
            "Realistic",
            "Silly/Fun",
            "Skills",
            "Textures",
            "Traits",
            "Vehicles",
            "Weapons",
            "WIP",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
    }
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else if byte == b' ' {
            encoded.push('+');
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

fn append_query_pair(query: &mut String, key: &str, value: impl ToString) {
    if !query.is_empty() {
        query.push('&');
    }
    query.push_str(&encode_query_component(key));
    query.push('=');
    query.push_str(&encode_query_component(&value.to_string()));
}

pub(crate) fn build_workshop_url(filters: &WorkshopFilters) -> String {
    let mut query = String::new();
    append_query_pair(&mut query, "appid", APP_ID);
    append_query_pair(&mut query, "section", "readytouseitems");
    append_query_pair(&mut query, "browsesort", &filters.browsesort);
    append_query_pair(&mut query, "p", filters.page);
    append_query_pair(&mut query, "num_per_page", filters.num_per_page);

    if filters.browsesort == "trend" {
        append_query_pair(&mut query, "days", filters.days);
    }
    if !filters.search_text.is_empty() {
        append_query_pair(&mut query, "searchtext", &filters.search_text);
    }
    for tag in &filters.required_tags {
        append_query_pair(&mut query, "requiredtags[]", tag);
    }
    for tag in &filters.excluded_tags {
        append_query_pair(&mut query, "excludedtags[]", tag);
    }

    format!("{STEAM_BASE_URL}?{query}")
}

pub(crate) fn extract_render_context(html_text: &str) -> Option<Value> {
    extract_ssr_data_render_context(html_text)
        .or_else(|| extract_render_context_from_json_parse(html_text))
}

/// Steam currently embeds the SSR payload in a `<script id="valve-ssr-data">`
/// element as raw JSON, with the workshop query cache under `renderContext`.
fn extract_ssr_data_render_context(html_text: &str) -> Option<Value> {
    let marker_index = html_text
        .find("id=\"valve-ssr-data\"")
        .or_else(|| html_text.find("id='valve-ssr-data'"))?;
    let open_end = marker_index + html_text[marker_index..].find('>')? + 1;
    let close = open_end + html_text[open_end..].find("</script>")?;
    let payload = html_text.get(open_end..close)?.trim();
    let root: Value = serde_json::from_str(payload).ok()?;
    root.get("renderContext").cloned()
}

/// Older Steam responses assigned `window.SSR.renderContext = JSON.parse("...")`.
fn extract_render_context_from_json_parse(html_text: &str) -> Option<Value> {
    let marker = "window.SSR.renderContext";
    let marker_index = html_text.find(marker)?;
    let json_parse_index = html_text[marker_index..].find("JSON.parse(")? + marker_index;
    let start = json_parse_index + "JSON.parse(".len();

    let mut depth = 1;
    let mut in_string = false;
    let mut escaped = false;
    let mut end = None;

    for (offset, character) in html_text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + offset);
                    break;
                }
            }
            _ => {}
        }
    }

    let argument = html_text.get(start..end?)?.trim();
    let inner_json: String = serde_json::from_str(argument).ok()?;
    serde_json::from_str(&inner_json).ok()
}

fn value_as_string(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        _ => default.to_string(),
    }
}

fn value_as_u64(value: Option<&Value>, default: u64) -> u64 {
    match value {
        Some(Value::Number(value)) => value.as_u64().unwrap_or(default),
        Some(Value::String(value)) => value.parse().unwrap_or(default),
        _ => default,
    }
}

fn value_as_f64(value: Option<&Value>, default: f64) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(default),
        Some(Value::String(value)) => value.parse().unwrap_or(default),
        _ => default,
    }
}

pub(crate) fn parse_workshop_items(
    context: &Value,
) -> Result<(Vec<WorkshopItem>, WorkshopMeta), String> {
    let Some(query_data) = context.get("queryData").and_then(Value::as_str) else {
        return Ok((Vec::new(), WorkshopMeta::default()));
    };
    let query_data: Value = serde_json::from_str(query_data).unwrap_or(Value::Null);
    let Some(queries) = query_data.get("queries").and_then(Value::as_array) else {
        return Ok((Vec::new(), WorkshopMeta::default()));
    };

    let mut items = Vec::new();
    let mut meta = WorkshopMeta::default();

    for query in queries {
        let is_workshop_query = query
            .get("queryKey")
            .and_then(Value::as_array)
            .and_then(|query_key| query_key.first())
            .and_then(Value::as_str)
            == Some("workshop_browse");
        if !is_workshop_query {
            continue;
        }

        let data = query
            .get("state")
            .and_then(|state| state.get("data"))
            .filter(|data| !data.is_null())
            .unwrap_or(&Value::Null);
        meta = WorkshopMeta {
            current_page: value_as_u64(data.get("current_page"), 1),
            total_pages: value_as_u64(data.get("total_pages"), 1),
            total_count: value_as_u64(data.get("total_count"), 0),
        };

        let mut creator_names = HashMap::new();
        if let Some(creators) = data
            .get("creator_player_link_details")
            .and_then(Value::as_array)
        {
            for creator in creators {
                let Some(public_data) = creator.get("public_data") else {
                    continue;
                };
                let steam_id = value_as_string(public_data.get("steamid"), "");
                let persona_name = value_as_string(public_data.get("persona_name"), "");
                if !steam_id.is_empty() && !persona_name.is_empty() {
                    creator_names.insert(steam_id, persona_name);
                }
            }
        }

        let Some(results) = data.get("results").and_then(Value::as_array) else {
            continue;
        };
        for raw in results {
            let id = value_as_string(raw.get("publishedfileid"), "");
            if id.is_empty() {
                continue;
            }
            let tags = raw
                .get("tags")
                .and_then(Value::as_array)
                .map(|tags| {
                    tags.iter()
                        .filter_map(|tag| tag.get("tag").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let creator_id = value_as_string(raw.get("creator"), "");
            let creator_name = creator_names
                .get(&creator_id)
                .cloned()
                .unwrap_or_else(|| creator_id.clone());

            items.push(WorkshopItem {
                file_url: format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"),
                id,
                title: value_as_string(raw.get("title"), ""),
                short_description: value_as_string(raw.get("short_description"), ""),
                preview_url: value_as_string(raw.get("preview_url"), ""),
                subscriptions: value_as_u64(raw.get("subscriptions"), 0),
                favorited: value_as_u64(raw.get("favorited"), 0),
                views: value_as_u64(raw.get("views"), 0),
                star_rating: value_as_f64(raw.get("star_rating"), -1.0),
                total_votes: value_as_u64(raw.get("total_votes"), 0),
                file_size: value_as_string(raw.get("file_size"), "0"),
                time_created: value_as_u64(raw.get("time_created"), 0),
                time_updated: value_as_u64(raw.get("time_updated"), 0),
                num_comments: value_as_u64(raw.get("num_comments_public"), 0),
                tags,
                creator_id,
                creator_name,
            });
        }
    }

    Ok((items, meta))
}

fn cached_html(url: &str) -> Option<String> {
    let cache = HTML_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let cache = cache.lock().ok()?;
    let entry = cache.get(url)?;
    (entry.created_at.elapsed() < CACHE_TTL).then(|| entry.html.clone())
}

fn cache_html(url: &str, html: String) {
    let cache = HTML_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.insert(
            url.to_string(),
            CachedHtml {
                created_at: Instant::now(),
                html,
            },
        );
    }
}

async fn fetch_steam_html(url: &str) -> Result<String, String> {
    if let Some(html) = cached_html(url) {
        return Ok(html);
    }

    // 直连被墙时自动切中转（设置「Steam 网络加速」），见 steam_relay 模块。
    let response = crate::steam_relay::send_with_relay(url, STEAM_HTML_TIMEOUT, |client, url| {
        client
            .get(url)
            .header("User-Agent", STEAM_USER_AGENT)
            .header("Accept-Language", STEAM_ACCEPT_LANGUAGE)
            .header("Referer", STEAM_REFERER)
    })
    .await?;
    let html = response
        .text()
        .await
        .map_err(|error| format!("读取 Steam 响应失败: {error}"))?;
    cache_html(url, html.clone());
    Ok(html)
}

pub(crate) async fn fetch_workshop(filters: WorkshopFilters) -> Result<WorkshopResponse, String> {
    let steam_url = build_workshop_url(&filters);
    let html = fetch_steam_html(&steam_url).await?;
    let Some(context) = extract_render_context(&html) else {
        return Err("无法解析 Steam 页面".to_string());
    };
    let (items, meta) = parse_workshop_items(&context)?;
    Ok(WorkshopResponse {
        items,
        meta,
        steam_url,
    })
}

/// 创意工坊项目详情：「窗口内打开」的模态框用。
/// 正文复用开发者新闻那条翻译管线——`bbcode_to_html` 把 BBCode 转成 HTML，
/// `add_translation_spans` 再把正文切成 `<span data-sid>` 占位并产出翻译任务；
/// 前端拿同一批 jobs 调 `translate_developer_news_batch`，再按 sid 替换回译文。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkshopItemDetail {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) title_sid: String,
    pub(crate) description_html: String,
    pub(crate) preview_url: String,
    pub(crate) file_url: String,
    pub(crate) subscriptions: u64,
    pub(crate) time_created: u64,
    pub(crate) time_updated: u64,
    pub(crate) file_size: String,
    pub(crate) tags: Vec<String>,
    pub(crate) creator_id: String,
    pub(crate) translation_jobs: Vec<TranslationJob>,
    pub(crate) language: String,
}

fn parse_workshop_item_detail(raw: &Value, language: &str) -> Option<WorkshopItemDetail> {
    let id = value_as_string(raw.get("publishedfileid"), "");
    if id.is_empty() {
        return None;
    }
    let wants_translation = language.trim() != "en";
    let mut translation_jobs = Vec::new();

    // 标题和正文用同一个 sid 前缀（w<id>），前端只按 sid 替换，不关心来源。
    // 工坊标题是纯文本，方括号是模组自己写的（[B42]、[ALPHA]…），
    // 不能按 BBCode 剥掉——实测 "[B42]" 会被整段吃掉。
    let title = value_as_string(raw.get("title"), "").trim().to_string();
    let title_sid = format!("w{id}_title");
    if wants_translation && !title.trim().is_empty() {
        translation_jobs.push(TranslationJob {
            sid: title_sid.clone(),
            text: title.clone(),
        });
    }

    // 官方 GetPublishedFileDetails 把 BBCode 原文放在 description；
    // 新版 IPublishedFileService/GetDetails 叫 file_description。两个都认，
    // 免得上游改名后详情整段变空白。
    let raw_description = {
        let primary = value_as_string(raw.get("description"), "");
        if primary.trim().is_empty() {
            value_as_string(raw.get("file_description"), "")
        } else {
            primary
        }
    };
    let raw_html = crate::news::bbcode_to_html(&raw_description);
    let description_html = if wants_translation {
        crate::news_translation::add_translation_spans(
            &raw_html,
            &format!("w{id}"),
            &mut translation_jobs,
        )
    } else {
        raw_html
    };

    let tags = raw
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| tag.get("tag").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(WorkshopItemDetail {
        file_url: format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}"),
        title_sid: if wants_translation {
            title_sid
        } else {
            String::new()
        },
        title,
        description_html,
        preview_url: value_as_string(raw.get("preview_url"), ""),
        subscriptions: value_as_u64(raw.get("subscriptions"), 0),
        time_created: value_as_u64(raw.get("time_created"), 0),
        time_updated: value_as_u64(raw.get("time_updated"), 0),
        file_size: value_as_string(raw.get("file_size"), "0"),
        creator_id: value_as_string(raw.get("creator"), ""),
        tags,
        translation_jobs,
        language: language.trim().to_string(),
        id,
    })
}

pub(crate) async fn fetch_workshop_item_detail(
    id: String,
    language: String,
) -> Result<WorkshopItemDetail, String> {
    let id = id.trim().to_string();
    if id.is_empty() || !id.chars().all(|character| character.is_ascii_digit()) {
        return Err("创意工坊项目 ID 无效。".to_string());
    }
    let payload: Value = crate::steam_relay::send_with_relay(
        STEAM_FILE_DETAILS_API,
        STEAM_DETAIL_TIMEOUT,
        |client, url| {
            client
                .post(url)
                .header("User-Agent", STEAM_USER_AGENT)
                .header("Accept-Language", STEAM_ACCEPT_LANGUAGE)
                .form(&[
                    ("itemcount", "1".to_string()),
                    ("publishedfileids[0]", id.clone()),
                ])
        },
    )
    .await?
    .json()
    .await
    .map_err(|error| format!("解析 Steam 响应失败: {error}"))?;

    let raw = payload
        .get("response")
        .and_then(|response| response.get("publishedfiledetails"))
        .and_then(Value::as_array)
        .and_then(|details| details.first())
        .ok_or_else(|| "Steam 没有返回该项目详情。".to_string())?;
    if value_as_u64(raw.get("result"), 0) != 1 {
        return Err("Steam 上没有该项目（可能已下架或不是公开项目）。".to_string());
    }
    parse_workshop_item_detail(raw, &language)
        .ok_or_else(|| "Steam 返回的项目详情不完整。".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        build_workshop_url, extract_render_context, parse_workshop_item_detail,
        parse_workshop_items, WorkshopFilters,
    };

    #[test]
    fn builds_python_compatible_workshop_url() {
        let filters = WorkshopFilters {
            browsesort: "trend".to_string(),
            days: 7,
            search_text: "Brita's".to_string(),
            page: 2,
            num_per_page: 15,
            required_tags: vec!["Build 42".to_string(), "Weapons".to_string()],
            excluded_tags: vec!["WIP".to_string()],
        };

        assert_eq!(
            build_workshop_url(&filters),
            "https://steamcommunity.com/workshop/browse/?appid=108600&section=readytouseitems&browsesort=trend&p=2&num_per_page=15&days=7&searchtext=Brita%27s&requiredtags%5B%5D=Build+42&requiredtags%5B%5D=Weapons&excludedtags%5B%5D=WIP"
        );
    }

    #[test]
    fn builds_non_trend_url_without_days() {
        let filters = WorkshopFilters {
            browsesort: "mostrecent".to_string(),
            days: 7,
            search_text: String::new(),
            page: 1,
            num_per_page: 30,
            required_tags: Vec::new(),
            excluded_tags: Vec::new(),
        };

        assert!(!build_workshop_url(&filters).contains("days="));
    }

    #[test]
    fn extracts_json_parse_argument_with_nested_parentheses() {
        let html = r#"<script>
window.SSR.renderContext = JSON.parse("{\"queryData\":\"x(y)\"}");
</script>"#;

        assert_eq!(extract_render_context(html).unwrap()["queryData"], "x(y)");
    }

    #[test]
    fn returns_none_when_render_context_is_missing() {
        assert!(extract_render_context("<html>no context</html>").is_none());
    }

    #[test]
    fn extracts_render_context_from_valve_ssr_data_json() {
        // Mirrors the current Steam markup: a raw JSON <script> whose
        // renderContext.queryData is itself a JSON string.
        let payload = r#"{"loaderData":{"x":1},"renderContext":{"localizationSettings":{},"queryData":"{\"queries\":[{\"queryKey\":[\"workshop_browse\"],\"state\":{\"data\":{\"current_page\":1,\"total_pages\":4,\"total_count\":57,\"results\":[{\"publishedfileid\":\"4242\",\"title\":\"Fetched from ssr data\",\"creator\":\"7656119\",\"subscriptions\":\"7\",\"tags\":[{\"tag\":\"Build 42\"}]}]}}}]}"},"Config":{},"UserConfig":{}}"#;
        let html = format!(
            "<html><body><script nonce=\"abc\" id=\"valve-ssr-data\" type=\"application/json\">{payload}</script>\
             <script>(()=>{{const e=document.getElementById(\"valve-ssr-data\");window.SSR={{renderContext:JSON.parse(e.textContent).renderContext}};}})();</script></body></html>"
        );

        let context = extract_render_context(&html).unwrap();
        let (items, meta) = parse_workshop_items(&context).unwrap();

        assert_eq!(meta.current_page, 1);
        assert_eq!(meta.total_pages, 4);
        assert_eq!(meta.total_count, 57);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "4242");
        assert_eq!(items[0].title, "Fetched from ssr data");
        assert_eq!(items[0].subscriptions, 7);
        assert_eq!(items[0].tags, ["Build 42"]);
        assert_eq!(
            items[0].file_url,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=4242"
        );
    }

    #[test]
    fn maps_workshop_query_data_to_item_and_meta() {
        let context = serde_json::json!({
            "queryData": serde_json::json!({
                "queries": [{
                    "queryKey": ["workshop_browse"],
                    "state": {
                        "data": {
                            "current_page": 2,
                            "total_pages": 9,
                            "total_count": 243,
                            "creator_player_link_details": [{
                                "public_data": {
                                    "steamid": "7656119",
                                    "persona_name": "作者"
                                }
                            }],
                            "results": [{
                                "publishedfileid": "123",
                                "title": "测试模组",
                                "short_description": "说明",
                                "preview_url": "https://cdn.example/preview.jpg",
                                "subscriptions": 42,
                                "favorited": 3,
                                "views": 99,
                                "star_rating": 4,
                                "total_votes": 12,
                                "file_size": "2048",
                                "time_created": 100,
                                "time_updated": 200,
                                "num_comments_public": 6,
                                "creator": "7656119",
                                "tags": [{"tag": "Build 42"}, {"tag": "Weapons"}]
                            }]
                        }
                    }
                }]
            })
            .to_string()
        });

        let (items, meta) = parse_workshop_items(&context).unwrap();

        assert_eq!(meta.current_page, 2);
        assert_eq!(meta.total_pages, 9);
        assert_eq!(meta.total_count, 243);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "123");
        assert_eq!(items[0].creator_name, "作者");
        assert_eq!(items[0].tags, vec!["Build 42", "Weapons"]);
        assert_eq!(
            items[0].file_url,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=123"
        );
    }

    /// Steam Web API GetPublishedFileDetails 的返回体（字段与线上一致）。
    fn file_details_payload() -> serde_json::Value {
        serde_json::json!({
            "response": {
                "publishedfiledetails": [{
                    "publishedfileid": "4242",
                    "result": 1,
                    "creator": "7656119",
                    "title": "Brita's Weapon Pack [B42]",
                    "description": "[h1]说明[/h1][b]加粗[/b][url=https://example.com]链接[/url]\n[img]https://cdn.example/a.jpg[/img]",
                    "preview_url": "https://cdn.example/preview.jpg",
                    "subscriptions": 12,
                    "time_created": 100,
                    "time_updated": 200,
                    "file_size": "2048",
                    "tags": [{"tag": "Build 42"}, {"tag": "Weapons"}]
                }]
            }
        })
    }

    #[test]
    fn builds_detail_with_translation_spans() {
        let payload = file_details_payload();
        let raw = &payload["response"]["publishedfiledetails"][0];
        let detail = parse_workshop_item_detail(raw, "zh-CN").expect("detail");

        assert_eq!(detail.id, "4242");
        assert_eq!(detail.title, "Brita's Weapon Pack [B42]");
        assert_eq!(detail.title_sid, "w4242_title");
        assert_eq!(detail.subscriptions, 12);
        assert_eq!(detail.file_size, "2048");
        assert_eq!(detail.tags, vec!["Build 42", "Weapons"]);
        assert_eq!(
            detail.file_url,
            "https://steamcommunity.com/sharedfiles/filedetails/?id=4242"
        );

        // 标签与图片原样保留，正文进占位 span。
        assert!(detail.description_html.contains("<h1>"));
        assert!(detail.description_html.contains("<strong>"));
        assert!(detail
            .description_html
            .contains("<img src=\"https://cdn.example/a.jpg\""));
        assert!(detail
            .description_html
            .contains("href=\"https://example.com\""));
        assert!(detail.description_html.contains("data-sid=\"w4242_"));
        assert!(detail.description_html.contains("class=\"i18n-pending\""));
        assert!(!detail.description_html.contains("data-sid=\"w4242_title\""));

        // 每个占位 span 恰好对应一条翻译任务，标题额外占一条。
        let spans = detail.description_html.matches("data-sid=\"").count();
        assert!(spans >= 3);
        assert_eq!(spans + 1, detail.translation_jobs.len());
        assert_eq!(detail.translation_jobs[0].sid, "w4242_title");
        assert_eq!(detail.translation_jobs[0].text, "Brita's Weapon Pack [B42]");
        assert!(detail
            .translation_jobs
            .iter()
            .all(|job| job.sid.starts_with("w4242_")));
    }

    #[test]
    fn skips_translation_for_english_detail() {
        let payload = file_details_payload();
        let raw = &payload["response"]["publishedfiledetails"][0];
        let detail = parse_workshop_item_detail(raw, "en").expect("detail");

        assert!(detail.translation_jobs.is_empty());
        assert_eq!(detail.title_sid, "");
        assert!(!detail.description_html.contains("data-sid"));
        assert!(detail.description_html.contains("<strong>加粗</strong>"));
    }

    #[test]
    fn falls_back_to_file_description_field() {
        let raw = serde_json::json!({
            "publishedfileid": "7",
            "result": 1,
            "title": "T",
            "file_description": "[b]新字段名[/b]"
        });
        let detail = parse_workshop_item_detail(&raw, "en").expect("detail");

        assert!(detail
            .description_html
            .contains("<strong>新字段名</strong>"));
    }
}
