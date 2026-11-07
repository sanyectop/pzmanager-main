import assert from "node:assert/strict";
import test from "node:test";
import {
  applyProfileToValues,
  collectOverrideValues,
  controlKindForOption,
  initialOptionValue,
  initialOverrideValues,
  isNumericValue,
  langDisplayName,
  optionDisplayName,
  parseMinecraftFormatting,
  stripMinecraftFormatting,
  valueDisplayLabel,
} from "../src/shaderpackConfigUi.ts";
import type { ShaderpackOptionInfo, ShaderProfileMeta } from "../src/app/types.ts";

function option(patch: Partial<ShaderpackOptionInfo>): ShaderpackOptionInfo {
  return {
    name: "OPT",
    label: null,
    comment: null,
    defaultValue: null,
    slider: false,
    values: [],
    valueLabels: [],
    liveValue: null,
    ...patch,
  };
}

test("control kind: slider with two or more observed values becomes a select", () => {
  // OptiFine 滑块常配离散档位（如 shadowMapResolution 1024/2048/4096），按钮组比连续滑条好用
  assert.equal(
    controlKindForOption(option({ slider: true, values: ["1024", "2048"] })),
    "select",
  );
  // 只有默认值的滑块 → 连续滑条
  assert.equal(controlKindForOption(option({ slider: true })), "slider");
});

test("control kind: boolean values become a toggle", () => {
  assert.equal(
    controlKindForOption(option({ values: ["true", "false"] })),
    "toggle",
  );
  assert.equal(controlKindForOption(option({ values: ["false"] })), "toggle");
});

test("control kind: multiple observed values become a select, rest text", () => {
  assert.equal(
    controlKindForOption(option({ values: ["Low", "Medium", "High"] })),
    "select",
  );
  assert.equal(controlKindForOption(option({})), "text");
  // 单个非布尔观测值也没有选择余地 → 文本
  assert.equal(controlKindForOption(option({ values: ["High"] })), "text");
});

test("initial value prefers live, then default, then empty", () => {
  assert.equal(initialOptionValue(option({ liveValue: "1", defaultValue: "0" })), "1");
  assert.equal(initialOptionValue(option({ defaultValue: "0.60" })), "0.60");
  assert.equal(initialOptionValue(option({})), "");
  assert.deepEqual(initialOverrideValues([option({ name: "A", defaultValue: "x" }), option({ name: "B" })]), {
    A: "x",
    B: "",
  });
});

test("profile prefill only touches options present in the pack", () => {
  const options = [option({ name: "SHADOWS" }), option({ name: "TAA" }), option({ name: "OTHER" })];
  const profile: ShaderProfileMeta = {
    name: "HIGH",
    tokens: [
      { option: "SHADOWS", value: "true" },
      { option: "TAA", value: "false" },
      // 预设里出现但包参数表没有的 token 忽略
      { option: "GHOST", value: "1" },
    ],
  };
  const next = applyProfileToValues(options, profile, { SHADOWS: "false", OTHER: "keep" });
  assert.deepEqual(next, { SHADOWS: "true", TAA: "false", OTHER: "keep" });
  // 空预设（不改预设）原样返回
  assert.deepEqual(applyProfileToValues(options, null, { SHADOWS: "false" }), { SHADOWS: "false" });
});

test("collect override keeps only values that differ from the dialog's initial values", () => {
  const options = [
    option({ name: "A", defaultValue: "1" }),
    option({ name: "B", defaultValue: "2" }),
    option({ name: "C", defaultValue: "3" }),
  ];
  const initial = { A: "1", B: "2", C: "3" };
  // 只改了 B、清空了 C
  assert.deepEqual(
    collectOverrideValues(options, initial, { A: "1", B: "9", C: "" }),
    { B: "9", C: "" },
  );
  // 全没动 → 空覆写
  assert.deepEqual(collectOverrideValues(options, initial, initial), {});
});

test("isNumericValue gates the continuous slider", () => {
  assert.equal(isNumericValue("0.60"), true);
  assert.equal(isNumericValue("1"), true);
  assert.equal(isNumericValue(""), false);
  assert.equal(isNumericValue("auto"), false);
});

test("option display name prefers the lang label and falls back to the raw name", () => {
  assert.equal(
    optionDisplayName(option({ name: "SHADOWS", label: "阴影" })),
    "阴影",
  );
  // 空白标签按没有处理
  assert.equal(optionDisplayName(option({ name: "SHADOWS", label: "  " })), "SHADOWS");
  assert.equal(optionDisplayName(option({ name: "SHADOWS" })), "SHADOWS");
});

test("value display label uses valueLabels and falls back to the raw value", () => {
  const labelled = option({
    name: "SHADOWS",
    valueLabels: [
      { value: "true", label: "开" },
      { value: "false", label: "关" },
    ],
  });
  assert.equal(valueDisplayLabel(labelled, "true"), "开");
  assert.equal(valueDisplayLabel(labelled, "false"), "关");
  // 没有标签的取值 / 没有标签的参数 → 原值
  assert.equal(valueDisplayLabel(labelled, "2048"), "2048");
  assert.equal(valueDisplayLabel(option({ name: "X" }), "true"), "true");
  // 空白标签按没有处理
  assert.equal(
    valueDisplayLabel(option({ name: "X", valueLabels: [{ value: "a", label: " " }] }), "a"),
    "a",
  );
});

test("lang display names map known stems and pass through unknown ones", () => {
  assert.equal(langDisplayName("zh_CN"), "中文");
  assert.equal(langDisplayName("zh_TW"), "繁體中文");
  assert.equal(langDisplayName("en_US"), "English");
  assert.equal(langDisplayName("ja_JP"), "日本語");
  // 大小写不敏感
  assert.equal(langDisplayName("ZH_cn"), "中文");
  // 认不出的语言原样显示字干
  assert.equal(langDisplayName("xx_XX"), "xx_XX");
});

test("parses MC color codes into colored segments", () => {
  const segments = parseMinecraftFormatting("§a开启§r关闭");
  assert.deepEqual(segments, [
    { text: "开启", color: "#55FF55" },
    { text: "关闭" },
  ]);
  // 无码文本 → 单段无样式
  assert.deepEqual(parseMinecraftFormatting("普通文字"), [{ text: "普通文字" }]);
  // 大小写不敏感
  assert.equal(parseMinecraftFormatting("§CGOT")[0].color, "#FF5555");
});

test("MC color codes reset formatting, §r resets everything", () => {
  // MC 语义：颜色码会禁用之前的格式——§l 之后来 §a，粗体被清掉
  const mixed = parseMinecraftFormatting("§l粗§a绿字");
  assert.deepEqual(mixed, [
    { text: "粗", bold: true },
    { text: "绿字", color: "#55FF55" },
  ]);
  // §r 之后回到默认
  const reset = parseMinecraftFormatting("§l§a粗绿§r普通");
  assert.deepEqual(reset, [
    { text: "粗绿", color: "#55FF55" },
    { text: "普通" },
  ]);
  // 格式码之间叠加，且保留当前颜色
  const stacked = parseMinecraftFormatting("§a绿§l粗斜§o体");
  assert.deepEqual(stacked, [
    { text: "绿", color: "#55FF55" },
    { text: "粗斜", color: "#55FF55", bold: true },
    { text: "体", color: "#55FF55", bold: true, italic: true },
  ]);
});

test("unknown MC codes and lone § stay literal", () => {
  // 未知代码 §z 按字面输出（§ 与后码都保留）
  assert.deepEqual(parseMinecraftFormatting("§z奇怪"), [{ text: "§z奇怪" }]);
  assert.deepEqual(parseMinecraftFormatting("结尾§"), [{ text: "结尾§" }]);
  assert.deepEqual(parseMinecraftFormatting("A & B"), [{ text: "A & B" }]);
});

test("strip removes all § codes leaving plain text", () => {
  assert.equal(stripMinecraftFormatting("§6金§l色§r标题"), "金色标题");
  assert.equal(stripMinecraftFormatting("§a"), "");
  assert.equal(stripMinecraftFormatting("没有码"), "没有码");
});

test("labels made only of formatting codes fall back to raw names", () => {
  assert.equal(
    optionDisplayName(option({ name: "SHADOWS", label: "§a§l" })),
    "SHADOWS",
  );
  assert.equal(
    valueDisplayLabel(option({ name: "X", valueLabels: [{ value: "a", label: "§r" }] }), "a"),
    "a",
  );
  // 正常彩字标签不受影响
  assert.equal(
    optionDisplayName(option({ name: "SHADOWS", label: "§a阴影§r" })),
    "§a阴影§r",
  );
});
