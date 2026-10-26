/** 游戏内存默认值:物理内存的一半,四舍五入到整 GB(如 23.8 GB 物理 → 12 GB)。 */
export const DEFAULT_MEMORY_ROUND_MB = 1024;

/** 滑杆下限,与后端 validate_memory_limit_mb 的下限保持一致。 */
export const MIN_MEMORY_MB = 1024;

/** 默认内存 = 物理内存的一半四舍五入到整 GB,但不低于 MIN_MEMORY_MB。 */
export function defaultMemoryLimitMb(totalMemoryMb: number): number {
  const rounded =
    Math.round(totalMemoryMb / 2 / DEFAULT_MEMORY_ROUND_MB) * DEFAULT_MEMORY_ROUND_MB;
  return Math.max(MIN_MEMORY_MB, rounded);
}
