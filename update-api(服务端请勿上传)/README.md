# PZManager 更新服务(Cloudflare Pages,纯 API 无页面版)

启动器「检查更新」功能的更新源。纯静态、无构建、无页面:根目录只有一行状态文本,
真正的接口是 `api/latest.json`。

```
update-api/
├── index.html      访问根路径时显示的一行状态文本「The PZManager API Is Running.」
├── api/
│   └── latest.json   机器读的更新源:启动器启动时拉取的就是它(唯一需要发版时修改的文件)
└── _headers        给 /api/latest.json 加 CORS 允许跨域 + 60 秒缓存
```

- 接口:`GET https://pzmapi.sanyec.top/api/latest.json`(纯静态 JSON,无服务端逻辑)
- 启动器侧实现在 `src-tauri/src/update.rs`(`UPDATE_FEED_URL` 常量指向本接口),
  前端入口在 `src/App.tsx`(启动静默检查 + 设置 › 关于 手动检查)
- `_headers` 必须放在**发布根目录**——Cloudflare Pages 不处理子目录里的 `_headers`,
  挪进 `api/` 会静默失效

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
   - **Direct Upload(推荐,最快)**:选 "Upload assets",把整个 `update-api/` 文件夹拖进去即可;
   - **连接 Git 仓库**:Framework preset 选 `None`,Build command 留空,Build output directory
     填 `update-api`(如果仓库根就是本目录,则填 `/`)。
3. 部署完成后记下分配的域名(形如 `https://<项目名>.pages.dev`),可选再绑自定义域名。
4. **域名要与启动器保持一致**:启动器 `src-tauri/src/update.rs` 的 `UPDATE_FEED_URL`
   当前指向 `https://pzmapi.sanyec.top/api/latest.json`。以后换域名/换项目时,改完部署
   必须同步改这个常量并重新打包发版——旧版启动器内嵌的还是旧地址,只对「下一次发版」
   之后的版本生效。

验证:

```bash
curl -sS https://pzmapi.sanyec.top/api/latest.json | head -n 5   # 应返回完整 JSON
curl -sSI https://pzmapi.sanyec.top/api/latest.json | grep -i "access-control\|cache-control"
# 期望:access-control-allow-origin: * 与 cache-control: public, max-age=60
```

## 发版流程(每次新版本)

1. 按主仓库 `docs/version-bump.md` 走完发版步骤、GitHub Release 附件上传完毕后,
   修改 `api/latest.json`:
   - `version` / `pubDate` / `notes`;
   - `downloads` 里 GitHub 两个直链的文件名(指向新版本 tag 的 GitHub Release 附件);
     123 云盘的 `cloudDrive` 分享链接若也换了新版本,一并更新。
2. 重新部署(Direct Upload 重传文件夹,或 git push 触发)。
3. `curl` 验证返回新版本号即完成。latest.json 缓存 60 秒,用户端最多延迟 1 分钟看到。

## 注意事项

- **不要给 api/latest.json 配长缓存**(CDN 层也不建议加页面规则覆盖),否则发版后旧内容
  会一直命中缓存;`_headers` 里 60 秒是有意为之。
- GitHub Release 附件直链形如
  `https://github.com/sanyectop/pzmanager-main/releases/download/v{版本}/PZManager_{版本}_x64-setup.exe`,
  上传附件后可到 Release 页面右键复制确认。
- 若换域名/换项目,记得同步 `UPDATE_FEED_URL`(见上)。
