import assert from "node:assert/strict";
import test from "node:test";
import { shouldShowNotification } from "../src/developerMode.ts";
import {
  DEFAULT_LAUNCH_MUSIC_STATE,
  formatMusicTime,
  LAUNCH_MUSIC_STATE_VERSION,
  LAUNCH_MUSIC_TRACK_TITLE,
  LAUNCH_MUSIC_URL,
  MUSIC_VOLUME_DEFAULT,
  musicAudioVolume,
  musicPlayLabel,
  musicProgressPercent,
  musicToggleLabel,
  normalizeLaunchMusicState,
  normalizeMusicPosition,
  normalizeMusicVolume,
  persistedPosition,
  pickNewerMusicState,
} from "../src/launchMusicUi.ts";

test("formats music time as m:ss", () => {
  assert.equal(formatMusicTime(0), "0:00");
  assert.equal(formatMusicTime(9), "0:09");
  assert.equal(formatMusicTime(61), "1:01");
  assert.equal(formatMusicTime(599), "9:59");
  assert.equal(formatMusicTime(3725), "62:05");
});

test("unknown or invalid duration shows a placeholder", () => {
  assert.equal(formatMusicTime(null), "--:--");
  assert.equal(formatMusicTime(Number.NaN), "--:--");
  assert.equal(formatMusicTime(Infinity), "--:--");
  assert.equal(formatMusicTime(-3), "--:--");
});

test("progress percent is clamped and safe without duration", () => {
  assert.equal(musicProgressPercent(0, 100), 0);
  assert.equal(musicProgressPercent(50, 100), 50);
  assert.equal(musicProgressPercent(100, 100), 100);
  // 超出范围夹紧
  assert.equal(musicProgressPercent(150, 100), 100);
  assert.equal(musicProgressPercent(-10, 100), 0);
  // 时长未知 / 为 0 / NaN → 0（避免 NaN 宽度）
  assert.equal(musicProgressPercent(10, null), 0);
  assert.equal(musicProgressPercent(10, 0), 0);
  assert.equal(musicProgressPercent(Number.NaN, 100), 0);
});

test("music source points at the builtin wallpaper directory", () => {
  assert.equal(LAUNCH_MUSIC_URL, "/backgrounds/main.mp3");
  assert.ok(LAUNCH_MUSIC_TRACK_TITLE.includes("万里升平"));
});

test("normalizes persisted music state and falls back on garbage", () => {
  // 当前版本（带 v）存档原样保留
  assert.deepEqual(
    normalizeLaunchMusicState({
      v: LAUNCH_MUSIC_STATE_VERSION,
      volume: 35,
      playing: true,
      positionSec: 42.5,
      open: true,
    }),
    {
      v: LAUNCH_MUSIC_STATE_VERSION,
      volume: 35,
      playing: true,
      positionSec: 42.5,
      open: true,
      savedAt: undefined,
    },
  );
  // 缺失 / 非对象 → 全默认
  assert.deepEqual(normalizeLaunchMusicState(null), DEFAULT_LAUNCH_MUSIC_STATE);
  assert.deepEqual(normalizeLaunchMusicState("oops"), DEFAULT_LAUNCH_MUSIC_STATE);
  assert.deepEqual(normalizeLaunchMusicState(undefined), DEFAULT_LAUNCH_MUSIC_STATE);
  // 坏字段逐个回落：音量越界夹紧、位置负数归零、非布尔按 false
  const repaired = normalizeLaunchMusicState({
    v: LAUNCH_MUSIC_STATE_VERSION,
    volume: 480,
    playing: "yes",
    positionSec: -12,
    open: 1,
  });
  // 非布尔的 playing 按默认值（自动播放）处理
  assert.deepEqual(repaired, {
    v: LAUNCH_MUSIC_STATE_VERSION,
    volume: 100,
    playing: true,
    positionSec: 0,
    open: false,
    savedAt: undefined,
  });
});

test("volume normalization clamps to 0-100 and rounds", () => {
  assert.equal(normalizeMusicVolume(0), 0);
  assert.equal(normalizeMusicVolume(100), 100);
  assert.equal(normalizeMusicVolume(-20), 0);
  assert.equal(normalizeMusicVolume(180), 100);
  assert.equal(normalizeMusicVolume(42.7), 43);
  // 非数字回落默认音量
  assert.equal(normalizeMusicVolume(Number.NaN), MUSIC_VOLUME_DEFAULT);
  assert.equal(normalizeMusicVolume("70"), MUSIC_VOLUME_DEFAULT);
});

test("volume percent maps to the audio element range", () => {
  assert.equal(musicAudioVolume(0), 0);
  assert.equal(musicAudioVolume(50), 0.5);
  assert.equal(musicAudioVolume(100), 1);
  assert.equal(musicAudioVolume(250), 1);
});

test("position normalization treats bad values as the start", () => {
  assert.equal(normalizeMusicPosition(12.5), 12.5);
  assert.equal(normalizeMusicPosition(0), 0);
  assert.equal(normalizeMusicPosition(-1), 0);
  assert.equal(normalizeMusicPosition(Number.NaN), 0);
  assert.equal(normalizeMusicPosition(Infinity), 0);
  assert.equal(normalizeMusicPosition("9"), 0);
});

test("player labels reflect current state", () => {
  assert.equal(musicToggleLabel(true), "收起背景音乐播放器");
  assert.equal(musicToggleLabel(false), "展开背景音乐播放器");
  assert.equal(musicPlayLabel(true), "暂停背景音乐");
  assert.equal(musicPlayLabel(false), "播放背景音乐");
});

test("default state autoplays at volume 30", () => {
  // 默认自动播放 + 默认音量 30：没写过状态的用户一进启动页就该起播。
  assert.equal(DEFAULT_LAUNCH_MUSIC_STATE.playing, true);
  assert.equal(DEFAULT_LAUNCH_MUSIC_STATE.volume, 30);
  assert.equal(MUSIC_VOLUME_DEFAULT, 30);
  assert.equal(DEFAULT_LAUNCH_MUSIC_STATE.open, false);
  // 旧配置里没有这个字段时，规整结果同样是"自动播放 + 30"。
  assert.equal(normalizeLaunchMusicState({}).playing, true);
  assert.equal(normalizeLaunchMusicState({}).volume, 30);
  assert.equal(normalizeLaunchMusicState(null).playing, true);
  // 用户手动暂停后存档的 false 必须被尊重（不能强拉回播放）。
  const paused = normalizeLaunchMusicState({
    v: LAUNCH_MUSIC_STATE_VERSION,
    volume: 40,
    playing: false,
    positionSec: 12,
    open: false,
  });
  assert.equal(paused.playing, false);
  assert.equal(paused.volume, 40);
  assert.equal(paused.positionSec, 12);
});

test("legacy saves migrate to the new autoplay + volume 30 defaults", () => {
  // 改版前的存档（没有 v）是"暂停 + 音量 60"写的；直接沿用会让"默认播放/音量 30"看起来没生效。
  const legacy = normalizeLaunchMusicState({
    volume: 60,
    playing: false,
    positionSec: 88,
    open: true,
  });
  assert.equal(legacy.playing, true);
  assert.equal(legacy.volume, 30);
  assert.equal(legacy.v, LAUNCH_MUSIC_STATE_VERSION);
  // 播放位置与面板展开保留
  assert.equal(legacy.positionSec, 88);
  assert.equal(legacy.open, true);
});

test("autoplay notice stays visible outside developer mode", () => {
  // 简单模式只显示 error；自动播放的提示必须是常驻的 notice，否则用户根本看不到。
  assert.equal(shouldShowNotification("notice", false), true);
  assert.equal(shouldShowNotification("info", false), false);
  assert.equal(shouldShowNotification("success", false), false);
  assert.equal(shouldShowNotification("error", false), true);
  // 开发者模式下全部可见
  assert.equal(shouldShowNotification("info", true), true);
});

test("restored position survives the autoplay flush", () => {
  // 恢复顺序：读存档(19.96s) → 起播触发 onPlay 立刻落盘 → 元数据就绪后定位。
  // 中间那次落盘时 audio.currentTime 还是 0，绝不能拿它覆盖存档（否则每次启动都从头播）。
  assert.equal(
    persistedPosition({ restorePending: true, audioTime: 0, storedPosition: 19.96 }),
    19.96,
  );
  // 定位已应用后，正常用音频的真实位置。
  assert.equal(
    persistedPosition({ restorePending: false, audioTime: 42.5, storedPosition: 19.96 }),
    42.5,
  );
  // 没有音频元素 / 位置非法时保留存档值，不写 NaN。
  assert.equal(persistedPosition({ restorePending: false, audioTime: null, storedPosition: 7 }), 7);
  assert.equal(
    persistedPosition({ restorePending: false, audioTime: Number.NaN, storedPosition: 7 }),
    7,
  );
  assert.equal(persistedPosition({ restorePending: false, audioTime: -1, storedPosition: 7 }), 7);
});

test("picks the newer of file state and local cache", () => {
  const file = {
    v: 2,
    volume: 30,
    playing: true,
    positionSec: 0,
    open: true,
    savedAt: 100,
  };
  const local = {
    v: 2,
    volume: 30,
    playing: false,
    positionSec: 60.86,
    open: true,
    savedAt: 200,
  };
  // 刷新瞬间 IPC 被丢、本地缓存更新：必须采用本地缓存（暂停 + 60.86s）
  assert.equal(pickNewerMusicState(file, local), local);
  // 反过来则以文件为准
  assert.equal(pickNewerMusicState(local, file), local);
  // 没有本地缓存时用文件
  assert.equal(pickNewerMusicState(file, null), file);
  // savedAt 缺失视为最旧
  const legacyLocal = { ...local, savedAt: undefined };
  assert.equal(pickNewerMusicState(file, legacyLocal), file);
});
