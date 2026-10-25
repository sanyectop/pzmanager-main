# pzm-steam-relay

Steam API 中转站(独立项目,零依赖 Node 服务器),解决大陆用户直连
`steamcommunity.com` 被墙导致创意工坊列表打不开的问题。**部署到自己的海外
服务器**(需能正常访问 Steam),不需要 npm install,上传即可跑。

## 端点

| 路径 | 上游 | 缓存 |
|---|---|---|
| `GET /community/*` | `https://steamcommunity.com/*`(工坊浏览 HTML) | 300s |
| `ANY /api/*` | `https://api.steampowered.com/*`(详情 / 新闻 API,POST 表单透传) | GET 300s,POST 不缓存 |
| `GET /images/<host>/*` | 白名单 Steam 图片 CDN(见 `IMAGE_HOSTS`) | 3600s |
| `GET /healthz` | 无上游调用,返回 `{"ok":true}` | 无 |

路径带不带 `/steam` 前缀等价(`/community/...` 与 `/steam/community/...`
均可),方便挂在独占子域或共享域名子路径下。缓存为进程内存(上限 300 条,
近似 LRU),重启清空。

## 部署

### 直接跑(最简)

```bash
# 服务器需 Node >= 18(node -v 确认)
scp -r steam-relay/ user@server:/opt/pzm-steam-relay
ssh user@server
cd /opt/pzm-steam-relay && node server.js          # 前台试跑,Ctrl+C 停
curl -s http://127.0.0.1:8787/healthz              # {"ok":true,...}
```

### systemd 常驻

```bash
sudo cp pzm-steam-relay.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now pzm-steam-relay
journalctl -u pzm-steam-relay -f                   # 看访问日志
```

### Docker(可选)

```bash
docker build -t pzm-steam-relay .
docker run -d --name pzm-steam-relay -p 127.0.0.1:8787:8787 --restart always pzm-steam-relay
```

### nginx 反代 + TLS(对外必备)

独占子域(推荐,客户端地址就是 `https://relay.example.com`):

```nginx
server {
    listen 443 ssl;
    server_name relay.example.com;         # 换成你的子域
    # ssl_certificate ...; ssl_certificate_key ...;
    location / {
        proxy_pass http://127.0.0.1:8787;
        proxy_set_header Host $host;
    }
}
```

挂在已有域名的 `/steam` 子路径也行(服务器两种前缀都认),此时客户端地址
填 `https://example.com/steam`。

防火墙:8787 只对 nginx(本机)开放,不要直接暴露到公网。

## 冒烟测试

```bash
curl -s http://127.0.0.1:8787/healthz
curl -s -o /dev/null -w "%{http_code}\n" \
  "http://127.0.0.1:8787/community/workshop/browse/?appid=108600&num_per_page=10"
# 期望 200 且正文含 valve-ssr-data(同时验证服务器出口能访问 steamcommunity)
curl -s "http://127.0.0.1:8787/api/ISteamNews/GetNewsForApp/v2/?appid=108600&count=1&format=json" | head -c 200
curl -s -o /dev/null -w "%{http_code}\n" \
  "http://127.0.0.1:8787/images/shared.cloudflare.steamstatic.com/"
# 再请求一次上面任一 GET,响应头出现 x-relay-cache: hit 即缓存生效
```

## 客户端对应关系

- 客户端设置页「通用 › Steam 网络加速」可填任意中转地址,「测试连接」
  按钮就是探 `/healthz`。
- 启动器**内置默认地址**当前是 `https://pzmlan.sanyec.top`(本服务器的
  实际地址),定义在两处常量(换服务器时同步改这两处并重新编译):
  - `src-tauri/src/steam_relay.rs` → `DEFAULT_RELAY_BASE`
  - `src/steamRelay.ts` → `DEFAULT_STEAM_RELAY_BASE`
- 客户端策略:自动(直连失败熔断后走中转,默认)/ 中转优先 / 仅直连。
- 图片主机白名单 `IMAGE_HOSTS` 与 `steam_relay.rs`、`steamRelay.ts`
  三处保持一致,改一处要同步另外三处。Steam 按地区返回不同 CDN 域名
  (steamusercontent / fastly / cloudflare / akamai 系),列表需随实测补充。

## 可选加固

- nginx 限流(防个别用户脚本刷中转):`limit_req_zone $binary_remote_addr
  zone=steam:10m rate=10r/s;` + `location / { limit_req zone=steam burst=20; ... }`
- 若不加 TLS 直接暴露 HTTP,Steam 内容无敏感信息,但地址会明文,不建议。
