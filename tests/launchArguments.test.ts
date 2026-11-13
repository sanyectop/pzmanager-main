import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { joinLaunchArguments } from "../src/launchArgs.ts";

import { appSource } from "./sourceText.ts";
import { rustSource } from "./sourceText.ts";

test("joins non-empty launch arguments with a single space", () => {
  assert.equal(
    joinLaunchArguments(["-agentlib:zbNative", " -- ", "", "-novid "]),
    "-agentlib:zbNative -- -novid",
  );
});

test("passes editable launch arguments from the details menu to the launcher", () => {
  assert.match(appSource, /const \[launchArguments, setLaunchArguments\] = useState<string\[\]>/);
  // 参数编辑走 updateLaunchArguments：立即更新界面 + 防抖落盘 + 失败回滚。
  assert.match(appSource, /updateLaunchArguments\(\[\.\.\.launchArguments, ""\]\)/);
  assert.match(appSource, /invoke<string\[\]>\("save_launch_arguments", \{ arguments: trimmed \}\)/);
  // 保存成功**不能**用后端裁剪结果覆盖编辑区：后端会去掉空条目，刚点「新增」
  // 插入的空行会在 500ms 防抖后被立刻删掉，用户来不及输入（2026-11-11 实测）。
  assert.doesNotMatch(appSource, /setLaunchArguments\(saved\)/);
  assert.match(appSource, /setLaunchArguments\(previous\)/);
  assert.match(appSource, /launchArgs: normalizedLaunchArguments/);
  assert.match(appSource, /joinLaunchArguments\(normalizedLaunchArguments\)/);
  // 设置加载时恢复持久化的启动参数；没存过回落默认列表。
  assert.match(
    appSource,
    /settingsResult\.value\.launchArguments[\s\S]{0,120}: defaultLaunchArguments,/,
  );
  assert.match(
    rustSource,
    /fn launch_project_zomboid\(\s*memory_mb: u64,\s*launch_args: Vec<String>,\s*fast_launch_dll_path: Option<String>,\s*launch_source: Option<String>,?\s*\)/,
  );
  assert.match(rustSource, /steam:\/\/rungameid\/\{PROJECT_ZOMBOID_APP_ID\}/);
  assert.match(
    rustSource,
    /patch_steam_launch_options_at_paths\(\s*&steam_configs,\s*Some\(memory_mb\),\s*&effective_launch_args,\s*policy_agent,?\s*\)/,
  );
  // 「从游戏文件启动」：不走 ProjectZomboid64.exe（其原生 Steam 初始化在无
  // Steam 环境时直接 fatal error），改为按游戏目录 bat 的 java 命令行启动。
  assert.match(rustSource, /fn parse_bat_launch_command\(/);
  assert.match(rustSource, /ProjectZomboid64\.bat/);
  assert.match(rustSource, /jre64/);
  assert.match(rustSource, /Command::new\(java\)\s*\.current_dir\(game_path\)/);
  // Steam 没在跑时把创意工坊模组联接进 Zomboid\mods，否则游戏扫不到工坊模组；
  // Steam 启动 / 启动器重启时拆除，避免与 Steam 工坊扫描重复。
  assert.match(rustSource, /offline_workshop::sync_offline_workshop_links\(\)/);
  assert.match(rustSource, /offline_workshop::remove_offline_workshop_links\(\)/);
  assert.match(rustSource, /pzmanager-ws-/);
  assert.match(appSource, /launchSource: fromFiles \? "files" : null/);
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
    /patch_steam_launch_options_at_paths\(\s*&steam_configs,\s*Some\(memory_mb\),\s*&\[\],/,
  );
  // 后端持久化：字段带 serde(default)，命令注册，空条目裁剪。
  assert.match(rustSource, /launch_arguments: Vec<String>/);
  assert.match(rustSource, /fn save_launch_arguments\(arguments: Vec<String>\)/);
  assert.match(rustSource, /            save_launch_arguments,/);
});

