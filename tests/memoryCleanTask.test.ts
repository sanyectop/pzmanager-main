import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { appSource } from "./sourceText.ts";
import { rustSource as backendSource } from "./sourceText.ts";
import {
  MEMORY_CLEAN_NO_DECREASE_GRACE_MS,
  MEMORY_CLEAN_POLL_MS,
  MEMORY_CLEAN_PLATEAU_GRACE_MS,
  applyMemoryCleanSample,
  createMemoryCleanTracker,
  memoryCleanFinishedDetail,
  memoryCleanProgressDetail,
  memoryCleanupSystemDetail,
  shouldFinishMemoryClean,
} from "../src/memoryClean.ts";

function sample(usedMb: number) {
  return { usedMb, usedPercent: 50 };
}

test("内存占用按每秒一次的节奏检测", () => {
  assert.equal(MEMORY_CLEAN_POLL_MS, 1000);
});

test("已清理量按点击前基准计算，并保留观察到的峰值", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(7500));
  assert.equal(tracker.peakFreedMb, 500);
  assert.equal(tracker.sawDecrease, true);
  assert.equal(tracker.done, false);

  tracker = applyMemoryCleanSample(tracker, sample(7000));
  assert.equal(tracker.peakFreedMb, 1000);
  assert.equal(tracker.done, false);

  // 物理内存不再减少、开始回升：任务完成，"已清理"仍记峰值。
  tracker = applyMemoryCleanSample(tracker, sample(7050));
  assert.equal(tracker.done, true);
  assert.equal(tracker.peakFreedMb, 1000);
});

test("内存没有先下降时不会因为波动判定完成", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(8100));
  assert.equal(tracker.sawDecrease, false);
  assert.equal(tracker.done, false);
});

test("后端命令返回前不按回升收尾", () => {
  let tracker = createMemoryCleanTracker(8000);
  tracker = applyMemoryCleanSample(tracker, sample(7000));
  tracker = applyMemoryCleanSample(tracker, sample(7100));
  assert.equal(tracker.done, true);

  assert.equal(
    shouldFinishMemoryClean(tracker, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: 300,
      cleanReturned: false,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(tracker, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: 300,
      cleanReturned: true,
    }),
    true,
  );
});

test("观察窗口兜底：一直没下降或下降后平稳都会收尾", () => {
  const flat = createMemoryCleanTracker(8000);
  assert.equal(
    shouldFinishMemoryClean(flat, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: MEMORY_CLEAN_NO_DECREASE_GRACE_MS - 1,
      cleanReturned: true,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(flat, {
      startedAt: 0,
      lastDecreaseAt: 0,
      now: MEMORY_CLEAN_NO_DECREASE_GRACE_MS,
      cleanReturned: true,
    }),
    true,
  );

  let dropped = createMemoryCleanTracker(8000);
  dropped = applyMemoryCleanSample(dropped, sample(7000));
  assert.equal(
    shouldFinishMemoryClean(dropped, {
      startedAt: 0,
      lastDecreaseAt: 1000,
      now: 1000 + MEMORY_CLEAN_PLATEAU_GRACE_MS - 1,
      cleanReturned: true,
    }),
    false,
  );
  assert.equal(
    shouldFinishMemoryClean(dropped, {
      startedAt: 0,
      lastDecreaseAt: 1000,
      now: 1000 + MEMORY_CLEAN_PLATEAU_GRACE_MS,
      cleanReturned: true,
    }),
    true,
  );
});

test("任务行文案带上当前已清理的 MB", () => {
  assert.equal(memoryCleanProgressDetail(0), "正在清理…");
  assert.equal(memoryCleanProgressDetail(128), "当前已清理 128 MB");

  assert.equal(memoryCleanFinishedDetail(0, 10), "清理完成，内存占用没有明显下降");
  assert.equal(memoryCleanFinishedDetail(256, 12), "清理完成，共释放 256 MB（12 个进程）");
  assert.equal(memoryCleanFinishedDetail(256, 0), "清理完成，共释放 256 MB");
});

test("设置里的清理按钮改为登记任务中心任务并采样物理内存", () => {
  assert.match(appSource, /kind: "memory-clean"/);
  assert.match(appSource, /const taskId = "memory-task-" \+ \(\+\+taskIdRef\.current\)/);
  assert.match(appSource, /applyMemoryCleanSample\(tracker/);
  assert.match(appSource, /setTaskPanelOpen\(true\)/);
  assert.match(appSource, /memoryCleanProgressDetail\(tracker\.peakFreedMb\)/);
  assert.match(appSource, /memoryCleanFinishedDetail\(freedMb, report\.trimmed\)/);
});

test("激进内存清理是独立开关，落盘后后端立刻生效", () => {
  // 命令注册 + 设置读写 + 同步给 memclean 的后台线程/清理入口。
  assert.match(backendSource, /fn save_aggressive_memory_clean\(enabled: bool\) -> Result<\(\), String>/);
  assert.match(backendSource, /settings\.aggressive_memory_clean = enabled;/);
  assert.match(backendSource, /memclean::set_aggressive_cleanup\(enabled\);/);
  assert.match(backendSource, /^\s+save_aggressive_memory_clean,$/m);
  assert.match(appSource, /invoke\("save_aggressive_memory_clean", \{ enabled \}\)/);
  assert.match(appSource, /aria-label="激进内存清理"/);
  // 长说明收进标题旁的问号提示，正文只留非管理员时的阻塞提示。
  assert.match(appSource, /label="激进内存清理说明"/);
  assert.doesNotMatch(appSource, /打开后还会多做两步系统级清理/);
  // 报告里带上系统级步骤的结果，界面据此提示权限不足（文案已抽到 memoryClean.ts）。
  assert.match(backendSource, /pub emptied_working_sets: bool,/);
  assert.match(backendSource, /pub flushed_file_cache: bool,/);
  assert.match(backendSource, /pub flushed_modified: bool,/);
  assert.match(backendSource, /pub purged_standby: bool,/);
  assert.match(backendSource, /pub protected_running: bool,/);
  assert.match(appSource, /memoryCleanupSystemDetail\(lastCleanupReport\)/);
});

test("激进清理的四步系统级清理都走官方内存管理 API", () => {
  // 全系统工作集 → 文件缓存 → 已修改页 → 待机列表。
  assert.match(backendSource, /const MEMORY_EMPTY_WORKING_SETS: u32 = 2;/);
  assert.match(backendSource, /SetSystemFileCacheSize\(usize::MAX, usize::MAX, 0\)/);
  assert.match(backendSource, /const MEMORY_FLUSH_MODIFIED_LIST: u32 = 3;/);
  assert.match(backendSource, /const MEMORY_PURGE_STANDBY_LIST: u32 = 4;/);
  assert.match(backendSource, /SeProfileSingleProcessPrivilege/);
  assert.match(backendSource, /SeIncreaseQuotaPrivilege/);
  // 全系统工作集无法排除单个进程：游戏在跑时跳过这一步。
  assert.match(backendSource, /emptied_working_sets: !protect_game && set_memory_list\(MEMORY_EMPTY_WORKING_SETS\)/);
});

test("非管理员时引导以管理员身份重启", () => {
  assert.match(backendSource, /fn is_launcher_elevated\(\) -> bool/);
  assert.match(backendSource, /fn restart_as_admin\(app: tauri::AppHandle\) -> Result<\(\), String>/);
  assert.match(backendSource, /ShellExecuteW\(None, w!\("runas"\)/);
  assert.match(appSource, /invoke<boolean>\("is_launcher_elevated"\)/);
  assert.match(appSource, /以管理员身份重启/);
  assert.match(appSource, /invoke\("restart_as_admin"\)/);
});

test("系统级结果行按实际执行结果拼文案", () => {
  const base = {
    trimmed: 12,
    skipped: 3,
    failed: 0,
    freedMb: 512,
    usedPercentBefore: 80,
    usedPercentAfter: 72,
    elapsedMs: 180,
    aggressive: true,
    emptiedWorkingSets: true,
    flushedFileCache: true,
    flushedModified: true,
    purgedStandby: true,
    protectedRunning: false,
  };
  assert.equal(
    memoryCleanupSystemDetail(base),
    "，系统缓存：已释放，文件缓存：已刷新，全系统工作集：已清空",
  );
  // 非管理员：系统级调用全部被拒绝，只刷了已修改页也如实说明。
  assert.equal(
    memoryCleanupSystemDetail({
      ...base,
      emptiedWorkingSets: false,
      flushedFileCache: false,
      flushedModified: true,
      purgedStandby: false,
    }),
    "，系统缓存：已刷盘、待机列表需要管理员权限，文件缓存：需要管理员权限，全系统工作集：需要管理员权限",
  );
  // 游戏运行中：全系统工作集被主动跳过，不说成权限问题。
  assert.equal(
    memoryCleanupSystemDetail({
      ...base,
      emptiedWorkingSets: false,
      protectedRunning: true,
    }),
    "，系统缓存：已释放，文件缓存：已刷新，全系统工作集：游戏运行中，已跳过",
  );
  // 常规模式不显示系统级结果。
  assert.equal(memoryCleanupSystemDetail({ ...base, aggressive: false }), "");
});

test("静默清理在游戏运行时只用常规模式", () => {
  assert.match(
    backendSource,
    /let aggressive = aggressive_cleanup\(\) && !protected_process_running\(\);/,
  );
  assert.match(backendSource, /clean_memory_with\(aggressive\)/);
  // 激进模式清的是系统缓存，重复太勤会拖慢读盘：间隔单独拉长。
  assert.match(backendSource, /const AGGRESSIVE_CLEANUP_COOLDOWN: Duration = Duration::from_secs\(300\);/);
  assert.match(backendSource, /let cooldown = if aggressive_cleanup\(\)/);
});
