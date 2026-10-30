/**
 * 「内存清理」任务的采样与完成判定（设置 › 启动 › 立即清理一次）。
 *
 * 后端的清理本身只有 100–300 ms，但工作集页还要一段时间才真正回到系统：
 * 已用物理内存会先下降（清理生效），随后系统重新占用内存而回升。
 * 所以任务行的进度不跟后端返回值走，而是按物理内存的走势判断：
 * 先看到下降、再看到回升，才算清理完成；到完成时把期间观察到的
 * 最大下降量当成"已清理"多少 MB。
 */

/** 采样间隔：够密，又不至于让 IPC 变成常态开销。 */
export const MEMORY_CLEAN_POLL_MS = 200;
/** 一直没看到内存下降时的观察窗口：过了这段时间就认为这次没什么可清。 */
export const MEMORY_CLEAN_NO_DECREASE_GRACE_MS = 2500;
/** 内存停止下降后的观察窗口：等它回升；一直不回升（保持平稳）也按完成处理。 */
export const MEMORY_CLEAN_PLATEAU_GRACE_MS = 3000;
/** 兜底上限：异常情况下任务不能一直挂在「进行中」。 */
export const MEMORY_CLEAN_MAX_WAIT_MS = 15000;

export type MemoryCleanSample = {
  /** 已用物理内存（MB）。 */
  usedMb: number;
  /** 已用物理内存百分比（0–100）。 */
  usedPercent: number;
};

export type MemoryCleanTracker = {
  /** 清理开始前的已用内存，作为「已清理」的基准。 */
  baselineUsedMb: number;
  /** 期间观察到的最大释放量（MB）。 */
  peakFreedMb: number;
  /** 上一个采样点的已用内存（MB）。 */
  previousUsedMb: number;
  /** 是否已经观察到内存下降。 */
  sawDecrease: boolean;
  /** 内存先下降、随后回升：清理完成。 */
  done: boolean;
};

export type MemoryCleanTiming = {
  startedAt: number;
  /** 最近一次观察到「已清理量创新高」的时刻。 */
  lastDecreaseAt: number;
  now: number;
  /** 后端的清理命令是否已经返回（返回前不判定完成）。 */
  cleanReturned: boolean;
};

export function createMemoryCleanTracker(baselineUsedMb: number): MemoryCleanTracker {
  const baseline = Math.max(0, Math.round(baselineUsedMb));
  return {
    baselineUsedMb: baseline,
    peakFreedMb: 0,
    previousUsedMb: baseline,
    sawDecrease: false,
    done: false,
  };
}

/** 写入一个采样点，返回新的追踪状态（不改原对象，方便单测）。 */
export function applyMemoryCleanSample(
  tracker: MemoryCleanTracker,
  sample: MemoryCleanSample,
): MemoryCleanTracker {
  const usedMb = Math.max(0, Math.round(sample.usedMb));
  const freedMb = Math.max(0, tracker.baselineUsedMb - usedMb);
  const sawDecrease = tracker.sawDecrease || usedMb < tracker.previousUsedMb;
  return {
    ...tracker,
    previousUsedMb: usedMb,
    sawDecrease,
    peakFreedMb: Math.max(tracker.peakFreedMb, freedMb),
    // 已经降过、现在又比上一个采样点高：物理内存不再减少并开始回升，清理结束。
    done: tracker.done || (sawDecrease && usedMb > tracker.previousUsedMb),
  };
}

/** 任务行在清理过程中的说明文字。 */
export function memoryCleanProgressDetail(peakFreedMb: number): string {
  return peakFreedMb > 0 ? `当前已清理 ${peakFreedMb} MB` : "正在清理…";
}

/** 任务行在清理结束后的说明文字。 */
export function memoryCleanFinishedDetail(
  peakFreedMb: number,
  trimmed: number | null,
): string {
  if (peakFreedMb <= 0) {
    return "清理完成，内存占用没有明显下降";
  }
  return `清理完成，共释放 ${peakFreedMb} MB${trimmed !== null && trimmed > 0 ? `（${trimmed} 个进程）` : ""}`;
}

/**
 * 这一轮采样是否该收尾。
 *
 * 后端命令返回前只看兜底上限：还在遍历进程时内存的短暂回升不能算清理结束。
 * 返回后按「先降后升」判定；若始终没有下降或下降后一直平稳，则按观察窗口收尾。
 */
export function shouldFinishMemoryClean(
  tracker: MemoryCleanTracker,
  timing: MemoryCleanTiming,
): boolean {
  if (timing.now - timing.startedAt >= MEMORY_CLEAN_MAX_WAIT_MS) {
    return true;
  }
  if (!timing.cleanReturned) {
    return false;
  }
  if (tracker.done) {
    return true;
  }
  if (!tracker.sawDecrease) {
    return timing.now - timing.startedAt >= MEMORY_CLEAN_NO_DECREASE_GRACE_MS;
  }
  return timing.now - timing.lastDecreaseAt >= MEMORY_CLEAN_PLATEAU_GRACE_MS;
}
