import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyGameOptionValue,
  filterGameOptions,
  gameOptionBoolean,
  groupGameOptions,
  isValidGameOptionNumber,
} from "../src/gameOptionsUi.ts";

test("classifies booleans, numbers and free text", () => {
  assert.equal(classifyGameOptionValue("true"), "bool");
  assert.equal(classifyGameOptionValue("false"), "bool");
  assert.equal(classifyGameOptionValue("100"), "number");
  assert.equal(classifyGameOptionValue("-1.5"), "number");
  // 颜色 / 列表 / 路径都按文本处理
  assert.equal(classifyGameOptionValue("1.0,0.0,0.0"), "text");
  assert.equal(classifyGameOptionValue("250;225;200"), "text");
  assert.equal(classifyGameOptionValue("麦克风 (YUKUI-780)"), "text");
  assert.equal(classifyGameOptionValue(""), "text");
});

test("parses booleans case-insensitively", () => {
  assert.equal(gameOptionBoolean("true"), true);
  assert.equal(gameOptionBoolean("TRUE"), true);
  assert.equal(gameOptionBoolean(" false "), false);
});

test("filters by key substring ignoring case", () => {
  const entries = [
    { key: "fullScreen", value: "false" },
    { key: "musicVolume", value: "10" },
    { key: "voiceVolumeMic", value: "10" },
  ];
  assert.deepEqual(
    filterGameOptions(entries, "VOLUME").map((entry) => entry.key),
    ["musicVolume", "voiceVolumeMic"],
  );
  assert.equal(filterGameOptions(entries, "   ").length, 3);
  assert.equal(filterGameOptions(entries, "nope").length, 0);
});

test("validates numeric inputs", () => {
  assert.equal(isValidGameOptionNumber("12"), true);
  assert.equal(isValidGameOptionNumber("-0.5"), true);
  assert.equal(isValidGameOptionNumber(""), false);
  assert.equal(isValidGameOptionNumber("1.0,0.0"), false);
  assert.equal(isValidGameOptionNumber("abc"), false);
});

test("search also matches the Chinese label", () => {
  const entries = [
    { key: "musicVolume", value: "10" },
    { key: "fullScreen", value: "false" },
  ];
  const labelOf = (key: string) => (key === "musicVolume" ? "音乐音量" : "全屏");
  assert.deepEqual(
    filterGameOptions(entries, "音量", labelOf).map((entry) => entry.key),
    ["musicVolume"],
  );
  // 键名仍然可搜
  assert.deepEqual(
    filterGameOptions(entries, "fullscreen", labelOf).map((entry) => entry.key),
    ["fullScreen"],
  );
  // 中文 / 键名都不匹配时为空
  assert.equal(filterGameOptions(entries, "不存在", labelOf).length, 0);
});

test("labels cover the real options.ini keys with Chinese names", async () => {
  const { GAME_OPTION_LABELS, gameOptionDisplayName, hasGameOptionLabel } = await import(
    "../src/gameOptionsLabels.ts"
  );
  // 常见项都有中文
  assert.equal(gameOptionDisplayName("musicVolume"), "音乐音量");
  assert.equal(gameOptionDisplayName("fullScreen"), "全屏");
  assert.equal(gameOptionDisplayName("language"), "游戏语言");
  assert.equal(gameOptionDisplayName("voiceRecordDeviceName"), "录音设备");
  // 未收录的键原样显示，不显示"有标签"的副标题
  assert.equal(gameOptionDisplayName("someModOption"), "someModOption");
  assert.equal(hasGameOptionLabel("someModOption"), false);
  // 粗略翻译也要覆盖本机真实文件里的绝大多数键（至少 120 个）
  assert.ok(Object.keys(GAME_OPTION_LABELS).length >= 120);
});


test("groups follow category order and drop empty groups", () => {
  const categories = [
    { id: "graphics", title: "画面与显示", keys: ["fullScreen"] },
    { id: "audio", title: "声音与音乐", keys: ["musicVolume", "soundVolume"] },
  ];
  const categoryOf = (key: string) =>
    categories.find((category) => category.keys.includes(key))?.id;
  const entries = [
    { key: "musicVolume", value: "10" },
    { key: "unknownModKey", value: "1" },
    { key: "fullScreen", value: "false" },
    { key: "soundVolume", value: "10" },
  ];
  const groups = groupGameOptions(entries, categories, categoryOf, {
    id: "other",
    title: "其他",
  });
  // 顺序：分类顺序 → 其他在最后；组内保持原顺序
  assert.deepEqual(
    groups.map((group) => group.title),
    ["画面与显示", "声音与音乐", "其他"],
  );
  assert.deepEqual(
    groups[1].entries.map((entry) => entry.key),
    ["musicVolume", "soundVolume"],
  );
  assert.deepEqual(
    groups[2].entries.map((entry) => entry.key),
    ["unknownModKey"],
  );
  // 搜索后为空的分类不出现
  const onlyAudio = groupGameOptions(
    entries.filter((entry) => entry.key.includes("Volume")),
    categories,
    categoryOf,
    { id: "other", title: "其他" },
  );
  assert.deepEqual(
    onlyAudio.map((group) => group.title),
    ["声音与音乐"],
  );
});
