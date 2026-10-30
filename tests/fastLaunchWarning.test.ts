import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);

test("enabling fast launch asks for confirmation first", () => {
  assert.match(
    appSource,
    /const \[fastLaunchWarningOpen, setFastLaunchWarningOpen\] = useState\(false\)/,
  );
  assert.match(appSource, /function closeFastLaunchWarning\(confirm: boolean\)/);
  // 打开开关只弹警告，不直接落盘；关闭开关仍然直接生效。
  assert.match(
    appSource,
    /if \(details\.checked\) \{\s*(?:\/\/[^\n]*\n\s*)?setFastLaunch\(true\);\s*setFastLaunchWarningOpen\(true\);\s*\} else \{\s*void updateFastLaunch\(false\);\s*\}/s,
  );
  assert.match(appSource, /checked=\{fastLaunch\}/);
  assert.doesNotMatch(
    appSource,
    /onCheckedChange=\{\(details\) => void updateFastLaunch\(details\.checked\)\}/,
  );
  assert.match(appSource, /<FastLaunchWarningDialog/);
  assert.match(appSource, /onConfirm=\{\(\) => closeFastLaunchWarning\(true\)\}/);
  // 确认后才真正落盘，失败回退到「关闭」而不是临时打开的状态。
  assert.match(appSource, /updateFastLaunch\(true, false\)/);
});

test("warning dialog states the risk and requires explicit confirmation", () => {
  assert.match(appSource, /aria-label="开启极速启动"/);
  assert.match(appSource, /开启极速启动？/);
  assert.match(appSource, /极速启动会修改游戏的启动方式/);
  assert.match(appSource, /仍要开启/);
  assert.match(appSource, />取消</);
  assert.match(cssSource, /\.pz-fast-launch-dialog\s*\{[^}]*width:\s*min\(32rem, 100%\);/s);
});

test("fast launch description collapses into a hint dot", () => {
  assert.match(appSource, /label="极速启动说明"/);
  const hintAt = appSource.indexOf('label="极速启动说明"');
  assert.ok(hintAt >= 0);
  const hintBlock = appSource.slice(hintAt, appSource.indexOf("/>", hintAt));
  assert.match(hintBlock, /让游戏跳过启动时的模组全量扫描/);
  assert.match(hintBlock, /fast-launch-backup/);
  // 原来的说明不再作为可见段落出现，只保留阻塞性提示和缓存状态。
  assert.doesNotMatch(
    appSource,
    /<Text color="gray\.500" fontSize="xs" mt="1">\s*让游戏跳过启动时的模组全量扫描/,
  );
});
