#!/usr/bin/env node
/**
 * PZ Manager 的 Steam API 中转（独立部署，零依赖 Node 服务器）。
 *
 * 只代理下面的固定白名单路径，不是任意代理：
 *   GET  /community/*      → https://steamcommunity.com/*        工坊浏览 HTML
 *   ANY  /api/*            → https://api.steampowered.com/*      工坊详情 + 新闻 API
 *   GET  /images/<host>/*  → 白名单 Steam 图片 CDN（预览图兜底）
 *   GET  /healthz          → {"ok":true}，客户端「测试连接」用
 *   POST /feedback         → 接收启动器「设置 › 关于 › 反馈信息」上传的日志+描述，
 *                            存到 FEEDBACK_DIR（默认 ./feedback）下的 JSON 文件
 *
 * 路由同时容忍带 /steam 前缀（反代挂在共享域名子路径时）和不带（独占子域时），
 * 例如 /steam/community/... 与 /community/... 等价。
 *
 * 启动：node server.js   （环境变量 PORT 可改端口，默认 8787；需 Node >= 18）
 * 部署（systemd / Docker / nginx 反代）见同目录 README.md。客户端对应实现在
 * src-tauri/src/steam_relay.rs 与 src/steamRelay.ts；IMAGE_HOSTS 三处必须保持一致。
 */

import { createServer } from "node:http";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { randomBytes } from "node:crypto";

const PORT = Number(process.env.PORT) || 8787;
const UPSTREAM_TIMEOUT_MS = 20_000;
// 工坊 HTML / Web API 的内存缓存时长，与客户端 300s HTML 缓存对齐；
// 既提速，也减少对 Steam 的出口请求量（降低被风控的概率）。
const CACHE_TTL_MS = 300_000;
// 图片 URL 带内容哈希，缓存久一点也没关系。
const IMAGE_CACHE_TTL_MS = 3_600_000;
const CACHE_MAX_ENTRIES = 300;

// ---- 反馈上传 ----
// 日志存这里（相对 server.js 的工作目录）；可用环境变量改到数据盘。
const FEEDBACK_DIR = process.env.FEEDBACK_DIR || join(process.cwd(), "feedback");
// 单个请求体上限：两个日志各封顶 4MB（客户端截断），留出 JSON 转义余量。
const FEEDBACK_MAX_BODY_BYTES = 10 * 1024 * 1024;
const FEEDBACK_MAX_DESCRIPTION = 5000;
const FEEDBACK_MAX_FILES = 4;
const FEEDBACK_RATE_LIMIT = 10; // 每 IP 每小时
const FEEDBACK_RATE_WINDOW_MS = 60 * 60 * 1000;
/** ip -> 提交时间戳数组（只在内存里，重启清零）。 */
const feedbackHits = new Map();

// 与 src/steamRelay.ts 的 STEAM_IMAGE_HOSTS、src-tauri/src/steam_relay.rs 的
// STEAM_IMAGE_HOSTS 保持一致；改这里必须同步另外两处。
// Steam 按出口地区返回不同 CDN 域名（2026-10 实测工坊预览图走
// images.steamusercontent.com / fastly 系，cloudflare / akamai 系是其它地区变体）。
const IMAGE_HOSTS = new Set([
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
]);

// 与 src-tauri/src/workshop.rs 的伪装头一致：Steam 对非浏览器 UA 更容易给风控页。
const UPSTREAM_HEADERS = {
  "user-agent":
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36",
  "accept-language": "zh-CN,zh;q=0.9",
};

/** 简单 TTL + 上限缓存（Map 按插入序淘汰最旧项，命中时刷新顺序近似 LRU）。 */
const cache = new Map();

function cacheGet(key) {
  const entry = cache.get(key);
  if (!entry) {
    return null;
  }
  if (Date.now() > entry.expiresAt) {
    cache.delete(key);
    return null;
  }
  cache.delete(key);
  cache.set(key, entry);
  return entry;
}

function cacheSet(key, entry) {
  cache.set(key, entry);
  while (cache.size > CACHE_MAX_ENTRIES) {
    cache.delete(cache.keys().next().value);
  }
}

function jsonResponse(res, status, body) {
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "access-control-allow-origin": "*",
  });
  res.end(body);
}

function logRequest(req, path, status, startedAt) {
  console.log(
    `${new Date().toISOString()} ${req.method} /${path} -> ${status} (${Date.now() - startedAt}ms)`,
  );
}

function clientIp(req) {
  // Cloudflare 代理开着时源站看到的是 CF 的 IP，真实来源在 CF-Connecting-IP。
  const forwarded = req.headers["cf-connecting-ip"];
  if (typeof forwarded === "string" && forwarded.trim()) {
    return forwarded.trim();
  }
  return req.socket.remoteAddress || "unknown";
}

/** 每 IP 每小时最多 FEEDBACK_RATE_LIMIT 次，超了返回 true。 */
function feedbackRateLimited(ip) {
  const now = Date.now();
  const hits = (feedbackHits.get(ip) || []).filter(
    (time) => now - time < FEEDBACK_RATE_WINDOW_MS,
  );
  const limited = hits.length >= FEEDBACK_RATE_LIMIT;
  if (!limited) {
    hits.push(now);
  }
  feedbackHits.set(ip, hits);
  // 顺手清掉已经过窗口的 IP，避免 map 无限增长。
  if (feedbackHits.size > 5000) {
    for (const [key, value] of feedbackHits) {
      if (value.every((time) => now - time >= FEEDBACK_RATE_WINDOW_MS)) {
        feedbackHits.delete(key);
      }
    }
  }
  return limited;
}

async function readBodyLimited(req, limit) {
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > limit) {
      const error = new Error("body_too_large");
      error.tooLarge = true;
      throw error;
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

/**
 * POST /feedback：启动器「设置 › 关于 › 反馈信息」上传的日志 + 错误描述。
 * 每条请求落一个 JSON 文件到 FEEDBACK_DIR，文件名即反馈编号。
 */
async function handleFeedback(req, res, startedAt) {
  if (req.method !== "POST") {
    logRequest(req, "feedback", 405, startedAt);
    jsonResponse(res, 405, '{"error":"method_not_allowed"}');
    return;
  }
  const ip = clientIp(req);
  if (feedbackRateLimited(ip)) {
    logRequest(req, "feedback", 429, startedAt);
    jsonResponse(res, 429, '{"error":"rate_limited"}');
    return;
  }

  let raw;
  try {
    raw = await readBodyLimited(req, FEEDBACK_MAX_BODY_BYTES);
  } catch (error) {
    const status = error.tooLarge ? 413 : 400;
    logRequest(req, "feedback", status, startedAt);
    jsonResponse(
      res,
      status,
      JSON.stringify({ error: error.tooLarge ? "body_too_large" : "body_unreadable" }),
    );
    return;
  }

  let payload;
  try {
    payload = JSON.parse(raw.toString("utf8"));
  } catch {
    logRequest(req, "feedback", 400, startedAt);
    jsonResponse(res, 400, '{"error":"invalid_json"}');
    return;
  }

  const description = String(payload?.description ?? "")
    .trim()
    .slice(0, FEEDBACK_MAX_DESCRIPTION);
  if (!description) {
    logRequest(req, "feedback", 400, startedAt);
    jsonResponse(res, 400, '{"error":"description_required"}');
    return;
  }

  const rawFiles = Array.isArray(payload?.files) ? payload.files.slice(0, FEEDBACK_MAX_FILES) : [];
  const files = rawFiles.map((file) => ({
    name: String(file?.name ?? "log").slice(0, 64),
    content: String(file?.content ?? ""),
    truncated: Boolean(file?.truncated),
    missing: Boolean(file?.missing),
  }));
  if (files.length === 0 || files.every((file) => file.missing || !file.content)) {
    logRequest(req, "feedback", 400, startedAt);
    jsonResponse(res, 400, '{"error":"no_logs"}');
    return;
  }

  const stamp = new Date().toISOString().replace(/[-:T]/g, "").slice(0, 14);
  const id = `${stamp}-${randomBytes(3).toString("hex")}`;
  const record = {
    id,
    receivedAt: new Date().toISOString(),
    ip,
    description,
    client: payload?.client ?? null,
    files,
  };
  try {
    await mkdir(FEEDBACK_DIR, { recursive: true });
    await writeFile(join(FEEDBACK_DIR, `${id}.json`), JSON.stringify(record, null, 2), "utf8");
  } catch (error) {
    console.error(`${new Date().toISOString()} feedback write failed:`, error);
    logRequest(req, "feedback", 500, startedAt);
    jsonResponse(res, 500, '{"error":"storage_failed"}');
    return;
  }

  console.log(
    `${new Date().toISOString()} feedback ${id} from ${ip}: ${description.slice(0, 80).replace(/\s+/g, " ")}`,
  );
  logRequest(req, "feedback", 200, startedAt);
  jsonResponse(res, 200, JSON.stringify({ ok: true, id }));
}

async function handle(req, res) {
  const startedAt = Date.now();
  if (req.method !== "GET" && req.method !== "HEAD" && req.method !== "POST") {
    logRequest(req, "", 405, startedAt);
    jsonResponse(res, 405, '{"error":"method_not_allowed"}');
    return;
  }

  const url = new URL(req.url, "http://localhost");
  // 剥掉可选的 /steam 前缀；suffix 保留 URL 已编码好的原始形态，不再二次编码。
  const suffix = url.pathname.replace(/^\/steam\/?/, "") || "/";
  const path = suffix.replace(/^\//, "");

  if (path === "healthz") {
    logRequest(req, path, 200, startedAt);
    jsonResponse(res, 200, JSON.stringify({ ok: true, now: Date.now() }));
    return;
  }

  if (path === "feedback") {
    await handleFeedback(req, res, startedAt);
    return;
  }

  let upstream;
  let cacheTtlMs = 0;
  if (path.startsWith("community/")) {
    upstream = "https://steamcommunity.com/" + suffix.slice("/community/".length) + url.search;
    cacheTtlMs = CACHE_TTL_MS;
  } else if (path.startsWith("api/")) {
    upstream = "https://api.steampowered.com/" + suffix.slice("/api/".length) + url.search;
    cacheTtlMs = CACHE_TTL_MS;
  } else if (path.startsWith("images/")) {
    const target = path.slice("images/".length);
    const host = target.split("/")[0];
    if (!host || !IMAGE_HOSTS.has(host)) {
      logRequest(req, path, 404, startedAt);
      jsonResponse(res, 404, '{"error":"image_host_not_allowed"}');
      return;
    }
    upstream = "https://" + target + url.search;
    cacheTtlMs = IMAGE_CACHE_TTL_MS;
  } else {
    logRequest(req, path, 404, startedAt);
    jsonResponse(res, 404, '{"error":"not_found"}');
    return;
  }

  if (req.method === "GET" || req.method === "HEAD") {
    const hit = cacheGet(`GET ${url.pathname}${url.search}`);
    if (hit) {
      const headers = {
        "access-control-allow-origin": "*",
        "x-relay-cache": "hit",
        "cache-control": `public, max-age=${Math.max(1, Math.round((hit.expiresAt - Date.now()) / 1000))}`,
      };
      if (hit.contentType) {
        headers["content-type"] = hit.contentType;
      }
      logRequest(req, path, hit.status, startedAt);
      res.writeHead(hit.status, headers);
      res.end(req.method === "HEAD" ? undefined : hit.body);
      return;
    }
  }

  const bodyChunks = [];
  for await (const chunk of req) {
    bodyChunks.push(chunk);
  }
  const requestBody = Buffer.concat(bodyChunks);

  const headers = { ...UPSTREAM_HEADERS };
  if (requestBody.length > 0 && req.headers["content-type"]) {
    headers["content-type"] = req.headers["content-type"];
  }
  if (path.startsWith("community/")) {
    headers["referer"] = "https://steamcommunity.com/";
  }

  let upstreamResponse;
  try {
    upstreamResponse = await fetch(upstream, {
      method: req.method === "HEAD" ? "GET" : req.method,
      headers,
      body: requestBody.length > 0 ? requestBody : undefined,
      signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
      redirect: "follow",
    });
  } catch (error) {
    logRequest(req, path, 502, startedAt);
    jsonResponse(
      res,
      502,
      JSON.stringify({ error: "upstream_unreachable", detail: String(error) }),
    );
    return;
  }

  const body = Buffer.from(await upstreamResponse.arrayBuffer());
  // 只透传状态码 + 正文 + Content-Type，不转发 Steam 的 Set-Cookie 等，
  // 再补上 CORS（与 update-api/_headers 的策略一致）。
  const headersOut = { "access-control-allow-origin": "*" };
  const contentType = upstreamResponse.headers.get("content-type");
  if (contentType) {
    headersOut["content-type"] = contentType;
  }
  if (cacheTtlMs > 0) {
    headersOut["cache-control"] = `public, max-age=${Math.round(cacheTtlMs / 1000)}`;
  }

  // 工坊 HTML 只有真带 SSR 数据的才缓存:Steam 偶尔回 200 的风控 / 插页,
  // 缓存它们会把坏页面钉住整个 TTL,客户端集体「无法解析」。
  if (
    cacheTtlMs > 0 &&
    upstreamResponse.status === 200 &&
    req.method === "GET" &&
    (path.startsWith("api/") || body.includes("renderContext"))
  ) {
    cacheSet(`GET ${url.pathname}${url.search}`, {
      status: upstreamResponse.status,
      contentType,
      body,
      expiresAt: Date.now() + cacheTtlMs,
    });
  }

  logRequest(req, path, upstreamResponse.status, startedAt);
  res.writeHead(upstreamResponse.status, headersOut);
  res.end(req.method === "HEAD" ? undefined : body);
}

createServer((req, res) => {
  handle(req, res).catch((error) => {
    console.error(`${new Date().toISOString()} handler error:`, error);
    if (!res.headersSent) {
      jsonResponse(res, 500, '{"error":"internal_error"}');
    } else {
      res.destroy();
    }
  });
}).listen(PORT, () => {
  console.log(
    `pzm-steam-relay listening on http://0.0.0.0:${PORT} (paths with or without /steam prefix both accepted)`,
  );
});
