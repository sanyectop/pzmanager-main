import assert from "node:assert/strict";
import test from "node:test";

import { appSource, rustSource } from "./sourceText.ts";

test("runs mod compatibility preflight before launching and can disable incompatible mods", () => {
  // 启动按钮先调预检；有缺失类引用时弹三选一（禁用并启动 / 仍然启动 / 取消）。
  assert.match(appSource, /invoke<ModCompatReport>\("check_mod_compatibility"\)/);
  assert.match(appSource, /模组与当前游戏版本不兼容/);
  assert.match(appSource, /禁用这些模组并启动/);
  assert.match(
    appSource,
    /invoke<number>\("disable_mods_for_launch", \{\s*modInfoPaths: report\.issues\.map\(\(issue\) => issue\.modInfoPath\)/,
  );
  // 「仍然启动」本次会话内不再重复询问。
  assert.match(appSource, /compatWarningIgnoredRef\.current = true/);
  // 预检失败只记日志，绝不阻断启动。
  assert.match(appSource, /模组兼容性预检失败（不影响启动）/);

  // 后端命令注册 + 实现在 mod_compat 模块。
  assert.match(rustSource, /check_mod_compatibility,/);
  assert.match(rustSource, /disable_mods_for_launch,/);
  assert.match(rustSource, /fn check_mod_compatibility\(\)/);
  assert.match(rustSource, /fn disable_mods_for_launch\(mod_info_paths: Vec<String>\)/);

  // 「被拒模组」误报修复：没有观察到任何 loading 行时不判定。
  assert.match(rustSource, /loaded_mod_ids\.is_empty\(\)/);
  assert.match(rustSource, /被拒模组判断跳过/);
});
