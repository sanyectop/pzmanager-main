/** 游戏设置（options.ini）页面的纯逻辑：值类型分类、搜索过滤、布尔解析。
 *  单独成文件，方便 node:test 直接覆盖。 */

export type GameOptionValueKind = "bool" | "number" | "text";

/** 值类型：`true/false` 用开关，纯数字用数字输入，其余用文本输入。
 *  游戏里不少"数字"其实是档位（fontSize=3、fogQuality=0），所以数字用输入框而不是滑条。 */
export function classifyGameOptionValue(value: string): GameOptionValueKind {
  const trimmed = value.trim();
  if (trimmed === "true" || trimmed === "false") return "bool";
  if (trimmed !== "" && /^-?\d+(\.\d+)?$/.test(trimmed)) return "number";
  return "text";
}

/** 布尔项的当前值（大小写不敏感；非 true 一律按 false）。 */
export function gameOptionBoolean(value: string): boolean {
  return value.trim().toLowerCase() === "true";
}

/** 搜索过滤：按键名（以及可选的中文显示名）做大小写不敏感的子串匹配；
 *  空白查询返回原列表。传了 `labelOf` 时，中文名命中也算。 */
export function filterGameOptions<T extends { key: string }>(
  entries: T[],
  query: string,
  labelOf?: (key: string) => string | undefined,
): T[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return entries;
  return entries.filter((entry) => {
    if (entry.key.toLowerCase().includes(needle)) return true;
    const label = labelOf?.(entry.key);
    return Boolean(label && label.toLowerCase().includes(needle));
  });
}

/** 数字输入是否合法（允许空串表示清空，但提交前会拦）。 */
export function isValidGameOptionNumber(value: string): boolean {
  const trimmed = value.trim();
  return trimmed !== "" && /^-?\d+(\.\d+)?$/.test(trimmed);
}

export type GameOptionGroup<T> = { id: string; title: string; entries: T[] };

/** 把设置项按 `categories` 的顺序分组；空组丢弃（搜索后只留有命中的分组），
 *  未收录的键落到最后的「其他」组。组内保持原始文件顺序。 */
export function groupGameOptions<T extends { key: string }>(
  entries: T[],
  categories: { id: string; title: string; keys: string[] }[],
  categoryOf: (key: string) => string | undefined,
  fallback: { id: string; title: string },
): GameOptionGroup<T>[] {
  const buckets = new Map<string, T[]>(categories.map((category) => [category.id, []]));
  const other: T[] = [];
  for (const entry of entries) {
    const bucket = buckets.get(categoryOf(entry.key) ?? "");
    if (bucket) bucket.push(entry);
    else other.push(entry);
  }
  const groups: GameOptionGroup<T>[] = [];
  for (const category of categories) {
    const bucket = buckets.get(category.id) ?? [];
    if (bucket.length > 0) {
      groups.push({ id: category.id, title: category.title, entries: bucket });
    }
  }
  if (other.length > 0) {
    groups.push({ id: fallback.id, title: fallback.title, entries: other });
  }
  return groups;
}
