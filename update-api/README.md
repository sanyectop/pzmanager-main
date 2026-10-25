# PZManager 更新服务(Cloudflare Pages,静态 JSON + Steam 中转 Function)

启动器「检查更新」功能的更新源,外加「Steam 网络加速」的中转站。静态部分无构建、
无页面:根目录只有一行状态文本,真正的接口是 `api/latest.json`;动态部分是一个
零依赖 Pages Function(`functions/steam/[[path]].js`),解决大陆用户直连
steamcommunity.com 被墙、创意工坊列表打不开的问题。

```
update-api/
├── index.html      访问根路径时显示的一行状态文本「The PZManager API Is Running.」
├── api/
│   └── latest.json   机器读的更新源:启动器启动时拉取的就是它(唯一需要发版时修改的文件)
├── _headers        给 /api/latest.json 加 CORS 允许跨域 + 60 秒缓存
└── functions/
    └── steam/
        └── [[path]].js   Steam API 中转(Pages Function,路径白名单反代)
```

- 更新接口:`GET https://pzmapi.sanyec.top/api/latest.json`(纯静态 JSON,无服务端逻辑)
- 启动器侧实现在 `src-tauri/src/update.rs`(`UPDATE_FEED_URL` 常量指向本接口),
  前端入口在 `src/App.tsx`(启动静默检查 + 设置 › 关于 手动检查)
- `_headers` 必须放在**发布根目录**——Cloudflare Pages 不处理子目录里的 `_headers`,
  挪进 `api/` 会静默失效

## Steam 中转(/steam/*)

`functions/steam/[[path]].js` 是路径白名单反向代理,**不是任意代理**:

| 路径 | 上游 | 缓存 |
| --- | --- | --- |
| `GET /steam/community/*` | `https://steamcommunity.com/*`(工坊浏览 HTML) | 300s |
| `ANY /steam/api/*` | `https://api.steampowered.com/*`(工坊详情 / 新闻,POST 表单透传) | GET 300s,POST 不缓存 |
| `GET /steam/images/<host>/*` | 白名单 Steam 图片 CDN | 3600s |
| `GET /steam/healthz` | 无上游调用,返回 `{"ok":true}` | 无 |

- 客户端实现在 `src-tauri/src/steam_relay.rs` 与 `src/steamRelay.ts`(设置页
  「通用 › Steam 网络加速」,默认自动:直连失败切中转并熔断 10 分钟)。
- 图片主机白名单(`IMAGE_HOSTS`)在 Function、`steam_relay.rs`、`steamRelay.ts`
  三处必须保持一致,改一处要同步另外两处。
- 上游 20s 超时;响应剥掉 Steam 的 Set-Cookie、补 CORS,与 `_headers` 策略一致。

## 接口字段

| 字段 | 类型 | 说明 |
| --- | --- | --- |
| `version` | string | 最新版本号,格式 `{主版本}.{次版本}.{YYMMDD}`。启动器据此判断是否有更新。 |
| `pubDate` | string | 发布日期(YYYY-MM-DD),仅展示用。 |
| `notes` | string | 更新说明,展示在启动器「设置 › 关于」;换行用 `\n`。 |
| `downloads.cloudDrive` | string | 123 云盘分享链接(国内直连)。启动器「下载更新」按钮的默认跳转目标。 |
| `downloads.windowsX64Installer` | string | Windows x64 NSIS 安装包直链(GitHub)。`cloudDrive` 缺省时「下载更新」回落到这里。 |
| `downloads.windowsX64Portable` | string | Windows x64 便携版 zip 直链(GitHub)。安装包也缺省时的最后回落。 |
| `releasePage` | string | 发布页地址(GitHub Releases,「打开发布页」按钮)。 |
| `name` | string | 产品名。启动器不读取,保留作文档说明。 |

版本判定规则:按 `.` 分段数字逐位比较,远端**严格大于**本地才提示(同版本重发不提示);
容忍 `v` 前缀、段内后缀(如 `-beta`)、段数不齐(缺段按 0)。除 `version` 外字段均可缺省,
未知字段忽略——给 JSON 加新字段安全,改老字段含义才危险。

## 首次部署(Cloudflare Pages)

1. 登录 Cloudflare Dashboard → Workers & Pages → Create → Pages。
2. 两种方式任选:
   - **Direct Upload(推荐,最快)**:见下面「部署命令」,用 wrangler 上传整个 `update-api/`;
   - **连接 Git 仓库**:Framework preset 选 `None`,Build command 留空,Build output directory
     填 `update-api`(如果仓库根就是本目录,则填 `/`)。
3. 部署完成后记下分配的域名(形如 `https://<项目名>.pages.dev`),可选再绑自定义域名。
4. **域名要与启动器保持一致**:启动器 `src-tauri/src/update.rs` 的 `UPDATE_FEED_URL`
   当前指向 `https://pzmapi.sanyec.top/api/latest.json`。以后换域名/换项目时,改完部署
   必须同步改这个常量并重新打包发版——旧版启动器内嵌的还是旧地址,只对「下一次发版」
   之后的版本生效。

### 部署命令(带 Functions 时必须用 wrangler)

> **重要**:加入 `functions/` 之后,Dashboard 拖拽上传**不再适用**——控制台的
> Direct Upload 不支持 Functions,而且这样传的部署会**静默丢掉**之前部署的
> Steam 中转(每次部署都是完整独立的一份,旧 Functions 不会自动保留)。
> 发版改 `latest.json` 也要用同一条 wrangler 命令重新部署。

```bash
npx wrangler login                        # 首次:浏览器授权,之后免登录
cd update-api
npx wrangler pages project list           # 确认项目名(绑定 pzmapi.sanyec.top 的那个)
npx wrangler pages deploy . --project-name=<项目名>
```

验证:

```bash
curl -sS https://pzmapi.sanyec.top/api/latest.json | head -n 5   # 应返回完整 JSON
curl -sSI https://pzmapi.sanyec.top/api/latest.json | grep -i "access-control\|cache-control"
# 期望:access-control-allow-origin: * 与 cache-control: public, max-age=60
```

Steam 中转冒烟(重点验证 Cloudflare 出口抓 steamcommunity HTML 不会被 Steam
风控,这是中转方案最大的风险点;客户端有直连兜底,中转失败不会更糟):

```bash
curl -sS https://pzmapi.sanyec.top/steam/healthz            # {"ok":true,...}
curl -sS -o /dev/null -w "%{http_code}\n" \
  "https://pzmapi.sanyec.top/steam/community/workshop/browse/?appid=108600&num_per_page=10"
# 期望 200,正文含 valve-ssr-data
curl -sS "https://pzmapi.sanyec.top/steam/api/ISteamNews/GetNewsForApp/v2/?appid=108600&count=1&format=json" | head -c 200
# 期望返回 JSON(appnews...)
```

## 发版流程(每次新版本)

1. 按主仓库 `docs/version-bump.md` 走完发版步骤、GitHub Release 附件上传完毕后,
   修改 `api/latest.json`:
   - `version` / `pubDate` / `notes`;
   - `downloads` 里 GitHub 两个直链的文件名(指向新版本 tag 的 GitHub Release 附件);
     123 云盘的 `cloudDrive` 分享链接若也换了新版本,一并更新。
2. 重新部署(上面「部署命令」里的 `wrangler pages deploy`,或 git push 触发)。
3. `curl` 验证返回新版本号即完成。latest.json 缓存 60 秒,用户端最多延迟 1 分钟看到。

## 注意事项

- **不要给 api/latest.json 配长缓存**(CDN 层也不建议加页面规则覆盖),否则发版后旧内容
  会一直命中缓存;`_headers` 里 60 秒是有意为之。
- GitHub Release 附件直链形如
  `https://github.com/sanyectop/pzmanager-main/releases/download/v{版本}/PZManager_{版本}_x64-setup.exe`,
  上传附件后可到 Release 页面右键复制确认。
- 若换域名/换项目,记得同步 `UPDATE_FEED_URL`(见上)。
