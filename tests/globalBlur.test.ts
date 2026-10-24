import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const cssSource = readFileSync(
  fileURLToPath(new URL("../src/App.css", import.meta.url)),
  "utf8",
);
const rustSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);

/** 注释里会写 blur(28px) 这类历史值，比较前先去掉。 */
const cssCode = cssSource.replace(/\/\*[\s\S]*?\*\//g, "");

test("every blur radius scales with --pz-blur-scale", () => {
  // 倍率本身要有兜底声明，否则 JS 还没跑起来时 var() 无值
  assert.match(cssCode, /--pz-blur-scale:\s*1;/);

  // 写死的半径会绕开「模糊度」设置：blur() 里要么是 calc(...)，要么是已经带倍率的令牌
  const rawBlur = cssCode.match(/blur\(\s*\d+(\.\d+)?px/g) ?? [];
  assert.deepEqual(rawBlur, [], "还有没接 --pz-blur-scale 的写死模糊半径");

  const blurDecls =
    cssCode.match(/(?:-webkit-)?backdrop-filter:[^;]+;/g) ?? [];
  assert.ok(blurDecls.length >= 8, "毛玻璃声明数量异常");
  for (const decl of blurDecls) {
    if (!decl.includes("blur(")) {
      continue;
    }
    assert.match(decl, /blur\((calc\(|var\()/, decl);
  }

  // 启动页的克隆壁纸层走 filter，同样必须吃倍率
  for (const decl of cssCode.match(/filter:[^;]+;/g) ?? []) {
    if (!decl.includes("blur(")) {
      continue;
    }
    assert.match(decl, /blur\((calc\(|var\()/, decl);
  }
});

test("settings › 通用 exposes 模糊度 and applies it globally", () => {
  assert.match(appSource, /const MIN_BLUR_STRENGTH = 0;/);
  assert.match(appSource, /const MAX_BLUR_STRENGTH = 200;/);
  assert.match(appSource, /const DEFAULT_BLUR_STRENGTH = 100;/);
  // 变量写在 documentElement 上：Portal 到 body 的菜单 / 弹窗 / Toast 也吃得到
  assert.match(
    appSource,
    /document\.documentElement\.style\.setProperty\("--pz-blur-scale", String\(blurStrength \/ 100\)\)/,
  );
  assert.match(appSource, /模糊度/);
  assert.match(appSource, /max=\{MAX_BLUR_STRENGTH\}/);
  assert.match(appSource, /invoke\("save_blur_strength", \{ value: clamped \}\)/);
  // 老 settings.json 没有这个字段时要回落到设计基准，不能变成 0（关掉模糊）
  assert.match(appSource, /setBlurStrength\(settingsResult\.value\.blurStrength \?\? DEFAULT_BLUR_STRENGTH\)/);
});

test("backend command name and range match the frontend", () => {
  // invoke 里的命令名必须和后端函数同名，否则运行时直接 404
  assert.match(rustSource, /fn save_blur_strength\(value: u8\)/);
  assert.match(rustSource, /save_blur_strength,/);
  assert.match(rustSource, /blur_strength: Option<u8>/);
  assert.match(rustSource, /const BLUR_STRENGTH_MAX: u8 = 200;/);
  assert.match(rustSource, /const BLUR_STRENGTH_DEFAULT: u8 = 100;/);
  assert.match(rustSource, /fn normalize_blur_strength\(value: u8\) -> u8/);
});
