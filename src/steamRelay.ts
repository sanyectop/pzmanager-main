/**
 * Steam 网络加速（中转）的前端纯逻辑。
 *
 * 白名单与默认地址必须和三处保持一致：
 * - 中转服务器：steam-relay/server.js（IMAGE_HOSTS）
 * - 后端：src-tauri/src/steam_relay.rs（STEAM_IMAGE_HOSTS、DEFAULT_RELAY_BASE）
 */

/** 与 steam_relay.rs 的 DEFAULT_RELAY_BASE 一致（自建中转，steam-relay/README.md）；
 *  后端 get_app_settings 会把空设置归一化成这个值，前端也用它区分「默认」和「用户自定义」。 */
export const DEFAULT_STEAM_RELAY_BASE = "https://pzmlan.sanyec.top";

/** Steam 图片 CDN 主机白名单（steam-relay/server.js 同名单会代理这些主机）。
 *  Steam 按地区返回不同 CDN 域名（steamusercontent / fastly / cloudflare / akamai 系）。 */
export const STEAM_IMAGE_HOSTS = [
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
] as const;

export type SteamRelayMode = "auto" | "relay" | "direct";

/** 设置页「通用 › Steam 网络加速」的模式下拉选项。
 *  标签保持短句（下拉宽度有限，长文案会被省略号截断），详细含义看卡片描述。 */
export const STEAM_RELAY_MODE_OPTIONS: { value: SteamRelayMode; label: string }[] = [
  { value: "auto", label: "自动（推荐）" },
  { value: "relay", label: "中转优先" },
  { value: "direct", label: "仅直连" },
];

/**
 * 工坊列表预览图的中转兜底地址：主机在 Steam CDN 白名单里才改写，
 * 否则返回 null（FallbackImage 收到 null 就直接显示占位图标）。
 * 仅直连模式下不提供兜底，尊重用户的选择。
 */
export function previewImageFallbackUrl(
  previewUrl: string,
  relayBase: string,
  mode: SteamRelayMode | string,
): string | null {
  if (mode === "direct" || !relayBase.trim()) {
    return null;
  }
  let parsed: URL;
  try {
    parsed = new URL(previewUrl);
  } catch {
    return null;
  }
  if (!STEAM_IMAGE_HOSTS.includes(parsed.host as (typeof STEAM_IMAGE_HOSTS)[number])) {
    return null;
  }
  const base = relayBase.trim().replace(/\/+$/, "");
  return `${base}/images/${parsed.host}${parsed.pathname}${parsed.search}`;
}
