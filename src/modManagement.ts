export type LuaModFilter = "enabled" | "disabled" | "b42";
export type JarModFilter = "all" | "enabled" | "disabled";

type LuaModLike = {
  name: string;
  enabled: boolean;
  loadOrder: number | null;
  hasB42: boolean;
};

export function sortLuaModsForDisplay<T extends LuaModLike>(mods: readonly T[]): T[] {
  return [...mods].sort((left, right) => {
    if (left.enabled !== right.enabled) {
      return left.enabled ? -1 : 1;
    }
    if (left.enabled && right.enabled) {
      const orderDifference =
        (left.loadOrder ?? Number.MAX_SAFE_INTEGER) -
        (right.loadOrder ?? Number.MAX_SAFE_INTEGER);
      if (orderDifference !== 0) {
        return orderDifference;
      }
    }
    return left.name.localeCompare(right.name, "zh-CN", { sensitivity: "base" });
  });
}

export function mergeItemsPreservingOrder<T>(
  current: readonly T[],
  latest: readonly T[],
  keyFor: (item: T) => string,
): T[] {
  const latestByKey = new Map(latest.map((item) => [keyFor(item), item]));
  const currentKeys = new Set(current.map(keyFor));
  const merged = current.map((item) => latestByKey.get(keyFor(item)) ?? item);

  for (const item of latest) {
    if (!currentKeys.has(keyFor(item))) {
      merged.push(item);
    }
  }
  return merged;
}

export function filterLuaMods<T extends Pick<LuaModLike, "enabled" | "hasB42">>(
  mods: readonly T[],
  filters: ReadonlySet<LuaModFilter>,
): T[] {
  const statusFilters = [
    filters.has("enabled") ? true : null,
    filters.has("disabled") ? false : null,
  ].filter((value): value is boolean => value !== null);

  return mods.filter((mod) => {
    const matchesStatus =
      statusFilters.length === 0 || statusFilters.some((enabled) => mod.enabled === enabled);
    const matchesB42 = !filters.has("b42") || mod.hasB42;
    return matchesStatus && matchesB42;
  });
}

export function jarModIsEnabled(mod: { decision: boolean | null; disabled: boolean }): boolean {
  return mod.decision === true && !mod.disabled;
}

export function filterJarMods<T extends { decision: boolean | null; disabled: boolean }>(
  mods: readonly T[],
  filter: JarModFilter,
): T[] {
  if (filter === "all") {
    return [...mods];
  }
  return mods.filter((mod) => jarModIsEnabled(mod) === (filter === "enabled"));
}

export function toggleSelection(current: ReadonlySet<string>, key: string): Set<string> {
  const next = new Set(current);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
  }
  return next;
}

export function setSelectionForKeys(
  current: ReadonlySet<string>,
  keys: readonly string[],
  selected: boolean,
): Set<string> {
  const next = new Set(current);
  for (const key of keys) {
    if (selected) {
      next.add(key);
    } else {
      next.delete(key);
    }
  }
  return next;
}

export function invertSelection(
  current: ReadonlySet<string>,
  keys: readonly string[],
): Set<string> {
  const next = new Set(current);
  for (const key of keys) {
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
  }
  return next;
}
