import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  DEFAULT_STEAM_RELAY_BASE,
  STEAM_RELAY_MODE_OPTIONS,
  previewImageFallbackUrl,
} from "../src/steamRelay.ts";

import { appSource } from "./sourceText.ts";

test("rewrites whitelisted steam image hosts through the relay", () => {
  assert.equal(
    previewImageFallbackUrl(
      "https://shared.cloudflare.steamstatic.com/ugc/123/preview.jpg",
      "https://pzmapi.sanyec.top/steam",
      "auto",
    ),
    "https://pzmapi.sanyec.top/steam/images/shared.cloudflare.steamstatic.com/ugc/123/preview.jpg",
  );
  // 中转地址带尾斜杠也要拼对；URL 自带的查询串原样保留。
  assert.equal(
    previewImageFallbackUrl(
      "https://steamuserimages-a.akamaihd.net/ugc/a.jpg?imw=320",
      "https://mirror.example/steam/",
      "relay",
    ),
    "https://mirror.example/steam/images/steamuserimages-a.akamaihd.net/ugc/a.jpg?imw=320",
  );
});

test("keeps query strings and handles avatars host", () => {
  assert.equal(
    previewImageFallbackUrl(
      "https://avatars.steamstatic.com/x.jpg",
      DEFAULT_STEAM_RELAY_BASE,
      "relay",
    ),
    `${DEFAULT_STEAM_RELAY_BASE}/images/avatars.steamstatic.com/x.jpg`,
  );
});

test("covers the cdn hosts steam actually returns today", () => {
  // 2026-10 实测:工坊预览图走 images.steamusercontent.com,
  // 页面资源还有 fastly / store 系 CDN,都必须能兜底。
  for (const host of [
    "images.steamusercontent.com",
    "cdn.fastly.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "store.steamstatic.com",
  ]) {
    assert.equal(
      previewImageFallbackUrl(`https://${host}/ugc/1.jpg`, DEFAULT_STEAM_RELAY_BASE, "auto"),
      `${DEFAULT_STEAM_RELAY_BASE}/images/${host}/ugc/1.jpg`,
    );
  }
});

test("returns null for unknown hosts, bad urls, direct mode, empty base", () => {
  assert.equal(
    previewImageFallbackUrl("https://example.com/a.jpg", DEFAULT_STEAM_RELAY_BASE, "auto"),
    null,
  );
  assert.equal(previewImageFallbackUrl("not-a-url", DEFAULT_STEAM_RELAY_BASE, "auto"), null);
  // 仅直连：不提供中转兜底。
  assert.equal(
    previewImageFallbackUrl(
      "https://shared.cloudflare.steamstatic.com/ugc/1.jpg",
      DEFAULT_STEAM_RELAY_BASE,
      "direct",
    ),
    null,
  );
  assert.equal(
    previewImageFallbackUrl(
      "https://shared.cloudflare.steamstatic.com/ugc/1.jpg",
      "  ",
      "auto",
    ),
    null,
  );
});

test("exposes exactly the three backend relay modes", () => {
  // 与后端 steam_relay.rs 的 RELAY_MODES 一一对应。
  assert.deepEqual(
    STEAM_RELAY_MODE_OPTIONS.map((option) => option.value),
    ["auto", "relay", "direct"],
  );
});

test("applies the relay fallback to workshop preview images in the app", () => {
  assert.match(appSource, /previewImageFallbackUrl\(\s*item\.previewUrl/);
  assert.match(appSource, /fallbackSrc=\{previewImageFallbackUrl/);
});

test("general settings › network section collapses its explanation into a hint", () => {
  assert.match(appSource, /<Text color="gray\.800" fontSize="sm" fontWeight="700">网络<\/Text>/);
  assert.match(appSource, /label="网络中转说明"/);
  assert.match(appSource, /大陆网络直连 Steam 创意工坊会失败/);
  // 说明只留在问号提示里，不再作为可见段落占位。
  assert.doesNotMatch(
    appSource,
    /<Text color="gray\.500" fontSize="xs" mt="1">\s*大陆网络直连 Steam 创意工坊会失败/,
  );
  // 中转模式和地址控件保持可见。
  assert.match(appSource, /ariaLabel="Steam 网络加速模式"/);
  assert.match(appSource, /aria-label="Steam 中转服务器地址"/);
});
