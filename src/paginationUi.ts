/** 分页条要显示哪些页码（纯逻辑，供 node:test 覆盖）。
 *
 *  与原来创意工坊的规则一致：始终显示首页、末页、当前页及前后各一页，
 *  去重后升序排列；页码按钮之间不补省略号（按钮数量有限，直接看数字更快）。
 */
export function paginationPageNumbers(current: number, totalPages: number): number[] {
  if (!Number.isFinite(current) || !Number.isFinite(totalPages) || totalPages <= 0) {
    return [];
  }
  const total = Math.floor(totalPages);
  const page = Math.min(Math.max(1, Math.floor(current)), total);
  return Array.from(new Set([1, total, page, page - 1, page + 1]))
    .filter((candidate) => candidate >= 1 && candidate <= total)
    .sort((left, right) => left - right);
}
