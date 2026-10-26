"""PZ Manager 反馈查看器（本地 Flask 页面）。

用途：管理员在自己电脑上查看中转服务器收集的反馈。
- 输入服务端启动日志里打印的 1024 位 token；
- 用该 token 派生 AES-256-GCM 密钥，加密「当前时间戳 + 随机串」作为 auth 参数；
  服务端解密并校验时间窗（±5 分钟）后才返回数据，token 不会出现在 URL 里；
- 列表按 12 条/页返回最新反馈，可点开单条查看完整日志。

启动：python app.py  →  http://127.0.0.1:10860
只监听 127.0.0.1，不要改成 0.0.0.0（页面本身没有任何登录保护）。
"""

import base64
import hashlib
import json
import os
import secrets
import time
import urllib.error
import urllib.parse
import urllib.request

from flask import Flask, redirect, render_template, request, session, url_for
from werkzeug.exceptions import HTTPException

app = Flask(__name__)
# 每次启动随机：重启后旧 session 失效，需要重新输入 token（符合"token 重启即换"）。
app.secret_key = secrets.token_hex(32)

DEFAULT_SERVER = os.environ.get("FEEDBACK_SERVER", "https://pzmlan.sanyec.top").rstrip("/")
PORT = int(os.environ.get("FEEDBACK_VIEWER_PORT", "10860"))
# 详情页只渲染前 200KB：console.txt 常有 1MB+，整段塞进页面会明显卡顿。
DETAIL_PREVIEW_CHARS = 200_000


def build_auth(token: str) -> str:
    """与 steam-relay/server.js 的 adminAuthValid 对应。

    密钥 = sha256(token)；密文 = iv(12) || AES-256-GCM(明文) 的输出（含 16 字节 tag）；
    整体 base64url（去掉 padding，Node 侧能直接解）。
    """
    try:
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    except ImportError as error:
        raise RuntimeError(
            "缺少 cryptography 依赖：请在 feedback-viewer 目录执行 "
            "`pip install -r requirements.txt`（或 `pip install cryptography`），然后重启。"
        ) from error

    key = hashlib.sha256(token.strip().encode("utf-8")).digest()
    iv = os.urandom(12)
    plaintext = f"{int(time.time())}:{secrets.token_hex(4)}".encode("utf-8")
    sealed = AESGCM(key).encrypt(iv, plaintext, None)  # ciphertext || tag
    return base64.urlsafe_b64encode(iv + sealed).decode("ascii").rstrip("=")


def fetch_json(server: str, path: str, params: dict, token: str):
    """带鉴权请求服务端，返回 (data, error)。"""
    query = dict(params)
    query["auth"] = build_auth(token)
    url = f"{server.rstrip('/')}{path}?{urllib.parse.urlencode(query)}"
    request_obj = urllib.request.Request(
        url, headers={"User-Agent": "pzmanager-feedback-viewer/1.0"}
    )
    try:
        with urllib.request.urlopen(request_obj, timeout=20) as response:
            return json.loads(response.read().decode("utf-8")), None
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8", errors="replace")
        try:
            reason = json.loads(body).get("error", body[:200])
        except ValueError:
            reason = body[:200]
        if error.code == 401:
            return None, "鉴权失败：token 不对，或服务端重启后换了新 token。"
        if error.code == 429:
            return None, "请求太频繁，稍后再试。"
        return None, f"服务端返回 {error.code}：{reason}"
    except Exception as error:  # noqa: BLE001 - 网络层异常统一展示
        return None, f"请求失败：{error}"


def current_connection():
    """取出已保存的连接信息；没连过返回 None。"""
    token = session.get("token")
    if not token:
        return None
    return session.get("server", DEFAULT_SERVER), token


@app.get("/")
def index():
    connection = current_connection()
    if connection:
        return redirect(url_for("browse", page=1))
    return render_template(
        "index.html",
        server=DEFAULT_SERVER,
        connected=False,
        data=None,
        page=1,
        error=None,
    )


@app.post("/connect")
def connect():
    token = (request.form.get("token") or "").strip()
    server = (request.form.get("server") or "").strip() or DEFAULT_SERVER
    if not token:
        return render_template(
            "index.html",
            server=server,
            connected=False,
            data=None,
            page=1,
            error="请粘贴服务端启动日志里的 token。",
        ), 400

    data, error = fetch_json(server, "/feedback", {"page": 1}, token)
    if error:
        return render_template(
            "index.html",
            server=server,
            connected=False,
            data=None,
            page=1,
            error=error,
        ), 401

    session["token"] = token
    session["server"] = server
    return redirect(url_for("browse", page=1))


@app.get("/browse")
def browse():
    connection = current_connection()
    if not connection:
        return redirect(url_for("index"))
    server, token = connection
    page = request.args.get("page", 1, type=int) or 1
    data, error = fetch_json(server, "/feedback", {"page": page}, token)
    if error:
        # token 失效时清掉，回到输入页。
        if "鉴权失败" in error:
            session.clear()
            return render_template(
                "index.html",
                server=server,
                connected=False,
                data=None,
                page=1,
                error=error,
            ), 401
    return render_template(
        "index.html",
        server=server,
        connected=True,
        data=data,
        page=page,
        error=error,
    )


@app.get("/detail/<feedback_id>")
def detail(feedback_id):
    connection = current_connection()
    if not connection:
        return redirect(url_for("index"))
    server, token = connection
    data, error = fetch_json(server, "/feedback/detail", {"id": feedback_id}, token)
    files = []
    if data:
        for file in data.get("files", []):
            content = file.get("content") or ""
            files.append(
                {
                    "name": file.get("name"),
                    "missing": bool(file.get("missing")),
                    "truncated": bool(file.get("truncated")),
                    "bytes": len(content.encode("utf-8")),
                    "content": content[:DETAIL_PREVIEW_CHARS],
                    "clipped": len(content) > DETAIL_PREVIEW_CHARS,
                    "preview_limit": DETAIL_PREVIEW_CHARS,
                }
            )
    status = 200
    if error:
        if "404" in error:
            status = 404
        elif "鉴权失败" in error:
            status = 401
        else:
            status = 502
    return render_template(
        "detail.html",
        item=data,
        files=files,
        error=error,
        page=request.args.get("page", 1, type=int) or 1,
    ), status


@app.get("/logout")
def logout():
    session.clear()
    return redirect(url_for("index"))


@app.errorhandler(Exception)
def surface_error(error):
    """本地工具：把真实异常直接显示在页面上（只监听 127.0.0.1，不对外）。

    Flask 默认的 500 页面不显示原因，排障时只能翻终端；这里把 traceback
    贴出来，并点出最常见的缺依赖问题。
    """
    if isinstance(error, HTTPException):
        return error
    import html
    import traceback

    traceback.print_exc()
    detail = html.escape(
        "".join(traceback.format_exception(type(error), error, error.__traceback__))
    )
    return (
        "<!doctype html><meta charset='utf-8'>"
        "<body style='margin:0;padding:24px;font:14px/1.6 \"Microsoft YaHei UI\",system-ui,sans-serif;"
        "background:#f5f7fa;color:#1f2933'>"
        "<h1 style='font-size:20px;color:#b42318'>查看器出错</h1>"
        "<pre style='padding:12px;background:#0f1720;color:#d7e2ee;border-radius:6px;"
        "overflow:auto;font:12px/1.5 Consolas,monospace;white-space:pre-wrap;word-break:break-all'>"
        f"{detail}</pre>"
        "<p>常见原因：缺少 <code>cryptography</code>。在该目录执行 "
        "<code>pip install -r requirements.txt</code> 后重启本程序。</p>"
        "</body>"
    ), 500


if __name__ == "__main__":
    missing = []
    for module_name, package_name in (("flask", "Flask"), ("cryptography", "cryptography")):
        try:
            __import__(module_name)
        except ImportError:
            missing.append(package_name)
    if missing:
        print("缺少依赖：" + "、".join(missing))
        print("请先执行：pip install -r requirements.txt")
        raise SystemExit(1)

    print(f"反馈查看器: http://127.0.0.1:{PORT}")
    print(f"服务端: {DEFAULT_SERVER}")
    app.run(host="127.0.0.1", port=PORT, debug=False)
