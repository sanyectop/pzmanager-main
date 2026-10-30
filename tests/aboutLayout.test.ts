import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
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
