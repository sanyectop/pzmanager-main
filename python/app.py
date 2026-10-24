# -*- coding: utf-8 -*-
"""
Project Zomboid 创意工坊筛选器（1:1 缩略图版）
Flask 服务器 + Steam 爬虫（无需 API Key）
启动: python app.py
访问: http://127.0.0.1:5000
"""
import json
import time
from urllib.parse import urlencode

import requests
from flask import Flask, request, jsonify, render_template_string

app = Flask(__name__)

# ============================================================
# 常量配置
# ============================================================
APP_ID = 108600
STEAM_HEADERS = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                  "(KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36",
    "Accept-Language": "zh-CN,zh;q=0.9",
    "Referer": "https://steamcommunity.com/",
}

SORT_OPTIONS = [
    {"value": "trend",                 "label": "🔥 最热门（趋势）"},
    {"value": "mostrecent",            "label": "🆕 最新发布"},
    {"value": "lastupdated",           "label": "🔄 最近更新"},
    {"value": "totaluniquesubscribers","label": "👑 订阅最多"},
    {"value": "toprated",              "label": "⭐ 评分最高"},
]

PZ_TAGS = [
    "Build 40", "Build 41", "Build 42",
    "Animals", "Audio", "Balance", "Building", "Clothing/Armor",
    "Farming", "Food", "Framework", "Hardmode", "Interface", "Items",
    "Language/Translation", "Literature", "Map", "Military", "Misc",
    "Models", "Multiplayer", "Pop Culture", "QoL", "Realistic",
    "Silly/Fun", "Skills", "Textures", "Traits", "Vehicles", "Weapons", "WIP",
]

_cache = {}
CACHE_TTL = 300


# ============================================================
# Steam 客户端
# ============================================================
def build_steam_url(params: dict) -> str:
    base = "https://steamcommunity.com/workshop/browse/"
    q = [
        ("appid", APP_ID),
        ("section", "readytouseitems"),
        ("browsesort", params.get("browsesort", "trend")),
        ("p", params.get("p", 1)),
        ("num_per_page", params.get("num_per_page", 30)),
    ]
    if params.get("browsesort") == "trend" and params.get("days"):
        q.append(("days", params["days"]))
    if params.get("search_text"):
        q.append(("searchtext", params["search_text"]))
    for t in params.get("requiredtags") or []:
        q.append(("requiredtags[]", t))
    for t in params.get("excludedtags") or []:
        q.append(("excludedtags[]", t))
    return base + "?" + urlencode(q)


def fetch_steam(url: str, timeout: int = 25) -> str:
    now = time.time()
    if url in _cache:
        ts, html = _cache[url]
        if now - ts < CACHE_TTL:
            return html
    r = requests.get(url, headers=STEAM_HEADERS, timeout=timeout)
    r.raise_for_status()
    r.encoding = "utf-8"
    _cache[url] = (now, r.text)
    return r.text


def extract_render_context(html_text: str):
    marker = "window.SSR.renderContext"
    idx = html_text.find(marker)
    if idx < 0:
        return None
    jp = html_text.find("JSON.parse(", idx)
    if jp < 0:
        return None

    start = jp + len("JSON.parse(")
    depth, in_str, esc = 1, False, False
    end = -1
    for i in range(start, len(html_text)):
        ch = html_text[i]
        if in_str:
            if esc:
                esc = False
            elif ch == "\\":
                esc = True
            elif ch == '"':
                in_str = False
        else:
            if ch == '"':
                in_str = True
            elif ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
                if depth == 0:
                    end = i
                    break
    if end < 0:
        return None

    arg = html_text[start:end].strip()
    try:
        inner = json.loads(arg)
        return json.loads(inner)
    except Exception as e:
        print(f"[extract] 失败: {e}")
        return None


def parse_workshop_items(ctx):
    if not ctx:
        return [], {}
    qd_str = ctx.get("queryData", "")
    if not qd_str:
        return [], {}
    try:
        qd = json.loads(qd_str)
    except Exception as e:
        print(f"[parse] queryData 失败: {e}")
        return [], {}

    items, meta = [], {}
    for q in qd.get("queries", []):
        qkey = q.get("queryKey", [])
        if not (isinstance(qkey, list) and qkey and qkey[0] == "workshop_browse"):
            continue
        data = (q.get("state", {}) or {}).get("data") or {}
        meta = {
            "current_page": data.get("current_page", 1),
            "total_pages": data.get("total_pages", 1),
            "total_count": data.get("total_count", 0),
        }
        cmap = {}
        for c in data.get("creator_player_link_details", []):
            pd = c.get("public_data", {})
            if pd.get("steamid") and pd.get("persona_name"):
                cmap[pd["steamid"]] = pd["persona_name"]

        for raw in data.get("results", []):
            pid = str(raw.get("publishedfileid", ""))
            if not pid:
                continue
            tags = [t.get("tag") for t in (raw.get("tags") or [])
                    if isinstance(t, dict)]
            sid = raw.get("creator", "")
            items.append({
                "id": pid,
                "title": raw.get("title", ""),
                "short_description": raw.get("short_description", ""),
                "preview_url": raw.get("preview_url", ""),
                "subscriptions": raw.get("subscriptions", 0) or 0,
                "favorited": raw.get("favorited", 0) or 0,
                "views": raw.get("views", 0) or 0,
                "star_rating": raw.get("star_rating", -1),
                "total_votes": raw.get("total_votes", 0) or 0,
                "file_size": raw.get("file_size", "0"),
                "time_created": raw.get("time_created", 0) or 0,
                "time_updated": raw.get("time_updated", 0) or 0,
                "num_comments": raw.get("num_comments_public", 0) or 0,
                "tags": tags,
                "creator_id": sid,
                "creator_name": cmap.get(sid, sid),
                "file_url": f"https://steamcommunity.com/sharedfiles/filedetails/?id={pid}",
            })
    return items, meta


# ============================================================
# Flask 路由
# ============================================================
@app.route("/")
def index():
    return render_template_string(
        INDEX_HTML,
        tags=PZ_TAGS,
        sort_options=SORT_OPTIONS,
    )


@app.route("/api/workshop")
def api_workshop():
    params = {
        "browsesort":   request.args.get("browsesort", "trend"),
        "days":         request.args.get("days", "7"),
        "search_text":  request.args.get("search_text", ""),
        "p":            request.args.get("p", "1"),
        "num_per_page": request.args.get("num_per_page", "30"),
        "requiredtags": request.args.getlist("requiredtags"),
        "excludedtags": request.args.getlist("excludedtags"),
    }

    url = build_steam_url(params)
    print(f"[API] {url}")

    try:
        html_text = fetch_steam(url)
    except Exception as e:
        return jsonify({"error": f"请求 Steam 失败: {e}"}), 502

    ctx = extract_render_context(html_text)
    if not ctx:
        return jsonify({"error": "无法解析 Steam 页面"}), 500

    items, meta = parse_workshop_items(ctx)
    return jsonify({
        "items": items,
        "meta": meta,
        "steam_url": url,
    })


# ============================================================
# 前端页面（内联）
# ============================================================
INDEX_HTML = r"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Project Zomboid 创意工坊筛选器</title>
<style>
:root {
  --bg:#0f1419; --panel:#131a22; --card:#1b222b; --card-hover:#232c37;
  --border:#2b3641; --text:#e6e9ee; --muted:#8a94a3;
  --accent:#66c0f4; --accent2:#4c9ee0; --tag:#2e3a48;
  --green:#6fcf97; --yellow:#f2c94c; --red:#eb5757;
}
* { box-sizing: border-box; }
body {
  margin:0; background:var(--bg); color:var(--text);
  font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","Microsoft YaHei",
              Roboto,sans-serif; line-height:1.5;
}

/* ---------- 顶部栏（含分页） ---------- */
header {
  position:sticky; top:0; z-index:100;
  background:linear-gradient(180deg,#171d26 0%,#131a22 100%);
  border-bottom:1px solid var(--border);
  padding:10px 20px;
  display:flex; align-items:center; gap:16px;
  flex-wrap:wrap;
}
header h1 {
  margin:0; font-size:17px; font-weight:600;
  white-space:nowrap;
}
header .status {
  color:var(--muted); font-size:12.5px;
  margin-left:auto;
  white-space:nowrap;
}

.top-pager {
  display:flex; align-items:center; gap:4px;
}
.top-pager .steam-link {
  margin-right:14px;
  color:var(--accent); text-decoration:none;
  font-size:12.5px;
  padding:4px 8px; border-radius:5px;
  transition:background .12s ease;
}
.top-pager .steam-link:hover { background:rgba(102,192,244,.1); }

.pager {
  display:flex; align-items:center; gap:4px;
}
.pager button {
  background:transparent; border:1px solid var(--border);
  color:var(--text); min-width:30px; height:30px;
  padding:0 8px; border-radius:6px;
  cursor:pointer; font-size:13px;
  transition:all .12s ease;
  display:inline-flex; align-items:center; justify-content:center;
}
.pager button:hover:not(:disabled) { border-color:var(--accent); color:var(--accent); }
.pager button:disabled { opacity:.3; cursor:not-allowed; }
.pager button.active {
  background:var(--accent); color:#06121c;
  border-color:var(--accent); font-weight:600;
}
.pager .dots { color:var(--muted); padding:0 4px; user-select:none; }

/* ---------- 主布局 ---------- */
.layout {
  display:grid; grid-template-columns:300px 1fr; gap:0;
  min-height:calc(100vh - 51px);
}
aside {
  background:var(--panel); border-right:1px solid var(--border);
  padding:16px; overflow-y:auto;
  max-height:calc(100vh - 51px);
  position:sticky; top:51px;
}
aside::-webkit-scrollbar { width:8px; }
aside::-webkit-scrollbar-thumb { background:#2b3641; border-radius:4px; }

.filter-group { margin-bottom:16px; }
.filter-group > label {
  display:block; font-size:11.5px; color:var(--muted);
  margin-bottom:6px; font-weight:600; letter-spacing:.4px;
  text-transform:uppercase;
}
.filter-group > label small { text-transform:none; opacity:.6; font-weight:400; }
.filter-group select,
.filter-group input[type=text],
.filter-group input[type=search] {
  width:100%; background:#0d1319; border:1px solid var(--border);
  color:var(--text); padding:8px 10px; border-radius:6px;
  font-size:13px; outline:none;
}
.filter-group select:focus,
.filter-group input:focus { border-color:var(--accent); }

.tag-list { display:flex; flex-wrap:wrap; gap:5px; }
.tag-chip {
  background:var(--tag); color:#b7c2cf; padding:3px 9px;
  border-radius:6px; font-size:11.5px; cursor:pointer;
  border:1px solid transparent; transition:all .12s ease;
  user-select:none;
}
.tag-chip:hover { background:#3a4756; }
.tag-chip.active {
  background:var(--accent); color:#06121c;
  font-weight:600; border-color:var(--accent);
}
.tag-chip.excluded.active {
  background:var(--red); color:#fff; border-color:var(--red);
}

.btn-row { display:flex; gap:8px; margin-top:20px; }
.btn-row button {
  flex:1; padding:9px 12px; border-radius:6px; font-size:13px;
  cursor:pointer; border:1px solid var(--border);
  background:transparent; color:var(--text);
  transition:all .12s ease;
}
.btn-row button:hover { border-color:var(--accent); color:var(--accent); }

/* ---------- 主内容 ---------- */
main { padding:16px 20px 60px; }

.toolbar-info {
  margin-bottom:14px; font-size:12.5px; color:var(--muted);
}

/* ---------- 网格：更紧凑，适配方形缩略图 ---------- */
.grid {
  display:grid;
  grid-template-columns:repeat(auto-fill,minmax(240px,1fr));
  gap:14px;
}

/* ---------- 卡片（整卡可点击） ---------- */
.card {
  position:relative;
  background:var(--card); border:1px solid var(--border);
  border-radius:10px; overflow:hidden;
  display:flex; flex-direction:column;
  transition:transform .12s ease, background .12s ease,
             border-color .12s ease, box-shadow .12s ease;
  cursor:pointer;
}
.card:hover {
  background:var(--card-hover); border-color:var(--accent2);
  transform:translateY(-2px);
  box-shadow:0 6px 20px rgba(0,0,0,.35);
}
.card:active { transform:translateY(0); }

/* ★ 关键：1:1 方形缩略图 ★ */
.thumb {
  aspect-ratio: 1 / 1;
  background:#0a0f14;
  overflow:hidden;
  position:relative;
}
.thumb img {
  width:100%; height:100%;
  object-fit:cover;
  display:block;
  transition:transform .3s ease;
}
.card:hover .thumb img { transform:scale(1.04); }
.thumb.no-img::after {
  content:"无预览图"; position:absolute; inset:0;
  display:flex; align-items:center; justify-content:center;
  color:var(--muted); font-size:13px;
}
.thumb .overlay {
  position:absolute; top:8px; right:8px;
  background:rgba(0,0,0,.65); color:#fff;
  padding:3px 8px; border-radius:5px; font-size:11px;
  opacity:0; transition:opacity .15s ease;
  backdrop-filter:blur(4px);
}
.card:hover .thumb .overlay { opacity:1; }

/* 卡片内容：紧凑排版 */
.body {
  padding:10px 12px 12px;
  display:flex; flex-direction:column; gap:6px;
  flex:1; min-height:0;
}
.title {
  margin:0; font-size:13.5px; line-height:1.35;
  display:-webkit-box; -webkit-line-clamp:2; -webkit-box-orient:vertical;
  overflow:hidden;
  color:var(--text);
  min-height:36px;
}
.card:hover .title { color:var(--accent); }

.desc {
  margin:0; font-size:11.5px; color:var(--muted); line-height:1.45;
  display:-webkit-box; -webkit-line-clamp:2; -webkit-box-orient:vertical;
  overflow:hidden;
  min-height:33px;
}

.tags { display:flex; flex-wrap:wrap; gap:3px; min-height:18px; }
.tag {
  background:var(--tag); color:#b7c2cf;
  padding:1px 6px; border-radius:5px; font-size:10.5px;
  white-space:nowrap;
}

/* 元信息：主行 + 次行 */
.meta {
  display:flex; flex-wrap:wrap; gap:8px;
  font-size:11px; color:var(--muted);
  margin-top:auto;
}
.meta em { font-style:normal; opacity:.65; }
.meta2 {
  display:flex; gap:8px; font-size:11px; color:var(--muted);
  overflow:hidden;
}
.meta2 .creator {
  flex:1; min-width:0;
  overflow:hidden; text-overflow:ellipsis; white-space:nowrap;
}
.meta2 .date { flex-shrink:0; }

.r-pos { color:var(--green); }
.r-mid { color:var(--yellow); }
.r-low { color:var(--red); }

.bar {
  height:3px; background:#0a1017; border-radius:3px;
  overflow:hidden;
}
.bar-fill {
  height:100%;
  background:linear-gradient(90deg,var(--accent2),var(--accent));
  transition:width .3s ease;
}

.empty {
  grid-column:1/-1; text-align:center;
  color:var(--muted); padding:60px 20px; font-size:14px;
}

/* ---------- 加载遮罩 ---------- */
.loading {
  position:fixed; inset:0; background:rgba(15,20,25,.7);
  display:none; align-items:center; justify-content:center;
  z-index:999;
}
.loading.show { display:flex; }
.spinner {
  width:40px; height:40px; border:3px solid var(--border);
  border-top-color:var(--accent); border-radius:50%;
  animation:spin .8s linear infinite;
}
@keyframes spin { to { transform:rotate(360deg); } }

@media (max-width: 900px) {
  .layout { grid-template-columns:1fr; }
  aside { position:static; max-height:none; }
  header .status { display:none; }
  .top-pager .steam-link { display:none; }
  .grid { grid-template-columns:repeat(auto-fill,minmax(160px,1fr)); gap:10px; }
  .title { font-size:12.5px; min-height:33px; }
  .desc { display:none; }
  .meta2 { display:none; }
}
</style>
</head>
<body>

<!-- ============ 顶部栏：标题 + 状态 + 右上角分页 ============ -->
<header>
  <h1>🧟 Project Zomboid 创意工坊</h1>
  <span class="status" id="status">准备就绪</span>

  <div class="top-pager">
    <a class="steam-link" id="steam-link" href="#" target="_blank" rel="noopener">
      Steam 原页面 ↗
    </a>
    <div class="pager" id="pager"></div>
  </div>
</header>

<div class="layout">
  <aside>
    <div class="filter-group">
      <label>排序方式</label>
      <select id="sort">
        {% for opt in sort_options %}
          <option value="{{ opt.value }}" {% if opt.value == 'trend' %}selected{% endif %}>{{ opt.label }}</option>
        {% endfor %}
      </select>
    </div>

    <div class="filter-group" id="days-group">
      <label>时间范围 <small>（仅趋势有效）</small></label>
      <select id="days">
        <option value="1">1 天</option>
        <option value="7" selected>7 天</option>
        <option value="30">30 天</option>
        <option value="90">90 天</option>
        <option value="365">1 年</option>
      </select>
    </div>

    <div class="filter-group">
      <label>搜索关键词</label>
      <input type="search" id="search" placeholder="标题 / 描述包含…">
    </div>

    <div class="filter-group">
      <label>每页数量</label>
      <select id="per-page">
        <option value="15">15</option>
        <option value="30" selected>30</option>
      </select>
    </div>

    <div class="filter-group">
      <label>必须包含标签 <small>（AND）</small></label>
      <div class="tag-list" id="required-tags">
        {% for t in tags %}
          <span class="tag-chip" data-tag="{{ t }}">{{ t }}</span>
        {% endfor %}
      </div>
    </div>

    <div class="filter-group">
      <label>必须排除标签</label>
      <div class="tag-list" id="excluded-tags">
        {% for t in tags %}
          <span class="tag-chip excluded" data-tag="{{ t }}">{{ t }}</span>
        {% endfor %}
      </div>
    </div>

    <div class="btn-row">
      <button id="reset-btn">↺ 重置全部</button>
    </div>
  </aside>

  <main>
    <div class="toolbar-info">
      <span id="info">加载中…</span>
    </div>
    <div class="grid" id="grid"></div>
  </main>
</div>

<div class="loading" id="loading"><div class="spinner"></div></div>

<script>
// ============================================================
// 状态
// ============================================================
const state = {
  browsesort: 'trend',
  days: 7,
  search_text: '',
  num_per_page: 30,
  p: 1,
  requiredtags: new Set(),
  excludedtags: new Set(),
};

let abortCtrl = null;
let reqId = 0;

// ============================================================
// DOM
// ============================================================
const $ = id => document.getElementById(id);
const grid = $('grid');
const pager = $('pager');
const statusEl = $('status');
const infoEl = $('info');
const steamLink = $('steam-link');
const loading = $('loading');

// ============================================================
// 实时应用：统一防抖入口
// ============================================================
let applyTimer = null;
function scheduleLoad(delay = 250) {
  clearTimeout(applyTimer);
  applyTimer = setTimeout(() => {
    state.p = 1;
    load();
  }, delay);
}

// ============================================================
// 标签多选
// ============================================================
function bindTagChips(containerId, targetSet) {
  const container = $(containerId);
  container.addEventListener('click', e => {
    const chip = e.target.closest('.tag-chip');
    if (!chip) return;
    const tag = chip.dataset.tag;
    if (targetSet.has(tag)) {
      targetSet.delete(tag);
      chip.classList.remove('active');
    } else {
      targetSet.add(tag);
      chip.classList.add('active');
    }
    scheduleLoad();
  });
}
bindTagChips('required-tags', state.requiredtags);
bindTagChips('excluded-tags', state.excludedtags);

// ============================================================
// 各控件事件
// ============================================================
$('sort').addEventListener('change', e => {
  state.browsesort = e.target.value;
  $('days-group').style.display = state.browsesort === 'trend' ? '' : 'none';
  scheduleLoad();
});

$('days').addEventListener('change', e => {
  state.days = +e.target.value;
  scheduleLoad();
});

$('search').addEventListener('input', e => {
  state.search_text = e.target.value.trim();
  scheduleLoad(400);
});

$('per-page').addEventListener('change', e => {
  state.num_per_page = +e.target.value;
  scheduleLoad(0);
});

$('reset-btn').addEventListener('click', () => {
  state.browsesort = 'trend';
  state.days = 7;
  state.search_text = '';
  state.num_per_page = 30;
  state.p = 1;
  state.requiredtags.clear();
  state.excludedtags.clear();

  $('sort').value = 'trend';
  $('days').value = '7';
  $('search').value = '';
  $('per-page').value = '30';
  document.querySelectorAll('.tag-chip.active').forEach(c => c.classList.remove('active'));
  $('days-group').style.display = '';

  scheduleLoad(0);
});

// ============================================================
// 加载数据
// ============================================================
async function load() {
  if (abortCtrl) abortCtrl.abort();
  abortCtrl = new AbortController();
  const myId = ++reqId;

  state.browsesort = $('sort').value;
  state.days = +$('days').value;

  const params = new URLSearchParams();
  params.set('browsesort', state.browsesort);
  if (state.browsesort === 'trend') params.set('days', state.days);
  if (state.search_text) params.set('search_text', state.search_text);
  params.set('p', state.p);
  params.set('num_per_page', state.num_per_page);
  state.requiredtags.forEach(t => params.append('requiredtags', t));
  state.excludedtags.forEach(t => params.append('excludedtags', t));

  loading.classList.add('show');
  statusEl.textContent = '加载中…';

  try {
    const r = await fetch('/api/workshop?' + params, { signal: abortCtrl.signal });
    const data = await r.json();

    if (myId !== reqId) return;

    if (data.error) {
      statusEl.textContent = '❌ ' + data.error;
      grid.innerHTML = `<div class="empty">${data.error}</div>`;
      pager.innerHTML = '';
      return;
    }
    render(data);
    const m = data.meta;
    statusEl.textContent = `✅ 共 ${m.total_count.toLocaleString()} 个结果`;
    infoEl.textContent = `第 ${m.current_page} / ${m.total_pages} 页 · 本页 ${data.items.length} 条`;
    steamLink.href = data.steam_url;
  } catch (e) {
    if (e.name === 'AbortError') return;
    if (myId !== reqId) return;
    statusEl.textContent = '❌ 加载失败';
    grid.innerHTML = `<div class="empty">加载失败：${e.message}</div>`;
  } finally {
    if (myId === reqId) loading.classList.remove('show');
  }
}

// ============================================================
// 工具函数
// ============================================================
function escapeHtml(s) {
  return String(s ?? '')
    .replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')
    .replace(/"/g,'&quot;').replace(/'/g,'&#39;');
}
function escapeAttr(s) {
  return escapeHtml(s).replace(/`/g,'&#96;');
}
function humanSize(n) {
  n = parseInt(n) || 0;
  if (n <= 0) return '-';
  const u = ['B','KB','MB','GB','TB']; let f = n, i = 0;
  while (f >= 1024 && i < u.length - 1) { f /= 1024; i++; }
  return f.toFixed(1) + ' ' + u[i];
}
function tsStr(ts) {
  if (!ts) return '-';
  const d = new Date(ts * 1000);
  return d.getFullYear() + '-' +
         String(d.getMonth()+1).padStart(2,'0') + '-' +
         String(d.getDate()).padStart(2,'0');
}
function starStr(r) {
  if (r == null || r < 0) return '—';
  const f = Math.floor(r);
  return '★'.repeat(f) + '☆'.repeat(5 - f);
}

// ============================================================
// 渲染结果
// ============================================================
function render(data) {
  const items = data.items;
  if (!items.length) {
    grid.innerHTML = '<div class="empty">😢 没有匹配的结果，试试放宽筛选条件</div>';
    pager.innerHTML = '';
    return;
  }
  const maxSubs = Math.max(...items.map(i => i.subscriptions), 1);

  grid.innerHTML = items.map(it => {
    const pct = Math.min(100, it.subscriptions / maxSubs * 100);
    const desc = escapeHtml((it.short_description || '').slice(0, 160));
    const creator = escapeHtml(it.creator_name || '未知');
    const ratingCls = it.star_rating >= 4 ? 'r-pos'
                    : it.star_rating >= 3 ? 'r-mid' : 'r-low';
    // 方形缩略图卡片：最多 3 个标签
    const tagsHtml = it.tags.slice(0, 3)
      .map(t => `<span class="tag">${escapeHtml(t)}</span>`).join('');
    const url = escapeAttr(it.file_url);

    return `
      <div class="card" data-href="${url}" role="link" tabindex="0"
           title="打开 Steam 创意工坊：${escapeAttr(it.title)}">
        <div class="thumb">
          <img loading="lazy" src="${escapeAttr(it.preview_url)}"
               alt="" onerror="this.style.display='none';this.parentNode.classList.add('no-img')">
          <div class="overlay">↗ 打开 Steam</div>
        </div>
        <div class="body">
          <h3 class="title">${escapeHtml(it.title)}</h3>
          <p class="desc">${desc}</p>
          <div class="tags">${tagsHtml}</div>
          <div class="meta">
            <span title="订阅数">👥 ${it.subscriptions.toLocaleString()}</span>
            <span class="${ratingCls}" title="评分">⭐ ${starStr(it.star_rating)}</span>
            <span title="浏览量">👁 ${it.views.toLocaleString()}</span>
            <span title="文件大小">💾 ${humanSize(it.file_size)}</span>
          </div>
          <div class="meta2">
            <span class="creator" title="${creator}">👤 ${creator}</span>
            <span class="date">📅 ${tsStr(it.time_updated)}</span>
          </div>
          <div class="bar"><div class="bar-fill" style="width:${pct.toFixed(1)}%"></div></div>
        </div>
      </div>`;
  }).join('');

  grid.querySelectorAll('.card').forEach(card => {
    card.addEventListener('click', e => {
      if (e.target.closest('a, button')) return;
      const href = card.dataset.href;
      if (href) window.open(href, '_blank', 'noopener');
    });
    card.addEventListener('keydown', e => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        const href = card.dataset.href;
        if (href) window.open(href, '_blank', 'noopener');
      }
    });
  });

  renderPager(data.meta);
}

// ============================================================
// 分页（右上角）
// ============================================================
function renderPager(meta) {
  const { current_page: cp, total_pages: tp } = meta;

  if (!tp || tp <= 1) { pager.innerHTML = ''; return; }

  const btns = [];
  btns.push(`<button ${cp <= 1 ? 'disabled' : ''} data-p="${cp - 1}" title="上一页">‹</button>`);

  const pages = new Set([1, tp, cp, cp - 1, cp + 1]);
  const list = [...pages].filter(p => p >= 1 && p <= tp).sort((a, b) => a - b);

  let prev = 0;
  for (const p of list) {
    if (p - prev > 1) btns.push(`<span class="dots">…</span>`);
    btns.push(`<button class="${p === cp ? 'active' : ''}" data-p="${p}">${p}</button>`);
    prev = p;
  }

  btns.push(`<button ${cp >= tp ? 'disabled' : ''} data-p="${cp + 1}" title="下一页">›</button>`);

  pager.innerHTML = btns.join('');

  pager.querySelectorAll('button[data-p]').forEach(btn => {
    btn.addEventListener('click', () => {
      const p = +btn.dataset.p;
      if (p >= 1 && p <= tp && p !== cp) {
        state.p = p;
        load();
        window.scrollTo({ top: 0, behavior: 'smooth' });
      }
    });
  });
}

// ============================================================
// 首次加载
// ============================================================
load();
</script>
</body>
</html>"""


# ============================================================
# 启动
# ============================================================
if __name__ == "__main__":
    print("=" * 60)
    print("  Project Zomboid 创意工坊筛选器（1:1 缩略图）")
    print("  打开浏览器访问: http://127.0.0.1:5000")
    print("=" * 60)
    app.run(host="127.0.0.1", port=15000, debug=False)