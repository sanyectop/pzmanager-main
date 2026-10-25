import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  DEFAULT_STEAM_RELAY_BASE,
  STEAM_RELAY_MODE_OPTIONS,
  previewImageFallbackUrl,
} from "../src/steamRelay.ts";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);

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
