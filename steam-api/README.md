# pzm-steam-relay

Steam API 中转站（Cloudflare Worker），解决大陆用户直连 `steamcommunity.com`
被墙导致创意工坊列表打不开的问题。零依赖单文件（`worker.js`），不需要
npm install / node_modules。

## 端点

| 路径 | 上游 | 缓存 |
|---|---|---|
| `GET /steam/community/*` | `https://steamcommunity.com/*`（工坊浏览 HTML） | 300s |
| `ANY /steam/api/*` | `https://api.steampowered.com/*`（详情 / 新闻 API，POST 表单透传） | GET 300s，POST 不缓存 |
| `GET /steam/images/<host>/*` | 白名单 Steam 图片 CDN（见 `IMAGE_HOSTS`） | 3600s |
| `GET /steam/healthz` | 无上游调用，返回 `{"ok":true}` | 无 |

只有上述白名单路径会被代理，其余一律 404；图片主机白名单与客户端
（`src-tauri/src/steam_relay.rs`、`src/steamRelay.ts`）三处保持一致。

## 首次部署

1. **登录**（在仓库根目录以外的任意目录执行均可）：

   ```bash
   npx wrangler login
   ```

2. **部署**（在本目录执行；`wrangler deploy` 只读 `worker.js`，不在本地落盘）：

   ```bash
   cd steam-api
   npx wrangler deploy
   ```

3. **绑定路由**：Cloudflare 控制台 → Workers & Pages → `pzm-steam-relay`
   → Settings → Domains & Routes → Add → **Route**：
   - Pattern：`pzmapi.sanyec.top/steam/*`
   - Zone：`sanyec.top`

   Workers 路由优先于同域名的 Pages 自定义域，`update-api` 的
   `/api/latest.json` 不受影响。**不要**用 `*.workers.dev` 域名当正式入口
   （大陆被墙）。

4. **冒烟测试**（重点验证 Cloudflare 出口抓 steamcommunity HTML 不会被
   Steam 风控，这是本方案最大的风险点）：

   ```bash
   curl -sS "https://pzmapi.sanyec.top/steam/healthz"
   curl -sS -o /dev/null -w "%{http_code}\n" \
     "https://pzmapi.sanyec.top/steam/community/workshop/browse/?appid=108600&num_per_page=10"
   curl -sS "https://pzmapi.sanyec.top/steam/api/ISteamNews/GetNewsForApp/v2/?appid=108600&count=1&format=json"
   curl -sS -o /dev/null -w "%{http_code}\n" \
     "https://pzmapi.sanyec.top/steam/images/shared.cloudflare.steamstatic.com/"
   ```

   第 2 条期望 200 且正文包含 `valve-ssr-data`；第 3 条期望返回 JSON。

5. **（可选）WAF 限流**：控制台 → sanyec.top zone → Security → WAF
   → Rate limiting rules，免费版可建一条：匹配
   `starts_with(http.request.uri.path, "/steam/")`，同 IP 超过
   10 请求/秒 时 Block 1 分钟。防个别用户脚本刷中转拖垮免费额度。

## 客户端对应关系

- 默认中转地址：`https://pzmapi.sanyec.top/steam`（`steam_relay.rs` 的
  `DEFAULT_RELAY_BASE`，设置页「通用 › Steam 网络加速」可改）。
- 客户端策略：自动（直连失败熔断后走中转）/ 中转优先 / 仅直连。
