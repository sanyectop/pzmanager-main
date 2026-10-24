import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { joinLaunchArguments } from "../src/launchArgs.ts";

const appSource = readFileSync(
  fileURLToPath(new URL("../src/App.tsx", import.meta.url)),
  "utf8",
);
const rustSource = readFileSync(
  fileURLToPath(new URL("../src-tauri/src/lib.rs", import.meta.url)),
  "utf8",
);

test("joins non-empty launch arguments with a single space", () => {
  assert.equal(
    joinLaunchArguments(["-agentlib:zbNative", " -- ", "", "-novid "]),
    "-agentlib:zbNative -- -novid",
  );
});

test("passes editable launch arguments from the details menu to the launcher", () => {
  assert.match(appSource, /const \[launchArguments, setLaunchArguments\] = useState<string\[\]>/);
  assert.match(appSource, /setLaunchArguments\(\(current\) => \[\.\.\.current, ""\]\)/);
  assert.match(appSource, /launchArgs: normalizedLaunchArguments/);
  assert.match(appSource, /joinLaunchArguments\(normalizedLaunchArguments\)/);
  assert.match(
    rustSource,
    /fn launch_project_zomboid\(\s*memory_mb: u64,\s*launch_args: Vec<String>,\s*fast_launch_dll_path: Option<String>,\s*\)/,
  );
  assert.match(rustSource, /steam:\/\/rungameid\/\{PROJECT_ZOMBOID_APP_ID\}/);
  assert.match(
    rustSource,
    /patch_steam_launch_options_at_paths\(\s*&steam_configs,\s*Some\(memory_mb\),\s*&effective_launch_args,?\s*\)/,
  );
  // 「模组加载器方案」是"不使用加载器"时，默认启动参数里的加载器 agent 会被摘掉，
  // 游戏自带启动文件那边也不写 agent（见 apply_launch_loader / strip_loader_agent_tokens）。
  assert.match(rustSource, /strip_loader_agent_tokens\(&mut effective_launch_args\)/);
  assert.match(
    rustSource,
    /apply_launch_loader\(\s*Some\(memory_mb\),\s*policy_agent,\s*fast_launch_agent\.as_deref\(\),?\s*\)/,
  );
  // 极速启动（实验性）：`-agentpath:` 只能在「开关打开 + 方案是 ZombieBuddy + DLL 真的存在」
  // 时写进游戏启动文件；否则必须传 None 把它摘掉（指向不存在的文件会让 JVM 直接起不来）。
  assert.match(appSource, /fastLaunchDllPath: overlayDllPath\.trim\(\) \|\| null/);
  assert.match(rustSource, /let fast_launch_agent = if fast_launch_enabled && policy_agent\.is_some\(\)/);
  assert.match(rustSource, /normalize_fast_launch_agent_tokens\(&mut tokens, fast_launch_agent\)/);
  assert.match(
    rustSource,
    /patch_steam_launch_options_at_paths\(&steam_configs, Some\(memory_mb\), &\[\]\)/,
  );
});

