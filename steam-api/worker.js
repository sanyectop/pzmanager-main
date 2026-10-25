/**
 * PZ Manager 的 Steam API 中转（Cloudflare Worker，零依赖单文件）。
 *
 * 只代理下面的固定白名单路径，不是任意代理：
 *   GET  /steam/community/*      → https://steamcommunity.com/*        工坊浏览 HTML
 *   ANY  /steam/api/*            → https://api.steampowered.com/*      工坊详情 + 新闻 API
 *   GET  /steam/images/<host>/*  → 白名单 Steam 图片 CDN（预览图兜底）
 *   GET  /steam/healthz          → {"ok":true}，客户端「测试连接」用
 *
 * 部署与冒烟步骤见同目录 README.md。客户端对应实现在
 * src-tauri/src/steam_relay.rs 与 src/steamRelay.ts；
 * IMAGE_HOSTS 三处必须保持一致。
 */

// 与 src/steamRelay.ts 的 STEAM_IMAGE_HOSTS、src-tauri/src/steam_relay.rs 的
// STEAM_IMAGE_HOSTS 保持一致；改这里必须同步另外两处。
const IMAGE_HOSTS = new Set([
  "shared.cloudflare.steamstatic.com",
  "clan.cloudflare.steamstatic.com",
  "steamuserimages-a.akamaihd.net",
  "cdn.akamai.steamstatic.com",
  "avatars.steamstatic.com",
  "avatars.akamai.steamstatic.com",
]);

// 与 src-tauri/src/workshop.rs 的伪装头一致：Steam 对非浏览器 UA 更容易给风控页。
const UPSTREAM_HEADERS = {
  "User-Agent":
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36",
  "Accept-Language": "zh-CN,zh;q=0.9",
};

const UPSTREAM_TIMEOUT_MS = 20_000;
// 工坊 HTML / Web API 的边缘缓存时长，与客户端 300s HTML 缓存对齐；
// 既提速，也减少对 Steam 的出口请求量（降低被风控的概率）。
const CACHE_TTL_SECONDS = 300;
// 图片 URL 带内容哈希，缓存久一点也没关系。
const IMAGE_CACHE_TTL_SECONDS = 3600;

function jsonResponse(body, status = 200) {
  return new Response(body, {
    status,
    headers: {
      "Content-Type": "application/json; charset=utf-8",
      "Access-Control-Allow-Origin": "*",
    },
  });
}

export default {
  async fetch(request, _env, ctx) {
    if (request.method !== "GET" && request.method !== "HEAD" && request.method !== "POST") {
      return jsonResponse('{"error":"method_not_allowed"}', 405);
    }

    const url = new URL(request.url);
    // 路由挂在 pzmapi.sanyec.top/steam/* 上；本地 wrangler dev / workers.dev
    // 调试时没有 /steam 前缀，两种都支持。
    const path = url.pathname.replace(/^\/steam\/?/, "").replace(/^\//, "");

    if (path === "healthz") {
      return jsonResponse(JSON.stringify({ ok: true, now: Date.now() }));
    }

    let upstream;
    let cacheTtl = 0;
    if (path.startsWith("community/")) {
      upstream = new URL(
        "https://steamcommunity.com/" + path.slice("community/".length) + url.search,
      );
      cacheTtl = CACHE_TTL_SECONDS;
    } else if (path.startsWith("api/")) {
      upstream = new URL(
        "https://api.steampowered.com/" + path.slice("api/".length) + url.search,
      );
      cacheTtl = CACHE_TTL_SECONDS;
    } else if (path.startsWith("images/")) {
      const target = path.slice("images/".length);
      const host = target.split("/")[0];
      if (!host || !IMAGE_HOSTS.has(host)) {
        return jsonResponse('{"error":"image_host_not_allowed"}', 404);
      }
      upstream = new URL("https://" + target + url.search);
      cacheTtl = IMAGE_CACHE_TTL_SECONDS;
    } else {
      return jsonResponse('{"error":"not_found"}', 404);
    }

    const cache = caches.default;
    if (request.method === "GET") {
      const cached = await cache.match(request.url);
      if (cached) {
        return cached;
      }
    }

    const headers = new Headers(UPSTREAM_HEADERS);
    const contentType = request.headers.get("content-type");
    if (contentType) {
      headers.set("Content-Type", contentType);
    }
    if (path.startsWith("community/")) {
      headers.set("Referer", "https://steamcommunity.com/");
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), UPSTREAM_TIMEOUT_MS);
    let upstreamResponse;
    try {
      upstreamResponse = await fetch(upstream, {
        method: request.method === "HEAD" ? "GET" : request.method,
        headers,
        body:
          request.method === "GET" || request.method === "HEAD"
            ? undefined
            : request.body,
        signal: controller.signal,
      });
    } catch (error) {
      return jsonResponse(
        JSON.stringify({ error: "upstream_unreachable", detail: String(error) }),
        502,
      );
    } finally {
      clearTimeout(timer);
    }

    // 只透传状态码 + 正文 + Content-Type，不转发 Steam 的 Set-Cookie 等，
    // 再补上 CORS（与 update-api/_headers 的策略一致）。
    const headersOut = new Headers();
    headersOut.set("Access-Control-Allow-Origin", "*");
    const upstreamType = upstreamResponse.headers.get("content-type");
    if (upstreamType) {
      headersOut.set("Content-Type", upstreamType);
    }
    if (cacheTtl > 0) {
      headersOut.set("Cache-Control", `public, max-age=${cacheTtl}`);
    }

    const response = new Response(upstreamResponse.body, {
      status: upstreamResponse.status,
      headers: headersOut,
    });
    if (request.method === "GET" && upstreamResponse.status === 200 && cacheTtl > 0) {
      ctx.waitUntil(cache.put(request.url, response.clone()));
    }
    return response;
  },
};
