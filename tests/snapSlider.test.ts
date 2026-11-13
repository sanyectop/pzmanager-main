import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { appSource } from "./sourceText.ts";

const sliderSource = readFileSync(
  fileURLToPath(new URL("../src/components/snapSlider.tsx", import.meta.url)),
  "utf8",
);

test("SnapSlider drags continuously and snaps to the nearest step on release", () => {
  // 拖动过程按 1 步进：视觉上没有档位粒度。
  assert.match(sliderSource, /step=\{1\}/);
  // 松手后就近吸附到 min + k × step（min 不一定是 step 的整数倍）。
  assert.match(
    sliderSource,
    /min \+ Math\.round\(\(nextValue - min\) \/ step\) \* step/,
  );
  assert.match(sliderSource, /onValueChangeEnd=/);
  // 吸附值和拖动值共用 onChange；提交回调拿到的是吸附后的值。
  assert.match(sliderSource, /onCommit\?\.\(snapped\)/);
});

test("coarse-step sliders all use SnapSlider", () => {
  // 内存 / 对比度 / 模糊度 / 字号 / 静默清理阈值：五个档位滑动条统一走 SnapSlider。
  assert.match(appSource, /import \{ SnapSlider \} from "\.\/components\/snapSlider";/);
  assert.match(appSource, /ariaLabel="游戏内存限制"/);
  assert.match(appSource, /ariaLabel="字体对比度"/);
  assert.match(appSource, /ariaLabel="模糊度"/);
  assert.match(appSource, /ariaLabel="界面字号"/);
  assert.match(appSource, /ariaLabel="静默内存清理触发阈值"/);
  // App.tsx 不再直接写 Slider.Root（壁纸日期 / 时刻与光影参数本就是细步长，不在此列）。
  const appTsSource = readFileSync(
    fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
    "utf8",
  );
  assert.doesNotMatch(appTsSource, /<Slider\.Root/);
  assert.doesNotMatch(appTsSource, /import \{[^}]*\bSlider\b[^}]*\} from "@chakra-ui\/react"/);
});

test("step constants stay the source of truth for snapping", () => {
  assert.match(appSource, /step=\{MEMORY_STEP_MB\}/);
  assert.match(appSource, /step=\{5\}/);
  assert.match(appSource, /step=\{BLUR_STRENGTH_STEP\}/);
  assert.match(appSource, /step=\{FONT_SCALE_STEP\}/);
  assert.match(appSource, /step=\{SILENT_MEMORY_PERCENT_STEP\}/);
});

test("snap animation comes from the shared .pz-memory-slider transitions", () => {
  // 吸附过渡写在消费 CSS 变量的 left/right 与 inset-inline-start 上（见 App.css 注释）。
  const cssSource = readFileSync(
    fileURLToPath(new URL("../src/App.css", import.meta.url)),
    "utf8",
  );
  assert.match(
    cssSource,
    /\.pz-memory-slider \[data-part="range"\] \{[\s\S]*?transition:\s*left/,
  );
  assert.match(
    cssSource,
    /\.pz-memory-slider \[data-part="thumb"\] \{[\s\S]*?transition: inset-inline-start/,
  );
});
