import assert from "node:assert/strict";
import test from "node:test";

import { appSource, cssSource, rustSource } from "./sourceText.ts";

test("launch button shows the last switched/saved version name under Project Zomboid", () => {
  const title = appSource.indexOf('<Text className="pz-launch-card-title">启动游戏</Text>');
  const subtitle = appSource.indexOf('<Text className="pz-launch-card-subtitle">Project Zomboid</Text>');
  const version = appSource.indexOf('className="pz-launch-card-version"');
  assert.ok(title >= 0 && subtitle > title, "启动游戏 / Project Zomboid 两行应存在");
  // 版本名称紧跟在 Project Zomboid 下方。
  assert.ok(version > subtitle, "版本名称应排在 Project Zomboid 之后");
  // 没有版本记录时整行不渲染（而不是留空行）。
  assert.match(appSource, /\{activeVersionLabel \? \(/);
  // 长名字截断成一行，完整名称放 title。
  assert.match(appSource, /<Text className="pz-launch-card-version" title=\{activeVersionLabel\}>/);
  const rule = cssSource.slice(
    cssSource.indexOf(".pz-launch-card-version {"),
    cssSource.indexOf("}", cssSource.indexOf(".pz-launch-card-version {")),
  );
  assert.match(rule, /text-overflow: ellipsis/, rule);
  assert.match(rule, /white-space: nowrap/, rule);
  assert.match(rule, /overflow: hidden/, rule);
});

test("version label comes from the active version in the version store", () => {
  // 后端只读 meta.json 的 active_version_id（保存与还原都会更新它）。
  assert.match(rustSource, /pub fn active_version_label\(store: &Path\)/);
  assert.match(rustSource, /meta\.active_version_id\.as_deref\(\)/);
  // 命令已注册，前端在进入启动页时读取。
  assert.match(rustSource, /fn get_active_version_label\(\) -> Result<Option<String>, String>/);
  assert.match(rustSource, /get_active_version_label,/);
  assert.match(appSource, /invoke<string \| null>\("get_active_version_label"\)/);
  assert.match(appSource, /if \(!isTauriRuntime\(\) \|\| activePage !== "launch"\) return;/);
});
