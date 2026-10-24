use crate::news::TranslationJob;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const BATCH_SIZE: usize = 15;
const MAX_TEXT_LEN: usize = 2000;
const TIMEOUT: Duration = Duration::from_secs(8);
const TRANSLATION_TIMEOUT: Duration = Duration::from_secs(5);
const GOOGLE_URL: &str = "https://translate.googleapis.com/translate_a/single";
const MYMEMORY_URL: &str = "https://api.mymemory.translated.net/get";
static CACHE: OnceLock<Mutex<HashMap<(String, String), (String, String)>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TranslationResult {
    pub(crate) sid: String,
    pub(crate) text: String,
    pub(crate) engine: String,
}

fn cache() -> &'static Mutex<HashMap<(String, String), (String, String)>> {
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
async fn translate_one(client: &Client, text: &str, language: &str) -> (String, String) {
    if let Some(value) = google(client, text, language).await {
        return (value, "google".into());
    }
    if let Some(value) = mymemory(client, text, language).await {
        return (value, "mymemory".into());
    }
    (text.into(), "none".into())
}
async fn translate_jobs(
    client: &Client,
    jobs: &[TranslationJob],
    language: &str,
) -> Vec<TranslationResult> {
    let mut values = HashMap::new();
    let mut pending = Vec::new();
    for job in jobs {
        if let Some(value) = cache()
            .lock()
            .ok()
            .and_then(|map| map.get(&(job.text.clone(), language.into())).cloned())
        {
            values.insert(job.sid.clone(), value);
        } else {
            pending.push(job.clone());
        }
    }
    if !pending.is_empty() {
        let joined = pending
            .iter()
            .map(|job| job.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let (translated, engine) = translate_one(client, &joined, language).await;
        let parts = translated.split('\n').collect::<Vec<_>>();
        if parts.len() == pending.len() {
            for (job, text) in pending.iter().zip(parts) {
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let value = (trimmed.to_string(), engine.clone());
                if let Ok(mut map) = cache().lock() {
                    map.insert((job.text.clone(), language.into()), value.clone());
                }
                values.insert(job.sid.clone(), value);
            }
        } else {
            for job in pending {
                let value = translate_one(client, &job.text, language).await;
                if value.1 != "none" {
                    if let Ok(mut map) = cache().lock() {
                        map.insert((job.text.clone(), language.into()), value.clone());
                    }
                }
                values.insert(job.sid.clone(), value);
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
    let mut results = Vec::new();
    for batch in batches(&jobs) {
        results.extend(translate_jobs(&client, &batch, &language).await);
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
