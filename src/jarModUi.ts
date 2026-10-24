const JAR_SCAN_FRAME_COUNT = 4;

export function jarScanStatusLabel(frame: number, scanned: number, total: number): string {
  const normalizedFrame = ((frame % JAR_SCAN_FRAME_COUNT) + JAR_SCAN_FRAME_COUNT) % JAR_SCAN_FRAME_COUNT;
  return `后台扫描中${".".repeat(normalizedFrame)}：${scanned} / ${total}`;
}

export type JarModConflictRisk = "high" | "mediumHigh" | "mediumLow";

export type JarModConflict = {
  risk: JarModConflictRisk;
  annotation: string;
  methodName: string;
  modIds: [string, string];
};

type JarModConflictInput = {
  id: string;
  decision: boolean | null;
  patchMethods: Array<{
    annotation: string;
    methodName: string;
  }>;
};

export function analyzeJarModConflicts(mods: JarModConflictInput[]): JarModConflict[] {
  const approvedMods = mods.filter((mod) => mod.decision === true);
  const conflicts: JarModConflict[] = [];

  for (let leftIndex = 0; leftIndex < approvedMods.length; leftIndex += 1) {
    const left = approvedMods[leftIndex];
    for (let rightIndex = leftIndex + 1; rightIndex < approvedMods.length; rightIndex += 1) {
      const right = approvedMods[rightIndex];
      const rightMethods = new Set(
        right.patchMethods.map((patchMethod) => `${patchMethod.annotation}\u0000${patchMethod.methodName}`),
      );

      for (const leftMethod of left.patchMethods) {
        const key = `${leftMethod.annotation}\u0000${leftMethod.methodName}`;
        if (!rightMethods.has(key)) {
          continue;
        }

        const risk = conflictRiskForAnnotation(leftMethod.annotation);
        if (risk === null) {
          continue;
        }
        conflicts.push({
          risk,
          annotation: leftMethod.annotation,
          methodName: leftMethod.methodName,
          modIds: [left.id, right.id],
        });
      }
    }
  }

  const riskOrder: Record<JarModConflictRisk, number> = {
    high: 0,
    mediumHigh: 1,
    mediumLow: 2,
  };
  conflicts.sort(
    (left, right) =>
      riskOrder[left.risk] - riskOrder[right.risk] ||
      left.methodName.localeCompare(right.methodName) ||
      left.annotation.localeCompare(right.annotation) ||
      left.modIds.join("\u0000").localeCompare(right.modIds.join("\u0000")),
  );
  return conflicts;
}

function conflictRiskForAnnotation(annotation: string): JarModConflictRisk | null {
  switch (annotation) {
    case "Overwrite":
      return "high";
    case "Redirect":
    case "ModifyConstant":
      return "mediumHigh";
    case "Inject":
    case "ModifyVariable":
      return "mediumLow";
    default:
      return null;
  }
}

export function toggleJarModExpansion(expandedKeys: ReadonlySet<string>, key: string): Set<string> {
  const next = new Set(expandedKeys);
  if (next.has(key)) {
    next.delete(key);
  } else {
    next.add(key);
  }
  return next;
}
