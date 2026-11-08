import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";

const about = appSource.slice(
  appSource.indexOf("const aboutSettingsContent = ("),
  appSource.indexOf("const versionBusy"),
);

test("官方网站和项目仓库排在作者下方", () => {
  const author = about.indexOf(">作者<");
  const website = about.indexOf(">官方网站<");
  const repository = about.indexOf(">项目仓库<");
  const version = about.indexOf(">PZMANAGER<");

  assert.ok(author >= 0, "missing 作者 section");
  assert.ok(website > author, "官方网站 should follow 作者");
  assert.ok(repository > website, "项目仓库 should follow 官方网站");
  assert.ok(version > repository, "版本介绍 should follow the links");
});

test("两个链接各出现一次并指向预期地址", () => {
  assert.equal(about.match(/>官方网站</g)?.length, 1);
  assert.equal(about.match(/>项目仓库</g)?.length, 1);
  assert.match(about, /https:\/\/pzm\.sanyec\.top/);
  assert.match(about, /https:\/\/github\.com\/sanyectop\/pzmanager-main/);
});

test("开源项目参考排在关于页最底部，并附上三个项目的图标", () => {
  const section = about.indexOf(">开源项目参考<");
  assert.ok(section >= 0, "missing 开源项目参考 section");
  // 底部：排在其他所有分节之后（检查更新是原本的最后一块）。
  assert.ok(section > about.indexOf(">检查更新<"), "开源项目参考 should be the last section");
  assert.ok(section > about.indexOf(">隐私协议<"));

  const projects = [
    ["ZombieBuddy", "zombiebuddy.png", "https://github.com/zed-0xff/ZombieBuddy"],
    ["PCL2-Language", "pcl2-language.png", "https://github.com/PCL-Community/PCL2-Language"],
    ["SJMCL", "sjmcl.png", "https://github.com/UNIkeEN/SJMCL"],
  ] as const;
  for (const [name, icon, url] of projects) {
    assert.ok(about.includes(`name: "${name}"`), `missing project ${name}`);
    assert.ok(about.includes(`url: "${url}"`), `missing url ${url}`);
    assert.ok(about.includes(`/projects/${icon}`), `missing icon reference ${icon}`);
    // 图标文件必须真的在 public/ 里（Vite 会原样拷进产物）。
    const path = fileURLToPath(new URL(`../public/projects/${icon}`, import.meta.url));
    assert.ok(existsSync(path), `missing icon file ${icon}`);
  }
  // 每行都有「打开」入口（复用外部打开箭头）。
  assert.match(about, /onClick=\{\(\) => void openUrl\(project\.url\)\}/);
  assert.match(about, /<ExternalArrowIcon \/>/);
});
