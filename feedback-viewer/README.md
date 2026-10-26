# 反馈查看器

本地 Flask 小页面，用来查看中转服务器收集到的反馈（`steam-relay` 的
`POST /feedback` 上传的日志 + 描述）。只跑在自己电脑上，不对外提供服务。

## 工作原理

1. `steam-relay` **每次启动**随机生成一个 1024 位 token，打印在启动日志里：

   ```
   ========================================================================
   反馈查看 token（1024 位随机，仅本次启动有效，重启后更换）：
   <一长串 base64>
   把它输入 feedback-viewer 本地页面即可查看反馈；不要外泄。
   ========================================================================
   ```

   服务器上查看：`journalctl -u pzm-steam-relay -n 30`（或你重定向的日志文件）。

2. 在本页面粘贴这个 token。页面会用 `sha256(token)` 派生 AES-256-GCM 密钥，
   加密「当前 Unix 时间戳 + 随机串」得到 `auth` 参数，发给服务端
   `GET /feedback?auth=…&page=N`。

3. 服务端用同一 token 解密并校验时间戳在 ±5 分钟内，成功才返回数据
   （每页 12 条，最新的在前）。**token 明文不会出现在 URL 或服务器访问日志里。**

## 安装与运行

```bash
cd feedback-viewer
python -m venv .venv
# Windows: .venv\Scripts\activate    Linux/macOS: source .venv/bin/activate
pip install -r requirements.txt
python app.py
```

打开 <http://127.0.0.1:10860>。

- 只监听 `127.0.0.1`（不要改成 `0.0.0.0`，页面本身没有登录保护）。
- 服务端地址默认 `https://pzmlan.sanyec.top`，页面里可改；也可用环境变量
  `FEEDBACK_SERVER` 指定。
- token 存在 Flask session 里，Flask 每次重启会换 `secret_key`，需要重新输入。

## 页面功能

- 列出最新反馈：描述、编号、接收时间、来源 IP、客户端版本、各日志文件大小；
- 分页：12 条/页；
- 「查看日志」：看完整描述与两个日志文件内容（`console.txt` 页面只渲染前
  200KB，避免大文件卡顿；完整内容在服务器的 feedback JSON 里）。

## 服务端对应接口

| 请求 | 说明 |
| --- | --- |
| `GET /feedback?auth=…&page=N` | 分页列表（12 条/页，新的在前），只含摘要 |
| `GET /feedback/detail?auth=…&id=…` | 单条完整记录（含日志正文） |

未带有效 `auth` 一律 401；每 IP 每小时最多 60 次查看请求（超过 429）。
