use regex::Regex;
use serde::{Deserialize, Serialize};

const APP_ID: u64 = 108600;
const STEAM_NEWS_API: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
const STEAM_STORE_BASE: &str = "https://store.steampowered.com";
const NEWS_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeveloperNewsItem {
    pub(crate) gid: String,
    pub(crate) title: String,
    pub(crate) title_sid: String,
    pub(crate) url: String,
    pub(crate) date: u64,
    pub(crate) author: String,
    pub(crate) feed_label: String,
    pub(crate) contents_html: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationJob {
    pub(crate) sid: String,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeveloperNewsResponse {
    pub(crate) app_id: u64,
    pub(crate) items: Vec<DeveloperNewsItem>,

    pub(crate) translation_jobs: Vec<TranslationJob>,
    pub(crate) language: String,
}

#[derive(Debug, Deserialize)]
struct SteamNewsResponse {
    appnews: SteamAppNews,
}

#[derive(Debug, Deserialize)]
struct SteamAppNews {
    #[serde(default)]
    newsitems: Vec<SteamNewsItem>,
}

#[derive(Debug, Deserialize)]
struct SteamNewsItem {
    gid: String,
    title: String,
    url: String,
    date: u64,
    #[serde(default)]
    author: String,
    #[serde(default)]
    feedlabel: String,
    #[serde(default)]
    contents: String,
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn safe_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.starts_with("/") {
        return format!("{STEAM_STORE_BASE}{trimmed}");
    }
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        return trimmed.to_string();
    }
    "#".to_string()
}

fn attr_url(raw: &str) -> String {
    escape_html(&safe_url(raw))
}

/// Tags we allow through from feeds that ship raw HTML instead of BBCode.
const ALLOWED_HTML_TAGS: &[&str] = &[
    "a",
    "b",
    "blockquote",
    "br",
    "code",
    "div",
    "em",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "i",
    "img",
    "li",
    "ol",
    "p",
    "pre",
    "s",
    "span",
    "strong",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "u",
    "ul",
];

fn attribute_value(raw_tag: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?is)(?:\s|^){name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#);
    let regex = Regex::new(&pattern).ok()?;
    let captures = regex.captures(raw_tag)?;
    captures
        .iter()
        .skip(1)
        .flatten()
        .next()
        .map(|value| value.as_str().trim().to_string())
}

/// Rebuilds a single raw HTML tag from an allow-list. Everything the feed sends
/// is discarded except the tag name and a validated `href` / `src`.
fn sanitize_html_tag(raw_tag: &str) -> String {
    let tag_re = Regex::new(r"(?is)^<\s*(/?)\s*([a-z][a-z0-9]*)").expect("valid html tag pattern");
    let Some(captures) = tag_re.captures(raw_tag) else {
        return String::new();
    };
    let closing = captures.get(1).is_some_and(|value| value.as_str() == "/");
    let name = captures
        .get(2)
        .map(|value| value.as_str().to_ascii_lowercase())
        .unwrap_or_default();
    if !ALLOWED_HTML_TAGS.contains(&name.as_str()) {
        return String::new();
    }
    if matches!(name.as_str(), "img" | "br" | "hr") {
        if closing {
            return String::new();
        }
    } else if closing {
        return format!("</{name}>");
    }
    match name.as_str() {
        "img" => {
            let url = attribute_value(raw_tag, "src")
                .map_or_else(|| "#".to_string(), |value| safe_url(&value));
            if url == "#" {
                String::new()
            } else {
                format!(
                    "<img src=\"{}\" alt=\"\" loading=\"lazy\">",
                    escape_html(&url)
                )
            }
        }
        "a" => {
            let url = attribute_value(raw_tag, "href")
                .map_or_else(|| "#".to_string(), |value| safe_url(&value));
            if url == "#" {
                "<a>".to_string()
            } else {
                format!(
                    "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">",
                    escape_html(&url)
                )
            }
        }
        _ => format!("<{name}>"),
    }
}

/// Third-party feeds (for example PlayGround.ru) embed raw HTML in `contents`,
/// while Valve announcements use BBCode. Keep the allow-listed HTML tags and
/// escape everything else so tags are neither shown as text nor translated.
fn sanitize_feed_html(input: &str) -> String {
    let mut stripped = input.to_string();
    for name in [
        "script", "style", "iframe", "object", "embed", "svg", "math", "template", "noscript",
        "form",
    ] {
        let paired = Regex::new(&format!(r"(?is)<\s*{name}\b[^>]*>.*?<\s*/\s*{name}\s*>"))
            .expect("valid dangerous tag pattern");
        stripped = paired.replace_all(&stripped, "").into_owned();
        let orphan = Regex::new(&format!(r"(?is)<\s*/?\s*{name}\b[^>]*>"))
            .expect("valid dangerous tag pattern");
        stripped = orphan.replace_all(&stripped, "").into_owned();
    }
    let tags = Regex::new(r"(?is)<[^>]*>").expect("valid html tag pattern");
    let mut out = String::with_capacity(stripped.len());
    let mut last = 0;
    for tag in tags.find_iter(&stripped) {
        out.push_str(&escape_html(&stripped[last..tag.start()]));
        out.push_str(&sanitize_html_tag(tag.as_str()));
        last = tag.end();
    }
    out.push_str(&escape_html(&stripped[last..]));
    out
}

/// Convert the Steam BBCode used by news posts to a small, deliberately safe HTML subset.
pub(crate) fn bbcode_to_html(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let mut text = sanitize_feed_html(&input.replace(
        "{STEAM_CLAN_IMAGE}",
        "https://clan.cloudflare.steamstatic.com/images/",
    ));

    let img_pair = Regex::new(r"(?is)\[img\](.*?)\[/img\]").expect("valid image regex");
    text = img_pair
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            format!(
                "<img src=\"{}\" alt=\"\" loading=\"lazy\">",
                attr_url(caps.get(1).map_or("", |m| m.as_str()))
            )
        })
        .into_owned();
    let img_self = Regex::new(r"(?is)\[img\s+src=(&quot;|')?([^&quot;'\]]+?)(&quot;|')?\s*/?\]")
        .expect("valid self-closing image regex");
    text = img_self
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            format!(
                "<img src=\"{}\" alt=\"\" loading=\"lazy\">",
                attr_url(caps.get(2).map_or("", |m| m.as_str()))
            )
        })
        .into_owned();

    let url_with_label =
        Regex::new(r"(?is)\[url=([^\]]+)\](.*?)\[/url\]").expect("valid url regex");
    text = url_with_label
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
                attr_url(caps.get(1).map_or("", |m| m.as_str())),
                caps.get(2).map_or("", |m| m.as_str())
            )
        })
        .into_owned();
    let plain_url = Regex::new(r"(?is)\[url\](.*?)\[/url\]").expect("valid plain url regex");
    text = plain_url
        .replace_all(&text, |caps: &regex::Captures<'_>| {
            let url = caps.get(1).map_or("", |m| m.as_str()).trim();
            format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
                attr_url(url),
                escape_html(url)
            )
        })
        .into_owned();

    for (bb, html) in [
        ("b", "strong"),
        ("i", "em"),
        ("u", "u"),
        ("s", "s"),
        ("h1", "h1"),
        ("h2", "h2"),
        ("h3", "h3"),
        ("h4", "h4"),
        ("h5", "h5"),
        ("h6", "h6"),
        ("p", "p"),
        ("quote", "blockquote"),
        ("code", "pre"),
        ("list", "ul"),
        ("olist", "ol"),
        ("spoiler", "details"),
        ("table", "table"),
        ("tr", "tr"),
        ("td", "td"),
        ("th", "th"),
    ] {
        let pattern =
            Regex::new(&format!(r"(?is)\[{bb}\](.*?)\[/{bb}\]")).expect("valid bbcode regex");
        text = pattern
            .replace_all(&text, format!("<{html}>$1</{html}>").as_str())
            .into_owned();
    }
    for tag in ["color", "size", "font", "center", "left", "right"] {
        let pattern = Regex::new(&format!(r"(?is)\[{tag}=[^\]]+\](.*?)\[/{tag}\]"))
            .expect("valid style regex");
        text = pattern.replace_all(&text, "$1").into_owned();
    }
    text = Regex::new(r"(?is)\[carousel\](.*?)\[/carousel\]")
        .expect("valid carousel regex")
        .replace_all(&text, "<div class=\"bbcode-carousel\">$1</div>")
        .into_owned();
    text = Regex::new(r"(?is)\[section\](.*?)\[/section\]")
        .expect("valid section regex")
        .replace_all(&text, "<div class=\"bbcode-section\">$1</div>")
        .into_owned();
    text = Regex::new(r"(?is)\[hr\](?:\[/hr\])?")
        .expect("valid hr regex")
        .replace_all(&text, "<hr>")
        .into_owned();
    text = Regex::new(r"(?is)\[br\s*/?\]")
        .expect("valid br regex")
        .replace_all(&text, "<br>")
        .into_owned();
    text = text.replace("[*]", "<li>").replace("[/*]", "");
    let text = text.replace('\n', "<br>");
    // Feeds that embed raw HTML pad their markup with blank lines; keep at most
    // one blank line instead of letting those runs push the article down.
    Regex::new(r"(?is)(?:<br\s*/?>\s*){3,}")
        .expect("valid line break regex")
        .replace_all(&text, "<br><br>")
        .into_owned()
}

fn strip_bbcode(value: &str) -> String {
    Regex::new(r"(?is)\[[^\]]+\]")
        .expect("valid strip regex")
        .replace_all(value, "")
        .trim()
        .to_string()
}

pub(crate) async fn fetch_developer_news(
    count: u32,
    language: String,
    only_gid: Option<String>,
) -> Result<DeveloperNewsResponse, String> {
    // 直连被墙时自动切中转（设置「Steam 网络加速」），见 steam_relay 模块。
    let payload = crate::steam_relay::send_with_relay(STEAM_NEWS_API, NEWS_REQUEST_TIMEOUT, |client, url| {
        client
            .get(url)
            .header("User-Agent", "ProjectZomboidManager/0.1")
            .query(&[
                ("appid", APP_ID.to_string()),
                ("count", count.clamp(1, 50).to_string()),
                ("maxlength", "0".to_string()),
                ("format", "json".to_string()),
            ])
    })
    .await?
    .json::<SteamNewsResponse>()
    .await
    .map_err(|e| format!("Steam 新闻解析失败: {e}"))?;
    let mut items = Vec::new();
    let mut translation_jobs = Vec::new();
    for (index, item) in payload.appnews.newsitems.into_iter().enumerate() {
        if only_gid.as_deref().is_some_and(|gid| gid != item.gid) {
            continue;
        }
        let title = strip_bbcode(&item.title);
        let title_sid = format!("n{index}_title");
        if language.trim() != "en" && !title.trim().is_empty() {
            translation_jobs.push(TranslationJob {
                sid: title_sid.clone(),
                text: title.clone(),
            });
        }
        let raw_html = bbcode_to_html(&item.contents);
        let contents_html = if language.trim() == "en" {
            raw_html
        } else {
            crate::news_translation::add_translation_spans(
                &raw_html,
                &format!("n{index}"),
                &mut translation_jobs,
            )
        };
        items.push(DeveloperNewsItem {
            gid: item.gid,
            title,
            title_sid: if language.trim() == "en" {
                String::new()
            } else {
                title_sid
            },
            url: item.url,
            date: item.date,
            author: item.author,
            feed_label: item.feedlabel,
            contents_html,
        });
    }
    Ok(DeveloperNewsResponse {
        app_id: APP_ID,
        items,
        translation_jobs,
        language: language.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{bbcode_to_html, strip_bbcode};

    #[test]
    fn converts_common_steam_markup() {
        let html = bbcode_to_html("[b]Hello[/b] [url=https://example.com]link[/url][br][img]https://example.com/a.png[/img]");
        assert!(html.contains("<strong>Hello</strong>"));
        assert!(html.contains("target=\"_blank\""));
        assert!(html.contains("<img"));
        assert!(html.contains("<br>"));
    }

    #[test]
    fn strips_title_markup() {
        assert_eq!(strip_bbcode("[b]Patch[/b] 42"), "Patch 42");
    }

    #[test]
    fn keeps_raw_feed_html_instead_of_escaping_it() {
        let html = bbcode_to_html(
            "<img src=\"https://i.playground.ru/e/example.jpeg\"/>\n<br />\n<p>Hello <strong>world</strong></p>",
        );

        assert!(html.contains(
            "<img src=\"https://i.playground.ru/e/example.jpeg\" alt=\"\" loading=\"lazy\">"
        ));
        assert!(html.contains("<br>"));
        assert!(html.contains("<p>Hello <strong>world</strong></p>"));
        assert!(!html.contains("&lt;img"));
        assert!(!html.contains("&lt;p&gt;"));
    }

    #[test]
    fn drops_unsafe_raw_feed_html() {
        let html = bbcode_to_html(
            "<p onclick=\"evil()\">Hi</p><img src=\"javascript:alert(1)\"><iframe src=\"https://evil.example\"></iframe><script>alert(1)</script><a href=\"javascript:evil()\">x</a>",
        );

        assert!(html.contains("<p>Hi</p>"));
        assert!(html.contains("<a>x</a>"));
        assert!(!html.contains("onclick"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("<iframe"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("alert(1)"));
    }

    #[test]
    fn collapses_runaway_blank_lines() {
        let html = bbcode_to_html("First\n\n\n\n\n\nSecond");
        assert_eq!(html, "First<br><br>Second");
    }
}
