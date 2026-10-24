import re
import json
import time
import uuid
import queue
import random
import threading
import requests
from html import escape as html_escape
from datetime import datetime
from concurrent.futures import ThreadPoolExecutor

from bs4 import BeautifulSoup
from flask import (
    Flask, render_template_string, abort, request,
    Response, stream_with_context,
)

# 优先使用 curl_cffi（模拟 Chrome TLS 指纹）
try:
    from curl_cffi import requests as cffi_requests
    HAS_CURL_CFFI = True
    print("[启动] curl_cffi 已就绪，Google 请求将模拟 Chrome TLS 指纹")
except ImportError:
    cffi_requests = None
    HAS_CURL_CFFI = False
    print("[启动] 未安装 curl_cffi，将使用 requests（Google 可能被限流）")
    print("[启动] 建议执行: pip install curl_cffi")

app = Flask(__name__)

# ==================== 基础配置 ====================
APP_ID = 108600  # Project Zomboid
API_URL = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/"

# ==================== HTTP Session（Steam / Yandex 用） ====================
_session = requests.Session()
_session.headers.update({
    "User-Agent": (
        "Mozilla/5.0 (Linux; Android 10; SM-G975F) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/120.0.0.0 Mobile Safari/537.36"
    ),
    "Accept": "application/json",
})

# ==================== Yandex 翻译配置 ====================
YANDEX_DETECT_URL = "https://translate.yandex.net/api/v1/tr.json/detect"
YANDEX_TRANSLATE_URL = "https://translate.yandex.net/api/v1/tr.json/translate"

YANDEX_LANG_MAP = {
    "auto": "auto", "zh_cn": "zh", "zh_tw": "zh", "en": "en",
    "ja": "ja", "ko": "ko", "fr": "fr", "es": "es", "ru": "ru",
    "de": "de", "it": "it", "tr": "tr", "pt": "pt", "vi": "vi",
    "id": "id", "th": "th", "ms": "ms", "ar": "ar", "hi": "hi",
}

# ==================== Google 翻译配置 ====================
GOOGLE_TRANSLATE_URL = "https://translate.googleapis.com/translate_a/single"
GOOGLE_TIMEOUT = 5
GOOGLE_RETRY_ON_429 = 1
GOOGLE_COOLDOWN = 300

GOOGLE_MIN_DELAY = 0.3
GOOGLE_MAX_DELAY = 1.0
GOOGLE_LOCK_TIMEOUT = 2.0

_google_lock = threading.Lock()
_google_last_call = 0.0


def generate_sid() -> str:
    return uuid.uuid4().hex


def to_yandex_lang(lang: str) -> str:
    return (lang or "en").replace("-", "_").lower()


def to_google_lang(lang: str) -> str:
    if not lang or lang == "auto":
        return "auto"
    return lang.replace("_", "-")


# ==================== 翻译失败分类 ====================
class GoogleFailed(Exception):
    pass


class GoogleRateLimit(Exception):
    pass


class GoogleBusy(Exception):
    pass


# ==================== Google 可用性开关 ====================
_google_disable_lock = threading.Lock()
_google_disabled_until = 0.0


def _is_google_available() -> bool:
    global _google_disabled_until
    if _google_disabled_until == 0:
        return True
    if time.time() >= _google_disabled_until:
        with _google_disable_lock:
            _google_disabled_until = 0.0
        print("[引擎] Google 冷却结束，恢复使用")
        return True
    return False


def _disable_google_for_a_while(reason: str):
    global _google_disabled_until
    with _google_disable_lock:
        _google_disabled_until = time.time() + GOOGLE_COOLDOWN
    print(f"[引擎] Google 已禁用 {GOOGLE_COOLDOWN}s，本批切换 Yandex: {reason}")


# ==================== Yandex 检测 & 翻译 ====================
def detect_language(text: str) -> str:
    if not text or not text.strip():
        return "_"
    params = {"srv": "android", "text": text[:500], "sid": generate_sid()}
    try:
        resp = _session.get(YANDEX_DETECT_URL, params=params, timeout=10)
        resp.raise_for_status()
        return resp.json().get("lang", "_")
    except Exception as e:
        print(f"[Yandex检测] 失败: {e}")
        return "_"


def translate_yandex(text, from_lang="auto", to_lang="zh-CN"):
    if not text or not text.strip():
        return text

    src = YANDEX_LANG_MAP.get(from_lang, from_lang)
    tgt_key = to_yandex_lang(to_lang)
    tgt = YANDEX_LANG_MAP.get(tgt_key, tgt_key)

    if tgt == "auto":
        return text

    if src == "auto":
        detected = detect_language(text)
        src = detected if detected and detected != "_" else "en"

    if src == tgt:
        return text

    sid = generate_sid()
    params = {"id": f"{sid}-0-0", "srv": "android"}
    data = {"source_lang": src, "target_lang": tgt, "text": text}

    try:
        resp = _session.post(YANDEX_TRANSLATE_URL, params=params, data=data, timeout=20)
        resp.raise_for_status()
        result = resp.json()
        if result.get("text"):
            return result["text"][0]
        print(f"[Yandex翻译] 接口返回异常: {result}")
        return None
    except Exception as e:
        print(f"[Yandex翻译] 请求失败: {e}")
        return None


# ==================== Google 翻译 ====================
def translate_google(text, from_lang="auto", to_lang="zh-CN"):
    if not text or not text.strip():
        return text

    params = {
        "client": "gtx",
        "sl": to_google_lang(from_lang),
        "tl": to_google_lang(to_lang),
        "dt": "t",
        "q": text,
    }

    if not _google_lock.acquire(timeout=GOOGLE_LOCK_TIMEOUT):
        raise GoogleBusy("另一个 Google 请求正在进行")

    try:
        global _google_last_call
        elapsed = time.time() - _google_last_call
        wait = random.uniform(GOOGLE_MIN_DELAY, GOOGLE_MAX_DELAY)
        if elapsed < wait:
            time.sleep(wait - elapsed)
        _google_last_call = time.time()

        try:
            if HAS_CURL_CFFI:
                resp = cffi_requests.get(
                    GOOGLE_TRANSLATE_URL,
                    params=params,
                    timeout=GOOGLE_TIMEOUT,
                    impersonate="chrome",
                )
            else:
                resp = requests.get(
                    GOOGLE_TRANSLATE_URL,
                    params=params,
                    timeout=GOOGLE_TIMEOUT,
                )
        except Exception as e:
            raise GoogleFailed(f"连接失败: {e}")
    finally:
        _google_lock.release()

    if resp.status_code == 429:
        raise GoogleRateLimit("Google 429 限流")
    if resp.status_code >= 500:
        raise GoogleFailed(f"Google {resp.status_code}")

    try:
        resp.raise_for_status()
    except Exception as e:
        raise GoogleFailed(f"HTTP 错误: {e}")

    try:
        data = resp.json()
    except ValueError as e:
        raise GoogleFailed(f"JSON 解析失败: {e}")

    if data and data[0]:
        return "".join(part[0] for part in data[0] if part[0])
    return None


def translate_google_with_retry(text, target_lang):
    attempts = 0
    while True:
        try:
            return translate_google(text, "auto", target_lang)
        except GoogleRateLimit as e:
            if attempts >= GOOGLE_RETRY_ON_429:
                raise
            attempts += 1
            wait = random.uniform(1.0, 1.5) * attempts
            print(f"[Google] 429 限流，{wait:.1f}s 后重试（第 {attempts} 次）")
            time.sleep(wait)


# ==================== 引擎选择 ====================
def translate_with_engine(text, target_lang):
    if not text or not text.strip():
        return text, "none"

    if _is_google_available():
        try:
            result = translate_google_with_retry(text, target_lang)
            if result:
                return result, "google"
        except GoogleBusy:
            pass
        except GoogleRateLimit as e:
            print(f"[引擎] Google 429 重试用尽，本批切 Yandex: {e}")
        except GoogleFailed as e:
            _disable_google_for_a_while(str(e))

    result = translate_yandex(text, "auto", target_lang)
    if result:
        return result, "yandex"
    return text, "none"


# ==================== 翻译缓存 ====================
_translate_cache: dict = {}
_translate_cache_lock = threading.Lock()


def _cache_lookup(text, target_lang):
    with _translate_cache_lock:
        return _translate_cache.get((text, "auto", target_lang))


def _cache_store(text, target_lang, result, engine):
    with _translate_cache_lock:
        _translate_cache[(text, "auto", target_lang)] = (result, engine)


def cached_translate(text, target_lang):
    cached = _cache_lookup(text, target_lang)
    if cached is not None:
        return cached
    result, engine = translate_with_engine(text, target_lang)
    if result:
        _cache_store(text, target_lang, result, engine)
    return result, engine


# ==================== 批量翻译 ====================
BATCH_SIZE = 50
MAX_TEXT_LEN = 2000


def _split_for_batch(items, batch_size=BATCH_SIZE, max_len=MAX_TEXT_LEN):
    batches = []
    cur = []
    cur_len = 0
    for item in items:
        t = item[1]
        add_len = len(t) + (1 if cur else 0)
        if cur and (len(cur) >= batch_size or cur_len + add_len > max_len):
            batches.append(cur)
            cur = []
            cur_len = 0
        cur.append(item)
        cur_len += add_len
    if cur:
        batches.append(cur)
    return batches


def batch_translate(items, target_lang):
    if not items:
        return []

    results = {}
    pending = []

    for sid, text in items:
        cached = _cache_lookup(text, target_lang)
        if cached is not None:
            results[sid] = cached
        else:
            pending.append((sid, text))

    if pending:
        joined = "\n".join(t for _, t in pending)
        translated, engine = translate_with_engine(joined, target_lang)

        parts = translated.split("\n") if translated else []

        if len(parts) == len(pending):
            for (sid, text), trans in zip(pending, parts):
                trans = trans.strip()
                if trans:
                    results[sid] = (trans, engine)
                    _cache_store(text, target_lang, trans, engine)
        else:
            print(f"[批量] 行数不匹配（期望 {len(pending)}，实际 {len(parts)}），回退逐句")
            for sid, text in pending:
                r, eng = translate_with_engine(text, target_lang)
                if r:
                    results[sid] = (r, eng)
                    _cache_store(text, target_lang, r, eng)

    out = []
    for sid, text in items:
        if sid in results:
            t, eng = results[sid]
            out.append((sid, t, eng))
        else:
            out.append((sid, text, "none"))
    return out


# ==================== 待翻译任务表 ====================
_pending_jobs: dict = {}
_pending_jobs_lock = threading.Lock()
_MAX_JOBS = 200


def _register_jobs(jobs: dict) -> str:
    token = uuid.uuid4().hex
    with _pending_jobs_lock:
        _pending_jobs[token] = jobs
        while len(_pending_jobs) > _MAX_JOBS:
            oldest = next(iter(_pending_jobs))
            _pending_jobs.pop(oldest, None)
    return token


def _pop_jobs(token: str) -> dict:
    with _pending_jobs_lock:
        return _pending_jobs.pop(token, {})


# ==================== 语言处理 ====================
LANG_MAP = {
    "zh": "zh-CN", "zh-cn": "zh-CN", "zh-hans": "zh-CN",
    "zh-tw": "zh-TW", "zh-hant": "zh-TW", "zh-hk": "zh-TW",
    "en": "en", "en-us": "en", "en-gb": "en",
    "ja": "ja", "ja-jp": "ja",
    "ko": "ko", "ko-kr": "ko",
    "ru": "ru", "de": "de", "fr": "fr",
    "es": "es", "pt": "pt", "it": "it",
}


def normalize_lang(lang: str) -> str:
    lang = (lang or "en").lower().strip()
    if lang in LANG_MAP:
        return LANG_MAP[lang]
    return LANG_MAP.get(lang.split("-")[0], "en")


def get_browser_lang() -> str:
    accept = request.headers.get("Accept-Language", "en-US,en;q=0.9")
    langs = []
    for part in accept.split(","):
        part = part.strip()
        if not part:
            continue
        if ";q=" in part:
            code, q = part.split(";q=")
            try:
                langs.append((code.strip(), float(q)))
            except ValueError:
                langs.append((code.strip(), 1.0))
        else:
            langs.append((part, 1.0))
    langs.sort(key=lambda x: x[1], reverse=True)

    for code, _ in langs:
        norm = normalize_lang(code)
        if norm:
            return norm
    return "en"


def resolve_lang() -> str:
    override = request.args.get("lang")
    if override:
        return normalize_lang(override)
    return get_browser_lang()


# ==================== BBCode → HTML ====================
# Steam 用 [] 代替 <>，且有一堆自定义标签

STEAM_CLAN_IMAGE_BASE = "https://clan.cloudflare.steamstatic.com/images/"

_RE_URL_ATTR = re.compile(
    r'\[url=(["\']?)(.*?)\1\](.*?)\[/url\]',
    re.DOTALL | re.IGNORECASE,
)
_RE_URL_PLAIN = re.compile(
    r'\[url\](.*?)\[/url\]',
    re.DOTALL | re.IGNORECASE,
)
# [img]...[/img] 成对
_RE_IMG_PAIR = re.compile(
    r'\[img\](.*?)\[/img\]',
    re.DOTALL | re.IGNORECASE,
)
# [img src="..."] 自闭合
_RE_IMG_SELF = re.compile(
    r'\[img\s+src=["\']?([^"\'\]]+?)["\']?\s*/?\]',
    re.DOTALL | re.IGNORECASE,
)

_SIMPLE_TAG_PAIRS = [
    ('b', 'b'), ('i', 'i'), ('u', 'u'), ('s', 's'),
    ('h1', 'h1'), ('h2', 'h2'), ('h3', 'h3'),
    ('h4', 'h4'), ('h5', 'h5'), ('h6', 'h6'),
    ('p', 'p'),
    ('quote', 'blockquote'), ('code', 'pre'),
    ('list', 'ul'), ('olist', 'ol'), ('spoiler', 'details'),
    ('table', 'table'), ('tr', 'tr'), ('td', 'td'), ('th', 'th'),
]

_STYLE_TAGS = ['color', 'size', 'font', 'center', 'left', 'right']


def _attr_escape(s: str) -> str:
    return html_escape(s, quote=True)


def bbcode_to_html(text: str) -> str:
    """把 Steam 的 [...] 风格 BBCode 转换为 HTML 标签"""
    if not text:
        return text

    # 0. Steam 图片 CDN 占位符
    text = text.replace("{STEAM_CLAN_IMAGE}", STEAM_CLAN_IMAGE_BASE)

    # 1. [img src="..."] 自闭合（必须放在成对 [img] 之前）
    text = _RE_IMG_SELF.sub(
        lambda m: (
            f'<img src="{_attr_escape(m.group(1).strip())}" '
            f'style="max-width:100%;height:auto;border-radius:6px;">'
        ),
        text,
    )

    # 2. [img]URL[/img] 成对
    text = _RE_IMG_PAIR.sub(
        lambda m: (
            f'<img src="{_attr_escape(m.group(1).strip())}" '
            f'style="max-width:100%;height:auto;border-radius:6px;">'
        ),
        text,
    )

    # 3. [url=URL]label[/url]
    def _url_attr_repl(m):
        url = m.group(2).strip()
        label = m.group(3)
        return (
            f'<a href="{_attr_escape(url)}" target="_blank" '
            f'rel="noopener noreferrer">{label}</a>'
        )
    text = _RE_URL_ATTR.sub(_url_attr_repl, text)

    # 4. [url]URL[/url]
    def _url_plain_repl(m):
        url = m.group(1).strip()
        return (
            f'<a href="{_attr_escape(url)}" target="_blank" '
            f'rel="noopener noreferrer">{url}</a>'
        )
    text = _RE_URL_PLAIN.sub(_url_plain_repl, text)

    # 5. [carousel] / [section]
    text = re.sub(
        r'\[carousel\](.*?)\[/carousel\]',
        r'<div class="bbcode-carousel">\1</div>',
        text, flags=re.DOTALL | re.IGNORECASE,
    )
    text = re.sub(
        r'\[section\](.*?)\[/section\]',
        r'<div class="bbcode-section">\1</div>',
        text, flags=re.DOTALL | re.IGNORECASE,
    )

    # 6. 通用成对标签
    for bb, html in _SIMPLE_TAG_PAIRS:
        pattern = re.compile(
            rf'\[{bb}\](.*?)\[/{bb}\]',
            re.DOTALL | re.IGNORECASE,
        )
        text = pattern.sub(
            lambda m, h=html: f'<{h}>{m.group(1)}</{h}>',
            text,
        )

    # 7. 样式标签（仅保留内容）
    for tag in _STYLE_TAGS:
        pattern = re.compile(
            rf'\[{tag}=[^\]]+\](.*?)\[/{tag}\]',
            re.DOTALL | re.IGNORECASE,
        )
        text = pattern.sub(r'\1', text)

    # 8. 单标签 / 杂项
    text = re.sub(r'\[hr\]\s*\[/hr\]', '<hr>', text, flags=re.IGNORECASE)
    text = re.sub(r'\[hr\]', '<hr>', text, flags=re.IGNORECASE)
    text = re.sub(r'\[/hr\]', '', text, flags=re.IGNORECASE)
    text = re.sub(r'\[br\s*/?\]', '<br>', text, flags=re.IGNORECASE)
    text = re.sub(r'\[/\*\]', '', text)
    text = re.sub(r'\[\*\]', '<li>', text)

    # 9. 清理空段落
    text = re.sub(r'<p>\s*</p>', '', text, flags=re.IGNORECASE)

    return text


# ==================== 断句 ====================
BRACKET_RE = re.compile(r'(\[[^\]]*\])')
SENTENCE_SPLIT_RE = re.compile(r'(?<!\d)([.!?]+)(?!\d)|(\n+)')

MIN_SEGMENT_LEN = 8
MAX_SEGMENT_LEN = 400


def is_translatable(s: str) -> bool:
    return bool(s) and any(c.isalpha() for c in s)


def _split_by_punct(text: str):
    parts = SENTENCE_SPLIT_RE.split(text)
    out = []
    for p in parts:
        if not p:
            continue
        out.append(("text" if is_translatable(p) else "punct", p))
    return out


def _merge_short(parts):
    if not parts:
        return parts

    merged = []
    i = 0
    while i < len(parts):
        kind, chunk = parts[i]

        if kind in ("punct", "skip"):
            merged.append((kind, chunk))
            i += 1
            continue

        buf = chunk
        j = i + 1
        while j < len(parts):
            nk, nc = parts[j]
            if nk == "skip":
                break
            if nk == "punct":
                if len(buf.strip()) >= MIN_SEGMENT_LEN:
                    break
                buf += nc
                j += 1
                continue
            if len(buf.strip()) < MIN_SEGMENT_LEN or len(nc.strip()) < MIN_SEGMENT_LEN:
                if len(buf) + len(nc) <= MAX_SEGMENT_LEN:
                    buf += nc
                    j += 1
                    continue
            break

        merged.append(("text", buf))
        i = j

    return merged


def split_sentences(text: str):
    result = []
    for part in BRACKET_RE.split(text):
        if not part:
            continue
        if part.startswith("[") and part.endswith("]"):
            result.append(("skip", part))
            continue
        result.extend(_split_by_punct(part))

    return _merge_short(result)


# ==================== 新闻获取 + 页面预处理 ====================
def build_news_page(target_lang: str, count: int = 20, only_gid: str = None):
    params = {"appid": APP_ID, "count": count, "maxlength": 0, "format": "json"}
    try:
        resp = _session.get(API_URL, params=params, timeout=10)
        resp.raise_for_status()
        news_items = resp.json()["appnews"]["newsitems"]
    except Exception as e:
        raise RuntimeError(f"Steam 新闻获取失败: {e}")

    if only_gid:
        news_items = [n for n in news_items if str(n.get("gid")) == str(only_gid)]

    jobs: dict = {}
    news_list = []

    for news_idx, item in enumerate(news_items):
        contents_html_raw = bbcode_to_html(item.get("contents", ""))
        soup = BeautifulSoup(contents_html_raw, "html.parser")

        for tag in soup(["script", "style"]):
            tag.decompose()

        for a in soup.find_all("a", href=True):
            if a["href"].startswith("/"):
                a["href"] = "https://store.steampowered.com" + a["href"]
            a["target"] = "_blank"
            a["rel"] = "noopener noreferrer"

        for img in soup.find_all("img"):
            src = img.get("src") or img.get("data-src") or ""
            if src.startswith("/"):
                src = "https://store.steampowered.com" + src
            if src:
                img["src"] = src
            img["style"] = "max-width:100%;height:auto;border-radius:6px;"

        if target_lang != "en":
            for node_idx, node in enumerate(list(soup.find_all(string=True))):
                text = str(node)
                if not text.strip() or not is_translatable(text):
                    continue

                sentences = split_sentences(text)
                if not sentences:
                    continue

                new_nodes = []
                s_idx = 0
                for kind, chunk in sentences:
                    if kind in ("punct", "skip"):
                        new_nodes.append(soup.new_string(chunk))
                    else:
                        sid = f"n{news_idx}_t{node_idx}_s{s_idx}"
                        jobs[sid] = chunk
                        span = soup.new_tag("span")
                        span["data-sid"] = sid
                        span["class"] = "i18n-pending"
                        span.string = chunk
                        new_nodes.append(span)
                        s_idx += 1

                if new_nodes:
                    node.replace_with(*new_nodes)

        title_sid = f"n{news_idx}_title"
        raw_title_html = bbcode_to_html(item.get("title", "无标题"))
        raw_title = (
            BeautifulSoup(raw_title_html, "html.parser").get_text().strip()
            or "无标题"
        )

        if target_lang != "en":
            title_sentences = split_sentences(raw_title)
            if any(k == "text" for k, _ in title_sentences):
                jobs[title_sid] = "".join(
                    c for k, c in title_sentences if k == "text"
                ) or raw_title
            else:
                title_sid = ""

        news_list.append({
            "gid": item.get("gid"),
            "title": raw_title,
            "title_sid": title_sid if title_sid in jobs else "",
            "url": item.get("url", ""),
            "date": datetime.fromtimestamp(item["date"]).strftime("%Y-%m-%d %H:%M:%S"),
            "author": item.get("author", ""),
            "feedlabel": item.get("feedlabel", ""),
            "contents_html": str(soup),
        })

    token = _register_jobs(jobs) if jobs else ""
    return news_list, token


# ==================== SSE 端点 ====================
def _sse_generate(jobs: dict, target_lang: str):
    yield f"data: {json.dumps({'event': 'start', 'total': len(jobs)}, ensure_ascii=False)}\n\n"

    if not jobs:
        yield f"data: {json.dumps({'event': 'done'}, ensure_ascii=False)}\n\n"
        return

    items = list(jobs.items())
    batches = _split_for_batch(items)
    total_batches = len(batches)

    q: queue.Queue = queue.Queue()

    def worker(batch):
        try:
            results = batch_translate(batch, target_lang)
            q.put(("ok", results))
        except Exception as e:
            print(f"[SSE worker] 异常: {e}")
            q.put(("err", (batch, str(e))))

    executor = ThreadPoolExecutor(max_workers=3)
    for b in batches:
        executor.submit(worker, b)

    completed = 0
    received_batches = 0

    try:
        while received_batches < total_batches:
            try:
                kind, payload = q.get(timeout=30)
            except queue.Empty:
                print("[SSE] 等待批次超时，提前结束")
                break

            received_batches += 1

            if kind == "ok":
                for sid, text, engine in payload:
                    if text:
                        evt = {
                            "event": "translate",
                            "sid": sid,
                            "text": text,
                            "engine": engine,
                        }
                        yield f"data: {json.dumps(evt, ensure_ascii=False)}\n\n"
                    completed += 1
            else:
                batch, err = payload
                for sid, original in batch:
                    evt = {
                        "event": "translate",
                        "sid": sid,
                        "text": original,
                        "engine": "none",
                    }
                    yield f"data: {json.dumps(evt, ensure_ascii=False)}\n\n"
                    completed += 1
    finally:
        executor.shutdown(wait=False)

    yield f"data: {json.dumps({'event': 'done', 'count': completed}, ensure_ascii=False)}\n\n"


@app.route("/stream")
def stream():
    token = request.args.get("token", "")
    target_lang = resolve_lang()
    jobs = _pop_jobs(token)

    resp = Response(
        stream_with_context(_sse_generate(jobs, target_lang)),
        mimetype="text/event-stream",
    )
    resp.headers["Cache-Control"] = "no-cache"
    resp.headers["X-Accel-Buffering"] = "no"
    resp.headers["Connection"] = "keep-alive"
    return resp


# ==================== 页面模板 ====================
PAGE_TEMPLATE = """
<!DOCTYPE html>
<html lang="{{ target_lang }}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Steam 游戏动态 · Project Zomboid ({{ target_lang }})</title>
<style>
    * { box-sizing: border-box; }
    body {
        font-family: -apple-system, "Segoe UI", "Microsoft YaHei", sans-serif;
        margin: 0; padding: 0;
        background: #1b2838; color: #c7d5e0;
    }
    header {
        background: linear-gradient(135deg, #171a21, #2a475e);
        padding: 24px 40px; border-bottom: 2px solid #66c0f4;
        display: flex; justify-content: space-between; align-items: center;
        flex-wrap: wrap; gap: 12px;
        position: sticky; top: 0; z-index: 10;
    }
    header h1 { margin: 0; font-size: 22px; color: #fff; }
    header .sub { margin-top: 6px; font-size: 13px; color: #8f98a0; }
    .lang-switch { font-size: 12px; }
    .lang-switch a {
        color: #66c0f4; text-decoration: none; margin: 0 6px;
        padding: 4px 10px; border: 1px solid #66c0f4; border-radius: 4px;
        display: inline-block; margin-bottom: 4px;
    }
    .lang-switch a.active { background: #66c0f4; color: #16202d; }
    .container { max-width: 960px; margin: 30px auto; padding: 0 20px; }
    .news-card {
        background: #16202d; border-left: 4px solid #66c0f4;
        border-radius: 8px; padding: 20px 24px; margin-bottom: 24px;
        box-shadow: 0 2px 8px rgba(0,0,0,0.4);
    }
    .news-card h2 { margin: 0 0 10px; font-size: 18px; color: #fff; }
    .meta { font-size: 12px; color: #8f98a0; margin-bottom: 14px; }
    .meta span { margin-right: 14px; }
    .content { line-height: 1.7; font-size: 14px; color: #acb2b8; word-wrap: break-word; }
    .content img { margin: 10px 0; }
    .content a { color: #66c0f4; text-decoration: none; }
    .content a:hover { text-decoration: underline; }
    .content p { margin: 8px 0; }
    .content h1, .content h2, .content h3,
    .content h4, .content h5, .content h6 {
        color: #e6eef4; margin: 14px 0 8px;
    }
    .content hr { border: none; border-top: 1px solid #2a475e; margin: 16px 0; }
    .content blockquote {
        margin: 10px 0; padding: 8px 14px;
        border-left: 3px solid #66c0f4; background: #1a2633;
    }
    .content pre {
        background: #0d1620; padding: 10px 14px; border-radius: 6px;
        overflow-x: auto; font-size: 13px;
    }
    .content ul, .content ol { padding-left: 22px; }
    .content li { margin: 4px 0; }

    /* BBCode 转换后新增的容器 */
    .content .bbcode-carousel {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        margin: 12px 0;
    }
    .content .bbcode-carousel img {
        flex: 1 1 200px;
        max-width: 100%;
        border-radius: 6px;
    }
    .content .bbcode-section { margin: 10px 0; }

    .content table {
        border-collapse: collapse;
        margin: 10px 0;
    }
    .content td, .content th {
        border: 1px solid #2a475e;
        padding: 6px 10px;
    }

    .news-footer {
        display: flex;
        justify-content: space-between;
        align-items: center;
        flex-wrap: wrap;
        gap: 8px;
        margin-top: 14px;
    }
    .read-more {
        display: inline-block; font-size: 13px;
        color: #66c0f4; text-decoration: none;
    }
    .read-more:hover { text-decoration: underline; }
    .engine-stats {
        font-size: 11px;
        color: #8f98a0;
        font-family: ui-monospace, Menlo, Consolas, monospace;
        letter-spacing: 0.3px;
        white-space: nowrap;
    }
    .engine-stats .g { color: #66c0f4; }
    .engine-stats .y { color: #d98c4a; }

    .error { color: #ff6b6b; padding: 40px; text-align: center; }

    .i18n-pending {
        background: linear-gradient(transparent 60%, rgba(102,192,244,0.15) 60%);
    }
    .i18n-done {
        background: none;
        transition: background 0.5s ease;
    }
    #i18n-status {
        position: fixed; right: 20px; bottom: 20px;
        background: #16202d; color: #66c0f4;
        padding: 8px 14px; border-radius: 6px;
        font-size: 12px; box-shadow: 0 2px 8px rgba(0,0,0,0.5);
        border: 1px solid #2a475e;
        z-index: 100;
    }
    #i18n-status.done { color: #7fdd7f; border-color: #2e5c2e; }
</style>
</head>
<body>
<header>
    <div>
        <h1>🎮 Steam 游戏动态 · Project Zomboid</h1>
        <div class="sub">
            AppID: {{ app_id }} · 共 {{ news|length }} 条 · 更新 {{ now }}
            · 语言 <b>{{ target_lang }}</b> · 引擎 <b>Google 优先 / Yandex 兜底</b>
        </div>
    </div>
    <div class="lang-switch">
        切换语言:
        <a href="?lang=zh-CN" class="{{ 'active' if target_lang=='zh-CN' }}">简体中文</a>
        <a href="?lang=zh-TW" class="{{ 'active' if target_lang=='zh-TW' }}">繁體中文</a>
        <a href="?lang=en"    class="{{ 'active' if target_lang=='en' }}">English</a>
        <a href="?lang=ja"    class="{{ 'active' if target_lang=='ja' }}">日本語</a>
        <a href="?lang=ko"    class="{{ 'active' if target_lang=='ko' }}">한국어</a>
        <a href="?lang=ru"    class="{{ 'active' if target_lang=='ru' }}">Русский</a>
    </div>
</header>

<div class="container">
    {% if error %}
        <div class="error">⚠️ 获取失败: {{ error }}</div>
    {% else %}
        {% for item in news %}
        <div class="news-card" data-news-idx="{{ loop.index0 }}">
            <h2>
                {% if item.title_sid %}
                    <span data-sid="{{ item.title_sid }}" class="i18n-pending">{{ item.title }}</span>
                {% else %}
                    {{ item.title }}
                {% endif %}
            </h2>
            <div class="meta">
                <span>🕒 {{ item.date }}</span>
                {% if item.author %}<span>✍️ {{ item.author }}</span>{% endif %}
                {% if item.feedlabel %}<span>🏷️ {{ item.feedlabel }}</span>{% endif %}
            </div>
            <div class="content">{{ item.contents_html | safe }}</div>
            <div class="news-footer">
                {% if item.url %}
                <a class="read-more" href="{{ item.url }}" target="_blank" rel="noopener noreferrer">
                    查看原文 →
                </a>
                {% else %}
                <span></span>
                {% endif %}
                <span class="engine-stats" id="engine-stats-{{ loop.index0 }}"></span>
            </div>
        </div>
        {% endfor %}
    {% endif %}
</div>

<div id="i18n-status">翻译准备中…</div>

<script>
(function detectBrowserLang() {
    const url = new URL(window.location.href);
    if (url.searchParams.has('lang')) return;
    const nav = navigator.language
        || (navigator.languages && navigator.languages[0]);
    if (!nav) return;
    function normalize(lang) {
        lang = (lang || '').toLowerCase();
        if (lang.startsWith('zh')) {
            if (lang.includes('tw') || lang.includes('hk') || lang.includes('hant')) {
                return 'zh-TW';
            }
            return 'zh-CN';
        }
        if (lang.startsWith('ja')) return 'ja';
        if (lang.startsWith('ko')) return 'ko';
        if (lang.startsWith('ru')) return 'ru';
        if (lang.startsWith('de')) return 'de';
        if (lang.startsWith('fr')) return 'fr';
        if (lang.startsWith('es')) return 'es';
        if (lang.startsWith('pt')) return 'pt';
        if (lang.startsWith('it')) return 'it';
        return 'en';
    }
    const jsLang = normalize(nav);
    const serverLang = "{{ target_lang }}";
    if (jsLang !== serverLang) {
        url.searchParams.set('lang', jsLang);
        window.location.replace(url.toString());
    }
})();

(function() {
    const token = "{{ token }}";
    const targetLang = "{{ target_lang }}";
    const statusEl = document.getElementById("i18n-status");

    const engineStats = {};

    function extractNewsIdx(sid) {
        const m = sid.match(/^n(\\d+)_/);
        return m ? m[1] : null;
    }

    function renderStats(nidx) {
        const stat = engineStats[nidx];
        if (!stat) return;
        const total = stat.google + stat.yandex;
        if (total === 0) return;
        const gPct = Math.round((stat.google / total) * 100);
        const yPct = 100 - gPct;
        const el = document.getElementById('engine-stats-' + nidx);
        if (!el) return;
        if (gPct === 0) {
            el.innerHTML = '<span class="y">Yandex 100%</span>';
        } else if (yPct === 0) {
            el.innerHTML = '<span class="g">Google 100%</span>';
        } else {
            el.innerHTML =
                '<span class="g">Google ' + gPct + '%</span>' +
                ' · ' +
                '<span class="y">Yandex ' + yPct + '%</span>';
        }
    }

    function bumpEngine(sid, engine) {
        const nidx = extractNewsIdx(sid);
        if (nidx === null) return;
        if (!engineStats[nidx]) {
            engineStats[nidx] = { google: 0, yandex: 0 };
        }
        if (engine === 'google') {
            engineStats[nidx].google++;
        } else if (engine === 'yandex') {
            engineStats[nidx].yandex++;
        } else {
            return;
        }
        renderStats(nidx);
    }

    if (!token || targetLang === "en") {
        statusEl.textContent = targetLang === "en" ? "原文模式" : "无翻译任务";
        statusEl.classList.add("done");
        return;
    }

    let replaced = 0;
    let total = 0;

    function replaceText(sid, text) {
        const el = document.querySelector('[data-sid="' + sid + '"]');
        if (!el) return;
        el.textContent = text;
        el.classList.remove("i18n-pending");
        el.classList.add("i18n-done");
        replaced++;
        statusEl.textContent = "翻译中 " + replaced + "/" + total;
    }

    function startStream() {
        const es = new EventSource(
            "/stream?token=" + encodeURIComponent(token) +
            "&lang=" + encodeURIComponent(targetLang)
        );

        es.onmessage = function(e) {
            let data;
            try { data = JSON.parse(e.data); } catch (err) { return; }

            if (data.event === "start") {
                total = data.total || 0;
                statusEl.textContent = "翻译中 0/" + total;
            } else if (data.event === "translate") {
                replaceText(data.sid, data.text);
                if (data.engine) {
                    bumpEngine(data.sid, data.engine);
                }
            } else if (data.event === "done") {
                es.close();
                statusEl.textContent = "✅ 翻译完成（" + replaced + " 句）";
                statusEl.classList.add("done");
            }
        };

        es.onerror = function() {
            es.close();
            statusEl.textContent = "⚠️ 翻译连接中断";
        };

        window.addEventListener("beforeunload", function() {
            try { es.close(); } catch (e) {}
        });
    }

    if (document.readyState === "complete") {
        startStream();
    } else {
        window.addEventListener("load", startStream, { once: true });
    }
})();
</script>
</body>
</html>
"""


# ==================== 路由 ====================
@app.route("/")
def index():
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    target_lang = resolve_lang()

    try:
        news, token = build_news_page(target_lang, count=20)
        return render_template_string(
            PAGE_TEMPLATE, news=news, app_id=APP_ID, now=now,
            error=None, target_lang=target_lang, token=token,
        )
    except Exception as e:
        return render_template_string(
            PAGE_TEMPLATE, news=[], app_id=APP_ID, now=now,
            error=str(e), target_lang=target_lang, token="",
        )


@app.route("/news/<gid>")
def news_detail(gid):
    now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    target_lang = resolve_lang()

    try:
        news, token = build_news_page(target_lang, count=50, only_gid=gid)
        if not news:
            abort(404)
        return render_template_string(
            PAGE_TEMPLATE, news=news, app_id=APP_ID, now=now,
            error=None, target_lang=target_lang, token=token,
        )
    except Exception as e:
        return render_template_string(
            PAGE_TEMPLATE, news=[], app_id=APP_ID, now=now,
            error=str(e), target_lang=target_lang, token="",
        )


# ==================== 入口 ====================
if __name__ == "__main__":
    app.run(host="0.0.0.0", port=10200, debug=True, threaded=True)