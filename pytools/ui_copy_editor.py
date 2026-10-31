"""UI 文案查看 / 修改工具。

扫描前端（src/**/*.{ts,tsx}）和后端（src-tauri/src/**/*.rs）里所有含中文的
文字描述：字符串字面量、JSX 文本节点，在浏览器里按文件分组展示，支持搜索和
手动修改；保存时写回源码原位置，并在 pytools/backups/<本次运行>/ 留一份备份。

用法：
    python pytools/ui_copy_editor.py
    然后打开 http://127.0.0.1:10199

只做文本替换，不做代码格式化；每处修改都先校验原文字仍在原位置，源码已变则拒绝。
"""

import html
import re
import shutil
import time
from pathlib import Path

from flask import Flask, jsonify, request

ROOT = Path(__file__).resolve().parent.parent
SCAN_DIRS = [
    ("src", ("*.ts", "*.tsx")),
    ("src-tauri/src", ("*.rs",)),
]
BACKUP_DIR = Path(__file__).resolve().parent / "backups"
HOST, PORT = "127.0.0.1", 10199

CJK = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff]")
# 行注释与块注释里的中文不算界面文案；抹掉注释时用空格占位，保持列号不变。
BLOCK_COMMENT = re.compile(r"/\*.*?\*/", re.S)
LINE_COMMENT = re.compile(r"(?<!:)//[^\n]*")
# 三种引号的字符串字面量（单行）。
STRING_PATTERNS = [
    re.compile(r'"((?:[^"\\\n]|\\.)*)"'),
    re.compile(r"'((?:[^'\\\n]|\\.)*)'"),
    re.compile(r"`([^`\\\n]*)`"),
]
# 单行 JSX 文本节点：<Text>刷新</Text>
JSX_INLINE = re.compile(r">\s*([^<>\n{}]*[\u3400-\u4dbf\u4e00-\u9fff][^<>\n{}]*)\s*<")
QUOTES = {'"': '"', "'": "'", "`": "`"}


def strip_comments(text: str) -> str:
    """把注释字符替换成空格（保留换行），扫描时自然跳过、列号不偏移。"""
    text = BLOCK_COMMENT.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)
    return LINE_COMMENT.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)


def unescape(inner: str, quote: str) -> str:
    if quote == "`":
        return inner
    return inner.replace(rf"\{quote}", quote).replace(r"\\", "\\")


def escape(inner: str, quote: str) -> str:
    if quote == "`":
        return inner
    return inner.replace("\\", r"\\").replace(quote, rf"\{quote}")


def source_files() -> list[Path]:
    files: list[Path] = []
    for folder, patterns in SCAN_DIRS:
        base = ROOT / folder
        if not base.exists():
            continue
        for pattern in patterns:
            files.extend(base.rglob(pattern))
    return sorted(set(files))


def scan_file(path: Path) -> list[dict]:
    """返回该文件里所有含中文的文案片段（含注释过滤后的位置信息）。"""
    try:
        content = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return []
    stripped_lines = strip_comments(content).split("\n")
    items: list[dict] = []

    for idx, bare in enumerate(stripped_lines):
        line_no = idx + 1
        # 字符串字面量（含模板字符串单行）。
        for pattern in STRING_PATTERNS:
            for match in pattern.finditer(bare):
                inner = match.group(1)
                if not CJK.search(inner):
                    continue
                quote = match.group(0)[0]
                items.append({
                    "kind": "string",
                    "line": line_no,
                    "col": match.start(),
                    "raw": match.group(0),
                    "text": unescape(inner, quote),
                    "quote": quote,
                })
        # 单行 JSX 文本：>中文<
        for match in JSX_INLINE.finditer(bare):
            text = match.group(1).strip()
            if not text:
                continue
            items.append({
                "kind": "jsx",
                "line": line_no,
                "col": bare.find(match.group(1)),
                "raw": match.group(1),
                "text": text,
            })

    # 独占一行的 JSX 文本（长说明常见写法：上一行以 > 结尾、内容自成一行）。
    for idx, bare in enumerate(stripped_lines):
        stripped = bare.strip()
        if not CJK.search(stripped) or any(ch in stripped for ch in '<>{}="`'):
            continue
        prev = stripped_lines[idx - 1].rstrip() if idx > 0 else ""
        nxt = stripped_lines[idx + 1].lstrip() if idx + 1 < len(stripped_lines) else ""
        if not (prev.endswith(">") or nxt.startswith("<")):
            continue
        items.append({
            "kind": "jsx-block",
            "line": idx + 1,
            "col": bare.find(stripped),
            "raw": stripped,
            "text": stripped,
        })

    for item in items:
        item["file"] = path.relative_to(ROOT).as_posix()

    # VSCode 式大纲：解析函数 / 组件 / impl·mod 结构，把每条文案归到所属节点。
    outline = compute_outline(stripped_lines, path.suffix)
    for item in items:
        item["outline"] = outline_path_at(outline, item["line"])
    return items


# ---- 函数大纲（VSCode Outline 风格）-----------------------------------------

# TS/TSX：函数声明、const 箭头函数 / function 表达式、纯 JSX 段落赋值（const x = (）。
TS_DECL_PATTERNS = [
    re.compile(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)"),
    re.compile(r"^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*(?::[^=]*)?=\s*(?:async\s+)?(?:function\b|\([^()]*\)\s*=>|[A-Za-z_$][\w$]*\s*=>)"),
    re.compile(r"^\s*(?:export\s+)?const\s+([A-Za-z_$][\w$]*)\s*=\s*\(\s*$"),
    # 类方法 / 对象方法：method() { 或 prop: (v) => {
    re.compile(r"^\s+(?:(?:public|private|protected|static|async)\s+)*([A-Za-z_$][\w$]*)\s*\([^()]*\)\s*\{\s*$"),
    re.compile(r"^\s+([A-Za-z_$][\w$]*)\s*:\s*(?:async\s+)?\([^()]*\)\s*=>\s*\{"),
    # 调用携带箭头函数体：useEffect(() => { / addEventListener("x", (e) => {
    re.compile(r"^\s*([A-Za-z_$][\w$]*)\s*\([^;{}]*=>\s*\{"),
]
# 控制流关键字不算方法节点。
TS_CONTROL_KEYWORDS = ("if", "for", "while", "switch", "catch", "return", "else", "do", "try", "await")
# Rust：fn / impl / mod（impl 提取类型名当节点名）。
RS_DECL_PATTERN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z_][\w]*)")
RS_CONTAINER_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(impl\b[^{]*?([A-Za-z_][\w]*)|mod\s+([A-Za-z_][\w]*))[^{]*\{"
)


def compute_outline(lines: list[str], suffix: str) -> list[dict]:
    """按花括号深度解析大纲节点：每个节点有 name / start / end（1 基、含端点）与嵌套序。

    用注释剥离后的文本逐行数净花括号；声明行入栈，深度回落到声明前水平时出栈。
    没有花括号的声明（const x = ( 的 JSX 段落）在出现 ) 收尾行、或父节点闭合时结束。
    """
    is_rust = suffix == ".rs"
    nodes: list[dict] = []
    stack: list[dict] = []
    depth = 0
    for idx, bare in enumerate(lines):
        line_no = idx + 1
        stripped = bare.strip()
        # 声明行：用该行之前的深度作为节点入口深度。
        name = None
        if is_rust:
            container = RS_CONTAINER_PATTERN.match(bare)
            fn = RS_DECL_PATTERN.match(bare)
            if container:
                type_name = container.group(2)
                mod_name = container.group(3)
                name = f"impl {type_name}" if type_name else mod_name
            elif fn:
                name = fn.group(1)
        else:
            first_word = stripped.split("(")[0].split(":")[0].strip()
            if not first_word.split(" ")[-1] in TS_CONTROL_KEYWORDS:
                for pattern in TS_DECL_PATTERNS:
                    found = pattern.match(bare)
                    if found:
                        name = found.group(1)
                        break
        if name:
            stack.append({
                "name": name,
                "start": line_no,
                "entry": depth,
                # 声明行没带花括号：构造在后续行展开（const x = ( 的 JSX 段落）。
                "braceless": "{" not in bare,
            })
        depth += bare.count("{") - bare.count("}")
        while stack:
            top = stack[-1]
            same_line = top["start"] == line_no
            if top["braceless"]:
                # 以 ) 收尾的行（且深度回到声明前水平）关闭该节点。
                if not same_line and depth == top["entry"] and stripped.startswith(")"):
                    node = stack.pop()
                    node["end"] = line_no
                    nodes.append(node)
                    continue
                # 它包着里面的所有节点，挡住下方节点的闭合判断。
                break
            # 普通节点：深度回落即闭合；但声明行自己净花括号 ≤ 0 时不能当行闭合。
            if top["entry"] > depth or (
                top["entry"] == depth and not (same_line and top["braceless"])
            ):
                stack.pop()
                top["end"] = line_no
                nodes.append(top)
                # 父节点闭合时，上面挂着的未闭合 braceless 节点一并闭合。
                while stack and stack[-1]["braceless"]:
                    child = stack.pop()
                    child["end"] = line_no
                    nodes.append(child)
                continue
            break
    for node in stack:
        node["end"] = len(lines)
        nodes.append(node)
    nodes.sort(key=lambda n: (n["start"], n["end"]))
    return nodes


def outline_path_at(outline: list[dict], line: int) -> str:
    """某一行所属的大纲路径：由外到内用 › 连接；顶层文案归到「（文件顶层）」。"""
    chain = [node for node in outline if node["start"] <= line <= node["end"]]
    if not chain:
        return "（文件顶层）"
    return " › ".join(node["name"] for node in chain)


def scan_all() -> list[dict]:
    items: list[dict] = []
    for path in source_files():
        items.extend(scan_file(path))
    items.sort(key=lambda i: (i["file"], i["line"], i["col"]))
    return items


def backup_file(rel_path: str, run_dir: Path) -> Path:
    """写回前把原文件备份到本次运行目录，文件名带路径避免重名。"""
    source = ROOT / rel_path
    destination = run_dir / rel_path.replace("/", "__").replace("\\", "__")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    return destination


def apply_update(payload: dict, run_dir: Path) -> dict:
    rel_path = payload.get("file", "")
    target = (ROOT / rel_path).resolve()
    if not target.is_file() or ROOT not in target.parents:
        return {"ok": False, "error": f"文件不在扫描范围内：{rel_path}"}

    new_text = payload.get("newText", "")
    if not new_text.strip():
        return {"ok": False, "error": "新文案不能为空"}
    kind = payload.get("kind")
    if kind in ("jsx", "jsx-block") and any(ch in new_text for ch in "<>{}"):
        return {"ok": False, "error": "JSX 文本里不能出现 < > { }，请改用其它写法"}
    if "\n" in new_text:
        return {"ok": False, "error": "暂不支持多行文案"}

    try:
        line_no = int(payload["line"])
        col = int(payload.get("col", 0))
    except (KeyError, TypeError, ValueError):
        return {"ok": False, "error": "位置信息缺失"}

    with open(target, "r", encoding="utf-8", newline="") as fh:
        content = fh.read()
    lines = content.split("\n")
    if not 1 <= line_no <= len(lines):
        return {"ok": False, "error": f"行号超出范围：{line_no}"}
    line = lines[line_no - 1]

    kind_label = {"string": "字符串", "jsx": "JSX 文本", "jsx-block": "JSX 块文本"}.get(kind, kind)
    if kind == "string":
        quote = payload.get("quote", '"')
        old_raw = payload.get("raw")
        new_raw = quote + escape(new_text, quote) + quote
    else:
        old_raw = payload.get("raw")
        new_raw = new_text
    if not old_raw:
        return {"ok": False, "error": "缺少原始文本"}

    at = col if 0 <= col <= len(line) and line[col: col + len(old_raw)] == old_raw else line.find(old_raw)
    if at < 0:
        return {"ok": False, "error": f"{kind_label}所在内容已变化（文件可能被改过），请刷新后重试"}

    backup_file(rel_path, run_dir)
    lines[line_no - 1] = line[:at] + new_raw + line[at + len(old_raw):]
    with open(target, "w", encoding="utf-8", newline="") as fh:
        fh.write("\n".join(lines))
    return {"ok": True, "file": rel_path, "line": line_no, "text": new_text}


app = Flask(__name__)
RUN_DIR = BACKUP_DIR / time.strftime("%Y%m%d-%H%M%S")

PAGE = """<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>UI 文案编辑器</title>
<style>
  :root { color-scheme: light; }
  * { box-sizing: border-box; }
  body { margin: 0; font: 14px/1.6 "Microsoft YaHei", system-ui, sans-serif; color: #1f2733; background: #f4f6f9; }
  header { position: sticky; top: 0; z-index: 2; display: flex; gap: 12px; align-items: center;
           padding: 10px 18px; background: #ffffff; border-bottom: 1px solid #dde3ea; }
  header h1 { font-size: 16px; margin: 0 12px 0 0; white-space: nowrap; }
  header input[type=search] { flex: 0 1 320px; padding: 6px 10px; border: 1px solid #c8d0da; border-radius: 6px; }
  header .stats { color: #5b6672; font-size: 13px; }
  header button { padding: 6px 14px; border: 1px solid #c8d0da; border-radius: 6px; background: #fff; cursor: pointer; }
  header button:hover { background: #eef2f7; }
  main { padding: 14px 18px 60px; }
  .file-group { margin-bottom: 22px; }
  .file-group > h2 { font-size: 14px; margin: 0 0 6px; color: #334155; }
  .outline-head { display: flex; align-items: center; gap: 6px; margin: 10px 0 5px; padding: 3px 8px;
                  background: #e9eef5; border-left: 3px solid #6f9bd1; border-radius: 4px;
                  font: 12px/1.6 Consolas, monospace; color: #3d5471; overflow-wrap: anywhere; }
  .outline-head::before { content: "ƒ"; font-weight: 700; color: #6f9bd1; }
  .item { display: flex; gap: 10px; align-items: flex-start; padding: 7px 10px; background: #fff;
          border: 1px solid #e3e8ef; border-radius: 8px; margin-bottom: 6px; }
  .item.saved { border-color: #34a46f; background: #f2fbf6; }
  .item.error { border-color: #d9534f; background: #fdf5f4; }
  .loc { flex: 0 0 auto; font: 12px/2.4 Consolas, monospace; color: #6b7686; min-width: 96px; text-align: right; }
  .kind { flex: 0 0 auto; font-size: 11px; padding: 1px 7px; border-radius: 99px; margin-top: 5px;
          background: #e8eef6; color: #44566e; }
  .item input { flex: 1 1 auto; padding: 6px 9px; border: 1px solid #c8d0da; border-radius: 6px; font: inherit; }
  .item input:focus { outline: 2px solid #9cc3f5; border-color: #5f9fe0; }
  .item button { flex: 0 0 auto; padding: 5px 14px; border: 1px solid #2f6fd0; border-radius: 6px;
                 background: #2f6fd0; color: #fff; cursor: pointer; }
  .item button:hover { background: #285fb4; }
  .msg { flex: 0 0 200px; font-size: 12px; color: #5b6672; align-self: center; }
  .item.error .msg { color: #c0392b; }
  .hidden { display: none; }
  .empty { color: #8592a3; text-align: center; padding: 40px 0; }
</style>
</head>
<body>
<header>
  <h1>UI 文案编辑器</h1>
  <input id="search" type="search" placeholder="搜索文案 / 文件名…">
  <select id="fileFilter"><option value="">全部文件</option></select>
  <span class="stats" id="stats"></span>
  <button id="rescan">重新扫描</button>
</header>
<main id="main"><div class="empty">正在扫描…</div></main>
<script>
let items = [];
const $ = (sel) => document.querySelector(sel);

async function load() {
  const res = await fetch('/api/items');
  items = await res.json();
  render();
}

function render() {
  const query = $('#search').value.trim().toLowerCase();
  const fileFilter = $('#fileFilter').value;
  const main = $('#main');
  main.innerHTML = '';

  const files = [...new Set(items.map((i) => i.file))].sort();
  const select = $('#fileFilter');
  const current = select.value;
  select.innerHTML = '<option value="">全部文件</option>' +
    files.map((f) => `<option${f === current ? ' selected' : ''}>${f}</option>`).join('');

  const visible = items.filter((i) =>
    (!fileFilter || i.file === fileFilter) &&
    (!query || i.text.toLowerCase().includes(query) || i.file.toLowerCase().includes(query)));

  $('#stats').textContent = `共 ${items.length} 处文案，显示 ${visible.length} 处（修改保存后写回源码）`;
  if (!visible.length) {
    main.innerHTML = '<div class="empty">没有匹配的文案</div>';
    return;
  }

  let lastFile = null;
  let lastOutline = null;
  let outlineGroup = null;
  for (const item of visible) {
    if (item.file !== lastFile) {
      lastFile = item.file;
      lastOutline = null;
      outlineGroup = null;
      const count = visible.filter((i) => i.file === item.file).length;
      const h = document.createElement('h2');
      h.className = 'file-group-title';
      const group = document.createElement('div');
      group.className = 'file-group';
      h.innerHTML = `${item.file} <small style="color:#8a97a8">（${count} 处）</small>`;
      group.appendChild(h);
      main.appendChild(group);
    }
    // 文件内再按所属函数大纲分组（VSCode 大纲风格，按嵌套路径 › 连接）。
    if (item.outline !== lastOutline) {
      lastOutline = item.outline;
      const head = document.createElement('div');
      head.className = 'outline-head';
      head.textContent = item.outline;
      outlineGroup = document.createElement('div');
      outlineGroup.className = 'outline-group';
      outlineGroup.appendChild(head);
      main.lastElementChild.appendChild(outlineGroup);
    }
    outlineGroup.appendChild(renderItem(item));
  }
}

function renderItem(item) {
  const row = document.createElement('div');
  row.className = 'item';
  row.dataset.key = `${item.file}:${item.line}:${item.col}`;
  row.innerHTML = `
    <span class="loc">L${item.line}</span>
    <span class="kind">${item.kind}</span>
    <input type="text" value="">
    <button>保存</button>
    <span class="msg"></span>`;
  const input = row.querySelector('input');
  input.value = item.text;
  input.dataset.original = item.text;
  row.querySelector('button').onclick = () => save(row, item, input);
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter') save(row, item, input); });
  return row;
}

async function save(row, item, input) {
  const msg = row.querySelector('.msg');
  const newText = input.value;
  if (!newText.trim()) { msg.textContent = '新文案不能为空'; row.classList.add('error'); return; }
  if (newText === input.dataset.original) { msg.textContent = '内容没有变化'; return; }
  const res = await fetch('/api/update', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...item, newText }),
  });
  const data = await res.json();
  if (data.ok) {
    item.text = newText;
    input.dataset.original = newText;
    row.classList.add('saved'); row.classList.remove('error');
    msg.textContent = '已写回源码';
  } else {
    row.classList.add('error'); row.classList.remove('saved');
    msg.textContent = data.error || '写入失败';
  }
}

$('#search').addEventListener('input', render);
$('#fileFilter').addEventListener('change', render);
$('#rescan').onclick = load;
load();
</script>
</body>
</html>"""


@app.get("/")
def index():
    return PAGE


@app.get("/api/items")
def api_items():
    return jsonify(scan_all())


@app.post("/api/update")
def api_update():
    return jsonify(apply_update(request.get_json(force=True), RUN_DIR))


if __name__ == "__main__":
    print(f"UI 文案编辑器：http://{HOST}:{PORT}  （备份目录：{RUN_DIR}）")
    app.run(host=HOST, port=PORT, debug=False)
