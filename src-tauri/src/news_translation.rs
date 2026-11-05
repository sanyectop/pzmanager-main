use crate::news::TranslationJob;
use regex::Regex;
use reqwest::header::{
    ACCEPT, ACCEPT_LANGUAGE, COOKIE, ORIGIN, REFERER, SET_COOKIE, USER_AGENT,
};
use reqwest::{Client, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const BATCH_SIZE: usize = 15;
const MAX_TEXT_LEN: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(8);
const TRANSLATION_TIMEOUT: Duration = Duration::from_secs(5);
const GOOGLE_URL: &str = "https://translate.googleapis.com/translate_a/single";
const MYMEMORY_URL: &str = "https://api.mymemory.translated.net/get";
/// relay 自带的离线翻译接口（`steam-relay/server.js` → `translate/app.py`，Argos）。
const RELAY_TRANSLATE_PATH: &str = "/translate";
/// 一批 15 句在 CPU 上通常 1–5 秒；Argos 冷启动第一句会更慢一点。
const RELAY_TRANSLATE_TIMEOUT: Duration = Duration::from_secs(20);

// ---- 必应翻译 -------------------------------------------------------------
//
// 手机版翻译页背后的 JSON 接口：先用手机 UA 拉一次 HTML 拿 IG / IID /
// params_AbusePreventionHelper(key, token) 和 Cookie，再 POST `ttranslatev3`。
// 纯 HTTP、不执行页面脚本；cn.bing.com 国内直连可达，是 Google 被墙时
// 比 MyMemory 质量高得多的回退。
const BING_ENTRY: &str = "https://cn.bing.com/translator?setmkt=zh-cn&setlang=zh-cn";
/// 手机端 UA：桌面页也会下发令牌，但手机版页面的令牌字段最稳定（脚本实测）。
const BING_MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_2_1 like Mac OS X) \
AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.2 Mobile/15E148 Safari/604.1";
const BING_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(10);
const BING_TIMEOUT: Duration = Duration::from_secs(15);
/// 单次请求文本上限（手机版页面约 1000 字符），超出按换行 / 句末标点切开。
/// 与参考实现（Downloads/bing_mobile_translator.py 的 MAX_CHUNK）保持一致。
const BING_MAX_CHUNK: usize = 1000;
/// 令牌有效期约 10 分钟，这里按 5 分钟主动刷新，避免踩 401/205。
const BING_SESSION_TTL: Duration = Duration::from_secs(300);
/// 必应失败后的熔断时长。必应是国内直连服务，失败通常是瞬时网络问题；
/// 参考脚本每次调用都会重试而不是长时间停用。这里只停 60 秒，避免一次抖动
/// 就让用户接下来十分钟都看不到必应（手动切换引擎时会直接解除）。
const BING_BACKOFF: Duration = Duration::from_secs(60);

/// 必应请求统一走独立客户端，**始终绕开系统代理**。
///
/// cn.bing.com / www.bing.com 国内直连可达；而系统代理（尤其境外出口）会让必应
/// 把请求判定为机房 IP，直接返回 401 —— 参考脚本默认也是直连（Python requests
/// 不读 Windows 系统代理）。与 `steam_relay::steam_client(true)` 同一个理由。
fn bing_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .no_proxy()
            .timeout(TIMEOUT)
            .build()
            .unwrap_or_else(|_| Client::new())
    })
}

/// 译文缓存键：(原文, 目标语言, 引擎偏好)。
/// 引擎偏好也进键：切换「通用 › 翻译引擎」后必须重新翻译，不能把旧引擎的结果
/// 直接当成新引擎的结果返回（否则选了必应也看不到必应）。
static CACHE: OnceLock<Mutex<HashMap<(String, String, String), (String, String)>>> =
    OnceLock::new();

#[derive(Clone)]
struct BingSession {
    origin: String,
    page_url: String,
    ig: String,
    iid: String,
    key: String,
    token: String,
    cookies: String,
    created: Instant,
}

static BING_SESSION: Mutex<Option<BingSession>> = Mutex::new(None);
static BING_DISABLED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
/// 页面里的 SFX 请求序号。
static BING_SEQ: AtomicU64 = AtomicU64::new(0);
static BING_IG_RE: OnceLock<Regex> = OnceLock::new();
static BING_IID_RE: OnceLock<Regex> = OnceLock::new();
static BING_ABUSE_RE: OnceLock<Regex> = OnceLock::new();
static BING_ABUSE_LOOSE_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationResult {
    pub(crate) sid: String,
    pub(crate) text: String,
    pub(crate) engine: String,
}

fn cache() -> &'static Mutex<HashMap<(String, String, String), (String, String)>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_language(language: &str) -> String {
    match language.trim().to_ascii_lowercase().as_str() {
        "zh" | "zh-cn" | "zh_cn" | "zh-hans" => "zh-CN".into(),
        "zh-tw" | "zh_tw" | "zh-hant" | "zh-hk" => "zh-TW".into(),
        "ja" | "ja-jp" => "ja".into(),
        "ko" | "ko-kr" => "ko".into(),
        "ru" | "de" | "fr" | "es" | "pt" | "it" | "tr" | "vi" | "id" | "th" | "ms" | "ar"
        | "hi" => language.to_ascii_lowercase(),
        _ => "en".into(),
    }
}

/// MyMemory 不做语言检测（Google 才有 auto），回退源按文字系统猜源语言，
/// 覆盖新闻 / 创意工坊文本的常见语种，混合文本以先命中的文字系统为准。
fn guess_source_lang(text: &str) -> &'static str {
    for ch in text.chars() {
        let c = ch as u32;
        if (0x3040..=0x30FF).contains(&c) {
            return "ja";
        }
        if (0xAC00..=0xD7AF).contains(&c) {
            return "ko";
        }
        if (0x0400..=0x04FF).contains(&c) {
            return "ru";
        }
        if (0x0E00..=0x0E7F).contains(&c) {
            return "th";
        }
        if (0x0600..=0x06FF).contains(&c) {
            return "ar";
        }
        if (0x0900..=0x097F).contains(&c) {
            return "hi";
        }
        if (0x4E00..=0x9FFF).contains(&c) {
            return "zh-CN";
        }
    }
    "en"
}

async fn mymemory(client: &Client, text: &str, language: &str) -> Option<String> {
    let langpair = format!("{}|{}", guess_source_lang(text), language);
    let value: Value = client
        .get(MYMEMORY_URL)
        .timeout(TRANSLATION_TIMEOUT)
        .query(&[("q", text), ("langpair", langpair.as_str())])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    parse_mymemory(&value)
}

/// 免费配额耗尽时接口仍返回 200，把警告文本塞进译文里，这类响应视为失败。
fn parse_mymemory(value: &Value) -> Option<String> {
    if value.get("responseStatus")?.as_i64()? != 200 {
        return None;
    }
    let translated = value
        .get("responseData")?
        .get("translatedText")?
        .as_str()?;
    let healthy = !translated.is_empty()
        && !translated.contains("MYMEMORY WARNING")
        && !translated.contains("QUERY LENGTH LIMIT");
    healthy.then(|| translated.to_string())
}

fn batches(jobs: &[TranslationJob]) -> Vec<Vec<TranslationJob>> {
    let mut all = Vec::new();
    let mut current = Vec::new();
    let mut length = 0;
    for job in jobs {
        let add = job.text.chars().count() + usize::from(!current.is_empty());
        if !current.is_empty() && (current.len() >= BATCH_SIZE || length + add > MAX_TEXT_LEN) {
            all.push(std::mem::take(&mut current));
            length = 0;
        }
        length += add;
        current.push(job.clone());
    }
    if !current.is_empty() {
        all.push(current);
    }
    all
}

fn is_text(value: &str) -> bool {
    value.chars().any(char::is_alphabetic)
}

/// Python's `\d` matches Unicode category Nd only. `char::is_numeric()` is wider
/// (it also accepts Roman numerals, superscripts and fractions), so match the
/// regex crate's `\d` instead to keep the splitter identical to anoucne.py.
fn is_unicode_digit(value: char) -> bool {
    static DIGIT: OnceLock<Regex> = OnceLock::new();
    let digit = DIGIT.get_or_init(|| Regex::new(r"^\d$").expect("valid digit matcher"));
    let mut buffer = [0u8; 4];
    digit.is_match(value.encode_utf8(&mut buffer))
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn decode_html_entities(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

/// Segment kinds, mirroring the `("text" | "punct" | "skip")` tuples of anoucne.py.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentKind {
    Text,
    Punct,
    Skip,
}

#[derive(Debug, Clone)]
struct Segment {
    kind: SegmentKind,
    text: String,
}

const MIN_SEGMENT_LEN: usize = 8;
const MAX_SEGMENT_LEN: usize = 400;

fn push_segment(out: &mut Vec<Segment>, chunk: &str) {
    if chunk.is_empty() {
        return;
    }
    let kind = if is_text(chunk) {
        SegmentKind::Text
    } else {
        SegmentKind::Punct
    };
    out.push(Segment {
        kind,
        text: chunk.to_string(),
    });
}

/// Mirrors `_split_by_punct` with `(?<!\d)([.!?]+)(?!\d)|(\n+)`.
fn split_by_punct(text: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut cursor = 0;
    let mut index = 0;
    while index < text.len() {
        let ch = text[index..].chars().next().expect("valid boundary");
        let mut end = index + ch.len_utf8();
        let mut delimiter = false;
        if ch == '\n' {
            while end < text.len() && text[end..].starts_with('\n') {
                end += 1;
            }
            delimiter = true;
        } else if matches!(ch, '.' | '!' | '?') {
            while end < text.len()
                && text[end..]
                    .chars()
                    .next()
                    .is_some_and(|next| matches!(next, '.' | '!' | '?'))
            {
                end += text[end..]
                    .chars()
                    .next()
                    .expect("valid boundary")
                    .len_utf8();
            }
            let before_digit = text[..index]
                .chars()
                .next_back()
                .is_some_and(is_unicode_digit);
            let after_digit = text[end..].chars().next().is_some_and(is_unicode_digit);
            delimiter = !before_digit && !after_digit;
        }
        if delimiter {
            if index > cursor {
                push_segment(&mut out, &text[cursor..index]);
            }
            push_segment(&mut out, &text[index..end]);
            cursor = end;
            index = end;
            continue;
        }
        index = end;
    }
    if cursor < text.len() {
        push_segment(&mut out, &text[cursor..]);
    }
    out
}

/// Mirrors `_merge_short`.
fn merge_short(parts: Vec<Segment>) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let current = &parts[index];
        if current.kind != SegmentKind::Text {
            merged.push(current.clone());
            index += 1;
            continue;
        }
        let mut buffer = current.text.clone();
        let mut next = index + 1;
        while next < parts.len() {
            let candidate = &parts[next];
            if candidate.kind == SegmentKind::Skip {
                break;
            }
            if candidate.kind == SegmentKind::Punct {
                if buffer.trim().chars().count() >= MIN_SEGMENT_LEN {
                    break;
                }
                buffer.push_str(&candidate.text);
                next += 1;
                continue;
            }
            if buffer.trim().chars().count() < MIN_SEGMENT_LEN
                || candidate.text.trim().chars().count() < MIN_SEGMENT_LEN
            {
                if buffer.chars().count() + candidate.text.chars().count() <= MAX_SEGMENT_LEN {
                    buffer.push_str(&candidate.text);
                    next += 1;
                    continue;
                }
            }
            break;
        }
        merged.push(Segment {
            kind: SegmentKind::Text,
            text: buffer,
        });
        index = next;
    }
    merged
}

/// Mirrors `split_sentences`: `[...]` spans are preserved verbatim and never translated.
fn split_sentences(text: &str) -> Vec<Segment> {
    let brackets = Regex::new(r"\[[^\]]*\]").expect("valid bracket matcher");
    let mut result = Vec::new();
    let mut last = 0;
    for bracket in brackets.find_iter(text) {
        if bracket.start() > last {
            result.extend(split_by_punct(&text[last..bracket.start()]));
        }
        result.push(Segment {
            kind: SegmentKind::Skip,
            text: bracket.as_str().to_string(),
        });
        last = bracket.end();
    }
    if last < text.len() {
        result.extend(split_by_punct(&text[last..]));
    }
    merge_short(result)
}

pub(crate) fn add_translation_spans(
    html: &str,
    prefix: &str,
    jobs: &mut Vec<TranslationJob>,
) -> String {
    let tags = Regex::new(r"(?s)(<[^>]+>)").expect("valid HTML tag matcher");
    let mut result = String::new();
    let mut last = 0;
    let mut node = 0;
    for tag in tags.find_iter(html) {
        append_spans(&html[last..tag.start()], prefix, node, &mut result, jobs);
        result.push_str(tag.as_str());
        last = tag.end();
        node += 1;
    }
    append_spans(&html[last..], prefix, node, &mut result, jobs);
    result
}
fn append_spans(
    text: &str,
    prefix: &str,
    node: usize,
    out: &mut String,
    jobs: &mut Vec<TranslationJob>,
) {
    let decoded = decode_html_entities(text);
    let mut span_index = 0;
    for part in split_sentences(&decoded) {
        if part.kind == SegmentKind::Text {
            let sid = format!("{prefix}_t{node}_s{span_index}");
            jobs.push(TranslationJob {
                sid: sid.clone(),
                text: part.text.clone(),
            });
            out.push_str(&format!(
                "<span data-sid=\"{sid}\" class=\"i18n-pending\">{}</span>",
                escape(&part.text)
            ));
            span_index += 1;
        } else {
            out.push_str(&escape(&part.text));
        }
    }
}

/// Google 不可用（超时 / 被墙 / 限流）后的熔断时长：期间所有请求直接走 MyMemory，
/// 不再每次都等 Google 超时（一次超时 5 秒，新闻页十几批就是一两分钟）。
const GOOGLE_BACKOFF: Duration = Duration::from_secs(600);
/// Google 熔断的截止时刻（Instant），None = 未熔断。
static GOOGLE_DISABLED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

fn google_enabled_now() -> bool {
    match GOOGLE_DISABLED_UNTIL.lock() {
        Ok(until) => until.map_or(true, |deadline| Instant::now() >= deadline),
        // 锁中毒按可用处理：宁可多等一次超时，也不能永久停掉 Google。
        Err(_) => true,
    }
}

fn disable_google_for_a_while() {
    if let Ok(mut until) = GOOGLE_DISABLED_UNTIL.lock() {
        *until = Some(Instant::now() + GOOGLE_BACKOFF);
    }
}

/// 中转翻译失败后的熔断时长：期间直接走 Google / MyMemory。
///
/// 新闻一批 15 句，失败后逐句重试时会再撞 15 次同样的失败，
/// 所以和 Google 一样：失败一次就先停掉一段时间。
const RELAY_TRANSLATE_BACKOFF: Duration = Duration::from_secs(600);
static RELAY_TRANSLATE_DISABLED_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

fn relay_translate_enabled_now() -> bool {
    match RELAY_TRANSLATE_DISABLED_UNTIL.lock() {
        Ok(until) => until.map_or(true, |deadline| Instant::now() >= deadline),
        Err(_) => true,
    }
}

fn disable_relay_translate_for_a_while() {
    if let Ok(mut until) = RELAY_TRANSLATE_DISABLED_UNTIL.lock() {
        *until = Some(Instant::now() + RELAY_TRANSLATE_BACKOFF);
    }
}

/// 中转翻译的目标语言：Argos 只有 zh / en / ru。
///
/// `zh-TW` 会拿到简体译文，宁可回退 Google，也不要给用户一份简繁混用的结果。
fn relay_target_language(language: &str) -> Option<&'static str> {
    match language {
        "zh-CN" => Some("zh"),
        "en" => Some("en"),
        "ru" => Some("ru"),
        _ => None,
    }
}

/// 当前是否值得尝试中转翻译：设置里选「仅直连」时不走中转，熔断期内也不试。
fn relay_translate_ready() -> bool {
    let (mode, _) = crate::steam_relay::relay_config();
    mode != "direct" && relay_translate_enabled_now()
}

/// 解析 relay 的 `{"results":[{"translated":"..."}]}`。
///
/// 条数对不上、缺字段、或出现空白译文都算整批失败——宁可回退 Google，
/// 也不要让一半句子保持原文却标成"已翻译"。
fn parse_relay_translation(value: &Value, expected: usize) -> Option<Vec<String>> {
    let results = value.get("results")?.as_array()?;
    if results.len() != expected {
        return None;
    }
    results
        .iter()
        .map(|item| {
            let text = item.get("translated")?.as_str()?.trim();
            (!text.is_empty()).then(|| text.to_string())
        })
        .collect()
}

/// 调 relay 的离线翻译接口，一次翻一整批。
async fn relay_translate(texts: &[String], target: &str) -> Option<Vec<String>> {
    let (_, base) = crate::steam_relay::relay_config();
    let url = format!("{}{RELAY_TRANSLATE_PATH}", base.trim_end_matches('/'));
    // 中转服务器大陆直连可达：始终绕开系统代理，避免残留的死代理拖死翻译。
    let client = crate::steam_relay::steam_client(true);
    let value: Value = client
        .post(url)
        .timeout(RELAY_TRANSLATE_TIMEOUT)
        .json(&serde_json::json!({
            "texts": texts,
            "source": "auto",
            "target": target,
        }))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    parse_relay_translation(&value, texts.len())
}

/// 用中转翻译翻这一批并写缓存；成功返回 true，调用方不用再走 Google / MyMemory。
async fn relay_translate_pending(
    pending: &[TranslationJob],
    language: &str,
    engine_preference: &str,
    values: &mut HashMap<String, (String, String)>,
) -> bool {
    let Some(target) = relay_target_language(language) else {
        return false;
    };
    if !relay_translate_ready() {
        return false;
    }
    let texts = pending
        .iter()
        .map(|job| job.text.clone())
        .collect::<Vec<_>>();
    let Some(translated) = relay_translate(&texts, target).await else {
        disable_relay_translate_for_a_while();
        return false;
    };
    for (job, text) in pending.iter().zip(translated) {
        let value = (text, "relay".to_string());
        if let Ok(mut map) = cache().lock() {
            map.insert(
                (
                    job.text.clone(),
                    language.to_string(),
                    engine_preference.to_string(),
                ),
                value.clone(),
            );
        }
        values.insert(job.sid.clone(), value);
    }
    true
}

async fn google(client: &Client, text: &str, language: &str) -> Option<String> {
    let value: Value = client
        .get(GOOGLE_URL)
        .timeout(TRANSLATION_TIMEOUT)
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", language),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let value = value
        .get(0)?
        .as_array()?
        .iter()
        .filter_map(|part| part.get(0).and_then(Value::as_str))
        .collect::<String>();
    (!value.is_empty()).then_some(value)
}
/// 必应熔断：失败一次先停一段时间，避免一批 15 句逐句白等超时。
fn bing_enabled_now() -> bool {
    match BING_DISABLED_UNTIL.lock() {
        Ok(until) => until.map_or(true, |deadline| Instant::now() >= deadline),
        // 锁中毒按可用处理：宁可多等一次超时，也不能永久停掉必应。
        Err(_) => true,
    }
}

fn disable_bing_for_a_while() {
    if let Ok(mut until) = BING_DISABLED_UNTIL.lock() {
        *until = Some(Instant::now() + BING_BACKOFF);
    }
}

/// 用户手动切换翻译引擎时解除必应的熔断：既然明确选了必应，就应立刻重新尝试，
/// 而不是继续沿用之前一次失败留下的静默期。
pub(crate) fn clear_bing_backoff() {
    if let Ok(mut until) = BING_DISABLED_UNTIL.lock() {
        *until = None;
    }
}

fn clear_bing_session() {
    if let Ok(mut session) = BING_SESSION.lock() {
        *session = None;
    }
}

/// 内部语言代码 → 必应语种代码（繁简必须分开，不能像 relay 那样拿简体顶替）。
fn bing_target_language(language: &str) -> String {
    match language {
        "zh-CN" => "zh-Hans".to_string(),
        "zh-TW" => "zh-Hant".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

/// 从翻译页 HTML 里取 IG / IID / key / token。页面结构变化时返回 None。
fn parse_bing_tokens(html: &str) -> Option<(String, String, String, String)> {
    let ig = BING_IG_RE
        .get_or_init(|| Regex::new(r#"IG:"([^"]+)""#).expect("必应 IG 正则"))
        .captures(html)?
        .get(1)?
        .as_str()
        .to_string();
    let iid = BING_IID_RE
        .get_or_init(|| Regex::new(r#"data-iid="([^"]+)""#).expect("必应 IID 正则"))
        .captures(html)?
        .get(1)?
        .as_str()
        .to_string();
    let strict = BING_ABUSE_RE
        .get_or_init(|| {
            Regex::new(r#"params_AbusePreventionHelper\s*=\s*\[(\d+),"([^"]+)",(\d+)\]"#)
                .expect("必应令牌正则")
        })
        .captures(html);
    let (key, token) = match strict {
        Some(captures) => (
            captures.get(1)?.as_str().to_string(),
            captures.get(2)?.as_str().to_string(),
        ),
        None => {
            // 宽松回退：逐项解析数组内容（字段顺序或空白变化时仍能取到）。
            let raw = BING_ABUSE_LOOSE_RE
                .get_or_init(|| {
                    Regex::new(r"params_AbusePreventionHelper\s*=\s*\[(.*?)\];")
                        .expect("必应令牌宽松正则")
                })
                .captures(html)?
                .get(1)?
                .as_str();
            let parts = raw
                .split(',')
                .map(|part| part.trim().trim_matches('"').to_string())
                .collect::<Vec<_>>();
            if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
                return None;
            }
            (parts[0].clone(), parts[1].clone())
        }
    };
    Some((ig, iid, key, token))
}

/// 必应单次请求的文本切分：优先在换行 / 句末标点后断开，单段超长再硬切；
/// 不丢任何字符，拼回去与原文完全一致。
fn split_bing_text(text: &str, limit: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    if text.chars().count() <= limit {
        return vec![text.to_string()];
    }
    let mut pieces: Vec<String> = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        current.push(character);
        if matches!(character, '\n' | '。' | '！' | '？' | '!' | '?' | '；' | ';') {
            pieces.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        pieces.push(current);
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut buffer = String::new();
    let mut length = 0usize;
    for piece in pieces {
        let mut rest = piece.as_str();
        while rest.chars().count() > limit {
            let cut = rest
                .char_indices()
                .nth(limit)
                .map(|(index, _)| index)
                .unwrap_or(rest.len());
            if !buffer.is_empty() {
                chunks.push(std::mem::take(&mut buffer));
                length = 0;
            }
            let (head, tail) = rest.split_at(cut);
            chunks.push(head.to_string());
            rest = tail;
        }
        let piece_len = rest.chars().count();
        if length + piece_len > limit && !buffer.is_empty() {
            chunks.push(std::mem::take(&mut buffer));
            length = 0;
        }
        buffer.push_str(rest);
        length += piece_len;
    }
    if !buffer.is_empty() {
        chunks.push(buffer);
    }
    chunks
}

/// 请求一次翻译页，拿令牌与 Cookie。
async fn bing_bootstrap() -> Option<BingSession> {
    let client = bing_client();
    let response = client
        .get(BING_ENTRY)
        .timeout(BING_BOOTSTRAP_TIMEOUT)
        .header(USER_AGENT, BING_MOBILE_UA)
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .header(ACCEPT_LANGUAGE, "zh-CN,zh-Hans;q=0.9")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let page_url = response.url().clone();
    let origin = format!("{}://{}", page_url.scheme(), page_url.host_str()?);
    let cookies = response
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .filter(|pair| pair.contains('='))
        .collect::<Vec<_>>()
        .join("; ");
    let html = response.text().await.ok()?;
    let (ig, iid, key, token) = parse_bing_tokens(&html)?;
    Some(BingSession {
        origin,
        page_url: page_url.to_string(),
        ig,
        iid,
        key,
        token,
        cookies,
        created: Instant::now(),
    })
}

/// 取当前会话；过期（或上次请求触发 401/205 被清空）时重新引导。
async fn bing_session() -> Option<BingSession> {
    if let Ok(session) = BING_SESSION.lock() {
        if let Some(session) = session.as_ref() {
            if session.created.elapsed() < BING_SESSION_TTL {
                return Some(session.clone());
            }
        }
    }
    let fresh = bing_bootstrap().await?;
    if let Ok(mut session) = BING_SESSION.lock() {
        *session = Some(fresh.clone());
    }
    Some(fresh)
}

fn parse_bing_translation(value: &Value) -> Option<String> {
    let first = value.as_array()?.first()?;
    let text = first
        .get("translations")?
        .as_array()?
        .first()?
        .get("text")?
        .as_str()?;
    (!text.trim().is_empty()).then(|| text.to_string())
}

/// 调一次 `ttranslatev3`。401 / statusCode 205 刷新令牌重试一次；429 等 2 秒重试一次。
async fn bing_translate_chunk(text: &str, target: &str) -> Option<String> {
    let client = bing_client();
    let mut retried = false;
    loop {
        let session = bing_session().await?;
        let sequence = BING_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let sfx = sequence.to_string();
        let mut request = client
            .post(format!("{}/ttranslatev3", session.origin))
            .timeout(BING_TIMEOUT)
            .header(USER_AGENT, BING_MOBILE_UA)
            .header(ACCEPT, "*/*")
            .header(ACCEPT_LANGUAGE, "zh-CN,zh-Hans;q=0.9")
            .header(ORIGIN, &session.origin)
            .header(REFERER, &session.page_url)
            .query(&[
                ("isVertical", "1"),
                ("IG", session.ig.as_str()),
                ("IID", session.iid.as_str()),
                ("SFX", sfx.as_str()),
            ])
            .form(&[
                ("fromLang", "auto-detect"),
                ("to", target),
                ("text", text),
                ("token", session.token.as_str()),
                ("key", session.key.as_str()),
            ]);
        if !session.cookies.is_empty() {
            request = request.header(COOKIE, &session.cookies);
        }
        let response = request.send().await.ok()?;
        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS {
            if retried {
                return None;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            retried = true;
            continue;
        }
        if status == StatusCode::UNAUTHORIZED {
            if retried {
                return None;
            }
            clear_bing_session();
            retried = true;
            continue;
        }
        if !status.is_success() {
            return None;
        }

        let value: Value = response.json().await.ok()?;
        if value.get("statusCode").and_then(Value::as_i64) == Some(205) {
            if retried {
                return None;
            }
            clear_bing_session();
            retried = true;
            continue;
        }
        return parse_bing_translation(&value);
    }
}

/// 必应翻译：按长度切段后逐段翻译，拼回原文结构。
async fn bing(text: &str, language: &str) -> Option<String> {
    if !bing_enabled_now() {
        return None;
    }
    let target = bing_target_language(language);
    let chunks = split_bing_text(text, BING_MAX_CHUNK);
    if chunks.is_empty() {
        return None;
    }
    let mut output = String::new();
    for chunk in chunks {
        match bing_translate_chunk(&chunk, &target).await {
            Some(value) => output.push_str(&value),
            None => {
                disable_bing_for_a_while();
                return None;
            }
        }
    }
    (!output.trim().is_empty()).then_some(output)
}

/// 默认引擎链：必应（cn.bing.com 国内直连、质量远好于 MyMemory）→ Google → MyMemory。
/// 中转翻译（relay）不在默认链里：它是批级引擎、只接 zh-CN/en/ru，
/// 只在「通用 › 翻译引擎」手动指定时才先试。
const DEFAULT_ENGINE_CHAIN: [&str; 3] = ["bing", "google", "mymemory"];

/// 手动指定的引擎排到最前，其余按默认顺序跟上；
/// auto / relay（relay 在批级单独处理）/ 未知值都走默认链。
fn engine_chain(preference: &str) -> Vec<&'static str> {
    let chosen = match preference {
        "bing" => Some("bing"),
        "google" => Some("google"),
        "mymemory" => Some("mymemory"),
        _ => None,
    };
    match chosen {
        Some(engine) => {
            let mut chain = vec![engine];
            chain.extend(
                DEFAULT_ENGINE_CHAIN
                    .into_iter()
                    .filter(|candidate| *candidate != engine),
            );
            chain
        }
        None => DEFAULT_ENGINE_CHAIN.to_vec(),
    }
}

async fn translate_one(
    client: &Client,
    text: &str,
    language: &str,
    chain: &[&str],
) -> (String, String) {
    for engine in chain {
        match *engine {
            "google" => {
                if google_enabled_now() {
                    if let Some(value) = google(client, text, language).await {
                        return (value, "google".into());
                    }
                    // Google 这次没成功（超时 / 被墙 / 限流）：接下来一段时间不再等它超时，
                    // 否则一批 15 句逐句重试时每句都要先白等满一个超时，回退看起来像卡死。
                    disable_google_for_a_while();
                }
            }
            "bing" => {
                // 必应失败会自己熔断，后面的句子直接落到下一个引擎。
                if let Some(value) = bing(text, language).await {
                    return (value, "bing".into());
                }
            }
            "mymemory" => {
                if let Some(value) = mymemory(client, text, language).await {
                    return (value, "mymemory".into());
                }
            }
            _ => {}
        }
    }
    (text.into(), "none".into())
}
async fn translate_jobs(
    client: &Client,
    jobs: &[TranslationJob],
    language: &str,
    engine_preference: &str,
) -> Vec<TranslationResult> {
    let mut values = HashMap::new();
    let mut pending = Vec::new();
    for job in jobs {
        if let Some(value) = cache().lock().ok().and_then(|map| {
            map.get(&(
                job.text.clone(),
                language.to_string(),
                engine_preference.to_string(),
            ))
            .cloned()
        }) {
            values.insert(job.sid.clone(), value);
        } else {
            pending.push(job.clone());
        }
    }
    if !pending.is_empty() {
        // 中转翻译（Argos）只在「通用 › 翻译引擎」手动指定时先试：一次翻整批，
        // 只接 zh-CN/en/ru；失败或语言不支持时再走下面的逐句引擎链。
        let relayed = if engine_preference == "relay" {
            relay_translate_pending(&pending, language, engine_preference, &mut values).await
        } else {
            false
        };
        if !relayed {
            let chain = engine_chain(engine_preference);
            let joined = pending
                .iter()
                .map(|job| job.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let (translated, engine) = translate_one(client, &joined, language, &chain).await;
            let parts = translated.split('\n').collect::<Vec<_>>();
            // 引擎全失败时 translate_one 返回 (原文, "none")，而原文按 \n 拆开的行数
            // 恰好等于任务数——会伪装成"翻译成功"。必须把 engine == "none" 视为失败，
            // 落到下面的逐句重试，否则后面的引擎根本没有出场机会，还会把 none 写进缓存。
            if engine != "none" && parts.len() == pending.len() {
                for (job, text) in pending.iter().zip(parts) {
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let value = (trimmed.to_string(), engine.clone());
                    if let Ok(mut map) = cache().lock() {
                        map.insert(
                            (
                                job.text.clone(),
                                language.to_string(),
                                engine_preference.to_string(),
                            ),
                            value.clone(),
                        );
                    }
                    values.insert(job.sid.clone(), value);
                }
            } else {
                for job in pending {
                    // 批量请求失败时，刚试过的引擎已各自触发熔断，
                    // 逐句重试会直接落到链上下一个可用引擎，不会白等超时。
                    let value = translate_one(client, &job.text, language, &chain).await;
                    if value.1 != "none" {
                        if let Ok(mut map) = cache().lock() {
                            map.insert(
                                (
                                    job.text.clone(),
                                    language.to_string(),
                                    engine_preference.to_string(),
                                ),
                                value.clone(),
                            );
                        }
                    }
                    values.insert(job.sid.clone(), value);
                }
            }
        }
    }
    jobs.iter()
        .map(|job| {
            let (text, engine) = values
                .remove(&job.sid)
                .unwrap_or((job.text.clone(), "none".into()));
            TranslationResult {
                sid: job.sid.clone(),
                text,
                engine,
            }
        })
        .collect()
}

pub(crate) async fn translate_batch(
    jobs: Vec<TranslationJob>,
    language: String,
) -> Result<Vec<TranslationResult>, String> {
    if jobs.len() > BATCH_SIZE {
        return Err(format!("翻译批次上限为 {BATCH_SIZE} 句"));
    }
    let language = normalize_language(&language);
    let client = Client::builder()
        .user_agent("ProjectZomboidManager/0.1")
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    // 一次请求里的所有批次用同一个引擎偏好，来源占比不会在批与批之间跳来跳去。
    let engine_preference = crate::configured_translation_engine();
    let mut results = Vec::new();
    for batch in batches(&jobs) {
        results.extend(translate_jobs(&client, &batch, &language, &engine_preference).await);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guesses_source_lang_by_script() {
        assert_eq!(guess_source_lang("hello world"), "en");
        assert_eq!(guess_source_lang("こんにちは、世界"), "ja");
        assert_eq!(guess_source_lang("안녕하세요"), "ko");
        assert_eq!(guess_source_lang("Привет, мир"), "ru");
        assert_eq!(guess_source_lang("僵尸毁灭工程"), "zh-CN");
        // 假名优先于汉字：日语文本里的汉字不应误判成中文
        assert_eq!(guess_source_lang("_zombie_は死んだ"), "ja");
        assert_eq!(guess_source_lang(""), "en");
    }

    /// Google 失败后进入熔断：期间不再探测，到期后恢复；测试之间要清掉全局状态。
    #[test]
    fn google_backoff_skips_google_until_deadline() {
        GOOGLE_DISABLED_UNTIL.lock().unwrap().take();
        assert!(google_enabled_now());

        disable_google_for_a_while();
        assert!(!google_enabled_now());

        *GOOGLE_DISABLED_UNTIL.lock().unwrap() = Some(Instant::now() - Duration::from_secs(1));
        assert!(google_enabled_now());

        GOOGLE_DISABLED_UNTIL.lock().unwrap().take();
    }
    /// 中转翻译只接 zh / en / ru：繁体、日文等一律回退 Google。
    #[test]
    fn relay_targets_only_supported_languages() {
        assert_eq!(relay_target_language("zh-CN"), Some("zh"));
        assert_eq!(relay_target_language("en"), Some("en"));
        assert_eq!(relay_target_language("ru"), Some("ru"));
        assert_eq!(relay_target_language("zh-TW"), None);
        assert_eq!(relay_target_language("ja"), None);
    }

    /// 默认链是必应 → Google → MyMemory；手动指定的引擎排最前，
    /// 其余保持默认相对顺序；relay 在批级单独处理，不进逐句链。
    #[test]
    fn engine_chain_prefers_manual_engine_then_default_order() {
        let default = vec!["bing", "google", "mymemory"];
        assert_eq!(engine_chain("auto"), default);
        assert_eq!(engine_chain("bing"), default);
        // relay 只在批级先试（translate_jobs），逐句链走默认顺序。
        assert_eq!(engine_chain("relay"), default);
        assert_eq!(engine_chain("不存在的引擎"), default);

        assert_eq!(engine_chain("google"), vec!["google", "bing", "mymemory"]);
        assert_eq!(engine_chain("mymemory"), vec!["mymemory", "bing", "google"]);
    }

    #[test]
    fn parses_relay_translation_and_rejects_bad_batches() {
        let ok: Value = serde_json::from_str(
            r#"{"target":"zh","results":[{"translated":"你好"},{"translated":"世界"}]}"#,
        )
        .unwrap();
        assert_eq!(
            parse_relay_translation(&ok, 2),
            Some(vec!["你好".to_string(), "世界".to_string()])
        );

        // 条数对不上、缺 results、空白译文都按整批失败处理，交给 Google 回退。
        assert_eq!(parse_relay_translation(&ok, 3), None);
        let blank: Value = serde_json::from_str(r#"{"results":[{"translated":"   "}]}"#).unwrap();
        assert_eq!(parse_relay_translation(&blank, 1), None);
        let missing: Value = serde_json::from_str(r#"{"target":"zh"}"#).unwrap();
        assert_eq!(parse_relay_translation(&missing, 1), None);
    }

    /// 中转翻译失败后熔断：期间直接走 Google / MyMemory；测试之间要清掉全局状态。
    #[test]
    fn relay_backoff_skips_until_deadline() {
        RELAY_TRANSLATE_DISABLED_UNTIL.lock().unwrap().take();
        assert!(relay_translate_enabled_now());

        disable_relay_translate_for_a_while();
        assert!(!relay_translate_enabled_now());

        *RELAY_TRANSLATE_DISABLED_UNTIL.lock().unwrap() = Some(Instant::now() - Duration::from_secs(1));
        assert!(relay_translate_enabled_now());

        RELAY_TRANSLATE_DISABLED_UNTIL.lock().unwrap().take();
    }

    /// 手机版翻译页里 IG / IID / key / token 的两种写法。
    #[test]
    fn parses_bing_bootstrap_tokens() {
        let strict = r#"<html><script>IG:"A1B2C3",data-iid="translator.5023"</script>
            <script>var params_AbusePreventionHelper = [1770000000000,"tok+en/value",3600000];</script></html>"#;
        assert_eq!(
            parse_bing_tokens(strict),
            Some((
                "A1B2C3".to_string(),
                "translator.5023".to_string(),
                "1770000000000".to_string(),
                "tok+en/value".to_string(),
            ))
        );

        // 字段间有空白时严格正则失配，宽松回退仍能取到 key/token。
        let loose = r#"<script>IG:"X9",data-iid="translator.0"</script>
            <script>params_AbusePreventionHelper = [ 1234, "abc" , 3600000 ];</script>"#;
        assert_eq!(
            parse_bing_tokens(loose),
            Some((
                "X9".to_string(),
                "translator.0".to_string(),
                "1234".to_string(),
                "abc".to_string(),
            ))
        );

        // 缺令牌（页面改版 / 被拦截）必须失败，交给后续引擎。
        assert_eq!(parse_bing_tokens("<html>blocked</html>"), None);
    }

    #[test]
    fn splits_bing_text_without_losing_characters() {
        let text = "第一句。第二句！\n第三行";
        let chunks = split_bing_text(text, 5);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 5));
        assert_eq!(chunks.concat(), text);
        // 上限内不切。
        assert_eq!(split_bing_text("hello", 900), vec!["hello".to_string()]);
        assert!(split_bing_text("", 900).is_empty());
    }

    #[test]
    fn maps_bing_target_languages() {
        // 繁简必须分开：Bing 有独立的 zh-Hant，不能拿简体顶替。
        assert_eq!(bing_target_language("zh-CN"), "zh-Hans");
        assert_eq!(bing_target_language("zh-TW"), "zh-Hant");
        assert_eq!(bing_target_language("ja"), "ja");
        assert_eq!(bing_target_language("pt"), "pt");
    }

    #[test]
    fn parses_bing_translation_and_rejects_expired_session() {
        let ok: Value = serde_json::from_str(
            r#"[{"detectedLanguage":{"language":"en"},"translations":[{"text":"你好","to":"zh-Hans"}]}]"#,
        )
        .unwrap();
        assert_eq!(parse_bing_translation(&ok), Some("你好".to_string()));

        let expired: Value = serde_json::from_str(r#"{"statusCode":205}"#).unwrap();
        assert_eq!(parse_bing_translation(&expired), None);
        let blank: Value =
            serde_json::from_str(r#"[{"translations":[{"text":"   "}]}]"#).unwrap();
        assert_eq!(parse_bing_translation(&blank), None);
    }

    /// 必应失败后熔断：期间直接走 MyMemory；测试之间要清掉全局状态。
    #[test]
    fn bing_backoff_skips_until_deadline() {
        BING_DISABLED_UNTIL.lock().unwrap().take();
        assert!(bing_enabled_now());

        disable_bing_for_a_while();
        assert!(!bing_enabled_now());

        *BING_DISABLED_UNTIL.lock().unwrap() = Some(Instant::now() - Duration::from_secs(1));
        assert!(bing_enabled_now());

        BING_DISABLED_UNTIL.lock().unwrap().take();
    }

    #[test]
    fn parses_mymemory_response_and_rejects_quota_warning() {
        let ok: Value = serde_json::from_str(
            r#"{"responseData":{"translatedText":"你好世界"},"responseStatus":200}"#,
        )
        .unwrap();
        assert_eq!(parse_mymemory(&ok), Some("你好世界".into()));

        let quota: Value = serde_json::from_str(
            r#"{"responseData":{"translatedText":"MYMEMORY WARNING: YOU USED ALL AVAILABLE FREE TRANSLATIONS FOR TODAY"},"responseStatus":200}"#,
        )
        .unwrap();
        assert_eq!(parse_mymemory(&quota), None);

        let error: Value = serde_json::from_str(r#"{"responseStatus":403}"#).unwrap();
        assert_eq!(parse_mymemory(&error), None);
    }
    #[test]
    fn groups_at_fifty() {
        let jobs = (0..51)
            .map(|i| TranslationJob {
                sid: i.to_string(),
                text: "words".into(),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            batches(&jobs).iter().map(Vec::len).collect::<Vec<_>>(),
            vec![15, 15, 15, 6]
        );
    }
    #[test]
    fn splits_short_sentence_like_python() {
        let parts = split_sentences("Hi. Next sentence! 42.1")
            .into_iter()
            .map(|segment| segment.text)
            .collect::<Vec<_>>();
        assert!(parts.join("").contains("Hi."));
        assert!(parts.join("").contains("42.1"));
    }

    #[test]
    fn keeps_bracket_spans_out_of_translation() {
        let segments = split_sentences("[B42] Hotfix is live. Read more.");
        assert_eq!(segments[0].kind, SegmentKind::Skip);
        assert_eq!(segments[0].text, "[B42]");
        assert!(segments
            .iter()
            .filter(|segment| segment.kind == SegmentKind::Text)
            .all(|segment| !segment.text.contains('[')));
    }

    #[test]
    fn only_wraps_translatable_segments_in_spans() {
        let mut jobs = Vec::new();
        let html =
            add_translation_spans("<p>[B42] Hotfix is live. Read more.</p>", "n0", &mut jobs);
        assert!(html.contains("[B42]"));
        assert!(!html.contains("data-sid=\"n0_t0_s0\">[B42]"));
        assert!(jobs.iter().all(|job| !job.text.contains('[')));
        assert!(jobs.iter().all(|job| !job.text.trim().is_empty()));
        assert!(jobs.iter().all(|job| job.text.trim() != "."));
        let joined = jobs
            .iter()
            .map(|job| job.text.as_str())
            .collect::<Vec<_>>()
            .join("|");
        assert_eq!(joined, " Hotfix is live| Read more");
    }

    /// Differential check against the reference implementation in python/anoucne.py.
    #[test]
    fn matches_python_reference_splits() {
        let raw = include_str!("../tests/fixtures/news_split_cases.json");
        let cases: Vec<serde_json::Value> = serde_json::from_str(raw).expect("valid fixture");
        let mut mismatches = Vec::new();
        for case in &cases {
            let text = case["text"].as_str().expect("text");
            let expected = case["pieces"]
                .as_array()
                .expect("pieces")
                .iter()
                .map(|piece| piece[1].as_str().expect("chunk").to_string())
                .collect::<Vec<_>>();
            let actual = split_sentences(text)
                .into_iter()
                .map(|segment| segment.text)
                .collect::<Vec<_>>();
            if actual != expected {
                mismatches.push(format!(
                    "text={text:?}\n  python={expected:?}\n  rust  ={actual:?}"
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "{} of {} cases differ:\n{}",
            mismatches.len(),
            cases.len(),
            mismatches.join("\n")
        );
    }
}
