use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use image::{DynamicImage, ImageFormat, RgbaImage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;
use zip::ZipArchive;

mod downloader;
mod feedback;
mod memclean;
mod news;
mod news_translation;
mod overlay;
mod overlay_session;
mod resource;
mod steam_relay;
mod update;
mod workshop;

const PROJECT_ZOMBOID_APP_ID: &str = "108600";
const VIEWPOINT_MOD_ID: &str = "3809306528";
const ZOMBIE_BUDDY_MOD_ID: &str = "3619862853";
const PZ_OPTIMIZATION_MOD_ID: &str = "3805285544";
const ZOMBIE_BUDDY_WORKSHOP_ID: u64 = 3_619_862_853;
const ZOMBIE_BUDDY_AGENT: &str = "-agentlib:zbNative";
const ZOMBIE_BUDDY_POLICY_AGENT: &str = "-agentlib:zbNative=policy=allow-all";

/// 「模组加载器方案」的取值（设置 › 模组加载器 › 模组加载器方案）。
/// 目前只有 ZombieBuddy 和"不使用"；将来加新加载器时：
/// 在这里加一项 + 在 `mod_loader_policy_agent` 里给出它的启动期 agent
/// + 在 `is_zombie_buddy_agent_token` 旁加一个同类的识别函数。
const MOD_LOADER_SCHEMES: [&str; 2] = ["zombieBuddy", "none"];
const DEFAULT_MOD_LOADER_SCHEME: &str = "zombieBuddy";

/// 方案对应的启动期 JVM agent；`None` = 不使用加载器（不往启动配置里写 agent）。
fn mod_loader_policy_agent(scheme: &str) -> Option<&'static str> {
    match scheme {
        "zombieBuddy" => Some(ZOMBIE_BUDDY_POLICY_AGENT),
        _ => None,
    }
}

/// 大小写不敏感地归一化到上面的取值；不在列表里返回 None。
fn normalize_mod_loader_scheme(scheme: &str) -> Option<String> {
    let trimmed = scheme.trim();
    MOD_LOADER_SCHEMES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}

static NEXT_JAR_SCAN_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_WORKSHOP_BACKGROUND_SCAN_ID: AtomicU64 = AtomicU64::new(1);

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
fn parse_quoted_tokens(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '"' {
            index += 1;
            continue;
        }

        index += 1;
        let mut token = String::new();
        while index < chars.len() {
            match chars[index] {
                '"' => {
                    index += 1;
                    break;
                }
                '\\' if index + 1 < chars.len() => {
                    index += 1;
                    token.push(match chars[index] {
                        '\\' => '\\',
                        '"' => '"',
                        escaped => escaped,
                    });
                    index += 1;
                }
                character => {
                    token.push(character);
                    index += 1;
                }
            }
        }
        tokens.push(token);
    }

    tokens
}

fn parse_vdf_pairs(content: &str) -> Vec<(String, String)> {
    content
        .lines()
        .flat_map(|line| {
            parse_quoted_tokens(line)
                .chunks_exact(2)
                .map(|tokens| (tokens[0].clone(), tokens[1].clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn vdf_value(content: &str, key: &str) -> Option<String> {
    parse_vdf_pairs(content)
        .into_iter()
        .find_map(|(pair_key, value)| (pair_key == key).then_some(value))
}

fn contains_zombie_buddy_agent(value: &str) -> bool {
    value.split_whitespace().any(is_zombie_buddy_agent_token)
}

fn is_zombie_buddy_agent_token(token: &str) -> bool {
    token == ZOMBIE_BUDDY_AGENT || token.starts_with("-agentlib:zbNative=")
}

/// 极速启动用的 JVMTI agent 参数前缀：`-agentpath:<覆盖层 DLL 绝对路径>`。
///
/// 由 JVM 在启动最早期自己加载本 DLL（`Agent_OnLoad`），从而拿到 JVMTI 环境、
/// 在 ZombieBuddy 加载 `PatchEngine` 的那一刻就地包装它的 `collectPatches`。
/// 详见 `docs/architecture.md` §6.3 与 `docs/fast-launch-status.md`。
const FAST_LAUNCH_AGENT_PREFIX: &str = "-agentpath:";

/// 这个 token 是不是极速启动的 agent 参数（大小写不敏感：写进去的一定是标准大小写，
/// 但用户/历史版本手改出来的变体也必须能被摘掉——`-AgentPath:` 这种非法写法会让 JVM 直接起不来）。
fn is_fast_launch_agent_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed
        .get(..FAST_LAUNCH_AGENT_PREFIX.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(FAST_LAUNCH_AGENT_PREFIX))
}

/// 把极速启动的 agent 参数归一化成"**有且只有一条，而且必须在最前**"：
///
/// * `Some(path)` —— 保证 `-agentpath:<path>` 是 `vmArgs[0]`；
/// * `None` —— 把所有 `-agentpath:` 条目全部摘掉。关掉开关、切到别的方案、
///   或 DLL 路径解析不出来时都必须摘干净：**`-agentpath` 指向的文件不存在会让 JVM 直接启动失败**，
///   而"任何失败都回退原生扫描"是硬约束。
///
/// **为什么必须在最前**：JVMTI agent 按命令行顺序加载。ZombieBuddy 自己的
/// `-agentlib:zbNative` 会在它的 `Agent_OnLoad` 里就把 `PatchEngine` 加载好，而我们的类加载 hook
/// 只能在**自己的** `Agent_OnLoad` 里打开——排在它后面就只有 VMInit 时机可用，那时 `PatchEngine`
/// 早就定义完了（实测：hook 一次都没触发，游戏照旧走原生全量扫描）。
///
/// 返回是否真的改动了 tokens。
fn normalize_fast_launch_agent_tokens(tokens: &mut Vec<String>, agent_path: Option<&str>) -> bool {
    let wanted = agent_path.map(|path| format!("{FAST_LAUNCH_AGENT_PREFIX}{path}"));
    let mut normalized: Vec<String> = tokens
        .iter()
        .filter(|token| !is_fast_launch_agent_token(token))
        .cloned()
        .collect();
    if let Some(wanted) = wanted {
        normalized.insert(0, wanted);
    }
    if normalized == *tokens {
        return false;
    }
    *tokens = normalized;
    true
}

/// `.bat` 的 `SET _JAVA_OPTIONS=...` 是按空白切分后交给 JVM 的，
/// 路径里带空格时会被拆成两个参数（JVM 甚至可能直接启动失败），所以只有无空格的路径才写进 .bat。
fn fast_launch_agent_for_batch(agent_path: Option<&str>) -> Option<&str> {
    agent_path.filter(|path| !path.chars().any(char::is_whitespace))
}

/// 把 ZB agent 参数统一成带 `policy=allow-all` 的形式（只保留一条）。
///
/// 背景：`ProjectZomboid64.json` / `.bat` / Steam 启动参数里任一处的 `-agentlib:zbNative`
/// 都会先于其它来源被 JVM 加载，而 ZB 的策略只在第一次加载时锁定
/// （[`Loader.setPolicy`] 是 write-once，重复 agent 会被忽略）。
/// 因此要强制跳过"模组审批确认界面"，必须保证**第一个** agent 参数就带 `policy=allow-all`。
fn normalize_zombie_buddy_agent_tokens(tokens: &mut Vec<String>) -> bool {
    normalize_loader_agent_tokens(tokens, ZOMBIE_BUDDY_POLICY_AGENT)
}

/// 方案不使用加载器时，把启动参数里的加载器 agent 摘掉：
/// 前端默认启动参数带 `-agentlib:zbNative`，不摘就等于背着方案偷偷挂上。
/// 返回 true 表示确实摘掉了参数。
fn strip_loader_agent_tokens(tokens: &mut Vec<String>) -> bool {
    let original_length = tokens.len();
    tokens.retain(|token| !is_zombie_buddy_agent_token(token.trim()));
    tokens.len() != original_length
}

/// 把选定加载器的 agent 参数归一化成 `policy_agent`：已有的只保留一条且值正确，
/// 缺失时插到最前（必须在其它 agent 之前，否则 write-once 的策略会被先到的那条锁死）。
/// 识别"这行是不是加载器 agent"仍由各加载器自己的判定函数负责（见 `is_zombie_buddy_agent_token`）。
fn normalize_loader_agent_tokens(tokens: &mut Vec<String>, policy_agent: &str) -> bool {
    let agent_indices = tokens
        .iter()
        .enumerate()
        .filter_map(|(index, token)| is_zombie_buddy_agent_token(token).then_some(index))
        .collect::<Vec<_>>();

    let mut changed = false;
    if let Some(first) = agent_indices.first().copied() {
        if tokens[first] != policy_agent {
            tokens[first] = policy_agent.to_string();
            changed = true;
        }
        for index in agent_indices.into_iter().skip(1).rev() {
            tokens.remove(index);
            changed = true;
        }
    } else {
        tokens.insert(0, policy_agent.to_string());
        changed = true;
    }

    changed
}

fn steam_launch_value_has_both(value: &str) -> bool {
    let tokens: Vec<_> = value.split_whitespace().collect();
    tokens.iter().enumerate().any(|(index, token)| {
        is_zombie_buddy_agent_token(token) && tokens[index + 1..].contains(&"--")
    })
}

fn patch_steam_launch_options_value_with_memory(
    value: &str,
    memory_mb: Option<u64>,
    extra_args: &[String],
) -> (String, bool) {
    let mut tokens: Vec<String> = value.split_whitespace().map(str::to_string).collect();
    let original = tokens.clone();

    normalize_zombie_buddy_agent_tokens(&mut tokens);
    let agent_index = tokens
        .iter()
        .position(|token| is_zombie_buddy_agent_token(token))
        .unwrap_or(0);
    // ZB 要求 agent 参数后带 `--` 结束符。
    if !tokens[agent_index + 1..].iter().any(|token| token == "--") {
        tokens.insert(agent_index + 1, "--".to_string());
    }

    if let Some(memory_mb) = memory_mb {
        let target = memory_arg(memory_mb);
        let memory_indices = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| is_memory_arg(token).then_some(index))
            .collect::<Vec<_>>();
        if let Some(first) = memory_indices.first().copied() {
            tokens[first] = target;
            for index in memory_indices.into_iter().skip(1).rev() {
                tokens.remove(index);
            }
        } else {
            let separator = tokens
                .iter()
                .position(|token| token == "--")
                .unwrap_or(tokens.len());
            tokens.insert(separator, target);
        }
    }

    for argument in extra_args {
        for token in argument.split_whitespace() {
            if token == "--"
                || is_zombie_buddy_agent_token(token)
                || is_memory_arg(token)
                || tokens.iter().any(|existing| existing == token)
            {
                continue;
            }
            tokens.push(token.to_string());
        }
    }

    if tokens == original {
        (value.to_string(), false)
    } else {
        (tokens.join(" "), true)
    }
}

fn is_memory_arg(arg: &str) -> bool {
    arg.trim_start()
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("-xmx"))
}

fn memory_arg(memory_mb: u64) -> String {
    format!("-Xmx{memory_mb}m")
}

fn patch_json_launcher_content_with_memory(
    content: &str,
    memory_mb: Option<u64>,
) -> Result<(String, bool), String> {
    // 历史调用点与测试沿用这个签名：默认按 ZombieBuddy 方案处理。
    patch_json_launcher_with_agent(content, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

/// `policy_agent` = `None`（「模组加载器方案」选了不使用加载器）时只改内存，
/// 完全不往 `ProjectZomboid64.json` 里塞 agent 参数。
/// `fast_launch_agent` = `Some(DLL 绝对路径)` 时额外写一条 `-agentpath:<DLL>`（极速启动）。
fn patch_json_launcher_with_agent(
    content: &str,
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Result<(String, bool), String> {
    let mut launcher: serde_json::Value = serde_json::from_str(content)
        .map_err(|error| format!("解析 ProjectZomboid64.json 失败: {error}"))?;
    let object = launcher
        .as_object_mut()
        .ok_or_else(|| "ProjectZomboid64.json 顶层不是 JSON 对象".to_string())?;
    let vm_args_key = object
        .keys()
        .find(|key| key.eq_ignore_ascii_case("vmArgs"))
        .cloned()
        .unwrap_or_else(|| "vmArgs".to_string());
    let vm_args = object
        .entry(vm_args_key)
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let vm_args = vm_args
        .as_array_mut()
        .ok_or_else(|| "ProjectZomboid64.json 的 VMArgs 不是数组".to_string())?;

    let mut changed = false;

    if let Some(memory_mb) = memory_mb {
        let target = memory_arg(memory_mb);
        let original_length = vm_args.len();
        vm_args.retain(|arg| {
            arg.as_str()
                .is_none_or(|value| !is_memory_arg(value) || value.eq_ignore_ascii_case(&target))
        });
        changed |= vm_args.len() != original_length;
        if !vm_args.iter().any(|arg| {
            arg.as_str()
                .is_some_and(|value| value.eq_ignore_ascii_case(&target))
        }) {
            vm_args.insert(0, serde_json::Value::String(target));
            changed = true;
        }
    }

    // 方案选定的加载器：把它的 agent 参数归一化成 policy=allow-all，并且只保留一条。
    // 方案是"不使用加载器"时整段跳过——用户没启用加载器，就不该被写进启动配置。
    if let Some(policy_agent) = policy_agent {
        let agent_indices = vm_args
            .iter()
            .enumerate()
            .filter_map(|(index, arg)| {
                arg.as_str()
                    .is_some_and(contains_zombie_buddy_agent)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        if let Some(first) = agent_indices.first().copied() {
            if vm_args[first].as_str() != Some(policy_agent) {
                vm_args[first] = serde_json::Value::String(policy_agent.to_string());
                changed = true;
            }
            for index in agent_indices.into_iter().skip(1).rev() {
                vm_args.remove(index);
                changed = true;
            }
        } else {
            vm_args.insert(0, serde_json::Value::String(policy_agent.to_string()));
            changed = true;
        }
    }

    // 极速启动的 JVMTI agent：写进 `vmArgs` 末尾（不抢 ZombieBuddy agent 的"必须在最前"位置）。
    // 只有确认 vmArgs 全是字符串时才改写，避免破坏用户手写的嵌套结构。
    let vm_args_all_strings = vm_args.iter().all(|arg| arg.is_string());
    let has_fast_launch_token = vm_args
        .iter()
        .any(|arg| arg.as_str().is_some_and(is_fast_launch_agent_token));
    if vm_args_all_strings && (fast_launch_agent.is_some() || has_fast_launch_token) {
        let mut tokens = vm_args
            .iter()
            .filter_map(|arg| arg.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        if normalize_fast_launch_agent_tokens(&mut tokens, fast_launch_agent) {
            *vm_args = tokens.into_iter().map(serde_json::Value::String).collect();
            changed = true;
        }
    }

    if !changed {
        return Ok((content.to_string(), false));
    }

    let patched = serde_json::to_string_pretty(&launcher)
        .map(|json| format!("{json}\n"))
        .map_err(|error| format!("写入 ProjectZomboid64.json 失败: {error}"))?;
    Ok((patched, true))
}

fn patch_json_launcher_content(content: &str) -> Result<(String, bool), String> {
    patch_json_launcher_content_with_memory(content, None)
}

fn patch_batch_content_with_memory(content: &str, memory_mb: Option<u64>) -> (String, bool) {
    // 历史调用点与测试沿用这个签名：默认按 ZombieBuddy 方案处理。
    patch_batch_with_agent(content, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

/// `policy_agent` = `None`（「模组加载器方案」选了不使用加载器）时只改内存，
/// 不往 `ProjectZomboid64.bat` 里塞 agent 参数。
/// `fast_launch_agent` = `Some(DLL 绝对路径)` 时额外写一条 `-agentpath:<DLL>`（极速启动）。
fn patch_batch_with_agent(
    content: &str,
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> (String, bool) {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = content.ends_with('\n');
    let normalized = content.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(str::to_string).collect();
    if had_trailing_newline {
        lines.pop();
    }

    for line in &mut lines {
        let trimmed = line.trim_start();
        if trimmed
            .to_ascii_uppercase()
            .starts_with("SET _JAVA_OPTIONS=")
        {
            let Some(equal_index) = line.find('=') else {
                continue;
            };
            let current_options = line[equal_index + 1..].trim();
            let mut tokens: Vec<String> = current_options
                .split_whitespace()
                .map(str::to_string)
                .collect();
            if let Some(memory_mb) = memory_mb {
                let target = memory_arg(memory_mb);
                tokens.retain(|token| !is_memory_arg(token) || token.eq_ignore_ascii_case(&target));
                if !tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(&target))
                {
                    // 有加载器 agent 时插到它后面，没有就追加到末尾。
                    // 注意用 len() 兜底：上面把旧内存参数删掉后 tokens 可能是空的，
                    // 直接 insert(1) 会越界 panic（旧实现就有这个雷）。
                    let insert_at = tokens
                        .iter()
                        .position(|token| is_zombie_buddy_agent_token(token))
                        .map_or(tokens.len(), |index| index + 1);
                    tokens.insert(insert_at, target);
                }
            }
            // 方案启用加载器时把它的 agent 归一化；不使用加载器时整段跳过。
            if let Some(policy_agent) = policy_agent {
                normalize_loader_agent_tokens(&mut tokens, policy_agent);
            }
            // 极速启动的 `-agentpath:`（也负责"摘掉旧的"这条路径）。
            normalize_fast_launch_agent_tokens(&mut tokens, fast_launch_agent);
            let updated_options = tokens.join(" ");
            if updated_options == current_options {
                return (content.to_string(), false);
            }
            let prefix = &line[..equal_index + 1];
            *line = format!("{prefix}{updated_options}");
            let patched = lines.join(newline);
            return (
                if had_trailing_newline {
                    format!("{patched}{newline}")
                } else {
                    patched
                },
                true,
            );
        }
    }

    // 原文件没有 _JAVA_OPTIONS 行时新建：启用加载器就带上它的 agent，否则只写内存。
    let mut tokens = Vec::new();
    if let Some(policy_agent) = policy_agent {
        tokens.push(policy_agent.to_string());
    }
    if let Some(memory_mb) = memory_mb {
        tokens.push(memory_arg(memory_mb));
    }
    if let Some(agent_path) = fast_launch_agent {
        normalize_fast_launch_agent_tokens(&mut tokens, Some(agent_path));
    }
    if tokens.is_empty() {
        return (content.to_string(), false);
    }
    let options = tokens.join(" ");
    lines.insert(0, format!("SET _JAVA_OPTIONS={options}"));
    let patched = lines.join(newline);
    (
        if had_trailing_newline {
            format!("{patched}{newline}")
        } else {
            patched
        },
        true,
    )
}

fn patch_batch_content(content: &str) -> (String, bool) {
    patch_batch_content_with_memory(content, None)
}

fn validate_memory_limit_mb(memory_mb: u64, total_memory_mb: u64) -> Result<(), String> {
    const MIN_MEMORY_MB: u64 = 1024;

    if memory_mb < MIN_MEMORY_MB {
        return Err(format!("内存限制不能低于 {MIN_MEMORY_MB} MB"));
    }
    if memory_mb > total_memory_mb {
        return Err(format!(
            "内存限制不能超过系统物理内存（{}）",
            format_memory_mb(total_memory_mb)
        ));
    }
    Ok(())
}

fn format_memory_mb(memory_mb: u64) -> String {
    if memory_mb % 1024 == 0 {
        format!("{} GB", memory_mb / 1024)
    } else {
        format!("{:.1} GB", memory_mb as f64 / 1024.0)
    }
}

fn quoted_key(line: &str, key: &str) -> bool {
    let tokens = parse_quoted_tokens(line);
    tokens.first().is_some_and(|value| value == key)
}

fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |delta, character| match character {
        '{' => delta + 1,
        '}' => delta - 1,
        _ => delta,
    })
}

fn line_indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn replace_vdf_value(line: &str, value: &str) -> Option<String> {
    let first_quote = line.find('"')?;
    let key_end = line[first_quote + 1..].find('"')? + first_quote + 1;
    let value_start = line[key_end + 1..].find('"')? + key_end + 1;
    let value_end = line[value_start + 1..].find('"')? + value_start + 1;
    let escaped_value = value.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!(
        "{}{}{}",
        &line[..value_start + 1],
        escaped_value,
        &line[value_end..]
    ))
}

fn patch_steam_launch_options_content_with_memory(
    content: &str,
    memory_mb: Option<u64>,
    extra_args: &[String],
) -> Result<(String, bool), String> {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = content.ends_with('\n');
    let normalized = content.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(str::to_string).collect();
    if had_trailing_newline {
        lines.pop();
    }

    let mut output = Vec::with_capacity(lines.len() + 4);
    let mut app_depth: Option<i32> = None;
    let mut app_found = false;
    let mut launch_options_found = false;
    let mut changed = false;

    for line in lines {
        if app_depth.is_none() && quoted_key(&line, PROJECT_ZOMBOID_APP_ID) {
            app_found = true;
            app_depth = Some(brace_delta(&line));
            output.push(line);
            continue;
        }

        if let Some(depth) = app_depth {
            let next_depth = depth + brace_delta(&line);
            if quoted_key(&line, "LaunchOptions") && depth > 0 {
                let current_value = parse_quoted_tokens(&line)
                    .get(1)
                    .cloned()
                    .unwrap_or_default();
                let (options, options_changed) = patch_steam_launch_options_value_with_memory(
                    &current_value,
                    memory_mb,
                    extra_args,
                );
                if !options_changed {
                    launch_options_found = true;
                    output.push(line);
                } else {
                    output.push(
                        replace_vdf_value(&line, &options)
                            .ok_or_else(|| "Steam LaunchOptions 格式无效".to_string())?,
                    );
                    launch_options_found = true;
                    changed = true;
                }
            } else if next_depth <= 0 && !launch_options_found {
                let indentation = line_indentation(&line);
                let launch_options =
                    patch_steam_launch_options_value_with_memory("", memory_mb, extra_args).0;
                output.push(format!(
                    "{indentation}    \"LaunchOptions\" \"{launch_options}\""
                ));
                output.push(line);
                launch_options_found = true;
                changed = true;
            } else {
                output.push(line);
            }

            if next_depth <= 0 {
                app_depth = None;
            } else {
                app_depth = Some(next_depth);
            }
            continue;
        }

        output.push(line);
    }

    if !app_found {
        let mut apps_depth: Option<i32> = None;
        let mut inserted = false;
        let mut with_app = Vec::with_capacity(output.len() + 4);

        for line in output {
            if apps_depth.is_none() && quoted_key(&line, "Apps") {
                apps_depth = Some(brace_delta(&line));
                with_app.push(line);
                continue;
            }

            if let Some(depth) = apps_depth {
                let next_depth = depth + brace_delta(&line);
                if next_depth <= 0 && !inserted {
                    let indentation = line_indentation(&line);
                    with_app.push(format!("{indentation}    \"{PROJECT_ZOMBOID_APP_ID}\""));
                    with_app.push(format!("{indentation}    {{"));
                    let launch_options =
                        patch_steam_launch_options_value_with_memory("", memory_mb, extra_args).0;
                    with_app.push(format!(
                        "{indentation}        \"LaunchOptions\" \"{launch_options}\""
                    ));
                    with_app.push(format!("{indentation}    }}"));
                    inserted = true;
                    changed = true;
                }
                with_app.push(line);
                if next_depth <= 0 {
                    apps_depth = None;
                } else {
                    apps_depth = Some(next_depth);
                }
                continue;
            }

            with_app.push(line);
        }

        if !inserted {
            return Err("Steam localconfig.vdf 中找不到 Apps 节点".to_string());
        }
        output = with_app;
    }

    let patched = output.join(newline);
    Ok((
        if had_trailing_newline {
            format!("{patched}{newline}")
        } else {
            patched
        },
        changed,
    ))
}

fn patch_steam_launch_options_content(content: &str) -> Result<(String, bool), String> {
    patch_steam_launch_options_content_with_memory(content, None, &[])
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ZombieBuddyStatus {
    available: bool,
    installed: bool,
    source_path: Option<String>,
    missing_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ZombieBuddyInstallResult {
    changed: bool,
    already_installed: bool,
    backup_path: Option<String>,
}

#[derive(Debug, Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct PzOptimizationStatus {
    available: bool,
    installed: bool,
    source_path: Option<String>,
    installer_path: Option<String>,
    installed_revision: Option<String>,
    available_revision: Option<String>,
    missing_components: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PzOptimizationInstallResult {
    changed: bool,
    already_installed: bool,
    source_path: String,
    installed_revision: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorkshopModStatus {
    name: String,
    workshop_id: String,
    subscribed: bool,
    source_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Viewpoint3dStatus {
    ready: bool,
    mods: Vec<WorkshopModStatus>,
    missing_components: Vec<String>,
}

fn zombie_buddy_source_from_library(library: &Path) -> Option<PathBuf> {
    let source_path = library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID)
        .join(ZOMBIE_BUDDY_MOD_ID)
        .join("mods")
        .join("ZombieBuddy");

    source_path.is_dir().then_some(source_path)
}

fn zombie_buddy_source_has_files(source_path: &Path) -> bool {
    source_path.join("libs").join("zbNative.dll").is_file()
        && source_path.join("libs").join("ZombieBuddy.jar").is_file()
}

fn zombie_buddy_source_from_libraries(libraries: &[PathBuf]) -> Option<PathBuf> {
    let mut first_available = None;

    for library in libraries {
        let Some(source_path) = zombie_buddy_source_from_library(library) else {
            continue;
        };
        if zombie_buddy_source_has_files(&source_path) {
            return Some(source_path);
        }
        if first_available.is_none() {
            first_available = Some(source_path);
        }
    }

    first_available
}

fn workshop_content_path(library: &Path, workshop_id: &str) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID)
        .join(workshop_id)
}

fn workshop_content_is_available(path: &Path) -> bool {
    path.is_dir()
        && fs::read_dir(path)
            .ok()
            .is_some_and(|mut entries| entries.next().is_some())
}

fn viewpoint_source_from_libraries(libraries: &[PathBuf]) -> Option<PathBuf> {
    libraries
        .iter()
        .map(|library| workshop_content_path(library, VIEWPOINT_MOD_ID))
        .find(|path| workshop_content_is_available(path))
}

fn workshop_mod_status(
    name: &str,
    workshop_id: &str,
    libraries: &[PathBuf],
    source_path: Option<PathBuf>,
) -> WorkshopModStatus {
    let content_path = libraries
        .iter()
        .map(|library| workshop_content_path(library, workshop_id))
        .find(|path| workshop_content_is_available(path));
    WorkshopModStatus {
        name: name.to_string(),
        workshop_id: workshop_id.to_string(),
        subscribed: content_path.is_some(),
        source_path: source_path
            .or(content_path)
            .map(|path| path.to_string_lossy().into_owned()),
    }
}

fn viewpoint_three_d_status(_game_path: Option<&Path>, libraries: &[PathBuf]) -> Viewpoint3dStatus {
    let zombie_buddy_source = zombie_buddy_source_from_libraries(libraries);
    let viewpoint_source = viewpoint_source_from_libraries(libraries);
    let mods = vec![
        workshop_mod_status(
            "Project Viewpoint",
            VIEWPOINT_MOD_ID,
            libraries,
            viewpoint_source.clone(),
        ),
        workshop_mod_status(
            "ZombieBuddy",
            ZOMBIE_BUDDY_MOD_ID,
            libraries,
            zombie_buddy_source.clone(),
        ),
    ];
    let mut missing_components = mods
        .iter()
        .filter(|item| !item.subscribed)
        .map(|item| format!("订阅并下载 {}", item.name))
        .collect::<Vec<_>>();

    if zombie_buddy_source.is_none() {
        if mods[1].subscribed {
            missing_components.push("ZombieBuddy/libs 文件".to_string());
        }
    } else if !zombie_buddy_source_has_files(zombie_buddy_source.as_deref().unwrap()) {
        missing_components.push("ZombieBuddy/libs 文件".to_string());
    }

    Viewpoint3dStatus {
        ready: missing_components.is_empty(),
        mods,
        missing_components,
    }
}

fn steam_user_data_id(profile: &SteamProfile) -> Option<String> {
    const STEAM_ID_64_BASE: u64 = 76_561_197_960_265_728;

    profile
        .steam_id
        .parse::<u64>()
        .ok()?
        .checked_sub(STEAM_ID_64_BASE)
        .filter(|account_id| *account_id <= u32::MAX as u64)
        .map(|account_id| account_id.to_string())
}

fn steam_config_paths(steam_root: &Path, profile: Option<&SteamProfile>) -> Vec<PathBuf> {
    let userdata = steam_root.join("userdata");

    if let Some(profile) = profile {
        let Some(account_id) = steam_user_data_id(profile) else {
            return Vec::new();
        };
        let path = userdata
            .join(account_id)
            .join("config")
            .join("localconfig.vdf");
        return path.is_file().then_some(path).into_iter().collect();
    }

    Vec::new()
}

fn current_unix_timestamp() -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .map_err(|error| format!("获取时间失败: {error}"))
}

fn write_file(path: &Path, content: &[u8]) -> Result<(), String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("无效文件路径: {}", path.display()))?;
    let suffix = current_unix_timestamp()?;
    let temp_path = path.with_file_name(format!(
        ".{file_name}.pzmanager-{}-{suffix}.tmp",
        std::process::id()
    ));
    let displaced_path = path.with_file_name(format!(
        ".{file_name}.pzmanager-{}-{suffix}.old",
        std::process::id()
    ));

    fs::write(&temp_path, content)
        .map_err(|error| format!("写入 {} 失败: {error}", path.display()))?;

    if path.exists() {
        fs::rename(path, &displaced_path).map_err(|error| {
            let _ = fs::remove_file(&temp_path);
            format!("暂存原文件 {} 失败: {error}", path.display())
        })?;
        if let Err(error) = fs::rename(&temp_path, path) {
            let restore_error = fs::rename(&displaced_path, path).err();
            let _ = fs::remove_file(&temp_path);
            return Err(match restore_error {
                Some(restore_error) => format!(
                    "替换 {} 失败: {error}；恢复原文件失败: {restore_error}，原文件保留在 {}",
                    path.display(),
                    displaced_path.display()
                ),
                None => format!("替换 {} 失败: {error}", path.display()),
            });
        }
        let _ = fs::remove_file(displaced_path);
    } else if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!("创建 {} 失败: {error}", path.display()));
    }

    Ok(())
}

fn apply_file_transaction(files: Vec<(PathBuf, Vec<u8>)>) -> Result<(), String> {
    let mut snapshots = Vec::with_capacity(files.len());
    for (path, _) in &files {
        let existed = path
            .try_exists()
            .map_err(|error| format!("检查 {} 失败: {error}", path.display()))?;
        let original = if existed {
            Some(fs::read(path).map_err(|error| format!("读取 {} 失败: {error}", path.display()))?)
        } else {
            None
        };
        snapshots.push((path.clone(), original));
    }

    for (index, (path, content)) in files.iter().enumerate() {
        if let Err(write_error) = write_file(path, content) {
            let mut rollback_errors = Vec::new();
            for (restore_path, original) in snapshots[..index].iter().rev() {
                let result = match original {
                    Some(original) => write_file(restore_path, original),
                    None => fs::remove_file(restore_path)
                        .map_err(|error| format!("移除 {} 失败: {error}", restore_path.display())),
                };
                if let Err(error) = result {
                    rollback_errors.push(error);
                }
            }

            if rollback_errors.is_empty() {
                return Err(write_error);
            }
            return Err(format!(
                "{write_error}；回滚未完全成功: {}",
                rollback_errors.join("；")
            ));
        }
    }

    Ok(())
}

fn backup_file(path: &Path, backup_dir: &Path, name: &str) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }

    fs::create_dir_all(backup_dir).map_err(|error| format!("创建备份目录失败: {error}"))?;
    fs::copy(path, backup_dir.join(name))
        .map_err(|error| format!("备份 {} 失败: {error}", path.display()))?;
    Ok(())
}

fn json_launcher_has_agent(content: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(content)
        .ok()
        .and_then(|value| {
            value
                .as_object()
                .and_then(|object| {
                    object
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("vmArgs"))
                        .map(|(_, value)| value)
                })
                .and_then(serde_json::Value::as_array)
                .map(|args| {
                    args.iter()
                        .any(|arg| arg.as_str().is_some_and(contains_zombie_buddy_agent))
                })
        })
        .unwrap_or(false)
}

fn batch_has_agent(content: &str) -> bool {
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .to_ascii_uppercase()
            .starts_with("SET _JAVA_OPTIONS=")
            && line
                .split_once('=')
                .is_some_and(|(_, value)| contains_zombie_buddy_agent(value))
    })
}

fn steam_config_has_both(content: &str) -> bool {
    let mut app_depth: Option<i32> = None;

    for line in content.lines() {
        if app_depth.is_none() && quoted_key(line, PROJECT_ZOMBOID_APP_ID) {
            app_depth = Some(brace_delta(line));
            continue;
        }

        if let Some(depth) = app_depth {
            let next_depth = depth + brace_delta(line);
            if quoted_key(line, "LaunchOptions") {
                let value = parse_quoted_tokens(line)
                    .get(1)
                    .cloned()
                    .unwrap_or_default();
                if steam_launch_value_has_both(&value) {
                    return true;
                }
            }

            if next_depth <= 0 {
                app_depth = None;
            } else {
                app_depth = Some(next_depth);
            }
        }
    }

    false
}

fn zombie_buddy_status(
    game_path: Option<&Path>,
    source_path: Option<&Path>,
    steam_configs: &[PathBuf],
) -> ZombieBuddyStatus {
    let mut missing_components = Vec::new();
    let available = source_path.is_some_and(zombie_buddy_source_has_files);

    if let Some(game_path) = game_path {
        if !game_path.join("zbNative.dll").is_file() {
            missing_components.push("zbNative.dll".to_string());
        }
        if !game_path.join("ZombieBuddy.jar").is_file() {
            missing_components.push("ZombieBuddy.jar".to_string());
        }

        let json_ready = fs::read_to_string(game_path.join("ProjectZomboid64.json"))
            .map(|content| json_launcher_has_agent(&content))
            .unwrap_or(false);
        if !json_ready {
            missing_components.push("ProjectZomboid64.json".to_string());
        }

        let batch_ready = fs::read_to_string(game_path.join("ProjectZomboid64.bat"))
            .map(|content| batch_has_agent(&content))
            .unwrap_or(false);
        if !batch_ready {
            missing_components.push("ProjectZomboid64.bat".to_string());
        }

        let steam_ready = !steam_configs.is_empty()
            && steam_configs.iter().all(|path| {
                fs::read_to_string(path)
                    .map(|content| steam_config_has_both(&content))
                    .unwrap_or(false)
            });
        if !steam_ready {
            missing_components.push("Steam LaunchOptions".to_string());
        }
    } else {
        missing_components.push("Project Zomboid 安装目录".to_string());
    }

    ZombieBuddyStatus {
        available,
        installed: game_path.is_some() && missing_components.is_empty(),
        source_path: source_path.map(|path| path.to_string_lossy().into_owned()),
        missing_components,
    }
}

fn simple_properties(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn pz_optimization_build_info(classes_path: &Path) -> Option<HashMap<String, String>> {
    let content =
        fs::read_to_string(classes_path.join("pzopt").join("build-info.properties")).ok()?;
    let properties = simple_properties(&content);
    properties.get("revision")?;
    Some(properties)
}

fn pz_optimization_files_complete(classes_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(classes_path.join("pzopt-files.txt")) else {
        return false;
    };
    let files = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    !files.is_empty()
        && files
            .iter()
            .all(|relative| classes_path.join(relative).is_file())
}

fn project_zomboid_revision(game_path: &Path) -> Option<String> {
    let file = fs::File::open(game_path.join("projectzomboid.jar")).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("zombie/GitVersion.class").ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    regex::Regex::new(r"\b[0-9a-f]{10}\b")
        .ok()?
        .find(&text)
        .map(|value| value.as_str().to_string())
}

fn pz_optimization_source_from_libraries(
    libraries: &[PathBuf],
    game_revision: Option<&str>,
) -> Option<(PathBuf, PathBuf, String)> {
    let mut best: Option<(PathBuf, PathBuf, String, u64)> = None;

    for library in libraries {
        let mod_root = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(PZ_OPTIMIZATION_MOD_ID)
            .join("mods")
            .join("PZ_Optimization");
        let Ok(entries) = fs::read_dir(&mod_root) else {
            continue;
        };

        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let version_root = entry.path();
            let classes_path = version_root.join("pzopt-classes");
            let installer_path = version_root.join("install.ps1");
            if !classes_path.is_dir() || !installer_path.is_file() {
                continue;
            }
            let Some(info) = pz_optimization_build_info(&classes_path) else {
                continue;
            };
            let Some(revision) = info.get("revision").cloned() else {
                continue;
            };
            if game_revision.is_some_and(|current| current != revision) {
                continue;
            }
            if !pz_optimization_files_complete(&classes_path) {
                continue;
            }
            let built = info
                .get("built")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            if best
                .as_ref()
                .is_none_or(|(_, _, _, best_built)| built > *best_built)
            {
                best = Some((classes_path, installer_path, revision, built));
            }
        }
    }

    best.map(|(classes, installer, revision, _)| (classes, installer, revision))
}

fn pz_optimization_status(game_path: Option<&Path>, libraries: &[PathBuf]) -> PzOptimizationStatus {
    let game_revision = game_path.and_then(project_zomboid_revision);
    let source = pz_optimization_source_from_libraries(libraries, game_revision.as_deref());
    let installed_revision = game_path
        .and_then(|path| fs::read_to_string(path.join("pzopt").join("build-info.properties")).ok())
        .and_then(|content| simple_properties(&content).get("revision").cloned());
    let installed = game_path.is_some()
        && (game_path.is_some_and(|path| path.join("pzopt-installed.txt").is_file())
            || installed_revision.is_some());

    let mut missing_components = Vec::new();
    if game_path.is_none() {
        missing_components.push("Project Zomboid 安装目录".to_string());
    }
    if source.is_none() {
        missing_components.push(if game_revision.is_some() {
            "匹配当前游戏版本的 PZ_Optimization Workshop 文件".to_string()
        } else {
            "PZ_Optimization Workshop 文件".to_string()
        });
    }

    let (source_path, installer_path, available_revision) =
        source.map_or((None, None, None), |(classes, installer, revision)| {
            (
                Some(classes.to_string_lossy().into_owned()),
                Some(installer.to_string_lossy().into_owned()),
                Some(revision),
            )
        });

    PzOptimizationStatus {
        available: source_path.is_some(),
        installed,
        source_path,
        installer_path,
        installed_revision,
        available_revision,
        missing_components,
    }
}

fn install_zombie_buddy_at_paths(
    game_path: &Path,
    source_path: &Path,
    steam_configs: &[PathBuf],
) -> Result<ZombieBuddyInstallResult, String> {
    if !game_path.is_dir() {
        return Err("Project Zomboid 安装目录不存在".to_string());
    }
    if steam_configs.is_empty() {
        return Err("找不到 Steam localconfig.vdf".to_string());
    }

    let before = zombie_buddy_status(Some(game_path), Some(source_path), steam_configs);
    if before.installed {
        return Ok(ZombieBuddyInstallResult {
            changed: false,
            already_installed: true,
            backup_path: None,
        });
    }

    let source_dll = source_path.join("libs").join("zbNative.dll");
    let source_jar = source_path.join("libs").join("ZombieBuddy.jar");
    if !source_dll.is_file() || !source_jar.is_file() {
        return Err("Workshop 中缺少 ZombieBuddy 的核心文件".to_string());
    }

    let json_path = game_path.join("ProjectZomboid64.json");
    let json_content = fs::read_to_string(&json_path)
        .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
    let (json_patched, _) = patch_json_launcher_content(&json_content)?;

    let batch_path = game_path.join("ProjectZomboid64.bat");
    let batch_content = fs::read_to_string(&batch_path)
        .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
    let (batch_patched, _) = patch_batch_content(&batch_content);

    let mut patched_steam_configs = Vec::with_capacity(steam_configs.len());
    for config_path in steam_configs {
        let config_content = fs::read_to_string(config_path)
            .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (config_patched, changed) = patch_steam_launch_options_content(&config_content)?;
        patched_steam_configs.push((config_path.clone(), config_patched, changed));
    }

    let timestamp = current_unix_timestamp()?;
    let backup_dir = game_path.join(".pzmanager-backups").join(timestamp);
    backup_file(&game_path.join("zbNative.dll"), &backup_dir, "zbNative.dll")?;
    backup_file(
        &game_path.join("ZombieBuddy.jar"),
        &backup_dir,
        "ZombieBuddy.jar",
    )?;
    backup_file(
        &game_path.join("ProjectZomboid64.json"),
        &backup_dir,
        "ProjectZomboid64.json",
    )?;
    backup_file(
        &game_path.join("ProjectZomboid64.bat"),
        &backup_dir,
        "ProjectZomboid64.bat",
    )?;
    for (index, (config_path, _, changed)) in patched_steam_configs.iter().enumerate() {
        if *changed {
            backup_file(
                config_path,
                &backup_dir,
                &format!("steam-localconfig-{index}.vdf"),
            )?;
        }
    }

    let mut files = vec![
        (
            game_path.join("zbNative.dll"),
            fs::read(&source_dll).map_err(|error| format!("读取 zbNative.dll 失败: {error}"))?,
        ),
        (
            game_path.join("ZombieBuddy.jar"),
            fs::read(&source_jar).map_err(|error| format!("读取 ZombieBuddy.jar 失败: {error}"))?,
        ),
        (json_path, json_patched.into_bytes()),
        (batch_path, batch_patched.into_bytes()),
    ];
    for (config_path, config_patched, changed) in patched_steam_configs {
        if changed {
            files.push((config_path, config_patched.into_bytes()));
        }
    }

    apply_file_transaction(files)?;

    Ok(ZombieBuddyInstallResult {
        changed: true,
        already_installed: false,
        backup_path: Some(backup_dir.to_string_lossy().into_owned()),
    })
}

fn regular_files_in_directory(directory: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("读取 {} 失败: {error}", directory.display()))?;
    let mut files = Vec::new();

    for entry in entries {
        let path = entry
            .map_err(|error| format!("读取 {} 目录项失败: {error}", directory.display()))?
            .path();
        if !is_regular_file(&path) {
            continue;
        }
        let content =
            fs::read(&path).map_err(|error| format!("读取 {} 失败: {error}", path.display()))?;
        files.push((path, content));
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn configure_viewpoint_three_d_at_paths(
    game_path: &Path,
    libraries: &[PathBuf],
    steam_configs: &[PathBuf],
) -> Result<ZombieBuddyInstallResult, String> {
    if !game_path.is_dir() {
        return Err("Project Zomboid 安装目录不存在".to_string());
    }

    let status = viewpoint_three_d_status(Some(game_path), libraries);
    if !status.ready {
        return Err(format!(
            "请先订阅并下载：{}",
            status.missing_components.join("、")
        ));
    }

    let source_path = zombie_buddy_source_from_libraries(libraries)
        .ok_or_else(|| "找不到 ZombieBuddy Workshop 文件".to_string())?;
    let library_files = regular_files_in_directory(&source_path.join("libs"))?;
    if library_files.is_empty() {
        return Err("ZombieBuddy/libs 目录中没有可复制的文件".to_string());
    }

    let json_path = game_path.join("ProjectZomboid64.json");
    let json_content = fs::read_to_string(&json_path)
        .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
    let (json_patched, _) = patch_json_launcher_content(&json_content)?;

    let batch_path = game_path.join("ProjectZomboid64.bat");
    let batch_content = fs::read_to_string(&batch_path)
        .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
    let (batch_patched, _) = patch_batch_content(&batch_content);

    let mut patched_steam_configs = Vec::with_capacity(steam_configs.len());
    for config_path in steam_configs {
        let config_content = fs::read_to_string(config_path)
            .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (config_patched, changed) = patch_steam_launch_options_content(&config_content)?;
        patched_steam_configs.push((config_path.clone(), config_patched, changed));
    }

    let timestamp = current_unix_timestamp()?;
    let backup_dir = game_path.join(".pzmanager-backups").join(timestamp);
    let mut files = Vec::new();
    let mut target_paths = HashSet::new();

    for (source_file, content) in library_files {
        let name = source_file
            .file_name()
            .ok_or_else(|| format!("无效 Workshop 文件路径: {}", source_file.display()))?;
        let name = name.to_string_lossy().into_owned();
        let target = game_path.join(&name);
        if target_paths.insert(target.clone()) {
            files.push((target, content));
        }
    }

    files.push((json_path.clone(), json_patched.into_bytes()));
    files.push((batch_path.clone(), batch_patched.into_bytes()));

    for (config_path, config_patched, changed) in patched_steam_configs {
        if changed {
            files.push((config_path, config_patched.into_bytes()));
        }
    }

    let mut backup_targets = HashSet::new();
    for (path, _) in &files {
        if !backup_targets.insert(path.clone()) {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("无效备份文件路径: {}", path.display()))?;
        backup_file(path, &backup_dir, name)?;
    }

    apply_file_transaction(files)?;

    Ok(ZombieBuddyInstallResult {
        changed: true,
        already_installed: false,
        backup_path: Some(backup_dir.to_string_lossy().into_owned()),
    })
}

fn all_libraries_from_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    for root in roots {
        for library in library_paths_from_config(root) {
            if !libraries.contains(&library) {
                libraries.push(library);
            }
        }
    }
    libraries
}

#[cfg(windows)]
fn steam_is_running() -> Result<bool, String> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`：避免 GUI 程序里弹出一闪而过的控制台窗口。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("检查 Steam 进程失败: {error}"))
        .and_then(|output| {
            if !output.status.success() {
                return Err("检查 Steam 进程失败".to_string());
            }
            Ok(String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("steam.exe"))
        })
}

#[cfg(not(windows))]
fn steam_is_running() -> Result<bool, String> {
    Err("ZombieBuddy 安装只支持 Windows".to_string())
}

#[allow(dead_code)]
fn steam_executable_from_roots(roots: &[PathBuf]) -> Option<PathBuf> {
    roots
        .iter()
        .map(|root| root.join("steam.exe"))
        .find(|path| path.is_file())
}

fn game_path_from_library(library: &Path) -> Option<String> {
    let manifest_path = library
        .join("steamapps")
        .join(format!("appmanifest_{PROJECT_ZOMBOID_APP_ID}.acf"));
    let manifest = fs::read_to_string(manifest_path).ok()?;
    let install_dir = vdf_value(&manifest, "installdir")?;
    if install_dir.is_empty() {
        return None;
    }

    let game_path = library.join("steamapps").join("common").join(install_dir);
    game_path
        .is_dir()
        .then(|| game_path.to_string_lossy().into_owned())
}

fn workshop_path_from_library(library: &Path) -> Option<String> {
    let workshop_path = library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID);

    workshop_path
        .is_dir()
        .then(|| workshop_path.to_string_lossy().into_owned())
}

fn install_locations_from_libraries(libraries: &[PathBuf]) -> Option<(String, PathBuf)> {
    let game_path = libraries
        .iter()
        .find_map(|library| game_path_from_library(library))?;
    let source_path = libraries.iter().find_map(|library| {
        let source_path = zombie_buddy_source_from_library(library)?;
        zombie_buddy_source_has_files(&source_path).then_some(source_path)
    })?;

    Some((game_path, source_path))
}

fn library_paths_from_config(steam_root: &Path) -> Vec<PathBuf> {
    let mut paths = vec![steam_root.to_path_buf()];
    let config_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(config) = fs::read_to_string(config_path) else {
        return paths;
    };

    for (_, path) in parse_vdf_pairs(&config)
        .into_iter()
        .filter(|(key, _)| key == "path")
    {
        let path = PathBuf::from(path);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    paths
}

#[cfg(windows)]
fn steam_roots() -> Vec<PathBuf> {
    use std::env;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut roots = Vec::new();
    let mut add_root = |root: PathBuf| {
        if !roots.contains(&root) {
            roots.push(root);
        }
    };

    let registry_locations = [
        (HKEY_CURRENT_USER, "Software\\Valve\\Steam"),
        (HKEY_CURRENT_USER, "Software\\Wow6432Node\\Valve\\Steam"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Wow6432Node\\Valve\\Steam"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam"),
    ];

    for (hive, key_path) in registry_locations {
        let hive = RegKey::predef(hive);
        if let Ok(key) = hive.open_subkey(key_path) {
            let value = key
                .get_value::<String, _>("SteamPath")
                .or_else(|_| key.get_value::<String, _>("InstallPath"));
            if let Ok(path) = value {
                add_root(PathBuf::from(path));
            }
        }
    }

    for variable in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(program_files) = env::var_os(variable) {
            add_root(PathBuf::from(program_files).join("Steam"));
        }
    }

    roots
}

#[cfg(not(windows))]
fn steam_roots() -> Vec<PathBuf> {
    Vec::new()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectZomboidPaths {
    profile: Option<SteamProfile>,
    install_path: Option<String>,
    workshop_path: Option<String>,
    zombie_buddy: ZombieBuddyStatus,
    pz_optimization: PzOptimizationStatus,
    viewpoint_3d: Viewpoint3dStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SteamProfile {
    steam_id: String,
    account_name: String,
    persona_name: String,
}

fn steam_profile_from_loginusers(content: &str) -> Option<SteamProfile> {
    let mut profiles = Vec::new();
    let mut current_steam_id: Option<String> = None;
    let mut account_name: Option<String> = None;
    let mut persona_name: Option<String> = None;
    let mut most_recent = false;

    for line in content.lines() {
        let tokens = parse_quoted_tokens(line);

        if current_steam_id.is_none() {
            if tokens.len() == 1
                && tokens[0].len() == 17
                && tokens[0]
                    .chars()
                    .all(|character| character.is_ascii_digit())
            {
                current_steam_id = Some(tokens[0].clone());
                account_name = None;
                persona_name = None;
                most_recent = false;
            }
            continue;
        }

        if line.trim() == "}" {
            if let (Some(steam_id), Some(account_name), Some(persona_name)) = (
                current_steam_id.take(),
                account_name.take(),
                persona_name.take(),
            ) {
                profiles.push((
                    SteamProfile {
                        steam_id,
                        account_name,
                        persona_name,
                    },
                    most_recent,
                ));
            }
            most_recent = false;
            continue;
        }

        if tokens.len() < 2 {
            continue;
        }

        match tokens[0].as_str() {
            "AccountName" => account_name = Some(tokens[1].clone()),
            "PersonaName" => persona_name = Some(tokens[1].clone()),
            "MostRecent" => most_recent = tokens[1] == "1",
            _ => {}
        }
    }

    profiles
        .iter()
        .find(|(_, most_recent)| *most_recent)
        .or_else(|| profiles.first())
        .map(|(profile, _)| profile.clone())
}

fn steam_profile_from_root(steam_root: &Path) -> Option<SteamProfile> {
    let loginusers_path = steam_root.join("config").join("loginusers.vdf");
    let content = fs::read_to_string(loginusers_path).ok()?;
    steam_profile_from_loginusers(&content)
}

fn find_project_zomboid_paths() -> Result<ProjectZomboidPaths, String> {
    #[cfg(not(windows))]
    {
        return Err("Project Zomboid detection is only supported on Windows".to_string());
    }

    #[cfg(windows)]
    {
        let mut paths = ProjectZomboidPaths {
            profile: None,
            install_path: None,
            workshop_path: None,
            zombie_buddy: ZombieBuddyStatus {
                available: false,
                installed: false,
                source_path: None,
                missing_components: vec!["Project Zomboid 安装目录".to_string()],
            },
            pz_optimization: PzOptimizationStatus::default(),
            viewpoint_3d: viewpoint_three_d_status(None, &[]),
        };
        let mut selected_steam_configs = Vec::new();
        let mut zombie_buddy_source = None;
        let mut all_libraries = Vec::new();

        for steam_root in steam_roots() {
            if paths.profile.is_none() {
                paths.profile = steam_profile_from_root(&steam_root);
                if paths.profile.is_some() {
                    selected_steam_configs =
                        steam_config_paths(&steam_root, paths.profile.as_ref());
                }
            }

            let libraries = library_paths_from_config(&steam_root);
            for library in &libraries {
                if !all_libraries.contains(library) {
                    all_libraries.push(library.clone());
                }
            }
            if zombie_buddy_source
                .as_deref()
                .is_none_or(|source| !zombie_buddy_source_has_files(source))
            {
                zombie_buddy_source = zombie_buddy_source_from_libraries(&libraries);
            }

            for library in libraries {
                let library_game_path = game_path_from_library(&library);
                let library_workshop_path = workshop_path_from_library(&library);
                if paths.install_path.is_none() {
                    paths.install_path = library_game_path.clone();
                }
                if paths.workshop_path.is_none() {
                    paths.workshop_path = library_workshop_path.clone();
                }
            }
        }

        paths.zombie_buddy = zombie_buddy_status(
            paths.install_path.as_deref().map(Path::new),
            zombie_buddy_source.as_deref(),
            &selected_steam_configs,
        );
        paths.viewpoint_3d =
            viewpoint_three_d_status(paths.install_path.as_deref().map(Path::new), &all_libraries);
        paths.pz_optimization =
            pz_optimization_status(paths.install_path.as_deref().map(Path::new), &all_libraries);

        Ok(paths)
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedFile {
    name: String,
    size_bytes: u64,
    modified_at: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedDirectory {
    directory: String,
    files: Vec<ManagedFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LuaModEntry {
    id: String,
    name: String,
    path: String,
    source: String,
    workshop_id: Option<String>,
    enabled: bool,
    load_order: Option<usize>,
    requires: Vec<String>,
    load_after: Vec<String>,
    load_before: Vec<String>,
    load_first: bool,
    load_last: bool,
    has_common: bool,
    has_b42: bool,
    is_loader: bool,
    poster_path: Option<String>,
}

/// 设置页「通用 › 翻译语言」的取值：和前端 TRANSLATION_LANGUAGE_OPTIONS 一一对应。
/// 翻译链路（`news_translation::normalize_language`）其实认更多语言，但设置里只暴露这 6 种，
/// 免得下拉框显示的语言和实际拿去翻译的语言对不上。
const TRANSLATION_LANGUAGES: [&str; 6] = ["zh-CN", "zh-TW", "en", "ja", "ko", "ru"];
const DEFAULT_TRANSLATION_LANGUAGE: &str = "zh-CN";

/// 大小写不敏感地归一化到上面的取值；不在列表里返回 None。
fn normalize_translation_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    TRANSLATION_LANGUAGES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}

/// 字体对比度：0（保持现有配色）到 100（浅色面上的文字压到近黑）。
/// 用来处理"文字与背景、尤其是亚克力面板叠加后的底色太接近"。设置页「通用 › 字体对比度」。
const FONT_CONTRAST_MAX: u8 = 100;

fn normalize_font_contrast(value: u8) -> u8 {
    value.min(FONT_CONTRAST_MAX)
}

/// 模糊度：0（关掉模糊）到 200（设计基准的两倍），100 = 设计基准。
/// 设置页「通用 › 模糊度」，前端换算成 `--pz-blur-scale` 作用到全站毛玻璃 / 亚克力。
const BLUR_STRENGTH_MAX: u8 = 200;
const BLUR_STRENGTH_DEFAULT: u8 = 100;

fn normalize_blur_strength(value: u8) -> u8 {
    value.min(BLUR_STRENGTH_MAX)
}

/// 界面字号缩放（百分比）：80–130，100 = 设计基准。
/// 前端换算成 `--pz-font-scale` 作用到根 font-size，全站 rem 字号随之缩放。设置页「通用 › 界面字号」。
const FONT_SCALE_MIN: u8 = 80;
const FONT_SCALE_MAX: u8 = 130;
const FONT_SCALE_DEFAULT: u8 = 100;

fn normalize_font_scale(value: u8) -> u8 {
    value.clamp(FONT_SCALE_MIN, FONT_SCALE_MAX)
}

/// 界面字体：存完整的 CSS font-family 列表；空 / 缺失 = 回落 App.css 的默认字体栈。
const FONT_FAMILY_MAX_LEN: usize = 200;

fn normalize_font_family(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(FONT_FAMILY_MAX_LEN).collect())
}

/// 覆盖层背景色：存 `#RRGGBB`；空 / 缺失 = 覆盖层默认纯红。
/// 只接受 3 位或 6 位十六进制（`#` 可省），统一规范成大写 `#RRGGBB`，
/// 这样状态文件里永远是一个确定格式，DLL 解析失败时会保留原色而不是变黑。
fn normalize_overlay_background_color(value: &str) -> Option<String> {
    let hex = value.trim().trim_start_matches('#');
    if !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let expanded = match hex.len() {
        3 => hex
            .chars()
            .map(|ch| format!("{ch}{ch}"))
            .collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };
    Some(format!("#{}", expanded.to_ascii_uppercase()))
}

/// 覆盖层默认背景色（与 overlay-dll/src/ui.rs 的 DEFAULT_BACKGROUND 一致）。
const DEFAULT_OVERLAY_BACKGROUND_COLOR: &str = "#FF0000";

/// 启动器窗口尺寸（逻辑像素）：下限与两份 tauri conf 的 minWidth/minHeight 一致；
/// 上限只是防御性的，防止手改 settings.json 写进离谱数值。
const WINDOW_WIDTH_MIN: u32 = 840;
const WINDOW_HEIGHT_MIN: u32 = 560;
const WINDOW_SIZE_MAX: u32 = 10000;

fn normalize_window_width(value: u32) -> Option<u32> {
    (WINDOW_WIDTH_MIN..=WINDOW_SIZE_MAX)
        .contains(&value)
        .then_some(value)
}

fn normalize_window_height(value: u32) -> Option<u32> {
    (WINDOW_HEIGHT_MIN..=WINDOW_SIZE_MAX)
        .contains(&value)
        .then_some(value)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    background_path: Option<String>,
    shader_download_url: Option<String>,
    shader_download_directory: Option<String>,
    shader_download_threads: Option<usize>,
    memory_limit_mb: Option<u64>,
    translation_language: Option<String>,
    font_contrast: Option<u8>,
    /// 「模糊度」百分比：0 = 关掉模糊，100 = 设计基准，上限 200。没写过按 100 处理。
    blur_strength: Option<u8>,
    /// 「界面字体」：完整的 CSS font-family 列表；空 / 缺失 = App.css 的默认字体栈。
    font_family: Option<String>,
    /// 「界面字号」缩放百分比：80–130，100 = 设计基准。没写过按 100 处理。
    font_scale: Option<u8>,
    /// 记住的窗口尺寸（逻辑像素）。缺失 = 用 tauri.conf.json 的默认 840×560。
    window_width: Option<u32>,
    window_height: Option<u32>,
    /// 覆盖层背景色（`#RRGGBB`）。缺失 = 默认纯红；启动器写状态文件时下发给 DLL。
    overlay_background_color: Option<String>,
    /// 「模组加载器方案」（`zombieBuddy` / `none`），决定启动前要不要挂加载器 agent。
    mod_loader_scheme: Option<String>,
    /// 「极速启动（实验性）」：把启动器预扫描出的模组/补丁清单喂给 ZombieBuddy，
    /// 短路它内部的 ClassGraph 扫描。只在方案 = zombieBuddy 时生效；任何失败都回退原生扫描。
    #[serde(default)]
    fast_launch: bool,
    #[serde(default)]
    developer_mode: bool,
    #[serde(default)]
    steam_account_hidden: bool,
    /// 「静默内存清理」（设置 › 启动）：系统内存占用达到阈值时在后台自动清理一次。
    #[serde(default)]
    silent_memory_cleanup: bool,
    /// 静默清理的触发阈值（百分比）。0 视为"没设置过"，按默认值处理。
    #[serde(default)]
    silent_memory_cleanup_threshold: u8,
    /// 「启动时清理内存」：点「启动游戏」时先清理一次，再把游戏拉起来。
    #[serde(default)]
    clean_memory_on_launch: bool,
    /// 内置四季壁纸的手动 / 固定设置，前端 builtinWallpaper.ts 的 WallpaperEffectSettings 原样存取。
    #[serde(default)]
    wallpaper_effect: Option<serde_json::Value>,
    /// 「Steam 网络加速」模式：auto（直连失败自动切中转）/ relay（中转优先）/ direct（仅直连）。
    steam_relay_mode: Option<String>,
    /// 「Steam 网络加速」的中转地址；空 = 内置默认（steam_relay::DEFAULT_RELAY_BASE）。
    steam_relay_base: Option<String>,
    /// 「Steam 网络加速 › 本地代理」：如 `http://127.0.0.1:7897` 或
    /// `socks5://127.0.0.1:1080`；空 / 无效 = 不用显式代理（走系统代理）。
    steam_proxy: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BackgroundEntry {
    id: String,
    name: String,
    path: String,
    source: String,
    workshop_id: Option<String>,
    mod_name: Option<String>,
}
fn background_entry(
    id: String,
    name: String,
    path: PathBuf,
    source: &str,
    workshop_id: Option<String>,
    mod_name: Option<String>,
) -> BackgroundEntry {
    BackgroundEntry {
        id,
        name,
        path: path.to_string_lossy().into_owned(),
        source: source.to_string(),
        workshop_id,
        mod_name,
    }
}

fn user_zomboid_directory() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|profile| profile.join("Zomboid"))
        .ok_or_else(|| "找不到当前 Windows 用户目录".to_string())
}

fn parse_default_mod_ids(content: &str) -> Vec<String> {
    let Some(mods_start) = content.find("mods") else {
        return Vec::new();
    };
    let Some(open) = content[mods_start..].find('{') else {
        return Vec::new();
    };
    let remainder = &content[mods_start + open + 1..];
    let end = remainder.find('}').unwrap_or(remainder.len());
    remainder[..end]
        .split(',')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("mod")
                .then(|| value.trim().trim_matches(['"', '\'']).to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default)]
struct LuaModInfo {
    id: Option<String>,
    name: Option<String>,
    poster: Option<String>,
    requires: Vec<String>,
    load_after: Vec<String>,
    load_before: Vec<String>,
    load_first: bool,
    load_last: bool,
}

fn split_lua_mod_ids(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|part| {
            part.trim()
                .trim_matches(['"', '\''])
                .trim()
                .trim_start_matches(['/', '\\'])
        })
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn lua_mod_flag_enabled(value: &str) -> bool {
    matches!(
        value
            .trim()
            .trim_matches(['"', '\''])
            .to_ascii_lowercase()
            .as_str(),
        "on" | "true" | "yes" | "1" | "0"
    )
}

fn parse_lua_mod_info(content: &str) -> LuaModInfo {
    let mut info = LuaModInfo::default();
    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if value.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "id" if info.id.is_none() => info.id = Some(value.to_string()),
            "name" if info.name.is_none() => info.name = Some(value.to_string()),
            "poster" if info.poster.is_none() => info.poster = Some(value.to_string()),
            "require" => info.requires.extend(split_lua_mod_ids(value)),
            "loadafter" | "loadmodafter" => info.load_after.extend(split_lua_mod_ids(value)),
            "loadbefore" | "loadmodbefore" => info.load_before.extend(split_lua_mod_ids(value)),
            "loadfirst" => info.load_first = lua_mod_flag_enabled(value),
            "loadlast" => info.load_last = lua_mod_flag_enabled(value),
            _ => {}
        }
    }
    info
}

#[derive(Debug, Clone, Default)]
struct LuaModSortingRule {
    load_after: Vec<String>,
    load_before: Vec<String>,
    load_first: bool,
    load_last: bool,
}

fn parse_lua_sorting_rules(content: &str) -> HashMap<String, LuaModSortingRule> {
    let mut rules = HashMap::<String, LuaModSortingRule>::new();
    let mut current_id: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_id = Some(trimmed[1..trimmed.len() - 1].trim().to_ascii_lowercase());
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let Some(id) = current_id.as_ref() else {
            continue;
        };
        let rule = rules.entry(id.clone()).or_default();
        match key.trim().to_ascii_lowercase().as_str() {
            "loadafter" | "loadmodafter" => {
                rule.load_after.extend(split_lua_mod_ids(value));
            }
            "loadbefore" | "loadmodbefore" => {
                rule.load_before.extend(split_lua_mod_ids(value));
            }
            "loadfirst" => rule.load_first = lua_mod_flag_enabled(value),
            "loadlast" => rule.load_last = lua_mod_flag_enabled(value),
            _ => {}
        }
    }
    rules
}

fn lua_mod_sorting_rules() -> HashMap<String, LuaModSortingRule> {
    let path = user_zomboid_directory()
        .map(|directory| directory.join("Lua").join("sorting_rules.txt"))
        .ok();
    path.and_then(|path| fs::read_to_string(path).ok())
        .map(|content| parse_lua_sorting_rules(&content))
        .unwrap_or_default()
}

fn lua_mod_info_files(root: &Path) -> Vec<PathBuf> {
    let mut files = vec![root.join("mod.info"), root.join("common").join("mod.info")];
    if let Ok(entries) = fs::read_dir(root) {
        files.extend(entries.flatten().filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            (entry.file_type().ok()?.is_dir() && (name.starts_with("41") || name.starts_with("42")))
                .then(|| entry.path().join("mod.info"))
        }));
    }
    files.into_iter().filter(|path| path.is_file()).collect()
}

fn lua_mod_poster_path(
    root: &Path,
    info_path: Option<&Path>,
    info: &LuaModInfo,
) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let (Some(poster), Some(parent)) = (info.poster.as_deref(), info_path.and_then(Path::parent))
    {
        let relative = Path::new(poster.trim());
        if !relative.is_absolute()
            && !relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            candidates.push(parent.join(relative));
        }
    }
    candidates.extend([
        root.join("preview.png"),
        root.join("poster.png"),
        root.join("preview.jpg"),
        root.join("poster.jpg"),
    ]);
    candidates
        .into_iter()
        .find(|path| is_supported_image(path) && is_regular_file(path))
}

fn extend_unique_mod_ids(target: &mut Vec<String>, values: impl IntoIterator<Item = String>) {
    for value in values {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            target.push(value);
        }
    }
}

fn lua_mod_metadata(root: &Path) -> (String, String, LuaModInfo, Option<PathBuf>) {
    for info_path in lua_mod_info_files(root) {
        let Ok(content) = fs::read_to_string(&info_path) else {
            continue;
        };
        let info = parse_lua_mod_info(&content);
        if info.id.is_some() || info.name.is_some() {
            let id = info.id.clone().unwrap_or_else(|| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            let name = info.name.clone().unwrap_or_else(|| id.clone());
            return (
                id,
                name,
                info.clone(),
                lua_mod_poster_path(root, Some(&info_path), &info),
            );
        }
    }
    let id = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    (
        id.clone(),
        id,
        LuaModInfo::default(),
        lua_mod_poster_path(root, None, &LuaModInfo::default()),
    )
}

fn merge_lua_mod_entry(existing: &mut LuaModEntry, candidate: LuaModEntry) {
    let prefer_candidate = candidate.has_b42 && !existing.has_b42;
    if prefer_candidate {
        existing.requires = candidate.requires.clone();
        existing.load_after = candidate.load_after.clone();
        existing.load_before = candidate.load_before.clone();
    } else if existing.has_b42 && !candidate.has_b42 {
        // Keep the Build 42 metadata when the same ID also has a legacy copy.
    } else {
        extend_unique_mod_ids(&mut existing.requires, candidate.requires.clone());
        extend_unique_mod_ids(&mut existing.load_after, candidate.load_after.clone());
        extend_unique_mod_ids(&mut existing.load_before, candidate.load_before.clone());
    }
    existing.load_first |= candidate.load_first;
    existing.load_last |= candidate.load_last;
    existing.has_common |= candidate.has_common;
    existing.has_b42 |= candidate.has_b42;
    existing.is_loader |= candidate.is_loader;
    existing.load_order = existing.load_order.or(candidate.load_order);
    existing.enabled = existing.load_order.is_some();
    if prefer_candidate {
        existing.name = candidate.name;
        existing.path = candidate.path;
        existing.source = candidate.source;
        existing.workshop_id = candidate.workshop_id;
        if candidate.poster_path.is_some() {
            existing.poster_path = candidate.poster_path;
        }
    } else if existing.poster_path.is_none() {
        existing.poster_path = candidate.poster_path;
    }
}

fn lua_mod_entries() -> Result<Vec<LuaModEntry>, String> {
    let zomboid = user_zomboid_directory()?;
    let sorting_rules = lua_mod_sorting_rules();
    let default_path = zomboid.join("mods").join("default.txt");
    let enabled_ids = fs::read_to_string(&default_path)
        .map(|content| parse_default_mod_ids(&content))
        .unwrap_or_default();
    let enabled_orders: HashMap<_, _> = enabled_ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.to_lowercase(), index + 1))
        .collect();
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut add_entry = |path: PathBuf, source: &str, workshop_id: Option<String>| {
        let key = format!("{source}:{}", path.display());
        if !seen.insert(key) {
            return;
        }
        let (id, name, info, poster) = lua_mod_metadata(&path);
        let requires = info.requires.clone();
        let mut load_after = info.load_after.clone();
        let mut load_before = info.load_before.clone();
        let mut load_first = info.load_first;
        let mut load_last = info.load_last;
        if let Some(rule) = sorting_rules.get(&id.to_ascii_lowercase()) {
            extend_unique_mod_ids(&mut load_after, rule.load_after.clone());
            extend_unique_mod_ids(&mut load_before, rule.load_before.clone());
            load_first |= rule.load_first;
            load_last |= rule.load_last;
        }
        let load_order = enabled_orders.get(&id.to_lowercase()).copied();
        let has_common = path.join("common").is_dir();
        let has_42 = fs::read_dir(&path)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry.file_type().is_ok_and(|kind| kind.is_dir())
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .starts_with("42")
            });
        entries.push(LuaModEntry {
            is_loader: id.eq_ignore_ascii_case("ZombieBuddy")
                || workshop_id.as_deref() == Some(ZOMBIE_BUDDY_MOD_ID),
            id,
            name,
            path: path.to_string_lossy().into_owned(),
            source: source.to_string(),
            workshop_id,
            enabled: load_order.is_some(),
            load_order,
            requires,
            load_after,
            load_before,
            load_first,
            load_last,
            has_common: path.join("common").is_dir(),
            has_b42: has_42 || has_common,
            poster_path: poster.map(|path| path.to_string_lossy().into_owned()),
        });
    };
    let local_mods = zomboid.join("mods");
    if let Ok(items) = fs::read_dir(&local_mods) {
        for entry in items
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            add_entry(entry.path(), "local", None);
        }
    }
    for root in workshop_background_roots() {
        let Ok(items) = fs::read_dir(&root) else {
            continue;
        };
        for workshop_item in items
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let workshop_id = workshop_item.file_name().to_string_lossy().into_owned();
            if workshop_id.parse::<u64>().is_err() {
                continue;
            }
            for mod_root in discover_mod_roots(&workshop_item.path()) {
                add_entry(mod_root, "workshop", Some(workshop_id.clone()));
            }
        }
    }
    let mut deduplicated = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Some(index) = deduplicated
            .iter()
            .position(|existing: &LuaModEntry| existing.id.eq_ignore_ascii_case(&entry.id))
        {
            merge_lua_mod_entry(&mut deduplicated[index], entry);
        } else {
            deduplicated.push(entry);
        }
    }
    entries = deduplicated;
    entries.sort_by(|left, right| {
        left.load_order
            .cmp(&right.load_order)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command(async)]
fn list_lua_mods() -> Result<Vec<LuaModEntry>, String> {
    lua_mod_entries()
}
fn shader_download_directory() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?.join("viewpoint-shaderpacks"))
}

fn pzmanager_directory() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?.join(".pzmanager"))
}

pub(crate) fn app_settings_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("settings.json"))
}

pub(crate) fn load_app_settings_at(path: &Path) -> Result<AppSettings, String> {
    if !path.is_file() {
        return Ok(AppSettings::default());
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取应用设置失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析应用设置失败: {error}"))
}

fn save_app_settings_at(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败: {error}"))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化应用设置失败: {error}"))?;
    fs::write(path, format!("{content}\n")).map_err(|error| format!("保存应用设置失败: {error}"))
}

fn image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp"
            )
        })
}

fn background_storage_directory(kind: &str) -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("backgrounds").join(kind))
}

fn unique_destination(directory: &Path, file_name: &str) -> PathBuf {
    let original = Path::new(file_name);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("background");
    let extension = original
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("png");
    let mut candidate = directory.join(format!("{stem}.{extension}"));
    let mut index = 2;
    while candidate.exists() {
        candidate = directory.join(format!("{stem}-{index}.{extension}"));
        index += 1;
    }
    candidate
}

fn compose_title_background_at(title_paths: [&Path; 4], destination: &Path) -> Result<(), String> {
    let images = title_paths
        .into_iter()
        .map(|path| image::open(path).map_err(|error| format!("读取背景图片失败: {error}")))
        .collect::<Result<Vec<DynamicImage>, _>>()?;
    let top_width = images[0].width().max(images[2].width());
    let bottom_width = images[1].width().max(images[3].width());
    let left_height = images[0].height().max(images[1].height());
    let right_height = images[2].height().max(images[3].height());
    let mut output = RgbaImage::new(top_width + bottom_width, left_height + right_height);

    for (image, x, y) in [
        (&images[0], 0, 0),
        (&images[1], top_width, 0),
        (&images[2], 0, left_height),
        (&images[3], top_width, left_height),
    ] {
        image::imageops::overlay(&mut output, &image.to_rgba8(), x.into(), y.into());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建拼接背景目录失败: {error}"))?;
    }
    output
        .save_with_format(destination, ImageFormat::Png)
        .map_err(|error| format!("保存拼接背景失败: {error}"))
}

fn list_custom_backgrounds_at(directory: &Path) -> Vec<BackgroundEntry> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut backgrounds = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| image_file(&entry.path()))
        .map(|entry| {
            let path = entry.path();
            background_entry(
                format!("custom:{}", path.to_string_lossy()),
                entry.file_name().to_string_lossy().into_owned(),
                path,
                "custom",
                None,
                None,
            )
        })
        .collect::<Vec<_>>();
    backgrounds.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    backgrounds
}

fn workshop_background_roots() -> Vec<PathBuf> {
    all_libraries_from_roots(&steam_roots())
        .into_iter()
        .map(|library| {
            library
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(PROJECT_ZOMBOID_APP_ID)
        })
        .filter(|path| path.is_dir())
        .collect()
}

fn workshop_backgrounds_at_with_callback<Item>(
    roots: &[PathBuf],
    mut on_item: Item,
) -> Vec<BackgroundEntry>
where
    Item: FnMut(&BackgroundEntry),
{
    let mut entries = Vec::new();
    for root in roots {
        let Ok(workshop_items) = fs::read_dir(root) else {
            continue;
        };
        for workshop_item in workshop_items.flatten() {
            let workshop_path = workshop_item.path();
            if !workshop_item.file_type().is_ok_and(|kind| kind.is_dir())
                || workshop_item
                    .file_name()
                    .to_string_lossy()
                    .parse::<u64>()
                    .is_err()
            {
                continue;
            }
            let workshop_id = workshop_item.file_name().to_string_lossy().into_owned();
            let mod_roots = discover_mod_roots(&workshop_path);
            for mod_root in mod_roots {
                let mod_name = mod_root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "未命名模组".to_string());
                let mut pending = vec![mod_root.clone()];
                let mut title_groups: HashMap<PathBuf, [Option<PathBuf>; 4]> = HashMap::new();

                while let Some(directory) = pending.pop() {
                    let Ok(files) = fs::read_dir(&directory) else {
                        continue;
                    };
                    for entry in files.flatten() {
                        let path = entry.path();
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        if kind.is_dir() {
                            pending.push(path);
                            continue;
                        }
                        if !kind.is_file() || !image_file(&path) {
                            continue;
                        }

                        let lower_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                        if directory.file_name().is_some_and(|name| {
                            name.to_string_lossy().eq_ignore_ascii_case("backgrounds")
                        }) {
                            let entry = background_entry(
                                format!("workshop:{}", path.to_string_lossy()),
                                format!("{mod_name} · {}", entry.file_name().to_string_lossy()),
                                path.clone(),
                                "workshop",
                                Some(workshop_id.clone()),
                                Some(mod_name.clone()),
                            );
                            on_item(&entry);
                            entries.push(entry);
                        }

                        let title_index = match lower_name.as_str() {
                            "title.png" => Some(0),
                            "title2.png" => Some(1),
                            "title3.png" => Some(2),
                            "title4.png" => Some(3),
                            _ => None,
                        };
                        if let Some(index) = title_index {
                            title_groups.entry(directory.clone()).or_default()[index] = Some(path);
                        }
                    }
                }

                for (directory, title_paths) in title_groups {
                    if title_paths.iter().any(Option::is_none) {
                        continue;
                    }
                    let [Some(title), Some(title2), Some(title3), Some(title4)] = title_paths
                    else {
                        continue;
                    };
                    let output_name = format!(
                        "{workshop_id}_{}_{}_title.png",
                        sanitize_background_name(&mod_name),
                        format!(
                            "{:x}",
                            Sha256::digest(directory.to_string_lossy().as_bytes())
                        )
                        .chars()
                        .take(8)
                        .collect::<String>()
                    );
                    let output_path = background_storage_directory("workshop")
                        .unwrap_or_else(|_| PathBuf::from(".pzmanager/backgrounds/workshop"))
                        .join(output_name);
                    if !output_path.is_file()
                        && compose_title_background_at(
                            [&title, &title2, &title3, &title4],
                            &output_path,
                        )
                        .is_err()
                    {
                        continue;
                    }
                    let entry = background_entry(
                        format!("workshop:title:{}", directory.to_string_lossy()),
                        format!("{mod_name} · 拼接标题背景"),
                        output_path,
                        "workshop",
                        Some(workshop_id.clone()),
                        Some(mod_name.clone()),
                    );
                    on_item(&entry);
                    entries.push(entry);
                }
            }
        }
    }
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    entries
}

fn workshop_backgrounds_at(roots: &[PathBuf]) -> Vec<BackgroundEntry> {
    workshop_backgrounds_at_with_callback(roots, |_| {})
}

fn discover_mod_roots(workshop_path: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut pending = vec![workshop_path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("mods")
            {
                if let Ok(mod_entries) = fs::read_dir(&path) {
                    roots.extend(
                        mod_entries
                            .flatten()
                            .filter(|mod_entry| {
                                mod_entry.file_type().is_ok_and(|kind| kind.is_dir())
                            })
                            .map(|mod_entry| mod_entry.path()),
                    );
                }
            } else {
                pending.push(path);
            }
        }
    }
    roots.sort();
    roots
}

fn sanitize_background_name(name: &str) -> String {
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn background_path_allowed(path: &Path) -> bool {
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    let custom_directory = background_storage_directory("custom")
        .ok()
        .and_then(|directory| fs::canonicalize(directory).ok());
    let generated_directory = background_storage_directory("workshop")
        .ok()
        .and_then(|directory| fs::canonicalize(directory).ok());
    let in_custom = custom_directory
        .as_deref()
        .is_some_and(|directory| path.starts_with(directory));
    let in_generated = generated_directory
        .as_deref()
        .is_some_and(|directory| path.starts_with(directory));
    let in_workshop = workshop_background_roots()
        .into_iter()
        .any(|root| fs::canonicalize(root).is_ok_and(|directory| path.starts_with(directory)));
    in_custom || in_generated || in_workshop
}

/// 内置背景在 settings.json 的 `backgroundPath` 里存成 `builtin:<id>`（例如 `builtin:default`）：
/// 图片随前端一起打包（`public/backgrounds/`），不是磁盘上的用户文件，
/// 所以不走 background_path_allowed 那份目录校验，前端直接用打包好的 URL 渲染。
const BUILTIN_BACKGROUND_PREFIX: &str = "builtin:";

fn is_builtin_background_path(path: &str) -> bool {
    path.trim().starts_with(BUILTIN_BACKGROUND_PREFIX)
}

fn import_custom_background_at(source: &Path) -> Result<BackgroundEntry, String> {
    if !image_file(source) || !is_regular_file(source) {
        return Err("请选择有效的图片文件".to_string());
    }
    let directory = background_storage_directory("custom")?;
    fs::create_dir_all(&directory).map_err(|error| format!("创建自定义背景目录失败: {error}"))?;
    let name = source
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "无法读取图片文件名".to_string())?;
    let destination = unique_destination(&directory, name);
    fs::copy(source, &destination).map_err(|error| format!("复制自定义背景失败: {error}"))?;
    Ok(background_entry(
        format!("custom:{}", destination.to_string_lossy()),
        name.to_string(),
        destination,
        "custom",
        None,
        None,
    ))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JarModStateEntry {
    mod_dir: String,
    mod_id: String,
    workshop_id: Option<u64>,
    package_name: String,
    disabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct JarModStateFile {
    format_version: u32,
    mods: Vec<JarModStateEntry>,
}

impl Default for JarModStateFile {
    fn default() -> Self {
        Self {
            format_version: 1,
            mods: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JarModDecisionResult {
    decision: bool,
    lua_mod_enabled: bool,
    lua_mod_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LuaModToggleResult {
    enabled: bool,
    load_order: Option<usize>,
    added: Vec<String>,
    missing_requires: Vec<String>,
    dependents: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LuaModBatchToggleResult {
    enabled: bool,
    changed: Vec<String>,
    added: Vec<String>,
    missing_requires: Vec<String>,
    dependents: Vec<String>,
    skipped: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JarModDecisionUpdate {
    mod_id: String,
    mod_info_path: String,
    package_name: String,
    workshop_id: Option<u64>,
    jar_hash: String,
    decision: bool,
    lua_mod_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JarModBatchDecisionResult {
    updated: usize,
    lua_mods_enabled: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JavaModInfo {
    name: String,
    id: String,
    jar_file: String,
    package_name: String,
    poster: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct JavaPatchMethod {
    annotation: String,
    method_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JavaClassAnnotations {
    class_name: String,
    patch: bool,
    lua_class: bool,
    patch_methods: Vec<JavaPatchMethod>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScannedJarMod {
    id: String,
    /// The `id` field of the mod's `mod.info`, used to locate the matching Lua mod.
    info_id: String,
    mod_info_path: String,
    disabled: bool,
    workshop_id: Option<u64>,
    jar_path: String,
    poster_path: Option<String>,
    package_name: String,
    jar_hash: String,
    patch_classes: Vec<String>,
    patch_methods: Vec<JavaPatchMethod>,
    lua_classes: Vec<String>,
    decision: Option<bool>,
}

fn parse_java_mod_info(content: &str) -> Option<JavaModInfo> {
    let mut name = None;
    let mut id = None;
    let mut jar_file = None;
    let mut package_name = None;
    let mut poster = None;

    for line in content.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }

        match key.trim().to_ascii_lowercase().as_str() {
            "name" if name.is_none() => name = Some(value.to_string()),
            "id" if id.is_none() => id = Some(value.to_string()),
            "javajarfile" if jar_file.is_none() => jar_file = Some(value.to_string()),
            "javapkgname" if package_name.is_none() => package_name = Some(value.to_string()),
            "poster" if poster.is_none() => poster = Some(value.to_string()),
            _ => {}
        }
    }

    Some(JavaModInfo {
        name: name?,
        id: id?,
        jar_file: jar_file?,
        package_name: package_name?,
        poster,
    })
}

fn is_ignored_mod_directory(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".git" | ".vscode" | "tmp" | "temp" | "node_modules"
    )
}

fn discover_mod_info_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut mod_info_files = Vec::new();

    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_file() {
                let file_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if file_name == "mod.info" {
                    mod_info_files.push(path);
                } else if file_name == "mod.info.disabled" && !directory.join("mod.info").is_file()
                {
                    mod_info_files.push(path);
                }
            } else if file_type.is_dir()
                && !is_ignored_mod_directory(&entry.file_name().to_string_lossy())
            {
                pending.push(path);
            }
        }
    }

    mod_info_files.sort();
    mod_info_files
}

fn mod_jar_path(mod_info_path: &Path, mod_info: &JavaModInfo) -> Option<PathBuf> {
    let mod_directory = mod_info_path.parent()?;
    let declared_path = Path::new(&mod_info.jar_file);
    let mut candidates = vec![mod_directory.join(declared_path)];

    if let Some(mods_directory) = mod_info_path
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "mods"))
    {
        if let Ok(relative_mod_info) = mod_directory.strip_prefix(mods_directory) {
            let mod_root = mods_directory.join(relative_mod_info.components().next()?);
            candidates.push(mod_root.join(declared_path));
            if mod_directory
                .file_name()
                .is_some_and(|name| name == "common")
            {
                let versions = fs::read_dir(&mod_root).ok()?;
                candidates.extend(versions.flatten().filter_map(|entry| {
                    entry
                        .file_type()
                        .ok()
                        .filter(|file_type| file_type.is_dir())
                        .map(|_| entry.path().join(declared_path))
                }));
            }
        }
    }

    candidates.into_iter().find(|path| path.is_file())
}

fn mod_poster_path(mod_info_path: &Path, mod_info: &JavaModInfo) -> Option<PathBuf> {
    let poster = mod_info.poster.as_deref()?.trim();
    if poster.is_empty() {
        return None;
    }
    let relative_path = Path::new(poster);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let path = mod_info_path.parent()?.join(relative_path);
    (is_supported_image(&path) && is_regular_file(&path)).then_some(path)
}

/// 统计一个 jar 里所有 class 声明的注入方法(advice)数量。
///
/// 启动界面进度条用它作为"本次要注入的总数"分母；与运行期
/// `[ZB] patching … with N advice(s)` 的 ΣN 高度吻合（实测 614 vs 611）。
pub(crate) fn count_jar_advice_methods(jar_path: &Path) -> usize {
    let Ok(file) = fs::File::open(jar_path) else {
        return 0;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return 0;
    };
    let mut total = 0;
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        if !entry.name().ends_with(".class") {
            continue;
        }
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() {
            continue;
        }
        if let Some(annotations) = class_annotations(&bytes) {
            total += annotations.patch_methods.len();
        }
    }
    total
}

fn class_annotations(class_bytes: &[u8]) -> Option<JavaClassAnnotations> {
    let (_, class_file) = java_class_rs::parse_classfile(class_bytes).ok()?;
    let class_name_index =
        match java_class_rs::get_entry(&class_file.constant_pool, class_file.this_class)? {
            java_class_rs::ConstantPoolEntry::Class(class_info) => class_info.name_index,
            _ => return None,
        };
    let class_name =
        java_class_rs::get_utf8(&class_file.constant_pool, class_name_index)?.replace('/', ".");
    let mut patch_annotation = false;
    let mut lua_annotation = false;
    let mut patch_methods = Vec::new();

    for attribute in &class_file.attributes {
        let attribute_name =
            java_class_rs::get_utf8(&class_file.constant_pool, attribute.attribute_name_index)?;
        if !matches!(
            attribute_name,
            "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
        ) {
            continue;
        }

        if let java_class_rs::ParsedAttribute::RuntimeVisibleAnnotations(annotations)
        | java_class_rs::ParsedAttribute::RuntimeInvisibleAnnotations(annotations) =
            java_class_rs::parse_specialized_attribute(attribute, &class_file.constant_pool)
        {
            for annotation in annotations.annotations {
                let Some(annotation_descriptor) =
                    java_class_rs::get_utf8(&class_file.constant_pool, annotation.type_index)
                else {
                    continue;
                };
                match annotation_descriptor {
                    "Lme/zed_0xff/zombie_buddy/Patch;" => patch_annotation = true,
                    "Lme/zed_0xff/zombie_buddy/Exposer$LuaClass;" => lua_annotation = true,
                    _ => {}
                }
            }
        }
    }

    for method in &class_file.methods {
        let method_name = java_class_rs::get_utf8(&class_file.constant_pool, method.name_index)?;
        for attribute in &method.attributes {
            let attribute_name =
                java_class_rs::get_utf8(&class_file.constant_pool, attribute.attribute_name_index)?;
            if !matches!(
                attribute_name,
                "RuntimeVisibleAnnotations" | "RuntimeInvisibleAnnotations"
            ) {
                continue;
            }

            let annotations = match java_class_rs::parse_specialized_attribute(
                attribute,
                &class_file.constant_pool,
            ) {
                java_class_rs::ParsedAttribute::RuntimeVisibleAnnotations(annotations)
                | java_class_rs::ParsedAttribute::RuntimeInvisibleAnnotations(annotations) => {
                    annotations.annotations
                }
                _ => continue,
            };
            for annotation in annotations {
                let Some(annotation_descriptor) =
                    java_class_rs::get_utf8(&class_file.constant_pool, annotation.type_index)
                else {
                    continue;
                };
                let Some(annotation_name) = patch_method_annotation_name(annotation_descriptor)
                else {
                    continue;
                };
                patch_methods.push(JavaPatchMethod {
                    annotation: annotation_name.to_string(),
                    method_name: method_name.to_string(),
                });
            }
        }
    }

    (patch_annotation || lua_annotation || !patch_methods.is_empty()).then_some(
        JavaClassAnnotations {
            class_name,
            patch: patch_annotation,
            lua_class: lua_annotation,
            patch_methods,
        },
    )
}

fn patch_method_annotation_name(descriptor: &str) -> Option<&'static str> {
    let name = descriptor
        .strip_prefix('L')?
        .strip_suffix(';')?
        .rsplit(|character| character == '/' || character == '$')
        .next()?;
    match name {
        "OnEnter" | "OnExit" | "Inject" => Some("Inject"),
        "Overwrite" => Some("Overwrite"),
        "Redirect" => Some("Redirect"),
        "ModifyConstant" => Some("ModifyConstant"),
        "ModifyVariable" => Some("ModifyVariable"),
        _ => None,
    }
}

fn scan_java_mod_jar(
    info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> Option<ScannedJarMod> {
    let jar_path = mod_jar_path(info_path, info)?;
    let mut jar = fs::File::open(&jar_path).ok()?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = jar.read(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }
        digest.update(&buffer[..bytes_read]);
    }
    let jar_hash = format!("{:x}", digest.finalize());
    let file = fs::File::open(&jar_path).ok()?;
    let mut archive = ZipArchive::new(file).ok()?;
    let package_prefix = format!("{}/", info.package_name.replace('.', "/"));
    let mut patch_classes = Vec::new();
    let mut patch_methods = Vec::new();
    let mut lua_classes = Vec::new();

    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        let entry_name = entry.name().to_string();
        if !entry_name.starts_with(&package_prefix)
            || !entry_name.ends_with(".class")
            || entry_name.starts_with("META-INF/versions/")
        {
            continue;
        }
        let mut class_bytes = Vec::new();
        if entry.read_to_end(&mut class_bytes).is_err() {
            continue;
        }
        let Some(class_info) = class_annotations(&class_bytes) else {
            continue;
        };
        // 收本包**及其子包**的类。ClassGraph 的 `acceptPackages(pkg)` 是递归的，
        // 而 ZombieBuddy 是按 jar 声明的包名调 `collectPatches`：它自己的补丁类在
        // `me.zed_0xff.zombie_buddy.patches`，mod.info 声明的包却是父包
        // `me.zed_0xff.zombie_buddy`。只收"正好等于声明包"的类会把这部分补丁全漏掉。
        let in_declared_package = class_info
            .class_name
            .rsplit_once('.')
            .map(|(package, _)| package)
            .is_some_and(|package| {
                package == info.package_name.as_str()
                    || package.starts_with(&format!("{}.", info.package_name))
            });
        if !in_declared_package {
            continue;
        }
        if class_info.lua_class {
            lua_classes.push(class_info.class_name.clone());
        }
        if class_info.patch {
            patch_classes.push(class_info.class_name);
        }
        patch_methods.extend(class_info.patch_methods);
    }

    if patch_classes.is_empty() && patch_methods.is_empty() && lua_classes.is_empty() {
        return None;
    }

    patch_classes.sort();
    patch_classes.dedup();
    patch_methods.sort_by(|left, right| {
        left.annotation
            .cmp(&right.annotation)
            .then_with(|| left.method_name.cmp(&right.method_name))
    });
    patch_methods.dedup();
    lua_classes.sort();
    lua_classes.dedup();
    let disabled = info_path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case("mod.info.disabled")
    });

    Some(ScannedJarMod {
        id: info.name.clone(),
        info_id: info.id.clone(),
        mod_info_path: info_path.to_string_lossy().into_owned(),
        disabled,
        workshop_id,
        jar_path: jar_path.to_string_lossy().into_owned(),
        poster_path: mod_poster_path(info_path, info)
            .map(|path| path.to_string_lossy().into_owned()),
        package_name: info.package_name.clone(),
        jar_hash,
        patch_classes,
        patch_methods,
        lua_classes,
        decision: Some(!disabled),
    })
}

fn jar_mod_state_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("jar-mod-state.json"))
}

fn read_jar_mod_state(path: &Path) -> Result<JarModStateFile, String> {
    if !path.is_file() {
        return Ok(JarModStateFile::default());
    }
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取 JAR 模组状态失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析 JAR 模组状态失败: {error}"))
}

/// 删掉极速启动的预扫描缓存。
///
/// ⚠️ **策略已改（2026-10-15）：缓存持久化**。决策/启停变更**不再**自动删缓存
/// （以前挂在 `write_jar_mod_state` / `set_jar_mod_info_enabled` 上），只有用户显式点
/// 「刷新 JAR 列表」（`start_zombie_buddy_jar_scan { refresh_cache: true }`）才重建。
///
/// 为什么改：缓存被自动删掉的那一局玩家完全没加速，而这件事在游戏里**看不出来**；
/// 玩家要的是"缓存稳定存在，什么时候换由我说了算"。代价是"改了模组但没刷新"时缓存可能过期，
/// 这由设置页的状态行（`fast_launch_cache_status().matches == false`）明确提示，而不是悄悄删掉。
///
/// 保留本函数供测试与将来的「清空缓存」入口使用。
#[allow(dead_code)]
fn invalidate_fast_launch_cache() {
    if let Ok(directory) = pzmanager_directory() {
        let _ = fs::remove_file(directory.join(FAST_LAUNCH_CACHE_FILE));
    }
}

fn write_jar_mod_state(path: &Path, state: &JarModStateFile) -> Result<(), String> {
    // 注意：这里**不**再删极速启动的缓存（见 `invalidate_fast_launch_cache` 的说明）：
    // 缓存只在用户显式刷新 JAR 列表时重建，平时保持持久化。
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("写入 JAR 模组状态失败: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建 JAR 模组状态目录失败: {error}"))?;
    }
    fs::write(path, format!("{content}\n"))
        .map_err(|error| format!("写入 JAR 模组状态失败: {error}"))
}

fn upsert_jar_mod_state_entry(state: &mut JarModStateFile, entry: JarModStateEntry) {
    state.format_version = 1;
    if let Some(existing) = state.mods.iter_mut().find(|existing| {
        existing.mod_dir.eq_ignore_ascii_case(&entry.mod_dir)
            || (existing.workshop_id.is_some()
                && existing.workshop_id == entry.workshop_id
                && existing.mod_id.eq_ignore_ascii_case(&entry.mod_id)
                && existing
                    .package_name
                    .eq_ignore_ascii_case(&entry.package_name))
    }) {
        *existing = entry;
    } else {
        state.mods.push(entry);
    }
}

fn jar_mod_state_matches(
    entry: &JarModStateEntry,
    mod_info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> bool {
    let mod_dir = mod_info_path.parent().unwrap_or(mod_info_path);
    entry
        .mod_dir
        .eq_ignore_ascii_case(&mod_dir.to_string_lossy())
        || (entry.mod_id.eq_ignore_ascii_case(&info.id)
            && entry.workshop_id == workshop_id
            && (entry.package_name.is_empty()
                || entry.package_name.eq_ignore_ascii_case(&info.package_name)))
}

fn apply_jar_mod_state_to_roots(
    roots: &[PathBuf],
    state: &JarModStateFile,
) -> Result<usize, String> {
    if state.mods.is_empty() {
        return Ok(0);
    }
    let candidates = jar_scan_candidates(roots);
    let mut changed = 0;
    for entry in &state.mods {
        let Some((info_path, _, _)) = candidates.iter().find(|(info_path, info, workshop_id)| {
            jar_mod_state_matches(entry, info_path, info, *workshop_id)
        }) else {
            continue;
        };
        let info_path_string = info_path.to_string_lossy().into_owned();
        let enabled = !entry.disabled;
        let currently_enabled = info_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("mod.info"));
        if enabled != currently_enabled {
            set_jar_mod_info_enabled(&info_path_string, enabled)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Byte range of the `mods { ... }` block inside default.txt: the `mods` keyword,
/// its opening brace and its closing brace.
fn default_mods_block(content: &str) -> Option<(usize, usize, usize)> {
    let mods_start = content.find("mods")?;
    let open = mods_start + content[mods_start..].find('{')?;
    let close = open + content[open..].find('}')?;
    Some((mods_start, open, close))
}

/// Rebuilds the `mods` block with `ids` in order. Everything outside the block,
/// plus the file's line endings and indentation, is preserved.
fn write_default_mod_ids(default_path: &Path, ids: &[String]) -> Result<(), String> {
    let existing = match fs::read_to_string(default_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("读取 default.txt 失败: {error}")),
    };
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let block = default_mods_block(&existing);
    let indent = block
        .map(|(_, open, close)| {
            existing[open + 1..close]
                .lines()
                .find(|line| line.trim_start().to_ascii_lowercase().starts_with("mod"))
                .map(|line| line[..line.len() - line.trim_start().len()].to_string())
                .unwrap_or_else(|| "    ".to_string())
        })
        .unwrap_or_else(|| "    ".to_string());

    let mut body = String::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        // 注意：PZ 自己的 default.txt 里是**不带引号**的 `mod = 模组ID,`。
        // 写成 `mod = "模组ID",` 会让游戏把引号当成 ID 的一部分，从而识别不出任何模组。
        body.push_str(&format!("{indent}mod = {id},{newline}"));
    }
    let block_text = format!("mods{newline}{{{newline}{body}}}");

    let updated = match block {
        Some((mods_start, _, close)) => {
            let mut content = String::with_capacity(existing.len() + block_text.len());
            content.push_str(&existing[..mods_start]);
            content.push_str(&block_text);
            content.push_str(&existing[close + 1..]);
            content
        }
        None => {
            let mut content = existing.clone();
            if content.trim().is_empty() {
                // 游戏自己的文件第一行是 `VERSION = 1,`，缺了它 PZ 可能不认这份配置。
                content.clear();
                content.push_str(&format!("VERSION = 1,{newline}{newline}"));
            } else if !content.ends_with('\n') {
                content.push_str(newline);
            }
            content.push_str(&block_text);
            content.push_str(newline);
            content
        }
    };

    if let Some(parent) = default_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 mods 目录失败: {error}"))?;
    }
    fs::write(default_path, updated).map_err(|error| format!("写入 default.txt 失败: {error}"))
}

fn read_default_mod_ids(default_path: &Path) -> Result<Vec<String>, String> {
    match fs::read_to_string(default_path) {
        Ok(content) => Ok(parse_default_mod_ids(&content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("读取 default.txt 失败: {error}")),
    }
}

/// Matches the load-order semantics used by Build 42 mod managers:
/// `require` and `loadAfter` edges run first, `loadBefore` is inverted, and
/// `loadFirst` / `loadLast` only affect the stable fallback order.
fn order_mod_ids_by_requires(entries: &[LuaModEntry], ids: &[String]) -> Vec<String> {
    fn visit(
        id: &str,
        by_id: &HashMap<String, &LuaModEntry>,
        before: &HashMap<String, Vec<String>>,
        ordered: &mut Vec<String>,
        done: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
    ) {
        let key = id.to_ascii_lowercase();
        if done.contains(&key) || !visiting.insert(key.clone()) {
            return;
        }
        if let Some(dependencies) = before.get(&key) {
            for dependency in dependencies {
                if by_id.contains_key(&dependency.to_ascii_lowercase()) {
                    visit(dependency, by_id, before, ordered, done, visiting);
                }
            }
        }
        visiting.remove(&key);
        if done.insert(key) {
            ordered.push(
                by_id
                    .get(&id.to_ascii_lowercase())
                    .map(|entry| entry.id.clone())
                    .unwrap_or_else(|| id.to_string()),
            );
        }
    }

    let active_ids = ids
        .iter()
        .map(|id| id.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let by_id = entries
        .iter()
        .map(|entry| (entry.id.to_ascii_lowercase(), entry))
        .collect::<HashMap<_, _>>();
    let mut before = HashMap::<String, Vec<String>>::new();
    for id in ids {
        let key = id.to_ascii_lowercase();
        let Some(entry) = by_id.get(&key) else {
            continue;
        };
        let dependencies = before.entry(key).or_default();
        extend_unique_mod_ids(
            dependencies,
            entry
                .requires
                .iter()
                .chain(&entry.load_after)
                .filter(|dependency| active_ids.contains(&dependency.to_ascii_lowercase()))
                .cloned(),
        );
    }
    for entry in entries {
        if !active_ids.contains(&entry.id.to_ascii_lowercase()) {
            continue;
        }
        for target in &entry.load_before {
            let target_key = target.to_ascii_lowercase();
            if active_ids.contains(&target_key) {
                extend_unique_mod_ids(before.entry(target_key).or_default(), [entry.id.clone()]);
            }
        }
    }

    let original_order = ids
        .iter()
        .enumerate()
        .map(|(index, id)| (id.to_ascii_lowercase(), index))
        .collect::<HashMap<_, _>>();
    let mut candidates = ids.to_vec();
    candidates.sort_by(|left, right| {
        let left_entry = by_id.get(&left.to_ascii_lowercase()).copied();
        let right_entry = by_id.get(&right.to_ascii_lowercase()).copied();
        let priority =
            |id: &str, entry: Option<&LuaModEntry>| match id.to_ascii_lowercase().as_str() {
                "modmanager" => 0,
                "modmanagerserver" => 1,
                "modoptions" => 2,
                _ if entry.is_some_and(|entry| entry.load_first) => 100,
                _ if entry.is_some_and(|entry| entry.load_last) => 10_000,
                _ => 1_000,
            };
        priority(left, left_entry)
            .cmp(&priority(right, right_entry))
            .then_with(|| {
                original_order
                    .get(&left.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(usize::MAX)
                    .cmp(
                        &original_order
                            .get(&right.to_ascii_lowercase())
                            .copied()
                            .unwrap_or(usize::MAX),
                    )
            })
    });

    let mut ordered = Vec::new();
    let mut done = HashSet::new();
    let mut visiting = HashSet::new();
    for id in candidates {
        visit(&id, &by_id, &before, &mut ordered, &mut done, &mut visiting);
    }
    ordered
}

/// Adds `id` plus every prerequisite it declares (recursively) to `ids`,
/// appending so prerequisites end up ahead of their dependents.
fn plan_lua_mod_enable(
    entries: &[LuaModEntry],
    id: &str,
    ids: &mut Vec<String>,
    added: &mut Vec<String>,
    missing: &mut Vec<String>,
    active: &mut HashSet<String>,
) {
    let key = id.to_ascii_lowercase();
    if !active.insert(key.clone()) {
        return;
    }
    match entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(id))
    {
        Some(entry) => {
            for require in &entry.requires {
                plan_lua_mod_enable(entries, require, ids, added, missing, active);
            }
            if !ids
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&entry.id))
            {
                ids.push(entry.id.clone());
                added.push(entry.id.clone());
            }
        }
        None => {
            if !ids.iter().any(|value| value.eq_ignore_ascii_case(id))
                && !missing.iter().any(|value| value.eq_ignore_ascii_case(id))
            {
                missing.push(id.to_string());
            }
        }
    }
    active.remove(&key);
}

/// Finds the Lua mod that belongs to a JAR mod, preferring `mod.info` id matches
/// and falling back to the mod's display name.
fn pick_lua_mod_id(
    entries: &[LuaModEntry],
    info_id: Option<&str>,
    fallback_name: &str,
) -> Option<String> {
    if let Some(target) = info_id.map(str::trim).filter(|value| !value.is_empty()) {
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(target))
        {
            return Some(entry.id.clone());
        }
    }
    let name = fallback_name.trim();
    if name.is_empty() {
        return None;
    }
    entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(name) || entry.name.eq_ignore_ascii_case(name))
        .map(|entry| entry.id.clone())
}

fn find_lua_mod_id(info_id: Option<&str>, fallback_name: &str) -> Option<String> {
    pick_lua_mod_id(&lua_mod_entries().ok()?, info_id, fallback_name)
}

/// 从 `mod.info` 路径推导创意工坊 id（形如 `.../<appid>/<workshop_id>/.../mod.info`）。
fn workshop_id_from_info_path(info_path: &Path) -> Option<u64> {
    info_path.ancestors().find_map(|ancestor| {
        let parent = ancestor.parent()?;
        (parent.file_name()?.to_str()? == PROJECT_ZOMBOID_APP_ID)
            .then(|| ancestor.file_name()?.to_str()?.parse().ok())
            .flatten()
    })
}

fn jar_scan_candidates(roots: &[PathBuf]) -> Vec<(PathBuf, JavaModInfo, Option<u64>)> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        for info_path in discover_mod_info_files(root) {
            let Ok(content) = fs::read_to_string(&info_path) else {
                continue;
            };
            let Some(info) = parse_java_mod_info(&content) else {
                continue;
            };
            let Some(jar_path) = mod_jar_path(&info_path, &info) else {
                continue;
            };
            let key = (
                jar_path.to_string_lossy().to_ascii_lowercase(),
                info.package_name.clone(),
            );
            if !seen.insert(key) {
                continue;
            }
            let workshop_id = workshop_id_from_info_path(&info_path);
            // 加载器本体不进「JAR 模组」列表（它是方案本身，不是可审批的模组），
            // 但它的补丁类必须进极速启动缓存，见 `zombie_buddy_loader_cache_mod_at`。
            if workshop_id == Some(ZOMBIE_BUDDY_WORKSHOP_ID) {
                continue;
            }
            candidates.push((info_path, info, workshop_id));
        }
    }
    candidates
}

fn sort_scanned_jar_mods(scanned: &mut [ScannedJarMod]) {
    scanned.sort_by(|left, right| {
        left.id
            .to_lowercase()
            .cmp(&right.id.to_lowercase())
            .then_with(|| left.package_name.cmp(&right.package_name))
    });
}

fn scan_jar_mods_at_with_callbacks<Start, Item>(
    roots: &[PathBuf],
    mut on_start: Start,
    mut on_item: Item,
) -> Vec<ScannedJarMod>
where
    Start: FnMut(usize),
    Item: FnMut(usize, usize, Option<&ScannedJarMod>),
{
    let candidates = jar_scan_candidates(roots);
    let total = candidates.len();
    on_start(total);
    let mut scanned = Vec::new();

    for (index, (info_path, info, workshop_id)) in candidates.into_iter().enumerate() {
        if let Some(mod_info) = scan_java_mod_jar(&info_path, &info, workshop_id) {
            scanned.push(mod_info);
            on_item(index + 1, total, scanned.last());
        } else {
            on_item(index + 1, total, None);
        }
    }

    sort_scanned_jar_mods(&mut scanned);
    scanned
}

fn scan_jar_mods_at_with_callback<Item>(roots: &[PathBuf], mut on_item: Item) -> Vec<ScannedJarMod>
where
    Item: FnMut(usize, usize, Option<&ScannedJarMod>),
{
    scan_jar_mods_at_with_callbacks(
        roots,
        |_| {},
        |scanned, total, item| {
            on_item(scanned, total, item);
        },
    )
}

fn scan_jar_mods_at(roots: &[PathBuf]) -> Vec<ScannedJarMod> {
    scan_jar_mods_at_with_callback(roots, |_, _, _| {})
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JarModScanUpdate {
    scan_id: u64,
    status: String,
    scanned: usize,
    total: usize,
    item: Option<ScannedJarMod>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkshopBackgroundScanUpdate {
    scan_id: u64,
    status: String,
    scanned: usize,
    total: usize,
    item: Option<BackgroundEntry>,
    error: Option<String>,
}

fn jar_scan_inputs() -> Vec<PathBuf> {
    all_libraries_from_roots(&steam_roots())
        .into_iter()
        .map(|library| {
            library
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(PROJECT_ZOMBOID_APP_ID)
        })
        .filter(|path| path.is_dir())
        .chain(
            std::env::var_os("USERPROFILE")
                .map(|profile| PathBuf::from(profile).join("Zomboid").join("mods")),
        )
        .filter(|path| path.is_dir())
        .collect()
}

/// 极速启动的预扫描缓存：启动器把「已批准且启用」的 JAR 模组及其注解类清单落盘，
/// 注入器（overlay-dll）在 ZombieBuddy 扫描前把它喂给 Loader，短路 ClassGraph 扫描。
/// 只收录已批准且启用的模组是硬要求：缓存与启用列表不一致会让补丁丢失。
const FAST_LAUNCH_CACHE_FORMAT: u32 = 1;
const FAST_LAUNCH_CACHE_FILE: &str = "zb-prescan-cache.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchCache {
    format: u32,
    generated_at: u64,
    /// 一致性指纹：注入器拿它和当前启用列表比对，对不上就放弃缓存、回退原生扫描。
    mod_set_hash: String,
    mods: Vec<FastLaunchCacheMod>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchCacheMod {
    mod_name: String,
    jar_path: String,
    base_package: String,
    jar_hash: String,
    patch_classes: Vec<String>,
    lua_classes: Vec<String>,
}

/// FNV-1a：只给缓存指纹用，不为这点需求引入哈希依赖。
fn fast_launch_fingerprint(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// 把扫描产出的**内部名**（`a/b/C`）转成**二进制名**（`a.b.C`）。
///
/// 缓存里的类名是给注入器直接喂 `Class.forName` 用的，必须是二进制名。
/// 扫描（`class_annotations`）给的是内部名 —— 早期版本直接把内部名写进缓存，
/// 结果注入器侧每个模组都抛 `ClassNotFoundException` 然后回退原生扫描（**一点没加速**），
/// 日志里只看到一串 CNFE。注入器侧现在也会兜一层转换，两边都改才算稳。
fn binary_class_name(internal_name: &str) -> String {
    if internal_name.contains('/') {
        internal_name.replace('/', ".")
    } else {
        internal_name.to_string()
    }
}

/// 由扫描结果构造缓存：过滤掉未批准/已禁用的模组，按 jar 路径排序后算指纹。
fn build_fast_launch_cache(mods: &[ScannedJarMod], generated_at: u64) -> FastLaunchCache {
    let mut entries: Vec<FastLaunchCacheMod> = mods
        .iter()
        .filter(|entry| !entry.disabled && entry.decision == Some(true))
        .map(|entry| FastLaunchCacheMod {
            mod_name: entry.info_id.clone(),
            jar_path: entry.jar_path.clone(),
            base_package: entry.package_name.clone(),
            jar_hash: entry.jar_hash.clone(),
            patch_classes: entry
                .patch_classes
                .iter()
                .map(|name| binary_class_name(name))
                .collect(),
            lua_classes: entry
                .lua_classes
                .iter()
                .map(|name| binary_class_name(name))
                .collect(),
        })
        .collect();
    entries.sort_by(|left, right| left.jar_path.cmp(&right.jar_path));
    let fingerprint = entries
        .iter()
        .map(|entry| format!("{}#{}", entry.jar_path, entry.jar_hash))
        .collect::<Vec<_>>()
        .join("|");
    FastLaunchCache {
        format: FAST_LAUNCH_CACHE_FORMAT,
        generated_at,
        mod_set_hash: fast_launch_fingerprint(&fingerprint),
        mods: entries,
    }
}

/// 极速启动缓存专用：ZombieBuddy 加载器本体的扫描结果。
///
/// 它不在「JAR 模组」列表里（`jar_scan_candidates` 刻意跳过），但 ZombieBuddy 自己会调
/// `collectPatches("me.zed_0xff.zombie_buddy.patches")` —— 缓存里没有它，强制模式下
/// ZB 自己的补丁就会被丢掉（实测表现：进游戏弹"ZombieBuddy 未安装"）。
fn zombie_buddy_loader_cache_mod_at(roots: &[PathBuf]) -> Option<ScannedJarMod> {
    for root in roots {
        for info_path in discover_mod_info_files(root) {
            let Ok(content) = fs::read_to_string(&info_path) else {
                continue;
            };
            let Some(info) = parse_java_mod_info(&content) else {
                continue;
            };
            if workshop_id_from_info_path(&info_path) != Some(ZOMBIE_BUDDY_WORKSHOP_ID) {
                continue;
            }
            if let Some(mod_info) =
                scan_java_mod_jar(&info_path, &info, Some(ZOMBIE_BUDDY_WORKSHOP_ID))
            {
                return Some(mod_info);
            }
        }
    }
    None
}

/// 构造预扫描缓存、校验指纹时使用的**完整**模组清单：JAR 模组 + ZombieBuddy 加载器本体。
///
/// 写入（「刷新 JAR 列表」）与校验（设置页状态行）必须走同一个函数，
/// 否则指纹永远对不上、状态行会一直提示"建议重建"。
fn fast_launch_cache_mods_at(roots: &[PathBuf]) -> Vec<ScannedJarMod> {
    let mut mods = scan_jar_mods_at(roots);
    if let Some(loader) = zombie_buddy_loader_cache_mod_at(roots) {
        mods.push(loader);
    }
    mods
}

fn fast_launch_cache_mods() -> Vec<ScannedJarMod> {
    fast_launch_cache_mods_at(&jar_scan_inputs())
}

/// 读取预扫描缓存（注入器与设置页状态行都读它）。
fn read_fast_launch_cache_at(path: &Path) -> Result<FastLaunchCache, String> {
    let content =
        fs::read_to_string(path).map_err(|error| format!("读取预扫描缓存失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析预扫描缓存失败: {error}"))
}

/// 缓存和「当前已批准且启用」的列表是否一致：不一致时注入器必须放弃缓存、走原生扫描。
fn fast_launch_cache_matches(cache: &FastLaunchCache, mods: &[ScannedJarMod]) -> bool {
    cache.format == FAST_LAUNCH_CACHE_FORMAT
        && cache.mod_set_hash == build_fast_launch_cache(mods, cache.generated_at).mod_set_hash
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchCacheStatus {
    exists: bool,
    generated_at: u64,
    mod_count: usize,
    mod_set_hash: String,
    /// 与当前扫描结果是否一致（注入器据此判断能不能用缓存）。
    matches: bool,
}

/// 设置 › 启动 的状态行用：报告缓存是否存在、何时生成、多少个模组、是否仍然一致。
#[tauri::command]
fn fast_launch_cache_status() -> Result<FastLaunchCacheStatus, String> {
    let Some(path) = fast_launch_cache_path() else {
        return Err("拿不到 .pzmanager 目录".to_string());
    };
    if !path.is_file() {
        return Ok(FastLaunchCacheStatus {
            exists: false,
            generated_at: 0,
            mod_count: 0,
            mod_set_hash: String::new(),
            matches: false,
        });
    }
    let cache = read_fast_launch_cache_at(&path)?;
    let mods = fast_launch_cache_mods();
    Ok(FastLaunchCacheStatus {
        exists: true,
        generated_at: cache.generated_at,
        mod_count: cache.mods.len(),
        mod_set_hash: cache.mod_set_hash.clone(),
        matches: fast_launch_cache_matches(&cache, &mods),
    })
}

/// 最近一次「窗口出现 → 移除覆盖层」的启动时长（含上一次，供前端对比）。
#[tauri::command]
fn startup_timing() -> overlay_session::StartupTiming {
    overlay_session::read_startup_timing()
}

/// 极速启动开关（读设置；读不到按关闭处理）。
fn fast_launch_enabled() -> bool {
    app_settings_path()
        .ok()
        .and_then(|path| load_app_settings_at(&path).ok())
        .is_some_and(|settings| settings.fast_launch)
}

/// 预扫描缓存文件的路径：`%USERPROFILE%\Zomboid\.pzmanager\zb-prescan-cache.json`。
fn fast_launch_cache_path() -> Option<PathBuf> {
    pzmanager_directory()
        .ok()
        .map(|directory| directory.join(FAST_LAUNCH_CACHE_FILE))
}

/// 这次 JAR 扫描要不要**重建**预扫描缓存？
///
/// * 用户显式点「刷新 JAR 列表」（`refresh_cache = true`）→ 重建（这是唯一会替换缓存的入口）；
/// * 自动扫描（进页面时触发）→ **只在缓存还不存在时**补一份，已有缓存保持不动。
///
/// 也就是说：缓存一旦生成就持久存在，玩家不动「刷新 JAR 列表」就不会被换掉。
fn should_write_fast_launch_cache(refresh_cache: bool, cache_exists: bool) -> bool {
    refresh_cache || !cache_exists
}

/// 写入缓存文件（目录不存在就创建）。返回写出的路径。
fn write_fast_launch_cache_at(
    directory: &Path,
    cache: &FastLaunchCache,
) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|error| format!("创建预扫描缓存目录失败: {error}"))?;
    let path = directory.join(FAST_LAUNCH_CACHE_FILE);
    let content = serde_json::to_string_pretty(cache)
        .map_err(|error| format!("序列化预扫描缓存失败: {error}"))?;
    fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("写入预扫描缓存失败: {error}"))?;
    Ok(path)
}

/// 扫描 JAR 模组（后台线程 + `jar-mod-scan-update` 事件推送）。
///
/// `refresh_cache` 决定这次扫描要不要**重建极速启动的预扫描缓存**：
/// * `true` —— 用户显式点了「刷新 JAR 列表」，缓存重建（这也是唯一会替换缓存的入口）；
/// * `false` —— 自动扫描（进「模组管理 › JAR」页面时触发）：**只有在缓存还不存在时**才写一份，
///   已有缓存一律保持不动 —— 玩家要的是"缓存稳定存在，什么时候换由我说了算"。
#[tauri::command]
fn start_zombie_buddy_jar_scan(
    app: tauri::AppHandle,
    refresh_cache: Option<bool>,
) -> Result<u64, String> {
    let refresh_cache = refresh_cache.unwrap_or(false);
    // 在扫描开始前就判断"要不要写缓存"，语义清楚也不会和扫描过程本身打架。
    let cache_exists = fast_launch_cache_path().is_some_and(|path| path.is_file());
    let should_write_cache = should_write_fast_launch_cache(refresh_cache, cache_exists);
    let scan_id = NEXT_JAR_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = app.emit(
                "jar-mod-scan-update",
                JarModScanUpdate {
                    scan_id,
                    status: "error".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: Some("JAR 模组扫描只支持 Windows".to_string()),
                },
            );
        }

        #[cfg(windows)]
        {
            let roots = jar_scan_inputs();
            let state_path = match jar_mod_state_path() {
                Ok(path) => path,
                Err(error) => {
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "error".to_string(),
                            scanned: 0,
                            total: 0,
                            item: None,
                            error: Some(error),
                        },
                    );
                    return;
                }
            };
            let state = match read_jar_mod_state(&state_path) {
                Ok(state) => state,
                Err(error) => {
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "error".to_string(),
                            scanned: 0,
                            total: 0,
                            item: None,
                            error: Some(error),
                        },
                    );
                    return;
                }
            };
            if let Err(error) = apply_jar_mod_state_to_roots(&roots, &state) {
                let _ = app.emit(
                    "jar-mod-scan-update",
                    JarModScanUpdate {
                        scan_id,
                        status: "error".to_string(),
                        scanned: 0,
                        total: 0,
                        item: None,
                        error: Some(error),
                    },
                );
                return;
            }
            let mut candidate_total = 0;
            // 顺手收集扫描结果：极速启动开启时用它生成 ZombieBuddy 预扫描缓存。
            let mut collected: Vec<ScannedJarMod> = Vec::new();
            let _ = scan_jar_mods_at_with_callbacks(
                &roots,
                |total| {
                    candidate_total = total;
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "started".to_string(),
                            scanned: 0,
                            total,
                            item: None,
                            error: None,
                        },
                    );
                },
                |scanned, total, item| {
                    if let Some(item) = item {
                        collected.push(item.clone());
                    }
                    let _ = app.emit(
                        "jar-mod-scan-update",
                        JarModScanUpdate {
                            scan_id,
                            status: "item".to_string(),
                            scanned,
                            total,
                            item: item.cloned(),
                            error: None,
                        },
                    );
                },
            );
            // 极速启动（实验性）：**只有**用户显式刷新 JAR 列表（`refresh_cache`）或缓存还不存在时
            // 才写预扫描缓存；其它扫描一律保持已有缓存不动（缓存持久化，见 `start_zombie_buddy_jar_scan`）。
            // 写失败只意味着"极速启动不会生效"，启动流程本身不受影响，所以这里不打断扫描。
            if should_write_cache && fast_launch_enabled() {
                let generated_at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_secs())
                    .unwrap_or(0);
                // 加载器本体（ZombieBuddy）也要进缓存，否则强制模式下 ZB 自己的补丁会被丢掉。
                let mut cache_mods = collected;
                if let Some(loader) = zombie_buddy_loader_cache_mod_at(&roots) {
                    cache_mods.push(loader);
                }
                let cache = build_fast_launch_cache(&cache_mods, generated_at);
                if let Some(path) = fast_launch_cache_path() {
                    if let Some(directory) = path.parent() {
                        match write_fast_launch_cache_at(directory, &cache) {
                            Ok(written) => eprintln!(
                                "极速启动：已重建预扫描缓存（{} 个模组）→ {}",
                                cache.mods.len(),
                                written.display()
                            ),
                            Err(error) => eprintln!("极速启动：写预扫描缓存失败: {error}"),
                        }
                    }
                }
            } else if fast_launch_enabled() {
                eprintln!("极速启动：本次是自动扫描且缓存已存在 —— 保持缓存不动（只有「刷新 JAR 列表」会重建）");
            }
            let _ = app.emit(
                "jar-mod-scan-update",
                JarModScanUpdate {
                    scan_id,
                    status: "completed".to_string(),
                    scanned: candidate_total,
                    total: candidate_total,
                    item: None,
                    error: None,
                },
            );
        }
    });
    Ok(scan_id)
}

#[tauri::command]
fn start_workshop_background_scan(app: tauri::AppHandle) -> Result<u64, String> {
    let scan_id = NEXT_WORKSHOP_BACKGROUND_SCAN_ID.fetch_add(1, Ordering::Relaxed);
    thread::spawn(move || {
        #[cfg(not(windows))]
        {
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "error".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: Some("创意工坊背景扫描只支持 Windows".to_string()),
                },
            );
        }

        #[cfg(windows)]
        {
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "started".to_string(),
                    scanned: 0,
                    total: 0,
                    item: None,
                    error: None,
                },
            );
            let roots = workshop_background_roots();
            let mut scanned = 0;
            let backgrounds = workshop_backgrounds_at_with_callback(&roots, |item| {
                scanned += 1;
                let _ = app.emit(
                    "workshop-background-scan-update",
                    WorkshopBackgroundScanUpdate {
                        scan_id,
                        status: "item".to_string(),
                        scanned,
                        total: 0,
                        item: Some(item.clone()),
                        error: None,
                    },
                );
            });
            let total = backgrounds.len();
            let _ = app.emit(
                "workshop-background-scan-update",
                WorkshopBackgroundScanUpdate {
                    scan_id,
                    status: "completed".to_string(),
                    scanned: total,
                    total,
                    item: None,
                    error: None,
                },
            );
        }
    });
    Ok(scan_id)
}

#[allow(dead_code)]
fn scan_zombie_buddy_jar_mods() -> Result<Vec<ScannedJarMod>, String> {
    #[cfg(not(windows))]
    {
        return Err("JAR 模组扫描只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = jar_scan_inputs();
        let state = read_jar_mod_state(&jar_mod_state_path()?)?;
        apply_jar_mod_state_to_roots(&roots, &state)?;
        Ok(scan_jar_mods_at(&roots))
    }
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
            )
        })
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

fn list_managed_files(
    directory: &Path,
    predicate: fn(&Path) -> bool,
) -> Result<Vec<ManagedFile>, String> {
    fs::create_dir_all(directory)
        .map_err(|error| format!("创建目录 {} 失败: {error}", directory.display()))?;
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("读取目录 {} 失败: {error}", directory.display()))?;
    let mut files = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !is_regular_file(&path) || !predicate(&path) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs());
            Some(ManagedFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                size_bytes: metadata.len(),
                modified_at,
            })
        })
        .collect::<Vec<_>>();
    files.sort_by(|left, right| {
        right
            .modified_at
            .cmp(&left.modified_at)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(files)
}

fn list_viewpoint_shots_at(directory: &Path) -> Result<Vec<ManagedFile>, String> {
    list_managed_files(directory, is_supported_image)
}

fn list_viewpoint_shaderpacks_at(directory: &Path) -> Result<Vec<ManagedFile>, String> {
    list_managed_files(directory, is_zip)
}

fn managed_file_path(
    directory: &Path,
    name: &str,
    predicate: fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let path = Path::new(name);
    if name.is_empty()
        || path.file_name().and_then(|file_name| file_name.to_str()) != Some(name)
        || !predicate(path)
    {
        return Err("无效文件名或文件格式".to_string());
    }
    Ok(directory.join(path))
}

fn read_viewpoint_shot_at(directory: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = managed_file_path(directory, name, is_supported_image)?;
    if !is_regular_file(&path) {
        return Err("截图不存在或不是普通文件".to_string());
    }
    fs::read(path).map_err(|error| format!("读取截图失败: {error}"))
}

fn import_shaderpack_to(source: &Path, directory: &Path) -> Result<String, String> {
    if !is_zip(source) || !is_regular_file(source) {
        return Err("请选择有效的 ZIP 光影包文件".to_string());
    }
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "无法读取 ZIP 文件名".to_string())?;
    fs::create_dir_all(directory).map_err(|error| format!("创建光影包目录失败: {error}"))?;
    let destination = directory.join(name);
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                format!("同名光影包已存在：{name}")
            } else {
                format!("创建光影包文件失败: {error}")
            }
        })?;
    let mut input =
        fs::File::open(source).map_err(|error| format!("读取 ZIP 文件失败: {error}"))?;
    if let Err(error) = io::copy(&mut input, &mut output) {
        let _ = fs::remove_file(&destination);
        return Err(format!("复制光影包失败: {error}"));
    }
    Ok(name.to_string())
}

fn delete_shaderpack_at(directory: &Path, name: &str) -> Result<(), String> {
    let path = managed_file_path(directory, name, is_zip)?;
    if !is_regular_file(&path) {
        return Err("光影包不存在或不是普通文件".to_string());
    }
    fs::remove_file(path).map_err(|error| format!("删除光影包失败: {error}"))
}

#[tauri::command(async)]
fn detect_project_zomboid() -> Result<ProjectZomboidPaths, String> {
    find_project_zomboid_paths()
}

#[tauri::command(async)]
fn list_viewpoint_shots() -> Result<ManagedDirectory, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shots");
    Ok(ManagedDirectory {
        files: list_viewpoint_shots_at(&directory)?,
        directory: directory.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
fn read_viewpoint_shot(name: String) -> Result<tauri::ipc::Response, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shots");
    read_viewpoint_shot_at(&directory, &name).map(tauri::ipc::Response::new)
}

#[tauri::command(async)]
fn list_viewpoint_shaderpacks() -> Result<ManagedDirectory, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    Ok(ManagedDirectory {
        files: list_viewpoint_shaderpacks_at(&directory)?,
        directory: directory.to_string_lossy().into_owned(),
    })
}

#[tauri::command(async)]
fn import_viewpoint_shaderpack(source_path: String) -> Result<String, String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    import_shaderpack_to(Path::new(&source_path), &directory)
}

#[tauri::command(async)]
fn delete_viewpoint_shaderpack(name: String) -> Result<(), String> {
    let directory = user_zomboid_directory()?.join("viewpoint-shaderpacks");
    delete_shaderpack_at(&directory, &name)
}

#[tauri::command]
fn save_shader_download_settings(
    url: String,
    directory: String,
    threads: usize,
) -> Result<(), String> {
    if !(1..=16).contains(&threads) {
        return Err("下载线程数必须在 1 到 16 之间".to_string());
    }
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.shader_download_url = if url.trim().is_empty() {
        None
    } else {
        Some(url.trim().to_string())
    };
    settings.shader_download_directory = if directory.trim().is_empty() {
        None
    } else {
        Some(directory.trim().to_string())
    };
    settings.shader_download_threads = Some(threads);
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
async fn download_shader_file(
    app: tauri::AppHandle,
    download_id: Option<String>,
    url: String,
    file_name: String,
    directory: String,
    threads: usize,
) -> Result<String, String> {
    // 前端任务中心会传入自己的任务 ID，用于把 shader-download-progress 精确对回任务；
    // 旧的「自定义下载」入口不传时仍由后端生成。
    let download_id = download_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            format!(
                "shader-{}",
                NEXT_WORKSHOP_BACKGROUND_SCAN_ID.fetch_add(1, Ordering::Relaxed)
            )
        });
    let directory = if directory.trim().is_empty() {
        shader_download_directory()?.to_string_lossy().into_owned()
    } else {
        directory
    };
    downloader::download_file(app, download_id, url, file_name, directory, threads).await
}
#[tauri::command]
fn get_app_settings() -> Result<AppSettings, String> {
    let mut settings = load_app_settings_at(&app_settings_path()?)?;
    if settings.shader_download_directory.is_none() {
        settings.shader_download_directory =
            Some(shader_download_directory()?.to_string_lossy().into_owned());
    }
    // settings.json 被手改坏、或写着设置页没提供的语言时回落到默认，
    // 保证下拉框选中的语言和实际拿去翻译的语言始终一致。
    settings.translation_language = Some(
        settings
            .translation_language
            .as_deref()
            .and_then(normalize_translation_language)
            .unwrap_or_else(|| DEFAULT_TRANSLATION_LANGUAGE.to_string()),
    );
    // 字体对比度没写过或写坏了都按 0（保持现有配色）处理。
    settings.font_contrast = Some(settings.font_contrast.map_or(0, normalize_font_contrast));
    // 模糊度没写过按 100（设计基准）处理。
    settings.blur_strength = Some(
        settings
            .blur_strength
            .map_or(BLUR_STRENGTH_DEFAULT, normalize_blur_strength),
    );
    // 模组加载器方案同理，坏值回落默认（ZombieBuddy）。
    settings.mod_loader_scheme = Some(
        settings
            .mod_loader_scheme
            .as_deref()
            .and_then(normalize_mod_loader_scheme)
            .unwrap_or_else(|| DEFAULT_MOD_LOADER_SCHEME.to_string()),
    );
    // 界面字号没写过按 100（设计基准）处理。
    settings.font_scale = Some(
        settings
            .font_scale
            .map_or(FONT_SCALE_DEFAULT, normalize_font_scale),
    );
    // 界面字体：空串按「默认字体栈」处理（回落 None，前端就不写 CSS 变量）。
    settings.font_family = settings
        .font_family
        .as_deref()
        .and_then(normalize_font_family);
    // 覆盖层背景色：没写过或格式坏掉都按默认纯红处理，前端颜色选择器才有值可显示。
    settings.overlay_background_color = Some(
        settings
            .overlay_background_color
            .as_deref()
            .and_then(normalize_overlay_background_color)
            .unwrap_or_else(|| DEFAULT_OVERLAY_BACKGROUND_COLOR.to_string()),
    );
    // Steam 网络加速：坏值回落默认（auto + 内置中转地址）。
    settings.steam_relay_mode = Some(
        settings
            .steam_relay_mode
            .as_deref()
            .and_then(steam_relay::normalize_relay_mode)
            .unwrap_or_else(|| steam_relay::DEFAULT_RELAY_MODE.to_string()),
    );
    settings.steam_relay_base = Some(steam_relay::normalize_relay_base(
        settings.steam_relay_base.as_deref().unwrap_or(""),
    ));
    // 本地代理：写坏了按「没配」处理（空串），退回系统代理行为。
    settings.steam_proxy = settings
        .steam_proxy
        .as_deref()
        .and_then(steam_relay::normalize_steam_proxy);
    Ok(settings)
}

#[tauri::command]
fn save_developer_mode(enabled: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.developer_mode = enabled;
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_steam_account_hidden(hidden: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_account_hidden = hidden;
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_translation_language(language: String) -> Result<(), String> {
    let normalized = normalize_translation_language(&language)
        .ok_or_else(|| format!("不支持的翻译语言: {language}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.translation_language = Some(normalized);
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_steam_relay_settings(mode: String, base: String) -> Result<(), String> {
    let mode = steam_relay::normalize_relay_mode(&mode)
        .ok_or_else(|| format!("不支持的 Steam 网络加速模式: {mode}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_relay_mode = Some(mode);
    settings.steam_relay_base = Some(steam_relay::normalize_relay_base(&base));
    save_app_settings_at(&path, &settings)
}

/// 设置页「本地代理」：空串 = 清空（退回系统代理）。填了但不合法时明确报错，
/// 免得用户以为设上了其实没生效。
#[tauri::command]
fn save_steam_proxy(proxy: String) -> Result<(), String> {
    let normalized = if proxy.trim().is_empty() {
        None
    } else {
        Some(steam_relay::normalize_steam_proxy(&proxy).ok_or_else(|| {
            format!("代理地址无效: {proxy}（示例：http://127.0.0.1:7897 或 socks5://127.0.0.1:1080）")
        })?)
    };
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_proxy = normalized;
    save_app_settings_at(&path, &settings)
}

/// 设置页「自动检测本地代理」：探常见 HTTP/SOCKS 端口，返回真能访问 Steam 的。
#[tauri::command]
async fn detect_steam_proxy() -> Vec<steam_relay::ProxyProbe> {
    steam_relay::detect_local_proxies().await
}

/// 设置 › 关于 › 反馈信息：上传最近一次启动的日志 + 用户描述。
#[tauri::command]
async fn submit_feedback(description: String) -> Result<feedback::FeedbackUploadResult, String> {
    feedback::submit_feedback(description).await
}

/// 设置页「Steam 网络加速 › 测试连接」：探一下中转的 /healthz，报耗时。
/// 探测失败也返回 Ok（结果里的 ok=false），让前端展示原因而不是当命令错误。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SteamRelayPingResult {
    ok: bool,
    latency_ms: u64,
    error: Option<String>,
}

#[tauri::command]
async fn ping_steam_relay(base: String) -> Result<SteamRelayPingResult, String> {
    let base = steam_relay::normalize_relay_base(&base);
    let started = std::time::Instant::now();
    // 与真实中转请求一致：绕过系统代理（残留死代理不该影响中转探测）。
    let result = steam_relay::steam_client(true)
        .get(format!("{base}/healthz"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    let latency_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(response) if response.status().is_success() => Ok(SteamRelayPingResult {
            ok: true,
            latency_ms,
            error: None,
        }),
        Ok(response) => Ok(SteamRelayPingResult {
            ok: false,
            latency_ms,
            error: Some(format!("中转服务返回 {}", response.status())),
        }),
        Err(error) => Ok(SteamRelayPingResult {
            ok: false,
            latency_ms,
            error: Some(error.to_string()),
        }),
    }
}

#[tauri::command]
fn save_font_contrast(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_contrast = Some(normalize_font_contrast(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_blur_strength(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.blur_strength = Some(normalize_blur_strength(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_font_scale(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_scale = Some(normalize_font_scale(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_font_family(value: String) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_family = normalize_font_family(&value);
    save_app_settings_at(&path, &settings)
}

/// 覆盖层背景色（设置 › 通用）：存 `#RRGGBB`，写空串 = 恢复默认纯红。
#[tauri::command]
fn save_overlay_background_color(value: String) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.overlay_background_color = normalize_overlay_background_color(&value);
    save_app_settings_at(&path, &settings)
}

/// 记住当前窗口尺寸（前端在拖动结束后防抖调用）。
///
/// 尺寸由后端自己读取（物理像素 ÷ 缩放系数 = 逻辑像素），前端就不需要
/// inner-size / scale-factor 两个窗口权限；最大化时不记忆（那不是用户想要的固定尺寸）。
#[tauri::command]
fn save_window_size(app: tauri::AppHandle) -> Result<(u32, u32), String> {
    use tauri::Manager;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口。".to_string())?;
    if window
        .is_maximized()
        .map_err(|error| format!("查询窗口状态失败: {error}"))?
    {
        return Err("窗口处于最大化状态，不记录尺寸。".to_string());
    }
    let size = window
        .inner_size()
        .map_err(|error| format!("读取窗口尺寸失败: {error}"))?;
    let factor = window
        .scale_factor()
        .map_err(|error| format!("读取屏幕缩放系数失败: {error}"))?;
    let width = (size.width as f64 / factor).round() as u32;
    let height = (size.height as f64 / factor).round() as u32;
    // 拖动过程会有低于下限的中间态：不落盘也不算错误，等拖完的下一帧再记。
    let (Some(width), Some(height)) =
        (normalize_window_width(width), normalize_window_height(height))
    else {
        return Err("窗口尺寸超出可记忆范围，已忽略。".to_string());
    };

    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.window_width = Some(width);
    settings.window_height = Some(height);
    save_app_settings_at(&path, &settings)?;
    Ok((width, height))
}

/// 恢复默认窗口尺寸：立即生效，同时清掉记住的尺寸（下次启动回到 conf 默认值）。
#[tauri::command]
fn restore_default_window_size(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;

    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口。".to_string())?;
    window
        .set_size(tauri::LogicalSize::new(WINDOW_WIDTH_MIN, WINDOW_HEIGHT_MIN))
        .map_err(|error| format!("设置窗口尺寸失败: {error}"))?;

    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.window_width = None;
    settings.window_height = None;
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn save_mod_loader_scheme(scheme: String) -> Result<(), String> {
    let normalized = normalize_mod_loader_scheme(&scheme)
        .ok_or_else(|| format!("不支持的模组加载器方案: {scheme}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.mod_loader_scheme = Some(normalized);
    save_app_settings_at(&path, &settings)
}

const FAST_LAUNCH_BACKUP_DIR: &str = "fast-launch-backup";
const FAST_LAUNCH_BACKUP_MANIFEST: &str = "manifest.json";
const FAST_LAUNCH_BACKUP_VERSION: u32 = 1;
const FAST_LAUNCH_LAUNCHER_FILES: [&str; 2] = ["ProjectZomboid64.json", "ProjectZomboid64.bat"];

/// 极速启动备份清单：只记录文件是否原本存在，文件内容本身放在同目录下。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchBackupEntry {
    name: String,
    existed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchBackupManifest {
    version: u32,
    created_at: String,
    game_path: String,
    files: Vec<FastLaunchBackupEntry>,
}

/// 启用极速启动时告诉前端：备份到了哪里；关闭时告诉前端是否真的恢复了备份。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FastLaunchSaveResult {
    backup_path: Option<String>,
    restored: bool,
    warning: Option<String>,
}

#[cfg(windows)]
fn project_zomboid_install_path() -> Option<PathBuf> {
    let roots = steam_roots();
    let libraries = all_libraries_from_roots(&roots);
    libraries
        .iter()
        .find_map(|library| game_path_from_library(library))
        .map(PathBuf::from)
}

fn fast_launch_backup_directory() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join(FAST_LAUNCH_BACKUP_DIR))
}

/// 备份前把文件里可能残留的极速启动 agent 摘掉：即使上一次开关没走完，
/// 备份也应该代表"没有极速启动"的干净配置。
fn sanitized_fast_launch_backup_content(name: &str, content: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(content) else {
        return content.to_vec();
    };
    match name {
        "ProjectZomboid64.json" => patch_json_launcher_with_agent(text, None, None, None)
            .map(|(patched, _)| patched.into_bytes())
            .unwrap_or_else(|_| content.to_vec()),
        "ProjectZomboid64.bat" => patch_batch_with_agent(text, None, None, None)
            .0
            .into_bytes(),
        _ => content.to_vec(),
    }
}

fn write_fast_launch_backup_at(
    game_path: &Path,
    backup_dir: &Path,
) -> Result<FastLaunchBackupManifest, String> {
    fs::create_dir_all(backup_dir).map_err(|error| format!("创建极速启动备份目录失败: {error}"))?;

    let mut entries = Vec::new();
    for name in FAST_LAUNCH_LAUNCHER_FILES {
        let source = game_path.join(name);
        let existed = source.is_file();
        if existed {
            let content = fs::read(&source)
                .map_err(|error| format!("读取 {} 失败: {error}", source.display()))?;
            let sanitized = sanitized_fast_launch_backup_content(name, &content);
            fs::write(backup_dir.join(name), sanitized)
                .map_err(|error| format!("写入极速启动备份 {} 失败: {error}", source.display()))?;
        }
        entries.push(FastLaunchBackupEntry {
            name: name.to_string(),
            existed,
        });
    }

    if entries.iter().all(|entry| !entry.existed) {
        return Err(
            "找不到 ProjectZomboid64.json 或 ProjectZomboid64.bat，无法备份极速启动配置"
                .to_string(),
        );
    }

    let manifest = FastLaunchBackupManifest {
        version: FAST_LAUNCH_BACKUP_VERSION,
        created_at: current_unix_timestamp()?,
        game_path: game_path.to_string_lossy().into_owned(),
        files: entries,
    };
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("序列化极速启动备份清单失败: {error}"))?;
    fs::write(
        backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST),
        format!("{content}\n"),
    )
    .map_err(|error| format!("写入极速启动备份清单失败: {error}"))?;
    Ok(manifest)
}

/// 先写到临时目录，成功后再替换旧备份；避免备份写到一半把上一份也弄丢。
fn create_fast_launch_backup_at(
    game_path: &Path,
    backup_dir: &Path,
) -> Result<FastLaunchBackupManifest, String> {
    let timestamp = current_unix_timestamp()?;
    let staging = backup_dir.with_file_name(format!("{FAST_LAUNCH_BACKUP_DIR}.tmp-{timestamp}"));
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("清理临时极速启动备份失败: {error}"))?;
    }
    let manifest = match write_fast_launch_backup_at(game_path, &staging) {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    if backup_dir.exists() {
        if let Err(error) = fs::remove_dir_all(backup_dir) {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("清理旧极速启动备份失败: {error}"));
        }
    }
    fs::rename(&staging, backup_dir).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        format!("保存极速启动备份失败: {error}")
    })?;
    Ok(manifest)
}

#[cfg(windows)]
fn create_fast_launch_backup() -> Result<Option<PathBuf>, String> {
    let game_path = project_zomboid_install_path()
        .ok_or_else(|| "找不到 Project Zomboid 安装目录，无法备份极速启动配置".to_string())?;
    let backup_dir = fast_launch_backup_directory()?;
    create_fast_launch_backup_at(&game_path, &backup_dir)?;
    Ok(Some(backup_dir))
}

#[cfg(not(windows))]
fn create_fast_launch_backup() -> Result<Option<PathBuf>, String> {
    Ok(None)
}

fn restore_fast_launch_backup_at(game_path: &Path, backup_dir: &Path) -> Result<bool, String> {
    let manifest_path = backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST);
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let content = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("读取极速启动备份清单失败: {error}"))?;
    let manifest: FastLaunchBackupManifest = serde_json::from_str(&content)
        .map_err(|error| format!("解析极速启动备份清单失败: {error}"))?;
    if manifest.version != FAST_LAUNCH_BACKUP_VERSION {
        return Err(format!("不支持的极速启动备份版本: {}", manifest.version));
    }

    let backup_game_path = PathBuf::from(manifest.game_path.as_str());
    let same_game_path = match (
        fs::canonicalize(&backup_game_path),
        fs::canonicalize(game_path),
    ) {
        (Ok(backup), Ok(current)) => backup == current,
        _ => backup_game_path.as_path() == game_path,
    };
    if !same_game_path {
        return Err(format!(
            "极速启动备份属于 {}，当前游戏目录是 {}；已保留备份，不自动恢复",
            backup_game_path.display(),
            game_path.display()
        ));
    }

    let safety_dir = game_path
        .join(".pzmanager-backups")
        .join(current_unix_timestamp()?);
    let mut writes = Vec::new();
    let mut removals = Vec::new();
    for entry in &manifest.files {
        if !FAST_LAUNCH_LAUNCHER_FILES.contains(&entry.name.as_str()) {
            return Err(format!("极速启动备份包含未知文件: {}", entry.name));
        }
        let target = game_path.join(&entry.name);
        if entry.existed {
            let backup = fs::read(backup_dir.join(&entry.name))
                .map_err(|error| format!("读取极速启动备份 {} 失败: {error}", entry.name))?;
            if fs::read(&target)
                .ok()
                .as_deref()
                .is_some_and(|current| current == backup.as_slice())
            {
                continue;
            }
            if target.is_file() {
                backup_file(&target, &safety_dir, &entry.name)?;
            }
            writes.push((target, backup));
        } else if target.is_file() {
            backup_file(&target, &safety_dir, &entry.name)?;
            removals.push(target);
        }
    }

    if writes.is_empty() && removals.is_empty() {
        return Ok(true);
    }
    apply_file_transaction(writes)?;
    for path in removals {
        fs::remove_file(&path)
            .map_err(|error| format!("删除极速启动残留文件 {} 失败: {error}", path.display()))?;
    }
    Ok(true)
}

#[cfg(windows)]
fn restore_fast_launch_backup() -> Result<bool, String> {
    let Some(game_path) = project_zomboid_install_path() else {
        return Ok(false);
    };
    restore_fast_launch_backup_at(&game_path, &fast_launch_backup_directory()?)
}

#[cfg(not(windows))]
fn restore_fast_launch_backup() -> Result<bool, String> {
    Ok(false)
}

#[tauri::command]
fn save_fast_launch(enabled: bool) -> Result<FastLaunchSaveResult, String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    let was_enabled = settings.fast_launch;

    // 先备份再落盘：备份失败就不启用，避免出现"开关开了但没有可恢复快照"。
    let backup_path = if enabled && !was_enabled {
        create_fast_launch_backup()?
    } else {
        None
    };

    settings.fast_launch = enabled;
    save_app_settings_at(&path, &settings)?;

    if enabled {
        return Ok(FastLaunchSaveResult {
            backup_path: backup_path.map(|path| path.to_string_lossy().into_owned()),
            restored: false,
            warning: None,
        });
    }

    // 关闭时优先整体恢复启用前的启动文件；恢复不了再退回只摘 -agentpath。
    let (restored, warning) = match restore_fast_launch_backup() {
        Ok(true) => (true, None),
        Ok(false) => (
            false,
            Some(
                "未找到可用的极速启动备份（或游戏目录不可用），已尝试只摘除 -agentpath。"
                    .to_string(),
            ),
        ),
        Err(error) => (
            false,
            Some(format!(
                "恢复极速启动备份失败，已只摘除 -agentpath：{error}"
            )),
        ),
    };
    if !restored {
        let policy_agent = settings
            .mod_loader_scheme
            .as_deref()
            .and_then(normalize_mod_loader_scheme);
        if let Err(error) = apply_launch_loader(None, policy_agent.as_deref(), None) {
            eprintln!("关闭极速启动后清理 -agentpath 失败（不影响设置已保存）: {error}");
        }
    }
    Ok(FastLaunchSaveResult {
        backup_path: None,
        restored,
        warning,
    })
}

/// 保存「静默内存清理」开关与阈值。
///
/// 阈值在这里就收敛到 `memclean` 的可用区间，并**立刻**同步给后台轮询线程：
/// 用户改完设置不需要重启启动器，下一次轮询就按新阈值判断。
#[tauri::command]
fn save_silent_memory_cleanup(enabled: bool, threshold_percent: u8) -> Result<(), String> {
    let threshold = memclean::normalize_threshold(threshold_percent);
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.silent_memory_cleanup = enabled;
    settings.silent_memory_cleanup_threshold = threshold;
    save_app_settings_at(&path, &settings)?;
    memclean::set_silent_cleanup(enabled, threshold);
    Ok(())
}

/// 保存「启动时清理内存」开关；真正的清理发生在 [`launch_project_zomboid`] 里。
#[tauri::command]
fn save_clean_memory_on_launch(enabled: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.clean_memory_on_launch = enabled;
    save_app_settings_at(&path, &settings)
}

/// 当前系统内存占用，给设置界面显示（也用来判断阈值合不合理）。
#[tauri::command]
fn get_memory_status() -> Result<memclean::MemoryStatus, String> {
    memclean::status().ok_or_else(|| "无法读取系统内存状态".to_string())
}

/// 立即清理一次（设置界面的「立即清理」按钮）。
#[tauri::command(async)]
fn clean_memory_now() -> Result<memclean::CleanupReport, String> {
    Ok(memclean::clean_memory())
}

fn current_steam_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for root in steam_roots() {
        let profile = steam_profile_from_root(&root);
        paths.extend(steam_config_paths(&root, profile.as_ref()));
    }
    paths.sort();
    paths.dedup();
    paths
}

fn patch_steam_launch_options_at_paths(
    config_paths: &[PathBuf],
    memory_mb: Option<u64>,
    extra_args: &[String],
) -> Result<bool, String> {
    let mut changed_files = Vec::new();
    for path in config_paths {
        let content =
            fs::read_to_string(path).map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
        let (patched, changed) =
            patch_steam_launch_options_content_with_memory(&content, memory_mb, extra_args)?;
        if changed {
            changed_files.push((path.clone(), patched.into_bytes()));
        }
    }
    if changed_files.is_empty() {
        return Ok(false);
    }
    apply_file_transaction(changed_files)?;
    Ok(true)
}

#[tauri::command(async)]
fn save_memory_limit(memory_mb: u64) -> Result<(), String> {
    validate_memory_limit_mb(memory_mb, system_memory_mb()?)?;
    let steam_configs = current_steam_config_paths();
    if steam_configs.is_empty() {
        return Err("找不到 Steam localconfig.vdf".to_string());
    }
    patch_steam_launch_options_at_paths(&steam_configs, Some(memory_mb), &[])?;
    // 同一份内存上限也写进游戏目录的 JSON / BAT，避免那里残留的旧 -Xmx 覆盖设置。
    force_zombie_buddy_policy(Some(memory_mb))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.memory_limit_mb = Some(memory_mb);
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
fn set_background_path(path: String) -> Result<(), String> {
    // `builtin:<id>` = 内置背景（随前端打包，磁盘上没有这个文件，不做目录校验）；
    // 其余一律按自定义图片 / 创意工坊背景校验目录。
    let trimmed = path.trim();
    let stored = if is_builtin_background_path(trimmed) {
        trimmed.to_string()
    } else {
        let candidate = PathBuf::from(trimmed);
        if !background_path_allowed(&candidate) {
            return Err("背景图片路径不在允许的背景目录中".to_string());
        }
        candidate.to_string_lossy().into_owned()
    };
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.background_path = Some(stored);
    save_app_settings_at(&settings_path, &settings)
}

/// 保存内置四季壁纸的手动 / 固定设置；内容由前端校验规整后再落盘。
#[tauri::command]
fn set_wallpaper_effect(effect: serde_json::Value) -> Result<(), String> {
    let settings_path = app_settings_path()?;
    let mut settings = load_app_settings_at(&settings_path)?;
    settings.wallpaper_effect = Some(effect);
    save_app_settings_at(&settings_path, &settings)
}

#[tauri::command]
fn list_custom_backgrounds() -> Result<Vec<BackgroundEntry>, String> {
    Ok(list_custom_backgrounds_at(&background_storage_directory(
        "custom",
    )?))
}

#[tauri::command(async)]
fn scan_workshop_backgrounds() -> Result<Vec<BackgroundEntry>, String> {
    #[cfg(not(windows))]
    {
        return Err("创意工坊背景扫描只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        Ok(workshop_backgrounds_at(&workshop_background_roots()))
    }
}

#[tauri::command(async)]
fn import_custom_background(source_path: String) -> Result<BackgroundEntry, String> {
    import_custom_background_at(Path::new(&source_path))
}

#[tauri::command]
fn read_background(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !background_path_allowed(&path) || !image_file(&path) || !is_regular_file(&path) {
        return Err("背景图片不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取背景图片失败: {error}"))
}

#[tauri::command]
fn read_jar_mod_poster(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !is_supported_image(&path) || !is_regular_file(&path) {
        return Err("JAR 模组海报不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取 JAR 模组海报失败: {error}"))
}

#[tauri::command]
fn read_lua_mod_poster(path: String) -> Result<tauri::ipc::Response, String> {
    let path = PathBuf::from(path);
    if !is_supported_image(&path) || !is_regular_file(&path) {
        return Err("Lua 模组海报不存在或路径无效".to_string());
    }
    fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|error| format!("读取 Lua 模组海报失败: {error}"))
}
#[tauri::command]
fn workshop_options() -> workshop::WorkshopOptions {
    workshop::workshop_options()
}

#[tauri::command]
async fn translate_developer_news_batch(
    jobs: Vec<news::TranslationJob>,
    language: String,
) -> Result<Vec<news_translation::TranslationResult>, String> {
    news_translation::translate_batch(jobs, language).await
}
#[tauri::command]
async fn browse_workshop(
    filters: workshop::WorkshopFilters,
) -> Result<workshop::WorkshopResponse, String> {
    workshop::fetch_workshop(filters).await
}

#[tauri::command]
async fn fetch_workshop_item_detail(
    id: String,
    language: String,
) -> Result<workshop::WorkshopItemDetail, String> {
    workshop::fetch_workshop_item_detail(id, language).await
}

#[tauri::command]
fn resource_options() -> resource::ResourceOptions {
    resource::resource_options()
}

#[tauri::command]
async fn fetch_shader_resources(
    query: resource::ResourceSearchQuery,
) -> Result<resource::ResourceSearchResponse, String> {
    resource::fetch_resources(query).await
}

#[tauri::command]
async fn fetch_shader_resource_files(
    resource_id: String,
    download_source: String,
) -> Result<Vec<resource::ResourceVersionPack>, String> {
    resource::fetch_resource_files(resource_id, download_source).await
}

#[tauri::command]
async fn fetch_minecraft_versions() -> Result<Vec<resource::MinecraftVersionOption>, String> {
    resource::fetch_minecraft_versions().await
}

#[tauri::command(async)]
fn install_zombie_buddy() -> Result<ZombieBuddyInstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("ZombieBuddy 安装只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = steam_roots();
        for steam_root in roots {
            let profile = steam_profile_from_root(&steam_root);
            let steam_configs = steam_config_paths(&steam_root, profile.as_ref());
            if steam_configs.is_empty() {
                continue;
            }

            let libraries = library_paths_from_config(&steam_root);
            let Some((game_path, source_path)) = install_locations_from_libraries(&libraries)
            else {
                continue;
            };

            if steam_configs.iter().try_fold(false, |needs_update, path| {
                let content = fs::read_to_string(path)
                    .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
                let (_, changed) = patch_steam_launch_options_content(&content)?;
                Ok::<_, String>(needs_update || changed)
            })? && steam_is_running()?
            {
                return Err("请先退出 Steam，再安装 ZombieBuddy".to_string());
            }

            return install_zombie_buddy_at_paths(
                Path::new(&game_path),
                &source_path,
                &steam_configs,
            );
        }

        Err("找不到 Project Zomboid 或 ZombieBuddy Workshop 文件".to_string())
    }
}

#[tauri::command(async)]
fn install_pz_optimization() -> Result<PzOptimizationInstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("PZ Optimization 一键安装只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let game_path = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
            .ok_or_else(|| "找不到 Project Zomboid 安装目录".to_string())?;
        let game_path = PathBuf::from(game_path);
        let status = pz_optimization_status(Some(&game_path), &libraries);

        if status.installed
            && status.installed_revision.is_some()
            && status.installed_revision == status.available_revision
        {
            return Ok(PzOptimizationInstallResult {
                changed: false,
                already_installed: true,
                source_path: status.source_path.unwrap_or_default(),
                installed_revision: status.installed_revision,
            });
        }

        let source_path = status
            .source_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| {
                "找不到匹配当前游戏版本的 PZ_Optimization Workshop 文件，请先在 Steam 订阅并等待下载完成。"
                    .to_string()
            })?;
        let installer_path = status
            .installer_path
            .as_deref()
            .map(PathBuf::from)
            .ok_or_else(|| "PZ_Optimization 缺少 install.ps1".to_string())?;

        if crate::overlay::imp::find_game_process().is_some() {
            return Err(
                "请先完全退出 Project Zomboid，再执行 PZ Optimization 一键安装。".to_string(),
            );
        }

        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&installer_path)
            .arg("-From")
            .arg(&source_path)
            .arg("-Dir")
            .arg(&game_path)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("启动 PZ Optimization 安装器失败: {error}"))?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = format!("{stdout}\n{stderr}").trim().to_string();
            return Err(if detail.is_empty() {
                format!("PZ Optimization 安装器退出码：{}", output.status)
            } else {
                format!("PZ Optimization 安装失败：{detail}")
            });
        }

        let installed = pz_optimization_status(Some(&game_path), &libraries);
        if !installed.installed {
            return Err("PZ Optimization 安装器已结束，但未检测到安装清单".to_string());
        }

        Ok(PzOptimizationInstallResult {
            changed: true,
            already_installed: false,
            source_path: source_path.to_string_lossy().into_owned(),
            installed_revision: installed.installed_revision,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PzOptimizationUninstallResult {
    changed: bool,
    message: String,
}

/// 卸载 PZ Optimization：调用安装器的 `-Uninstall`，按安装清单还原游戏目录里的类文件与启动文件。
///
/// 安装器自带 `pzopt\uninstall\install.ps1`（安装时复制到游戏目录），优先使用它，
/// 这样即使已经在创意工坊取消订阅也能卸载。游戏运行中直接拒绝，不让安装器无限等待。
#[tauri::command(async)]
fn uninstall_pz_optimization() -> Result<PzOptimizationUninstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("PZ Optimization 卸载只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let game_path = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
            .ok_or_else(|| "找不到 Project Zomboid 安装目录".to_string())?;
        let game_path = PathBuf::from(game_path);
        let status = pz_optimization_status(Some(&game_path), &libraries);

        if !status.installed {
            return Ok(PzOptimizationUninstallResult {
                changed: false,
                message: "PZ Optimization 未安装，无需卸载。".to_string(),
            });
        }

        if crate::overlay::imp::find_game_process().is_some() {
            return Err("请先完全退出 Project Zomboid，再卸载 PZ Optimization。".to_string());
        }

        let game_installer = game_path
            .join("pzopt")
            .join("uninstall")
            .join("install.ps1");
        let installer_path = if game_installer.is_file() {
            game_installer
        } else {
            status
                .installer_path
                .as_deref()
                .map(PathBuf::from)
                .ok_or_else(|| {
                    "找不到 PZ Optimization 卸载脚本（pzopt\\uninstall\\install.ps1）。".to_string()
                })?
        };

        let output = std::process::Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&installer_path)
            .arg("-Dir")
            .arg(&game_path)
            .arg("-Uninstall")
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("启动 PZ Optimization 卸载器失败: {error}"))?;

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = format!("{stdout}\n{stderr}").trim().to_string();
            return Err(if detail.is_empty() {
                format!("PZ Optimization 卸载器退出码：{}", output.status)
            } else {
                format!("PZ Optimization 卸载失败：{detail}")
            });
        }

        let after = pz_optimization_status(Some(&game_path), &libraries);
        if after.installed {
            return Err("卸载器已结束，但仍检测到 PZ Optimization 安装清单".to_string());
        }

        Ok(PzOptimizationUninstallResult {
            changed: true,
            message: "PZ Optimization 已卸载，下次启动为原版游戏。".to_string(),
        })
    }
}

/// 找到游戏安装目录，按「模组加载器方案」改写启动文件（内存必写，agent 视方案而定）。
///
/// 返回是否真的改写了文件。找不到游戏目录时返回 `Ok(false)`（例如未安装）。
#[cfg(windows)]
fn apply_launch_loader(
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    let roots = steam_roots();
    let libraries = all_libraries_from_roots(&roots);
    let Some(game_path) = libraries
        .iter()
        .find_map(|library| game_path_from_library(library))
    else {
        return Ok(false);
    };
    apply_launch_loader_at_paths(
        Path::new(&game_path),
        memory_mb,
        policy_agent,
        fast_launch_agent,
    )
}

/// 无条件按 ZombieBuddy 方案改写（显式动作与历史调用点）。
#[cfg(windows)]
fn force_zombie_buddy_policy(memory_mb: Option<u64>) -> Result<bool, String> {
    apply_launch_loader(memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

#[cfg(not(windows))]
fn apply_launch_loader(
    _memory_mb: Option<u64>,
    _policy_agent: Option<&str>,
    _fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(windows))]
fn force_zombie_buddy_policy(memory_mb: Option<u64>) -> Result<bool, String> {
    Ok(false)
}

/// 把 `ProjectZomboid64.json` / `ProjectZomboid64.bat` 里的 ZB agent 参数统一成
/// `-agentlib:zbNative=policy=allow-all`，让 ZombieBuddy 跳过模组审批确认界面。
/// 按「模组加载器方案」在启动前改写游戏自带的 `ProjectZomboid64.json` / `.bat`：
/// 内存上限始终写（和加载器无关，否则 JSON 里旧的 -Xmx 会覆盖启动器设置），
/// 加载器的 agent 参数只在方案启用了加载器时归一化（`policy_agent`），
/// 极速启动的 `-agentpath:<DLL>` 只在 `fast_launch_agent` 给了路径时写、否则一律摘掉。
fn apply_launch_loader_at_paths(
    game_path: &Path,
    memory_mb: Option<u64>,
    policy_agent: Option<&str>,
    fast_launch_agent: Option<&str>,
) -> Result<bool, String> {
    let json_path = game_path.join("ProjectZomboid64.json");
    let batch_path = game_path.join("ProjectZomboid64.bat");

    // `.bat` 的 `_JAVA_OPTIONS` 按空白切分，路径带空格会被拆坏，所以只有无空格路径才写进 .bat。
    let batch_agent = fast_launch_agent_for_batch(fast_launch_agent);
    if fast_launch_agent.is_some() && batch_agent.is_none() {
        // 只记日志：JSON 那条已经足够让 Steam 启动路径生效。
        eprintln!("极速启动：DLL 路径含空格，跳过 .bat 的 _JAVA_OPTIONS 改写");
    }

    let mut files = Vec::new();
    if json_path.is_file() {
        let content = fs::read_to_string(&json_path)
            .map_err(|error| format!("读取 ProjectZomboid64.json 失败: {error}"))?;
        let (patched, changed) =
            patch_json_launcher_with_agent(&content, memory_mb, policy_agent, fast_launch_agent)?;
        if changed {
            files.push((json_path.clone(), patched.into_bytes()));
        }
    }
    if batch_path.is_file() {
        let content = fs::read_to_string(&batch_path)
            .map_err(|error| format!("读取 ProjectZomboid64.bat 失败: {error}"))?;
        let (patched, changed) =
            patch_batch_with_agent(&content, memory_mb, policy_agent, batch_agent);
        if changed {
            files.push((batch_path.clone(), patched.into_bytes()));
        }
    }
    if files.is_empty() {
        return Ok(false);
    }

    let backup_dir = game_path
        .join(".pzmanager-backups")
        .join(current_unix_timestamp()?);
    for (path, _) in &files {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("无效备份文件路径: {}", path.display()))?;
        backup_file(path, &backup_dir, name)?;
    }
    apply_file_transaction(files)?;
    Ok(true)
}

/// 显式动作（例如配置 Viewpoint 3D）用的入口：无条件按 ZombieBuddy 方案改写。
fn force_zombie_buddy_policy_at_paths(
    game_path: &Path,
    memory_mb: Option<u64>,
) -> Result<bool, String> {
    apply_launch_loader_at_paths(game_path, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

#[tauri::command(async)]
fn configure_viewpoint_three_d() -> Result<ZombieBuddyInstallResult, String> {
    #[cfg(not(windows))]
    {
        return Err("Viewpoint 3D 模组配置只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let roots = steam_roots();
        let libraries = all_libraries_from_roots(&roots);
        let Some(game_path) = libraries
            .iter()
            .find_map(|library| game_path_from_library(library))
        else {
            return Err("找不到 Project Zomboid 安装目录".to_string());
        };

        let mut steam_configs = Vec::new();
        for root in &roots {
            let profile = steam_profile_from_root(root);
            steam_configs.extend(steam_config_paths(root, profile.as_ref()));
        }
        steam_configs.sort();
        steam_configs.dedup();

        let needs_steam_update = steam_configs.iter().try_fold(false, |needs_update, path| {
            let content = fs::read_to_string(path)
                .map_err(|error| format!("读取 Steam 配置失败: {error}"))?;
            let (_, changed) = patch_steam_launch_options_content(&content)?;
            Ok::<_, String>(needs_update || changed)
        })?;
        if needs_steam_update && steam_is_running()? {
            return Err("请先退出 Steam，再配置 Project Viewpoint 3D 模组".to_string());
        }

        configure_viewpoint_three_d_at_paths(Path::new(&game_path), &libraries, &steam_configs)
    }
}

/// 系统物理内存总量（MB）。
///
/// 读取内存状态、清理工作集都归 `memclean`；这里只做一次转交，
/// 免得项目里出现两套 `GlobalMemoryStatusEx` 声明（字段一改就两边不同步）。
fn system_memory_mb() -> Result<u64, String> {
    memclean::status()
        .map(|status| status.total_mb)
        .ok_or_else(|| "无法读取系统物理内存".to_string())
}

#[tauri::command]
fn get_system_memory_mb() -> Result<u64, String> {
    system_memory_mb()
}

/// 把界面给的覆盖层 DLL 路径解析成绝对路径，并**确认文件真的存在**。
///
/// 不存在就返回 `None`（= 不写 `-agentpath`）：`-agentpath` 指向的文件找不到会让
/// JVM 直接启动失败，而我们的硬约束是"任何一步不成立就回退原生扫描"，绝不能把游戏弄起不来。
fn resolve_fast_launch_agent_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    crate::overlay::imp::normalize_dll_path(trimmed)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command(async)]
fn launch_project_zomboid(
    memory_mb: u64,
    launch_args: Vec<String>,
    fast_launch_dll_path: Option<String>,
) -> Result<(), String> {
    #[cfg(not(windows))]
    {
        let _ = (memory_mb, launch_args, fast_launch_dll_path);
        return Err("启动 Project Zomboid 只支持 Windows".to_string());
    }

    #[cfg(windows)]
    {
        let total_memory_mb = system_memory_mb()?;
        validate_memory_limit_mb(memory_mb, total_memory_mb)?;
        let steam_configs = current_steam_config_paths();
        if steam_configs.is_empty() {
            return Err("找不到 Steam localconfig.vdf".to_string());
        }
        // 读一次设置：「模组加载器方案」决定 agent 处理，`fastLaunch` 决定要不要写极速启动的 agent。
        let settings = load_app_settings_at(&app_settings_path()?).ok();
        let scheme = settings
            .as_ref()
            .and_then(|settings| settings.mod_loader_scheme.clone())
            .and_then(|value| normalize_mod_loader_scheme(&value))
            .unwrap_or_else(|| DEFAULT_MOD_LOADER_SCHEME.to_string());
        let policy_agent = mod_loader_policy_agent(&scheme);

        // 极速启动（`-agentpath`）的三个前提：设置开关打开、方案是 ZombieBuddy、DLL 真的存在。
        // 任一不满足都传 None —— 那会**摘掉**已经写在启动文件里的 `-agentpath:`，
        // 保证"关掉开关后下一次启动一定回到 ZombieBuddy 原生扫描"。
        let fast_launch_enabled = settings
            .as_ref()
            .is_some_and(|settings| settings.fast_launch);
        let fast_launch_agent = if fast_launch_enabled && policy_agent.is_some() {
            fast_launch_dll_path
                .as_deref()
                .and_then(resolve_fast_launch_agent_path)
        } else {
            None
        };
        match fast_launch_agent.as_deref() {
            Some(path) => eprintln!("极速启动：本次启动会带 -agentpath:{path}"),
            None if fast_launch_enabled && policy_agent.is_some() => eprintln!(
                "极速启动：开关已开但拿不到可用的覆盖层 DLL 路径，本次不注入（走 ZombieBuddy 原生扫描）"
            ),
            None => {}
        }

        // 方案不使用加载器时，先把默认启动参数里的加载器 agent 摘掉，别背着方案偷偷挂上。
        let mut effective_launch_args = launch_args;
        if policy_agent.is_none() {
            strip_loader_agent_tokens(&mut effective_launch_args);
        }
        patch_steam_launch_options_at_paths(
            &steam_configs,
            Some(memory_mb),
            &effective_launch_args,
        )?;

        // 再改写游戏自带的启动文件：内存上限始终写（与加载器无关，否则 JSON 里旧的 -Xmx
        // 会覆盖启动器设置）；只有方案启用了加载器时才把它的 agent 归一化成 policy=allow-all
        // （策略只在 agent 第一次加载时锁定，而 JSON/BAT 往往先于 Steam 参数被 JVM 读到）。
        apply_launch_loader(Some(memory_mb), policy_agent, fast_launch_agent.as_deref())?;

        // 「启动时清理内存」：拉起游戏之前先把各进程的工作集还给系统。
        // 此刻游戏还没起来，所以这次清理不会被 memclean 的保护名单挡掉任何东西。
        // 清理失败/读不到内存状态都不影响启动——这是纯优化，不能变成启动的前置条件。
        if settings
            .as_ref()
            .is_some_and(|settings| settings.clean_memory_on_launch)
        {
            let report = memclean::clean_memory();
            eprintln!(
                "启动前清理内存：释放 {} MB（清理 {} 个进程，跳过 {} 个，耗时 {} ms）",
                report.freed_mb, report.trimmed, report.skipped, report.elapsed_ms
            );
        }

        Command::new("cmd")
            .args([
                "/C",
                "start",
                "",
                &format!("steam://rungameid/{PROJECT_ZOMBOID_APP_ID}"),
            ])
            .spawn()
            .map_err(|error| format!("启动 Project Zomboid 失败: {error}"))?;
        Ok(())
    }
}

/// WebView2 默认把用户数据目录放在 `%LOCALAPPDATA%\<identifier>`。
/// 不再探测默认位置是否可写：普通文件写入能成功，并不代表 WebView2 的浏览器进程
/// 能真正使用该目录，探测结果无法作为依据。开发期直接使用固定的、已验证可启动的目录。
#[cfg(target_os = "windows")]
fn ensure_webview2_data_folder() {
    // 外部已指定则尊重
    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some() {
        return;
    }

    // 不再探测默认位置:默认位置"可写"不代表 WebView2 能用
    let dir = if cfg!(debug_assertions) {
        // 开发期:放在项目外,避免被 Vite 监视
        std::path::PathBuf::from(r"F:\webview2-data\pzmanager")
    } else {
        // 发布版:仍然用默认位置,不改变用户机器上的行为
        return;
    };

    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("无法创建 WebView2 数据目录 {dir:?}: {e}"));

    // 启动阶段单线程;如果 edition 是 2024,要包进 unsafe { }
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &dir);
}
/// 目录能被创建、且确实写得进文件才算可写。
/// 只判断存在性不够：受限环境下目录可能"可读不可写"。
#[cfg(target_os = "windows")]
fn dir_is_writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".pzmanager-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 必须在 tauri::Builder 构建之前设置，WebView2 才会读取它。
    #[cfg(target_os = "windows")]
    ensure_webview2_data_folder();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // 静默内存清理：把持久化的开关/阈值交给后台轮询线程。
            // 读配置失败就按"关闭"处理——启动流程绝不能被清理功能拖住。
            let loaded_settings = app_settings_path()
                .and_then(|path| load_app_settings_at(&path))
                .ok();
            let (enabled, threshold) = loaded_settings
                .as_ref()
                .map(|settings| {
                    (
                        settings.silent_memory_cleanup,
                        settings.silent_memory_cleanup_threshold,
                    )
                })
                .unwrap_or((false, memclean::DEFAULT_THRESHOLD_PERCENT));
            memclean::set_silent_cleanup(enabled, threshold);
            memclean::start_silent_loop(app.handle().clone());

            // 恢复上一次记住的窗口尺寸（拖动窗口边缘调整后会自动记住）。
            // 默认 840×560 由 tauri.conf.json 提供，这里只在有有效记录时覆盖。
            if let Some(settings) = loaded_settings.as_ref() {
                let remembered_size = match (settings.window_width, settings.window_height) {
                    (Some(width), Some(height)) => {
                        match (normalize_window_width(width), normalize_window_height(height)) {
                            (Some(width), Some(height)) => Some((width, height)),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((width, height)) = remembered_size {
                    use tauri::Manager;
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_size(tauri::LogicalSize::new(width, height));
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_project_zomboid,
            get_system_memory_mb,
            get_app_settings,
            save_memory_limit,
            save_developer_mode,
            save_steam_account_hidden,
            save_translation_language,
            save_font_contrast,
            save_blur_strength,
            save_font_scale,
            save_font_family,
            save_overlay_background_color,
            save_window_size,
            restore_default_window_size,
            save_mod_loader_scheme,
            save_fast_launch,
            save_silent_memory_cleanup,
            save_clean_memory_on_launch,
            save_steam_relay_settings,
            save_steam_proxy,
            detect_steam_proxy,
            submit_feedback,
            ping_steam_relay,
            get_memory_status,
            clean_memory_now,
            fast_launch_cache_status,
            startup_timing,
            set_background_path,
            set_wallpaper_effect,
            list_custom_backgrounds,
            scan_workshop_backgrounds,
            start_workshop_background_scan,
            import_custom_background,
            read_background,
            read_jar_mod_poster,
            read_lua_mod_poster,
            start_zombie_buddy_jar_scan,
            list_lua_mods,
            set_lua_mod_enabled,
            set_lua_mods_enabled,
            save_lua_mod_order,
            set_zombie_buddy_mod_decision,
            set_zombie_buddy_mods_decisions,
            apply_jar_mod_file_state,
            list_mod_presets,
            save_mod_preset,
            apply_mod_preset,
            delete_mod_preset,
            install_zombie_buddy,
            install_pz_optimization,
            uninstall_pz_optimization,
            configure_viewpoint_three_d,
            launch_project_zomboid,
            overlay::inject_overlay,
            overlay_session::start_overlay_session,
            overlay_session::check_game_process,
            overlay_session::stop_game_process,
            list_viewpoint_shots,
            read_viewpoint_shot,
            list_viewpoint_shaderpacks,
            import_viewpoint_shaderpack,
            delete_viewpoint_shaderpack,
            workshop_options,
            browse_workshop,
            fetch_workshop_item_detail,
            resource_options,
            fetch_shader_resources,
            fetch_shader_resource_files,
            save_shader_download_settings,
            download_shader_file,
            fetch_minecraft_versions,
            fetch_developer_news,
            translate_developer_news_batch,
            check_launcher_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        apply_file_transaction, apply_jar_mod_state_to_roots, apply_launch_loader_at_paths,
        build_fast_launch_cache, class_annotations, compose_title_background_at,
        configure_viewpoint_three_d_at_paths, create_fast_launch_backup_at, delete_shaderpack_at,
        discover_mod_info_files, fast_launch_cache_matches, fast_launch_cache_mods_at,
        force_zombie_buddy_policy_at_paths, game_path_from_library, import_shaderpack_to,
        install_locations_from_libraries, install_zombie_buddy_at_paths,
        is_builtin_background_path, jar_scan_candidates, jar_scan_inputs,
        library_paths_from_config, list_viewpoint_shaderpacks_at, list_viewpoint_shots_at,
        load_app_settings_at, merge_lua_mod_entry, mod_jar_path, mod_loader_policy_agent,
        mod_poster_path, normalize_blur_strength, normalize_font_contrast, normalize_font_family,
        normalize_font_scale, normalize_mod_loader_scheme, normalize_translation_language,
        normalize_window_height, normalize_window_width, order_mod_ids_by_requires,
        normalize_overlay_background_color,
        parse_default_mod_ids, parse_java_mod_info, parse_lua_mod_info, parse_lua_sorting_rules,
        patch_batch_content, patch_batch_content_with_memory, patch_batch_with_agent,
        patch_json_launcher_content, patch_json_launcher_content_with_memory,
        patch_json_launcher_with_agent, patch_steam_launch_options_content,
        patch_steam_launch_options_content_with_memory, pick_lua_mod_id, plan_lua_mod_enable,
        pz_optimization_status, read_default_mod_ids, read_fast_launch_cache_at,
        read_jar_mod_state, read_viewpoint_shot_at, restore_fast_launch_backup_at,
        save_app_settings_at, scan_jar_mods_at_with_callback, set_jar_mod_info_enabled,
        should_write_fast_launch_cache, steam_config_paths, steam_executable_from_roots,
        steam_profile_from_loginusers, steam_user_data_id, strip_loader_agent_tokens,
        validate_memory_limit_mb, vdf_value, viewpoint_three_d_status, workshop_path_from_library,
        write_default_mod_ids, write_fast_launch_cache_at, write_jar_mod_state,
        zombie_buddy_source_from_libraries, zombie_buddy_status, AppSettings, FastLaunchCache,
        JarModStateEntry, JarModStateFile, JavaPatchMethod, LuaModEntry, ScannedJarMod,
        SteamProfile, FAST_LAUNCH_BACKUP_MANIFEST, FAST_LAUNCH_BACKUP_VERSION,
        FAST_LAUNCH_CACHE_FILE, FAST_LAUNCH_CACHE_FORMAT, PROJECT_ZOMBOID_APP_ID,
        VIEWPOINT_MOD_ID, ZOMBIE_BUDDY_AGENT, ZOMBIE_BUDDY_MOD_ID,
        ZOMBIE_BUDDY_POLICY_AGENT,
    };
    use image::{GenericImageView, RgbaImage};
    use std::{fs, io::Write, path::Path};
    use zip::{write::SimpleFileOptions, ZipWriter};

    #[test]
    fn parses_java_mod_info_fields_case_insensitively() {
        let mod_info = parse_java_mod_info(
            "name=Wind Sway\nid=WindSway\nJavaJarFile=media/java/client/windsway.jar\nJavaPkgName=pzmod.windsway\n",
        )
        .unwrap();

        assert_eq!(mod_info.name, "Wind Sway");
        assert_eq!(mod_info.id, "WindSway");
        assert_eq!(mod_info.jar_file, "media/java/client/windsway.jar");
        assert_eq!(mod_info.package_name, "pzmod.windsway");
    }

    #[test]
    fn ignores_mod_info_without_a_jar_or_package() {
        assert!(parse_java_mod_info("name=Lua only mod\nid=lua_mod\n").is_none());
        assert!(parse_java_mod_info("javaJarFile=mod.jar\n").is_none());
    }

    #[test]
    fn detects_pz_optimization_workshop_and_installed_versions() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        let classes = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("3805285544")
            .join("mods")
            .join("PZ_Optimization")
            .join("42")
            .join("pzopt-classes");
        fs::create_dir_all(classes.join("pzopt")).unwrap();
        fs::write(classes.join("pzopt-files.txt"), "pzopt/Test.class\n").unwrap();
        fs::write(classes.join("pzopt").join("Test.class"), b"class").unwrap();
        fs::write(
            classes.join("pzopt").join("build-info.properties"),
            "revision=abc1234567\nbuilt=42\n",
        )
        .unwrap();
        fs::write(classes.parent().unwrap().join("install.ps1"), "# fixture\n").unwrap();

        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        let libraries = [library];
        let available = pz_optimization_status(Some(&game), &libraries);
        assert!(available.available);
        assert!(!available.installed);
        assert_eq!(available.available_revision.as_deref(), Some("abc1234567"));

        fs::write(game.join("pzopt-installed.txt"), "# installed\n").unwrap();
        fs::create_dir_all(game.join("pzopt")).unwrap();
        fs::write(
            game.join("pzopt").join("build-info.properties"),
            "revision=abc1234567\n",
        )
        .unwrap();
        let installed = pz_optimization_status(Some(&game), &libraries);
        assert!(installed.installed);
        assert_eq!(installed.installed_revision.as_deref(), Some("abc1234567"));
    }

    #[test]
    fn persists_background_and_memory_settings() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let settings = AppSettings {
            background_path: Some("C:\\background.png".to_string()),
            shader_download_url: None,
            shader_download_directory: None,
            shader_download_threads: None,
            memory_limit_mb: Some(8192),
            translation_language: Some("ja".to_string()),
            font_contrast: Some(45),
            blur_strength: Some(120),
            font_family: Some("\"Microsoft YaHei UI\", sans-serif".to_string()),
            font_scale: Some(110),
            window_width: Some(970),
            window_height: Some(600),
            overlay_background_color: Some("#1E90FF".to_string()),
            mod_loader_scheme: Some("none".to_string()),
            fast_launch: false,
            developer_mode: true,
            steam_account_hidden: true,
            silent_memory_cleanup: true,
            silent_memory_cleanup_threshold: 85,
            clean_memory_on_launch: true,
            wallpaper_effect: None,
            steam_relay_mode: Some("relay".to_string()),
            steam_relay_base: Some("https://mirror.example/steam".to_string()),
            steam_proxy: Some("socks5://127.0.0.1:1080".to_string()),
        };

        save_app_settings_at(&path, &settings).unwrap();

        assert_eq!(load_app_settings_at(&path).unwrap(), settings);
    }

    #[test]
    fn loads_legacy_settings_without_developer_mode() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        std::fs::write(&path, r#"{"backgroundPath":null,"memoryLimitMb":8192}"#).unwrap();

        let settings = load_app_settings_at(&path).unwrap();

        assert!(!settings.developer_mode);
        assert!(!settings.steam_account_hidden);
        assert_eq!(settings.memory_limit_mb, Some(8192));
        assert_eq!(settings.translation_language, None);
        assert_eq!(settings.font_contrast, None);
        assert_eq!(settings.blur_strength, None);
        assert_eq!(settings.mod_loader_scheme, None);
        assert_eq!(settings.font_family, None);
        assert_eq!(settings.font_scale, None);
        assert_eq!(settings.window_width, None);
        assert_eq!(settings.window_height, None);
        assert_eq!(settings.overlay_background_color, None);
    }

    /// 覆盖层背景色：接受 #RGB / #RRGGBB，统一成大写 #RRGGBB；坏值按"没设置过"处理。
    #[test]
    fn normalizes_overlay_background_colors() {
        assert_eq!(
            normalize_overlay_background_color("#1e90ff"),
            Some("#1E90FF".to_string())
        );
        assert_eq!(
            normalize_overlay_background_color("ff0"),
            Some("#FFFF00".to_string())
        );
        assert_eq!(
            normalize_overlay_background_color("  #AABBCC  "),
            Some("#AABBCC".to_string())
        );
        // 坏值不能变成黑色：返回 None，覆盖层继续用默认纯红。
        assert_eq!(normalize_overlay_background_color("#GGGGGG"), None);
        assert_eq!(normalize_overlay_background_color("#12345"), None);
        assert_eq!(normalize_overlay_background_color(""), None);
        assert_eq!(
            normalize_overlay_background_color("red"),
            None,
            "颜色名不支持，只接受十六进制"
        );
    }

    /// 界面字号/字体与窗口尺寸的归一化：坏值要么夹到范围内，要么按"没设置过"处理。
    #[test]
    fn clamps_font_scale_and_validates_window_size() {
        use super::{FONT_SCALE_MAX, FONT_SCALE_MIN, WINDOW_WIDTH_MIN};

        assert_eq!(normalize_font_scale(0), FONT_SCALE_MIN);
        assert_eq!(normalize_font_scale(100), 100);
        assert_eq!(normalize_font_scale(255), FONT_SCALE_MAX);
        // 空字体 = 默认字体栈；前后空白要裁掉。
        assert_eq!(normalize_font_family("   "), None);
        assert_eq!(
            normalize_font_family("  Consolas, monospace "),
            Some("Consolas, monospace".to_string())
        );
        // 窗口尺寸低于下限 / 超过防御上限都不落盘。
        assert_eq!(normalize_window_width(WINDOW_WIDTH_MIN - 1), None);
        assert_eq!(normalize_window_width(WINDOW_WIDTH_MIN), Some(WINDOW_WIDTH_MIN));
        assert_eq!(normalize_window_height(560), Some(560));
        assert_eq!(normalize_window_height(559), None);
        assert_eq!(normalize_window_width(10001), None);
    }

    fn scanned_jar(id: &str, decision: Option<bool>, disabled: bool) -> ScannedJarMod {
        ScannedJarMod {
            id: id.to_string(),
            info_id: id.to_string(),
            mod_info_path: format!("H:/mods/{id}/mod.info"),
            disabled,
            workshop_id: None,
            jar_path: format!("H:/mods/{id}/{id}.jar"),
            poster_path: None,
            package_name: id.to_lowercase(),
            jar_hash: format!("hash-{id}"),
            // 真实扫描产出的是**内部名**（斜杠分隔）——测试也照这个来，
            // 否则"缓存里是内部名、注入器却当二进制名用"这类 bug 测不出来。
            patch_classes: vec![format!("{}/Patch_A", id.to_lowercase())],
            patch_methods: Vec::new(),
            lua_classes: vec![format!("{}/Lua_A", id.to_lowercase())],
            decision,
        }
    }

    #[test]
    fn builds_fast_launch_cache_from_approved_and_enabled_mods_only() {
        let scanned = vec![
            scanned_jar("Viewpoint", Some(true), false),
            scanned_jar("Pending", None, false),
            scanned_jar("Rejected", Some(false), false),
            scanned_jar("DisabledApproved", Some(true), true),
        ];
        let cache = build_fast_launch_cache(&scanned, 42);
        assert_eq!(cache.format, FAST_LAUNCH_CACHE_FORMAT);
        assert_eq!(cache.generated_at, 42);
        // 未批准 / 已拒绝 / 已禁用的都不能进缓存：缓存和启用列表不一致会丢补丁。
        assert_eq!(cache.mods.len(), 1);
        assert_eq!(cache.mods[0].mod_name, "Viewpoint");
        // 扫描给的是内部名（viewpoint/Patch_A），缓存里必须落成二进制名（注入器要用 forName）。
        assert_eq!(
            cache.mods[0].patch_classes,
            vec!["viewpoint.Patch_A".to_string()]
        );
        assert_eq!(
            cache.mods[0].lua_classes,
            vec!["viewpoint.Lua_A".to_string()]
        );

        // 指纹只跟「启用列表 + jar 内容」有关：换个生成时间不影响。
        assert_eq!(
            build_fast_launch_cache(&scanned, 99).mod_set_hash,
            cache.mod_set_hash
        );
        // jar 内容变了（hash 变）指纹必须跟着变，注入器才能发现缓存过期。
        let mut changed = scanned.clone();
        changed[0].jar_hash = "hash-changed".to_string();
        assert_ne!(
            build_fast_launch_cache(&changed, 42).mod_set_hash,
            cache.mod_set_hash
        );
    }

    #[test]
    fn fast_launch_cache_consistency_check() {
        let temp = tempfile::tempdir().unwrap();
        let approved = vec![
            scanned_jar("Viewpoint", Some(true), false),
            scanned_jar("Rejected", Some(false), false),
        ];
        let cache = build_fast_launch_cache(&approved, 11);
        let path = write_fast_launch_cache_at(temp.path(), &cache).unwrap();

        // 读回来的缓存必须依然判定为"一致"。
        let restored = read_fast_launch_cache_at(&path).unwrap();
        assert!(fast_launch_cache_matches(&restored, &approved));

        // 启用列表变了（多批准一个）→ 指纹变 → 必须判定不一致，注入器要回退原生扫描。
        let mut changed = approved.clone();
        changed[1].decision = Some(true);
        assert!(!fast_launch_cache_matches(&restored, &changed));

        // jar 内容变了（hash 变）也要判定不一致。
        let mut rehashed = approved.clone();
        rehashed[0].jar_hash = "hash-other".to_string();
        assert!(!fast_launch_cache_matches(&restored, &rehashed));
    }

    #[test]
    fn writes_fast_launch_cache_file() {
        let temp = tempfile::tempdir().unwrap();
        let scanned = vec![scanned_jar("Viewpoint", Some(true), false)];
        let cache = build_fast_launch_cache(&scanned, 7);

        let path = write_fast_launch_cache_at(temp.path(), &cache).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            FAST_LAUNCH_CACHE_FILE
        );

        // 落盘内容必须能原样读回：注入器就是读这个文件拿模组清单和指纹。
        let raw = fs::read_to_string(&path).unwrap();
        let restored: FastLaunchCache = serde_json::from_str(&raw).unwrap();
        assert_eq!(restored, cache);
        assert_eq!(restored.mods.len(), 1);
        assert_eq!(restored.mods[0].mod_name, "Viewpoint");
    }

    #[test]
    fn normalizes_mod_loader_scheme_setting() {
        assert_eq!(
            normalize_mod_loader_scheme("zombieBuddy").as_deref(),
            Some("zombieBuddy")
        );
        assert_eq!(
            normalize_mod_loader_scheme(" ZOMBIEBUDDY ").as_deref(),
            Some("zombieBuddy")
        );
        assert_eq!(normalize_mod_loader_scheme("none").as_deref(), Some("none"));
        assert_eq!(normalize_mod_loader_scheme("vfe"), None);
        assert_eq!(
            mod_loader_policy_agent("zombieBuddy"),
            Some(ZOMBIE_BUDDY_POLICY_AGENT)
        );
        assert_eq!(mod_loader_policy_agent("none"), None);
    }

    #[test]
    fn strips_loader_agent_from_launch_args_without_loader() {
        // 前端默认启动参数带 -agentlib:zbNative；方案不使用加载器时要摘掉，别偷偷挂上。
        let mut args = vec![ZOMBIE_BUDDY_AGENT.to_string(), "--".to_string()];
        assert!(strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["--".to_string()]);

        // 带策略的写法、以及夹杂空白的写法都算加载器 agent。
        let mut args = vec![
            " -agentlib:zbNative=policy=deny-new ".to_string(),
            "-Xmx4096m".to_string(),
        ];
        assert!(strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["-Xmx4096m".to_string()]);

        // 没有加载器参数时原样返回。
        let mut args = vec!["--".to_string()];
        assert!(!strip_loader_agent_tokens(&mut args));
        assert_eq!(args, vec!["--".to_string()]);
    }

    #[test]
    fn skips_loader_agent_when_scheme_has_no_loader() {
        // 方案选了"不使用加载器"：只改内存，绝不能往启动文件里塞 agent。
        let json = r#"{"vmArgs":["-Xmx3072m","-Dfoo=bar"]}"#;
        let (patched_json, changed) =
            patch_json_launcher_with_agent(json, Some(8192), None, None).unwrap();
        assert!(changed);
        assert!(!patched_json.contains("zbNative"));
        assert!(patched_json.contains("-Xmx8192m"));

        let batch = "SET _JAVA_OPTIONS=-Xmx3072m\r\n";
        let (patched_batch, changed) = patch_batch_with_agent(batch, Some(8192), None, None);
        assert!(changed);
        assert!(!patched_batch.contains("zbNative"));
        assert!(patched_batch.contains("-Xmx8192m"));

        // 启用加载器时照旧写入并归一化 agent。
        let (with_agent, _) =
            patch_json_launcher_with_agent(json, Some(8192), Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
                .unwrap();
        assert!(with_agent.contains(ZOMBIE_BUDDY_POLICY_AGENT));
    }

    #[test]
    fn clamps_font_contrast_setting() {
        assert_eq!(normalize_font_contrast(0), 0);
        assert_eq!(normalize_font_contrast(45), 45);
        assert_eq!(normalize_font_contrast(100), 100);
        assert_eq!(normalize_font_contrast(255), 100);
    }

    #[test]
    fn clamps_blur_strength_setting() {
        assert_eq!(normalize_blur_strength(0), 0);
        assert_eq!(normalize_blur_strength(120), 120);
        assert_eq!(normalize_blur_strength(200), 200);
        assert_eq!(normalize_blur_strength(255), 200);
    }

    #[test]
    fn recognizes_builtin_background_paths() {
        assert!(is_builtin_background_path("builtin:default"));
        assert!(is_builtin_background_path(" builtin:default "));
        assert!(!is_builtin_background_path(r"C:\Users\me\Pictures\a.png"));
        assert!(!is_builtin_background_path(""));
    }

    #[test]
    fn normalizes_translation_language_setting() {
        assert_eq!(
            normalize_translation_language("zh-CN").as_deref(),
            Some("zh-CN")
        );
        assert_eq!(
            normalize_translation_language("zh-cn").as_deref(),
            Some("zh-CN")
        );
        assert_eq!(
            normalize_translation_language(" zh-tw ").as_deref(),
            Some("zh-TW")
        );
        assert_eq!(normalize_translation_language("RU").as_deref(), Some("ru"));
        // 设置页没提供的语言和空值不能被悄悄当成 English，交给默认值处理。
        assert_eq!(normalize_translation_language("de"), None);
        assert_eq!(normalize_translation_language(""), None);
    }

    #[test]
    fn composes_four_title_images_in_quadrants() {
        let temp = tempfile::tempdir().unwrap();
        let paths = [
            temp.path().join("title.png"),
            temp.path().join("title2.png"),
            temp.path().join("title3.png"),
            temp.path().join("title4.png"),
        ];
        for (index, path) in paths.iter().enumerate() {
            let image = RgbaImage::from_pixel(2, 3, image::Rgba([index as u8, 0, 0, 255]));
            image.save(path).unwrap();
        }
        let output = temp.path().join("joined.png");

        compose_title_background_at([&paths[0], &paths[1], &paths[2], &paths[3]], &output).unwrap();

        let joined = image::open(output).unwrap();
        assert_eq!(joined.dimensions(), (4, 6));
        assert_eq!(joined.get_pixel(0, 0)[0], 0);
        assert_eq!(joined.get_pixel(2, 0)[0], 1);
        assert_eq!(joined.get_pixel(0, 3)[0], 2);
        assert_eq!(joined.get_pixel(2, 3)[0], 3);
    }

    #[test]
    fn resolves_common_mod_info_jar_from_a_version_directory() {
        let temp = tempfile::tempdir().unwrap();
        let mod_root = temp.path().join("mods").join("Viewpoint");
        let common = mod_root.join("common");
        let versioned_jar = mod_root.join("42").join("media/java/client/Viewpoint.jar");
        fs::create_dir_all(&common).unwrap();
        fs::create_dir_all(versioned_jar.parent().unwrap()).unwrap();
        fs::write(&versioned_jar, b"jar").unwrap();
        let info = parse_java_mod_info(
            "name=Viewpoint\nid=Viewpoint\njavaJarFile=media/java/client/Viewpoint.jar\njavaPkgName=viewpoint\n",
        )
        .unwrap();
        let info_path = common.join("mod.info");

        assert_eq!(mod_jar_path(&info_path, &info), Some(versioned_jar));
    }

    #[test]
    fn resolves_mod_info_poster_from_its_own_directory() {
        let temp = tempfile::tempdir().unwrap();
        let info_path = temp.path().join("42").join("mod.info");
        let poster_path = info_path.parent().unwrap().join("preview.png");
        fs::create_dir_all(info_path.parent().unwrap()).unwrap();
        fs::write(&poster_path, b"image").unwrap();
        let info = parse_java_mod_info(
            "name=Example Mod\nid=Example\njavaJarFile=libs/example.jar\njavaPkgName=example\nposter=preview.png\n",
        )
        .unwrap();

        assert_eq!(info.poster.as_deref(), Some("preview.png"));
        assert_eq!(mod_poster_path(&info_path, &info), Some(poster_path));
    }

    #[test]
    fn excludes_zombie_buddy_loader_from_jar_mod_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let loader_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy")
            .join("41");
        let regular_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456789")
            .join("mods")
            .join("Example")
            .join("42");

        for (root, jar_name) in [
            (&loader_root, "ZombieBuddy.jar"),
            (&regular_root, "Example.jar"),
        ] {
            fs::create_dir_all(root.join("libs")).unwrap();
            fs::write(
                root.join("mod.info"),
                format!(
                    "name={jar_name}\nid={jar_name}\njavaJarFile=libs/{jar_name}\njavaPkgName=example\n"
                ),
            )
            .unwrap();
            fs::write(root.join("libs").join(jar_name), b"jar").unwrap();
        }

        let candidates = jar_scan_candidates(&[temp.path().to_path_buf()]);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, Some(123456789));
    }

    /// 极速启动缓存必须同时含 ZombieBuddy 加载器本体：它自己的补丁类在**子包**
    /// `me.zed_0xff.zombie_buddy.patches` 里，而 mod.info 声明的包是父包。
    /// 漏掉它的后果就是游戏里弹"ZombieBuddy 未安装"（强制模式下每个包都查不到）。
    #[test]
    fn includes_zombie_buddy_loader_and_subpackages_in_fast_launch_cache() {
        let temp = tempfile::tempdir().unwrap();
        let loader_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy")
            .join("42");
        let regular_root = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456789")
            .join("mods")
            .join("Example")
            .join("42");

        for (root, jar_name, package_name, sub) in [
            (
                &loader_root,
                "ZombieBuddy.jar",
                "me.zed_0xff.zombie_buddy",
                "patches/Patch_Core",
            ),
            (
                &regular_root,
                "Example.jar",
                "example.patch",
                "Patch_Example",
            ),
        ] {
            fs::create_dir_all(root.join("libs")).unwrap();
            fs::write(
                root.join("mod.info"),
                format!(
                    "name={jar_name}
id={jar_name}
javaJarFile=libs/{jar_name}
javaPkgName={package_name}
"
                ),
            )
            .unwrap();
            let prefix = package_name.replace('.', "/");
            let file = fs::File::create(root.join("libs").join(jar_name)).unwrap();
            let mut archive = ZipWriter::new(file);
            archive
                .start_file(
                    format!("{prefix}/{sub}.class"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
            archive
                .write_all(&annotated_class_bytes(
                    &format!("{prefix}/{sub}"),
                    "Lme/zed_0xff/zombie_buddy/Patch;",
                ))
                .unwrap();
            archive.finish().unwrap();
        }

        // 界面上的「JAR 模组」列表仍然排除加载器本体。
        let candidates = jar_scan_candidates(&[temp.path().to_path_buf()]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].2, Some(123456789));

        // 缓存清单：普通模组 + 加载器本体，且子包里的类也要收到、落成二进制名。
        let mods = fast_launch_cache_mods_at(&[temp.path().to_path_buf()]);
        assert_eq!(mods.len(), 2);
        let loader = mods
            .iter()
            .find(|entry| entry.package_name == "me.zed_0xff.zombie_buddy")
            .expect("缓存必须包含 ZombieBuddy 加载器本体");
        assert_eq!(
            loader.patch_classes,
            vec!["me.zed_0xff.zombie_buddy.patches.Patch_Core"]
        );

        let cache = build_fast_launch_cache(&mods, 1);
        assert!(cache.mods.iter().any(|entry| entry
            .patch_classes
            .iter()
            .any(|class| class == "me.zed_0xff.zombie_buddy.patches.Patch_Core")));
    }

    #[test]
    fn detects_zombie_buddy_patch_annotation_in_classfile() {
        let class_bytes =
            annotated_class_bytes("viewpoint/Patch_Foo", "Lme/zed_0xff/zombie_buddy/Patch;");

        let annotations = class_annotations(&class_bytes).unwrap();

        assert_eq!(annotations.class_name, "viewpoint.Patch_Foo");
        assert!(annotations.patch);
        assert!(!annotations.lua_class);
    }

    #[test]
    fn detects_concrete_patch_method_annotations() {
        let overwrite = class_annotations(&annotated_patch_method_class_bytes(
            "example/patch/Patch_Example",
            "apply",
            "Lorg/spongepowered/asm/mixin/Overwrite;",
        ))
        .unwrap();
        let inject = class_annotations(&annotated_patch_method_class_bytes(
            "example/patch/Patch_Example",
            "enter",
            "Lme/zed_0xff/zombie_buddy/Patch$OnEnter;",
        ))
        .unwrap();

        assert_eq!(overwrite.patch_methods.len(), 1);
        assert_eq!(overwrite.patch_methods[0].annotation, "Overwrite");
        assert_eq!(overwrite.patch_methods[0].method_name, "apply");
        assert_eq!(inject.patch_methods.len(), 1);
        assert_eq!(inject.patch_methods[0].annotation, "Inject");
        assert_eq!(inject.patch_methods[0].method_name, "enter");
    }

    #[test]
    fn scans_annotated_jar_mod() {
        let temp = tempfile::tempdir().unwrap();
        let mod_directory = temp
            .path()
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID)
            .join("123456")
            .join("mods")
            .join("Example");
        let jar_path = mod_directory.join("42").join("libs").join("example.jar");
        fs::create_dir_all(jar_path.parent().unwrap()).unwrap();
        fs::write(
            mod_directory.join("42").join("mod.info"),
            "name=Example Mod\nid=Example\njavaJarFile=libs/example.jar\njavaPkgName=example.patch\nposter=preview.png\n",
        )
        .unwrap();
        fs::write(mod_directory.join("42").join("preview.png"), b"image").unwrap();
        let file = fs::File::create(&jar_path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file(
                "example/patch/Patch_Example.class",
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(&annotated_class_bytes(
                "example/patch/Patch_Example",
                "Lme/zed_0xff/zombie_buddy/Patch;",
            ))
            .unwrap();
        archive
            .start_file(
                "example/patch/MethodPatch.class",
                SimpleFileOptions::default(),
            )
            .unwrap();
        archive
            .write_all(&annotated_patch_method_class_bytes(
                "example/patch/MethodPatch",
                "apply",
                "Lorg/spongepowered/asm/mixin/Overwrite;",
            ))
            .unwrap();
        archive.finish().unwrap();
        let mut progress = Vec::new();
        let mods =
            scan_jar_mods_at_with_callback(&[temp.path().to_path_buf()], |scanned, total, item| {
                progress.push((scanned, total, item.is_some()))
            });

        assert_eq!(mods.len(), 1);
        assert_eq!(progress, vec![(1, 1, true)]);
        assert_eq!(mods[0].id, "Example Mod");
        assert_eq!(mods[0].workshop_id, Some(123456));
        assert_eq!(
            mods[0].poster_path,
            Some(
                mod_directory
                    .join("42")
                    .join("preview.png")
                    .to_string_lossy()
                    .into_owned()
            )
        );
        assert_eq!(mods[0].patch_classes, vec!["example.patch.Patch_Example"]);
        assert_eq!(
            mods[0].patch_methods,
            vec![JavaPatchMethod {
                annotation: "Overwrite".to_string(),
                method_name: "apply".to_string(),
            }]
        );
        assert!(mods[0].lua_classes.is_empty());
        assert_eq!(mods[0].decision, Some(true));
    }

    fn annotated_class_bytes(class_name: &str, annotation: &str) -> Vec<u8> {
        fn utf8(bytes: &mut Vec<u8>, value: &str) {
            bytes.push(1);
            bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&52_u16.to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        utf8(&mut bytes, class_name);
        bytes.push(7);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        utf8(&mut bytes, "java/lang/Object");
        bytes.push(7);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        utf8(&mut bytes, "RuntimeVisibleAnnotations");
        utf8(&mut bytes, annotation);
        bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&4_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&5_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes
    }

    fn annotated_patch_method_class_bytes(
        class_name: &str,
        method_name: &str,
        method_annotation: &str,
    ) -> Vec<u8> {
        fn utf8(bytes: &mut Vec<u8>, value: &str) {
            bytes.push(1);
            bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&52_u16.to_be_bytes());
        bytes.extend_from_slice(&9_u16.to_be_bytes());
        utf8(&mut bytes, class_name);
        bytes.push(7);
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        utf8(&mut bytes, "java/lang/Object");
        bytes.push(7);
        bytes.extend_from_slice(&3_u16.to_be_bytes());
        utf8(&mut bytes, "RuntimeVisibleAnnotations");
        utf8(&mut bytes, method_name);
        utf8(&mut bytes, "()V");
        utf8(&mut bytes, method_annotation);
        bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
        bytes.extend_from_slice(&2_u16.to_be_bytes());
        bytes.extend_from_slice(&4_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&0x0009_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u16.to_be_bytes());
        bytes.extend_from_slice(&7_u16.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&5_u16.to_be_bytes());
        bytes.extend_from_slice(&6_u32.to_be_bytes());
        bytes.extend_from_slice(&1_u16.to_be_bytes());
        bytes.extend_from_slice(&8_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes.extend_from_slice(&0_u16.to_be_bytes());
        bytes
    }

    #[test]
    fn lists_only_supported_screenshot_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("first.PNG"), b"png").unwrap();
        fs::write(temp.path().join("notes.txt"), b"text").unwrap();
        fs::create_dir(temp.path().join("nested.jpg")).unwrap();

        let shots = list_viewpoint_shots_at(temp.path()).unwrap();

        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].name, "first.PNG");
    }

    /// 诊断（默认忽略）：用启动器自己的 class 注解解析器扫描本机 workshop，
    /// 打印每个 Java 模组 jar 声明的注入方法(advice)数，并汇总未禁用模组的总数。
    ///
    /// 这个总数就是启动界面进度条的分母——运行期 `[ZB] patching … with N advice(s)` 的 ΣN
    /// 与之高度吻合（实测 614 vs 611）。
    ///
    /// 运行：`cargo test --lib stats_workshop_patch_methods -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stats_workshop_patch_methods() {
        let roots = jar_scan_inputs();
        if roots.is_empty() {
            println!("没有找到 workshop 目录，跳过。");
            return;
        }
        let jars: Vec<String> = scan_jar_mods_at_with_callback(&roots, |_, _, _| {})
            .into_iter()
            .map(|module| module.jar_path)
            .collect();

        let mut total_advice = 0;
        let mut total_patch_classes = 0;
        for path in &jars {
            let Ok(file) = fs::File::open(path) else {
                continue;
            };
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                continue;
            };
            let mut advice = 0;
            let mut patch_classes = 0;
            for index in 0..zip.len() {
                let Ok(mut entry) = zip.by_index(index) else {
                    continue;
                };
                if !entry.name().ends_with(".class") {
                    continue;
                }
                let mut bytes = Vec::new();
                if std::io::Read::read_to_end(&mut entry, &mut bytes).is_err() {
                    continue;
                }
                if let Some(annotations) = class_annotations(&bytes) {
                    if annotations.patch {
                        patch_classes += 1;
                    }
                    advice += annotations.patch_methods.len();
                }
            }
            println!(
                "{advice:>4} advice {patch_classes:>3} @Patch class  {}",
                Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            );
            total_advice += advice;
            total_patch_classes += patch_classes;
        }
        println!(
            "TOTAL advice={total_advice}  @Patch classes={total_patch_classes}  jars={}",
            jars.len()
        );
    }

    #[test]
    fn finds_steam_executable_in_registered_roots() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path().join("Steam");
        fs::create_dir_all(&steam_root).unwrap();
        fs::write(steam_root.join("steam.exe"), b"steam").unwrap();

        assert_eq!(
            steam_executable_from_roots(std::slice::from_ref(&steam_root)),
            Some(steam_root.join("steam.exe"))
        );
    }

    #[test]
    fn reads_screenshot_bytes_without_allowing_directory_traversal() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("shot.png"), b"image-data").unwrap();
        fs::write(
            temp.path().parent().unwrap().join("outside.png"),
            b"outside",
        )
        .unwrap();

        assert_eq!(
            read_viewpoint_shot_at(temp.path(), "shot.png").unwrap(),
            b"image-data"
        );
        assert!(read_viewpoint_shot_at(temp.path(), "../outside.png").is_err());
    }

    #[test]
    fn imports_zip_without_overwriting_existing_shaderpacks() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("lighting.ZIP");
        let target = temp.path().join("shaderpacks");
        fs::write(&source, b"new-pack").unwrap();
        fs::create_dir_all(&target).unwrap();

        import_shaderpack_to(&source, &target).unwrap();

        assert_eq!(fs::read(target.join("lighting.ZIP")).unwrap(), b"new-pack");
        fs::write(&source, b"replacement").unwrap();
        assert!(import_shaderpack_to(&source, &target).is_err());
        assert_eq!(fs::read(target.join("lighting.ZIP")).unwrap(), b"new-pack");
        assert_eq!(list_viewpoint_shaderpacks_at(&target).unwrap().len(), 1);
    }

    #[test]
    fn rejects_non_zip_shaderpack_imports() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("readme.txt");
        fs::write(&source, b"not a zip").unwrap();

        assert!(import_shaderpack_to(&source, &temp.path().join("shaderpacks")).is_err());
    }

    #[test]
    fn deletes_only_shaderpacks_inside_the_managed_directory() {
        let temp = tempfile::tempdir().unwrap();
        let pack = temp.path().join("shaderpacks").join("pack.zip");
        fs::create_dir_all(pack.parent().unwrap()).unwrap();
        fs::write(&pack, b"zip").unwrap();

        delete_shaderpack_at(pack.parent().unwrap(), "pack.zip").unwrap();

        assert!(!pack.exists());
        assert!(delete_shaderpack_at(pack.parent().unwrap(), "../outside.zip").is_err());
        assert!(delete_shaderpack_at(pack.parent().unwrap(), "missing.zip").is_err());
    }

    #[test]
    fn parses_escaped_windows_paths() {
        let content = r#""path" "D:\\SteamLibrary"
"label" ""
"contentid" "123""#;

        assert_eq!(
            vdf_value(content, "path"),
            Some(r#"D:\SteamLibrary"#.to_string())
        );
    }

    #[test]
    fn finds_game_directory_from_library_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let manifest_dir = library.join("steamapps");
        let game_dir = library
            .join("steamapps")
            .join("common")
            .join("ProjectZomboid");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            manifest_dir.join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        assert_eq!(
            game_path_from_library(&library),
            Some(game_dir.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn returns_none_when_game_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let manifest_dir = library.join("steamapps");
        fs::create_dir_all(&manifest_dir).unwrap();
        fs::write(
            manifest_dir.join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        assert_eq!(game_path_from_library(&library), None);
    }

    #[test]
    fn reads_library_paths_from_config() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path().join("Steam");
        let second_library = temp.path().join("SecondaryLibrary");
        let config_dir = steam_root.join("steamapps");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(&second_library).unwrap();
        let escaped_path = second_library.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            config_dir.join("libraryfolders.vdf"),
            format!(
                r#""libraryfolders"
{{
    "1"
    {{
        "path" "{escaped_path}"
    }}
}}"#
            ),
        )
        .unwrap();

        let paths = library_paths_from_config(&steam_root);

        assert!(paths.contains(&steam_root));
        assert!(paths.contains(&second_library));
    }

    #[test]
    fn finds_workshop_directory_for_project_zomboid() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop_path = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("108600");
        fs::create_dir_all(&workshop_path).unwrap();

        assert_eq!(
            workshop_path_from_library(&library),
            Some(workshop_path.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn returns_none_when_workshop_directory_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        fs::create_dir_all(library.join("steamapps")).unwrap();

        assert_eq!(workshop_path_from_library(&library), None);
    }

    #[test]
    fn emits_each_workshop_background_as_it_is_found() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp
            .path()
            .join("workshop/content/108600/123/mods/TestMod/backgrounds");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("background.png"), b"image").unwrap();

        let mut emitted = Vec::new();
        let backgrounds = super::workshop_backgrounds_at_with_callback(
            &[temp.path().join("workshop/content/108600")],
            |entry| emitted.push(entry.id.clone()),
        );

        assert_eq!(backgrounds.len(), 1);
        assert_eq!(emitted, vec![backgrounds[0].id.clone()]);
    }

    #[test]
    fn reads_most_recent_steam_profile() {
        let content = r#""users"
{
    "76561198000000001"
    {
        "AccountName" "older_account"
        "PersonaName" "Older Profile"
        "MostRecent" "0"
    }
    "76561198000000002"
    {
        "AccountName" "current_account"
        "PersonaName" "Current Profile"
        "MostRecent" "1"
    }
}"#;

        assert_eq!(
            steam_profile_from_loginusers(content),
            Some(SteamProfile {
                steam_id: "76561198000000002".to_string(),
                account_name: "current_account".to_string(),
                persona_name: "Current Profile".to_string(),
            })
        );
    }

    #[test]
    fn patches_json_launcher_with_allow_all_policy() {
        let content = r#"{
    "mainClass": "zombie.game",
    "vmArgs": ["-Xmx4G"]
}"#;

        let (patched, changed) = patch_json_launcher_content(content).unwrap();

        assert!(changed);
        assert!(patched.contains(r#""-agentlib:zbNative=policy=allow-all""#));
        assert!(patched.contains(r#""-Xmx4G""#));

        let (patched_again, changed_again) = patch_json_launcher_content(&patched).unwrap();
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn patches_batch_java_options_with_allow_all_policy() {
        let content = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        let (patched, changed) = patch_batch_content(content);

        assert!(changed);
        assert!(patched.contains("SET _JAVA_OPTIONS=-agentlib:zbNative=policy=allow-all -Xmx4G"));
        assert!(patched.contains("\r\n"));

        let (patched_again, changed_again) = patch_batch_content(&patched);
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn replaces_json_memory_limit_without_removing_zombie_buddy_agent() {
        let content = r#"{
    "vmArgs": ["-agentlib:zbNative", "-Xmx4G", "-Dexample=true"]
}"#;

        let (patched, changed) =
            patch_json_launcher_content_with_memory(content, Some(8192)).unwrap();

        assert!(changed);
        assert!(patched.contains(r#""-agentlib:zbNative=policy=allow-all""#));
        assert!(!patched.contains(r#""-agentlib:zbNative""#));
        assert!(patched.contains(r#""-Xmx8192m""#));
        assert!(!patched.contains(r#""-Xmx4G""#));
        assert!(patched.contains(r#""-Dexample=true""#));
    }

    #[test]
    fn replaces_batch_memory_limit_without_removing_zombie_buddy_agent() {
        let content =
            "SET _JAVA_OPTIONS=-agentlib:zbNative -Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        let (patched, changed) = patch_batch_content_with_memory(content, Some(16384));

        assert!(changed);
        assert!(
            patched.contains("SET _JAVA_OPTIONS=-agentlib:zbNative=policy=allow-all -Xmx16384m")
        );
        assert!(!patched.contains("-Xmx4G"));
        assert!(patched.contains("\r\n"));
    }

    #[test]
    fn forces_zombie_buddy_allow_all_policy_on_launch_files() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();

        // 模拟"默认 prompt"的游戏配置：JSON 与 BAT 都是不带 policy 的 agent。
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative","-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\nstart ProjectZomboid64.exe\r\n",
        )
        .unwrap();

        // 同时带上内存上限：JSON/BAT 里的 -Xmx 也要被改成启动器设置的值。
        assert!(force_zombie_buddy_policy_at_paths(&game, Some(12288)).unwrap());

        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        assert!(json.contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(!json.contains(r#""-agentlib:zbNative""#));
        assert!(
            json.contains(r#""-Xmx12288m""#),
            "JSON 里的 -Xmx 应被改写为启动器设置: {json}"
        );
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(batch.contains(&format!("SET _JAVA_OPTIONS={ZOMBIE_BUDDY_POLICY_AGENT}")));
        assert!(
            batch.contains("-Xmx12288m"),
            "BAT 里的 -Xmx 应被改写: {batch}"
        );
        assert!(game.join(".pzmanager-backups").is_dir());

        // 幂等：第二次不应再改动。
        assert!(!force_zombie_buddy_policy_at_paths(&game, Some(12288)).unwrap());
    }

    /// 从 `ProjectZomboid64.json` 文本里取出 `vmArgs`（JSON 里反斜杠是转义的，所以必须解析后再比）。
    fn json_vm_args(json: &str) -> Vec<String> {
        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        parsed["vmArgs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn writes_and_strips_fast_launch_agent_token() {
        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        let expected = format!("-agentpath:{dll}");

        // 写入：`-agentpath:<DLL>` 进 vmArgs，且不动 ZombieBuddy agent 的归一化。
        let json = r#"{"vmArgs":["-Xmx4G","-agentlib:zbNative"]}"#;
        let (patched, changed) =
            patch_json_launcher_with_agent(json, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), Some(dll))
                .unwrap();
        assert!(changed);
        assert!(patched.contains(ZOMBIE_BUDDY_POLICY_AGENT));
        // **必须在最前**：JVMTI agent 按命令行顺序加载，我们要赶在 ZombieBuddy 自己的
        // `-agentlib:zbNative` 之前打开类加载 hook（否则 PatchEngine 早就定义完了）。
        assert_eq!(
            json_vm_args(&patched).first().map(String::as_str),
            Some(expected.as_str()),
            "`-agentpath:` 必须是 vmArgs[0]: {patched}"
        );

        // 幂等：同样的输入再来一次不该有变化。
        let (again, changed) = patch_json_launcher_with_agent(
            &patched,
            None,
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll),
        )
        .unwrap();
        assert!(!changed, "重复写入必须是幂等的: {again}");
        assert_eq!(again.matches("-agentpath:").count(), 1);

        // 重复条目会被收敛成一条（否则 DLL 会被加载两次、Agent_OnLoad 走两遍）。
        let duplicated = r#"{"vmArgs":["-Xmx4G","-agentpath:F:\\a.dll","-agentpath:F:\\a.dll"]}"#;
        let (deduped, changed) =
            patch_json_launcher_with_agent(duplicated, None, None, Some(r"F:\a.dll")).unwrap();
        assert!(changed);
        assert_eq!(deduped.matches("-agentpath:").count(), 1);

        // 关掉开关（None）= 必须把已有的 `-agentpath:` 全部摘掉：
        // 指向不存在的 DLL 会让 JVM 直接启动失败，所以"关开关"必须真的清干净。
        let (cleaned, changed) =
            patch_json_launcher_with_agent(&patched, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
                .unwrap();
        assert!(changed);
        assert!(
            !cleaned.contains("-agentpath"),
            "关掉开关后不能残留: {cleaned}"
        );

        // 路径变化时改写同一条，不新增。
        let (moved, changed) =
            patch_json_launcher_with_agent(&patched, None, None, Some(r"F:\other\pz_overlay.dll"))
                .unwrap();
        assert!(changed);
        assert_eq!(moved.matches("-agentpath:").count(), 1);
        assert!(json_vm_args(&moved).contains(&r"-agentpath:F:\other\pz_overlay.dll".to_string()));

        // 已经干净、且不需要写时不应报"改过"。
        let (_, changed) = patch_json_launcher_with_agent(json, None, None, None).unwrap();
        assert!(!changed);
    }

    #[test]
    fn skips_fast_launch_agent_in_batch_when_path_has_spaces() {
        let batch = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";

        // 无空格路径：写进 .bat 的 _JAVA_OPTIONS。
        let (patched, changed) = patch_batch_with_agent(
            batch,
            None,
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(r"F:\a\pz_overlay.dll"),
        );
        assert!(changed);
        assert!(patched.contains("-agentpath:F:\\a\\pz_overlay.dll"));

        // 含空格路径：不能写进 .bat（`_JAVA_OPTIONS` 按空白切分会被拆坏）。
        let (untouched, changed) =
            patch_batch_with_agent(batch, None, Some(ZOMBIE_BUDDY_POLICY_AGENT), None);
        assert!(changed, "归一化 ZB agent 仍应生效");
        assert!(!untouched.contains("-agentpath"));

        // 已有 `-agentpath:` 时传 None 必须摘掉。
        let with_agent = "SET _JAVA_OPTIONS=-agentpath:F:\\a.dll -Xmx4G\r\n";
        let (stripped, changed) = patch_batch_with_agent(with_agent, None, None, None);
        assert!(changed);
        assert!(!stripped.contains("-agentpath"));
    }

    #[test]
    fn applies_fast_launch_agent_to_launch_files_with_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n",
        )
        .unwrap();

        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll)
        )
        .unwrap());
        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        assert!(
            json_vm_args(&json).contains(&format!("-agentpath:{dll}")),
            "{json}"
        );
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(batch.contains(&format!("-agentpath:{dll}")), "{batch}");
        assert!(game.join(".pzmanager-backups").is_dir());

        // 关掉极速启动（None）：两个文件里的 `-agentpath:` 都必须消失。
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            None
        )
        .unwrap());
        let json = fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap();
        let batch = fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap();
        assert!(!json.contains("-agentpath"), "{json}");
        assert!(!batch.contains("-agentpath"), "{batch}");
    }

    #[test]
    fn restores_launcher_files_from_fast_launch_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        let json_original = r#"{"vmArgs":["-Xmx4G"]}"#;
        let bat_original = "SET _JAVA_OPTIONS=-Xmx4G\r\nstart ProjectZomboid64.exe\r\n";
        fs::write(game.join("ProjectZomboid64.json"), json_original).unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), bat_original).unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        let manifest = create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        assert_eq!(manifest.version, FAST_LAUNCH_BACKUP_VERSION);
        assert!(backup_dir.join(FAST_LAUNCH_BACKUP_MANIFEST).is_file());

        let dll = r"F:\pzmanager\overlay-dll\target\release\pz_overlay.dll";
        assert!(apply_launch_loader_at_paths(
            &game,
            Some(8192),
            Some(ZOMBIE_BUDDY_POLICY_AGENT),
            Some(dll)
        )
        .unwrap());
        assert!(fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains("-agentpath"));

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap(),
            json_original
        );
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.bat")).unwrap(),
            bat_original
        );
        assert!(game.join(".pzmanager-backups").is_dir());
    }

    #[test]
    fn fast_launch_backup_removes_files_created_after_backup() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("ProjectZomboid64.json"), r#"{"vmArgs":[]}"#).unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\n",
        )
        .unwrap();

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert!(!game.join("ProjectZomboid64.bat").is_file());
    }

    #[test]
    fn fast_launch_backup_strips_stale_agent_from_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("game");
        fs::create_dir_all(&game).unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentpath:F:\\old.dll","-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentpath:F:\\old.dll -Xmx4G\r\n",
        )
        .unwrap();

        let backup_dir = temp.path().join("fast-launch-backup");
        create_fast_launch_backup_at(&game, &backup_dir).unwrap();
        assert!(
            !fs::read_to_string(backup_dir.join("ProjectZomboid64.json"))
                .unwrap()
                .contains("-agentpath")
        );
        assert!(!fs::read_to_string(backup_dir.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains("-agentpath"));

        assert!(restore_fast_launch_backup_at(&game, &backup_dir).unwrap());
        assert!(!fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains("-agentpath"));
        assert!(!fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains("-agentpath"));
    }

    #[test]
    fn keeps_fast_launch_cache_unless_explicitly_refreshed() {
        // 显式点「刷新 JAR 列表」→ 一定重建。
        assert!(should_write_fast_launch_cache(true, true));
        assert!(should_write_fast_launch_cache(true, false));
        // 自动扫描：已有缓存一律保持不动（缓存持久化），没有缓存才补一份。
        assert!(!should_write_fast_launch_cache(false, true));
        assert!(should_write_fast_launch_cache(false, false));
    }

    #[test]
    fn rejects_memory_limit_above_physical_memory() {
        assert!(validate_memory_limit_mb(16384, 8192).is_err());
        assert!(validate_memory_limit_mb(8192, 16384).is_ok());
    }

    #[test]
    fn patches_steam_launch_options_for_project_zomboid() {
        let content = r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                    "108600"
                    {
                        "LaunchOptions" "-novid"
                    }
                }
            }
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content(content).unwrap();

        assert!(changed);
        assert!(
            patched.contains(r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""#)
        );

        let (patched_again, changed_again) = patch_steam_launch_options_content(&patched).unwrap();
        assert!(!changed_again);
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn adds_required_separator_after_an_existing_agent() {
        let content = r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative -novid"
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content(content).unwrap();

        assert!(changed);
        assert!(
            patched.contains(r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""#)
        );
    }

    #[test]
    fn writes_memory_and_extra_arguments_to_steam_launch_options() {
        let content = r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-novid"
        }
    }
}"#;

        let (patched, changed) = patch_steam_launch_options_content_with_memory(
            content,
            Some(8192),
            &["-debug".to_string()],
        )
        .unwrap();

        assert!(changed);
        assert!(patched.contains(
            r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -Xmx8192m -- -novid -debug""#
        ));

        let (patched_again, changed_again) =
            patch_steam_launch_options_content_with_memory(&patched, Some(16384), &[]).unwrap();
        assert!(changed_again);
        assert!(patched_again
            .contains(r#""-agentlib:zbNative=policy=allow-all -Xmx16384m -- -novid -debug""#));
        assert!(!patched_again.contains("-Xmx8192m"));
    }

    #[test]
    fn installs_both_targets_and_creates_a_backup() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp
            .path()
            .join("workshop")
            .join("mods")
            .join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp
            .path()
            .join("userdata")
            .join("123")
            .join("config")
            .join("localconfig.vdf");
        fs::create_dir_all(source.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(steam_config.parent().unwrap()).unwrap();

        fs::write(source.join("libs").join("zbNative.dll"), b"dll").unwrap();
        fs::write(source.join("libs").join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-Xmx4G\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                }
            }
        }
    }
}"#,
        )
        .unwrap();

        let result =
            install_zombie_buddy_at_paths(&game, &source, &[steam_config.clone()]).unwrap();

        assert!(result.changed);
        assert!(game.join("zbNative.dll").is_file());
        assert!(game.join("ZombieBuddy.jar").is_file());
        assert!(fs::read_to_string(game.join("ProjectZomboid64.json"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(fs::read_to_string(&steam_config)
            .unwrap()
            .contains("-agentlib:zbNative=policy=allow-all --"));
        let backup_path = result.backup_path.unwrap();
        assert_eq!(
            fs::read_to_string(Path::new(&backup_path).join("ProjectZomboid64.json")).unwrap(),
            r#"{"vmArgs":["-Xmx4G"]}"#
        );
        assert_eq!(
            fs::read_to_string(Path::new(&backup_path).join("steam-localconfig-0.vdf")).unwrap(),
            r#""UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "Apps"
                {
                }
            }
        }
    }
}"#
        );
    }

    #[test]
    fn finds_game_and_workshop_source_in_separate_libraries() {
        let temp = tempfile::tempdir().unwrap();
        let game_library = temp.path().join("GameLibrary");
        let incomplete_library = temp.path().join("IncompleteWorkshopLibrary");
        let workshop_library = temp.path().join("WorkshopLibrary");
        let game = game_library
            .join("steamapps")
            .join("common")
            .join("ProjectZomboid");
        let workshop = workshop_library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join("108600")
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy");
        fs::create_dir_all(
            incomplete_library
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(PROJECT_ZOMBOID_APP_ID)
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs"),
        )
        .unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(workshop.join("libs")).unwrap();
        fs::write(workshop.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(workshop.join("libs/ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game_library
                .join("steamapps")
                .join("appmanifest_108600.acf"),
            r#""AppState"
{
    "appid" "108600"
    "installdir" "ProjectZomboid"
}"#,
        )
        .unwrap();

        let source_libraries = [incomplete_library.clone(), workshop_library.clone()];
        assert_eq!(
            zombie_buddy_source_from_libraries(&source_libraries),
            Some(workshop.clone())
        );

        let locations =
            install_locations_from_libraries(&[game_library, incomplete_library, workshop_library])
                .unwrap();

        assert_eq!(locations.0, game.to_string_lossy().into_owned());
        assert_eq!(locations.1, workshop);
    }

    #[test]
    fn detects_existing_install_without_workshop_source() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("zbNative.dll"), b"dll").unwrap();
        fs::write(game.join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative --"
        }
    }
}"#,
        )
        .unwrap();

        let status = zombie_buddy_status(Some(&game), None, &[steam_config]);

        assert!(status.installed);
        assert!(!status.available);
        assert!(status.missing_components.is_empty());
    }

    #[test]
    fn selects_only_the_most_recent_steam_user_config() {
        let temp = tempfile::tempdir().unwrap();
        let steam_root = temp.path();
        let profile = SteamProfile {
            steam_id: "76561197960389184".to_string(),
            account_name: "current_account".to_string(),
            persona_name: "Current Profile".to_string(),
        };
        let current_config = steam_root
            .join("userdata")
            .join("123456")
            .join("config")
            .join("localconfig.vdf");
        let other_config = steam_root
            .join("userdata")
            .join("654321")
            .join("config")
            .join("localconfig.vdf");
        fs::create_dir_all(current_config.parent().unwrap()).unwrap();
        fs::create_dir_all(other_config.parent().unwrap()).unwrap();
        fs::write(&current_config, "").unwrap();
        fs::write(&other_config, "").unwrap();

        assert_eq!(
            steam_config_paths(steam_root, Some(&profile)),
            vec![current_config]
        );
        assert!(steam_config_paths(steam_root, None).is_empty());
    }

    #[test]
    fn rolls_back_earlier_writes_when_a_later_write_fails() {
        let temp = tempfile::tempdir().unwrap();
        let existing_file = temp.path().join("existing.txt");
        let missing_parent_file = temp.path().join("missing").join("later.txt");
        fs::write(&existing_file, b"original").unwrap();

        let result = apply_file_transaction(vec![
            (existing_file.clone(), b"updated".to_vec()),
            (missing_parent_file.clone(), b"later".to_vec()),
        ]);

        assert!(result.is_err());
        assert_eq!(fs::read(existing_file).unwrap(), b"original");
        assert!(!missing_parent_file.exists());
    }

    #[test]
    fn converts_steam_id_64_to_userdata_account_id() {
        let profile = SteamProfile {
            steam_id: "76561197960389184".to_string(),
            account_name: "test".to_string(),
            persona_name: "Test".to_string(),
        };

        assert_eq!(steam_user_data_id(&profile), Some("123456".to_string()));
    }

    #[test]
    fn invalid_steam_config_does_not_partially_install_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        fs::create_dir_all(source.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(source.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(source.join("libs/ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-Xmx4G"]}"#,
        )
        .unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), "start game\r\n").unwrap();
        fs::write(&steam_config, "not a valid Steam VDF").unwrap();

        let result = install_zombie_buddy_at_paths(&game, &source, &[steam_config]);

        assert!(result.is_err());
        assert!(!game.join("zbNative.dll").exists());
        assert!(!game.join("ZombieBuddy.jar").exists());
        assert_eq!(
            fs::read_to_string(game.join("ProjectZomboid64.json")).unwrap(),
            r#"{"vmArgs":["-Xmx4G"]}"#
        );
    }

    #[test]
    fn installed_loader_is_reported_as_already_installed_without_source_files() {
        let temp = tempfile::tempdir().unwrap();
        let game = temp.path().join("ProjectZomboid");
        let steam_config = temp.path().join("localconfig.vdf");
        let missing_source = temp.path().join("unsubscribed-workshop-mod");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("zbNative.dll"), b"dll").unwrap();
        fs::write(game.join("ZombieBuddy.jar"), b"jar").unwrap();
        fs::write(
            game.join("ProjectZomboid64.json"),
            r#"{"vmArgs":["-agentlib:zbNative"]}"#,
        )
        .unwrap();
        fs::write(
            game.join("ProjectZomboid64.bat"),
            "SET _JAVA_OPTIONS=-agentlib:zbNative\r\n",
        )
        .unwrap();
        fs::write(
            &steam_config,
            r#""UserLocalConfigStore"
{
    "Apps"
    {
        "108600"
        {
            "LaunchOptions" "-agentlib:zbNative --"
        }
    }
}"#,
        )
        .unwrap();

        let result =
            install_zombie_buddy_at_paths(&game, &missing_source, &[steam_config]).unwrap();

        assert!(result.already_installed);
        assert!(!result.changed);
        assert!(result.backup_path.is_none());
    }

    #[test]
    fn detects_viewpoint_mod_subscription() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID);

        fs::create_dir_all(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint")
                .join("mod.info"),
            "name=Project Viewpoint\n",
        )
        .unwrap();
        fs::create_dir_all(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs")
                .join("zbNative.dll"),
            b"dll",
        )
        .unwrap();
        fs::write(
            workshop
                .join(ZOMBIE_BUDDY_MOD_ID)
                .join("mods")
                .join("ZombieBuddy")
                .join("libs")
                .join("ZombieBuddy.jar"),
            b"jar",
        )
        .unwrap();

        let status = viewpoint_three_d_status(None, &[library]);

        assert!(status.ready);
        assert!(status.mods.iter().all(|item| item.subscribed));
        assert!(status.missing_components.is_empty());
    }

    #[test]
    fn configures_viewpoint_files_and_copies_loader_jar() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("SteamLibrary");
        let workshop = library
            .join("steamapps")
            .join("workshop")
            .join("content")
            .join(PROJECT_ZOMBOID_APP_ID);
        let zombie_buddy = workshop
            .join(ZOMBIE_BUDDY_MOD_ID)
            .join("mods")
            .join("ZombieBuddy");
        let game = temp.path().join("ProjectZomboid");

        fs::create_dir_all(zombie_buddy.join("libs")).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint"),
        )
        .unwrap();
        fs::write(
            workshop
                .join(VIEWPOINT_MOD_ID)
                .join("mods")
                .join("ProjectViewpoint")
                .join("mod.info"),
            "name=Project Viewpoint\n",
        )
        .unwrap();
        fs::write(zombie_buddy.join("libs/zbNative.dll"), b"dll").unwrap();
        fs::write(zombie_buddy.join("libs/ZombieBuddy.jar"), b"loader").unwrap();
        fs::write(zombie_buddy.join("libs/extra-native.dll"), b"extra").unwrap();
        fs::write(game.join("ProjectZomboid64.json"), r#"{"vmArgs":[]}"#).unwrap();
        fs::write(game.join("ProjectZomboid64.bat"), "start game\r\n").unwrap();

        let result = configure_viewpoint_three_d_at_paths(&game, &[library], &[]).unwrap();

        assert!(result.changed);
        assert_eq!(fs::read(game.join("zbNative.dll")).unwrap(), b"dll");
        assert_eq!(fs::read(game.join("extra-native.dll")).unwrap(), b"extra");
        // 不再用 Temporary Fix 覆盖：游戏目录的 jar 直接来自 ZombieBuddy 工坊自带的 libs。
        assert_eq!(fs::read(game.join("ZombieBuddy.jar")).unwrap(), b"loader");
        assert!(fs::read_to_string(game.join("ProjectZomboid64.bat"))
            .unwrap()
            .contains(ZOMBIE_BUDDY_POLICY_AGENT));
        assert!(result.backup_path.is_some());
    }

    #[test]
    fn rewrites_default_mod_block_and_keeps_other_content() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.txt");
        fs::write(
            &path,
            "-- keep this comment\r\nmods\r\n{\r\n    mod = \"Existing\",\r\n}\r\nother = true\r\n",
        )
        .unwrap();

        write_default_mod_ids(&path, &["Existing".to_string(), "NewMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(
            content,
            "-- keep this comment\r\nmods\r\n{\r\n    mod = Existing,\r\n    mod = NewMod,\r\n}\r\nother = true\r\n"
        );
        // 关键回归：写回的 ID 不能带引号（PZ 会把引号当成 ID 的一部分而识别不到模组）。
        assert!(!content.contains("mod = \""));
        assert_eq!(parse_default_mod_ids(&content), ["Existing", "NewMod"]);
    }

    #[test]
    fn creates_default_mod_list_with_version_header() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("default.txt");

        write_default_mod_ids(&path, &["SomeMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("VERSION = 1,"));
        assert!(content.contains("mod = SomeMod,"));
        assert_eq!(parse_default_mod_ids(&content), ["SomeMod"]);
    }

    #[test]
    fn reads_empty_default_list_when_file_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.txt");

        assert!(read_default_mod_ids(&path).unwrap().is_empty());
    }

    #[test]
    fn creates_default_mod_block_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("mods").join("default.txt");

        write_default_mod_ids(&path, &["FreshMod".to_string()]).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(parse_default_mod_ids(&content), ["FreshMod"]);
        assert!(content.contains("mod = FreshMod,"));
        assert!(!content.contains("mod = \""));
    }

    #[test]
    fn parses_required_mod_ids_from_mod_info() {
        let info = parse_lua_mod_info(
            "id=Addon\nname=Addon\nrequire=Core, \"Extras\"\nloadAfter=Framework\nloadBefore=Overhaul\nloadFirst=on\nloadLast=off\n",
        );

        assert_eq!(info.requires, ["Core", "Extras"]);
        assert_eq!(info.load_after, ["Framework"]);
        assert_eq!(info.load_before, ["Overhaul"]);
        assert!(info.load_first);
        assert!(!info.load_last);
    }

    #[test]
    fn parses_lua_sorting_rules() {
        let rules = parse_lua_sorting_rules(
            "[ZombieBuddy]\nloadBefore=ModLoadOrderSorter_b42\nloadFirst=on\n[Addon]\nloadAfter=Core\nloadLast=on\n",
        );

        assert_eq!(rules["zombiebuddy"].load_before, ["ModLoadOrderSorter_b42"]);
        assert!(rules["zombiebuddy"].load_first);
        assert_eq!(rules["addon"].load_after, ["Core"]);
        assert!(rules["addon"].load_last);
    }

    #[test]
    fn applies_build_42_and_mlos_load_rules() {
        let mut zombie_buddy = lua_mod("ZombieBuddy", "ZombieBuddy");
        zombie_buddy.load_first = true;
        zombie_buddy.load_before = vec!["ModLoadOrderSorter_b42".to_string()];
        let mut sorter = lua_mod("ModLoadOrderSorter_b42", "Sorter");
        sorter.load_after = vec!["ZombieBuddy".to_string()];
        let mut manager = lua_mod("ModManager", "Mod Manager");
        manager.requires = vec!["ModLoadOrderSorter_b42".to_string()];
        let entries = vec![lua_mod("Other", "Other"), manager, sorter, zombie_buddy];
        let ids = vec![
            "Other".to_string(),
            "ModManager".to_string(),
            "ModLoadOrderSorter_b42".to_string(),
            "ZombieBuddy".to_string(),
        ];

        assert_eq!(
            order_mod_ids_by_requires(&entries, &ids),
            [
                "ZombieBuddy",
                "ModLoadOrderSorter_b42",
                "ModManager",
                "Other"
            ]
        );
    }

    #[test]
    fn deduplicates_mod_ids_and_prefers_the_b42_entry() {
        let mut legacy = lua_mod("ModManager", "Mod Manager");
        legacy.path = "old/ModManager".to_string();
        legacy.has_b42 = false;
        legacy.requires = vec!["LegacyDependency".to_string()];
        let mut modern = lua_mod("modmanager", "[B42] Mod Manager");
        modern.path = "new/[B42] Mod Manager".to_string();
        modern.requires = vec!["ModLoadOrderSorter_b42".to_string()];

        merge_lua_mod_entry(&mut legacy, modern);

        assert_eq!(legacy.name, "[B42] Mod Manager");
        assert_eq!(legacy.path, "new/[B42] Mod Manager");
        assert!(legacy.has_b42);
        assert_eq!(legacy.requires, ["ModLoadOrderSorter_b42"]);
    }

    #[test]
    fn orders_prerequisites_before_dependents() {
        let core = lua_mod("Core", "Core");
        let mut addon = lua_mod("Addon", "Addon");
        addon.requires = vec!["Core".to_string()];
        let entries = vec![core, addon];

        let ordered = order_mod_ids_by_requires(
            &entries,
            &["Addon".to_string(), "Core".to_string(), "Other".to_string()],
        );

        assert_eq!(ordered, ["Core", "Addon", "Other"]);
    }

    #[test]
    fn plans_recursive_prerequisites_and_reports_missing() {
        let mut core = lua_mod("Core", "Core");
        core.requires = vec!["Base".to_string()];
        let mut addon = lua_mod("Addon", "Addon");
        addon.requires = vec!["Core".to_string()];
        let entries = vec![core, addon];

        let mut ids = vec!["Existing".to_string()];
        let mut added = Vec::new();
        let mut missing = Vec::new();
        let mut active = std::collections::HashSet::new();
        plan_lua_mod_enable(
            &entries,
            "Addon",
            &mut ids,
            &mut added,
            &mut missing,
            &mut active,
        );

        assert_eq!(added, ["Core", "Addon"]);
        assert_eq!(missing, ["Base"]);
        assert_eq!(
            order_mod_ids_by_requires(&entries, &ids),
            ["Existing", "Core", "Addon"]
        );
    }

    fn lua_mod(id: &str, name: &str) -> LuaModEntry {
        LuaModEntry {
            id: id.to_string(),
            name: name.to_string(),
            path: format!("mods/{name}"),
            source: "workshop".to_string(),
            workshop_id: Some("1".to_string()),
            enabled: false,
            load_order: None,
            requires: Vec::new(),
            load_after: Vec::new(),
            load_before: Vec::new(),
            load_first: false,
            load_last: false,
            has_common: false,
            has_b42: true,
            is_loader: false,
            poster_path: None,
        }
    }

    #[test]
    fn matches_lua_mod_by_info_id_before_name() {
        let entries = vec![lua_mod("BritaWeapons", "Brita's Weapon Pack")];

        assert_eq!(
            pick_lua_mod_id(&entries, Some("britaweapons"), "ignored"),
            Some("BritaWeapons".to_string())
        );
        assert_eq!(
            pick_lua_mod_id(&entries, None, "Brita's Weapon Pack"),
            Some("BritaWeapons".to_string())
        );
        assert_eq!(pick_lua_mod_id(&entries, Some("Missing"), "Missing"), None);
    }

    #[test]
    fn keeps_jar_mod_info_id_on_scanned_entries() {
        let info = parse_java_mod_info(
            "name=Brita's Weapon Pack\nid=BritaWeapons\njavajarfile=Brita.jar\njavapkgname=brita.core\n",
        )
        .unwrap();

        assert_eq!(info.name, "Brita's Weapon Pack");
        assert_eq!(info.id, "BritaWeapons");
    }

    #[test]
    fn disabling_jar_mod_overwrites_existing_disabled_file() {
        let temp = tempfile::tempdir().unwrap();
        let enabled_path = temp.path().join("mod.info");
        let disabled_path = temp.path().join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        let result = set_jar_mod_info_enabled(enabled_path.to_str().unwrap(), false).unwrap();

        assert_eq!(result, disabled_path);
        assert!(!enabled_path.exists());
        assert_eq!(fs::read(&disabled_path).unwrap(), b"id=Authoritative\n");
    }

    #[test]
    fn enabling_jar_mod_keeps_mod_info_as_the_authoritative_file() {
        let temp = tempfile::tempdir().unwrap();
        let enabled_path = temp.path().join("mod.info");
        let disabled_path = temp.path().join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        let result = set_jar_mod_info_enabled(enabled_path.to_str().unwrap(), true).unwrap();

        assert_eq!(result, enabled_path);
        assert!(!disabled_path.exists());
        assert_eq!(fs::read(&enabled_path).unwrap(), b"id=Authoritative\n");
    }

    #[test]
    fn jar_scan_prefers_mod_info_and_falls_back_to_disabled_file() {
        let temp = tempfile::tempdir().unwrap();
        let mod_root = temp.path().join("mods").join("Example");
        fs::create_dir_all(&mod_root).unwrap();
        let enabled_path = mod_root.join("mod.info");
        let disabled_path = mod_root.join("mod.info.disabled");
        fs::write(&enabled_path, b"id=Authoritative\n").unwrap();
        fs::write(&disabled_path, b"id=Stale\n").unwrap();

        assert_eq!(
            discover_mod_info_files(temp.path()),
            vec![enabled_path.clone()]
        );

        fs::remove_file(&enabled_path).unwrap();
        assert_eq!(discover_mod_info_files(temp.path()), vec![disabled_path]);
    }

    #[test]
    fn reapplies_user_state_after_mod_info_is_redownloaded() {
        let temp = tempfile::tempdir().unwrap();
        let mods_root = temp.path().join("mods");
        let mod_root = mods_root.join("Example");
        fs::create_dir_all(&mod_root).unwrap();
        fs::write(
            mod_root.join("mod.info"),
            "name=Example Mod\nid=Example\njavaJarFile=Example.jar\njavaPkgName=example.patch\n",
        )
        .unwrap();
        fs::write(mod_root.join("Example.jar"), b"jar").unwrap();
        let state = JarModStateFile {
            format_version: 1,
            mods: vec![JarModStateEntry {
                mod_dir: mod_root.to_string_lossy().into_owned(),
                mod_id: "Example".to_string(),
                workshop_id: None,
                package_name: "example.patch".to_string(),
                disabled: true,
            }],
        };

        apply_jar_mod_state_to_roots(std::slice::from_ref(&mods_root), &state).unwrap();
        assert!(!mod_root.join("mod.info").exists());
        assert!(mod_root.join("mod.info.disabled").exists());

        fs::write(
            mod_root.join("mod.info"),
            "name=Example Mod Updated\nid=Example\njavaJarFile=Example.jar\njavaPkgName=example.patch\n",
        )
        .unwrap();
        apply_jar_mod_state_to_roots(std::slice::from_ref(&mods_root), &state).unwrap();

        assert!(!mod_root.join("mod.info").exists());
        assert!(fs::read_to_string(mod_root.join("mod.info.disabled"))
            .unwrap()
            .contains("Example Mod Updated"));
    }

    #[test]
    fn persists_jar_mod_state_in_user_data_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("jar-mod-state.json");
        let state = JarModStateFile {
            format_version: 1,
            mods: vec![JarModStateEntry {
                mod_dir: "C:\\mods\\Example".to_string(),
                mod_id: "Example".to_string(),
                workshop_id: Some(123),
                package_name: "example.patch".to_string(),
                disabled: true,
            }],
        };

        write_jar_mod_state(&path, &state).unwrap();

        let loaded = read_jar_mod_state(&path).unwrap();
        assert_eq!(loaded.mods.len(), 1);
        assert_eq!(loaded.mods[0].mod_id, "Example");
        assert!(loaded.mods[0].disabled);
    }
}
fn set_jar_mod_info_enabled(mod_info_path: &str, enabled: bool) -> Result<PathBuf, String> {
    // 注意：启用/禁用**不再**删极速启动的缓存（缓存持久化，只在显式刷新 JAR 列表时重建）。
    // 代价是启用列表与缓存可能不一致；注入器侧的桥接类按包名查缓存，
    // 新启用的模组查不到就会回退原生扫描，所以不会因此丢补丁。
    let path = PathBuf::from(mod_info_path);
    let parent = path
        .parent()
        .ok_or_else(|| "JAR 模组 mod.info 路径无效".to_string())?;
    let enabled_path = parent.join("mod.info");
    let disabled_path = parent.join("mod.info.disabled");

    if enabled {
        if enabled_path.is_file() {
            if disabled_path.is_file() {
                fs::remove_file(&disabled_path)
                    .map_err(|error| format!("清理 {} 失败: {error}", disabled_path.display()))?;
            }
        } else if disabled_path.is_file() {
            fs::rename(&disabled_path, &enabled_path)
                .map_err(|error| format!("恢复 {} 失败: {error}", disabled_path.display()))?;
        } else {
            return Err(format!("找不到 {}", enabled_path.display()));
        }
        Ok(enabled_path)
    } else {
        if enabled_path.is_file() {
            if disabled_path.is_file() {
                fs::remove_file(&disabled_path)
                    .map_err(|error| format!("覆盖 {} 失败: {error}", disabled_path.display()))?;
            }
            fs::rename(&enabled_path, &disabled_path)
                .map_err(|error| format!("禁用 {} 失败: {error}", enabled_path.display()))?;
        } else if !disabled_path.is_file() {
            return Err(format!("找不到 {}", enabled_path.display()));
        }
        Ok(disabled_path)
    }
}

#[tauri::command(async)]
fn set_lua_mod_enabled(mod_id: String, enabled: bool) -> Result<LuaModToggleResult, String> {
    let target_id = mod_id.trim();
    if target_id.is_empty() {
        return Err("缺少模组 ID".to_string());
    }
    let entries = lua_mod_entries()?;
    let Some(target) = entries
        .iter()
        .find(|entry| entry.id.eq_ignore_ascii_case(target_id))
    else {
        return Err(format!("未找到模组 {target_id}"));
    };
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let mut ids = read_default_mod_ids(&default_path)?;
    let default_path_has_file = default_path.is_file();
    let backup_default = |path: &Path| -> Result<(), String> {
        if default_path_has_file {
            let backup_dir = user_zomboid_directory()?
                .join(".pzmanager")
                .join("default-backups")
                .join(current_unix_timestamp()?);
            backup_file(path, &backup_dir, "default.txt")?;
        }
        Ok(())
    };

    if !enabled {
        ids.retain(|id| !id.eq_ignore_ascii_case(&target.id));
        let dependents = entries
            .iter()
            .filter(|entry| ids.iter().any(|id| id.eq_ignore_ascii_case(&entry.id)))
            .filter(|entry| {
                entry
                    .requires
                    .iter()
                    .any(|require| require.eq_ignore_ascii_case(&target.id))
            })
            .map(|entry| entry.id.clone())
            .collect::<Vec<_>>();
        backup_default(&default_path)?;
        write_default_mod_ids(&default_path, &ids)?;
        return Ok(LuaModToggleResult {
            enabled: false,
            load_order: None,
            added: Vec::new(),
            missing_requires: Vec::new(),
            dependents,
        });
    }

    if !target.has_b42 {
        return Err(format!("{} 不是 B42 模组，无法启用", target.id));
    }

    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut active = HashSet::new();
    plan_lua_mod_enable(
        &entries,
        &target.id,
        &mut ids,
        &mut added,
        &mut missing_requires,
        &mut active,
    );
    let ordered = order_mod_ids_by_requires(&entries, &ids);
    backup_default(&default_path)?;
    write_default_mod_ids(&default_path, &ordered)?;
    let load_order = ordered
        .iter()
        .position(|id| id.eq_ignore_ascii_case(&target.id))
        .map(|index| index + 1);
    Ok(LuaModToggleResult {
        enabled: true,
        load_order,
        added,
        missing_requires,
        dependents: Vec::new(),
    })
}

#[tauri::command(async)]
fn set_lua_mods_enabled(
    mod_ids: Vec<String>,
    enabled: bool,
) -> Result<LuaModBatchToggleResult, String> {
    let mut requested = Vec::new();
    for id in mod_ids {
        let id = id.trim();
        if !id.is_empty()
            && !requested
                .iter()
                .any(|value: &String| value.eq_ignore_ascii_case(id))
        {
            requested.push(id.to_string());
        }
    }
    if requested.is_empty() {
        return Err("没有选中 Lua 模组".to_string());
    }

    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let original = read_default_mod_ids(&default_path)?;
    let mut ids = original.clone();
    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut dependents = Vec::new();
    let mut skipped = Vec::new();

    let changed = if !enabled {
        let target_ids = requested
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let changed = original
            .iter()
            .filter(|id| target_ids.contains(&id.to_ascii_lowercase()))
            .cloned()
            .collect();
        ids.retain(|id| !target_ids.contains(&id.to_ascii_lowercase()));
        let remaining_enabled = ids
            .iter()
            .map(|id| id.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        dependents = entries
            .iter()
            .filter(|entry| remaining_enabled.contains(&entry.id.to_ascii_lowercase()))
            .filter(|entry| {
                entry
                    .requires
                    .iter()
                    .any(|require| target_ids.contains(&require.to_ascii_lowercase()))
            })
            .map(|entry| entry.id.clone())
            .collect();
        changed
    } else {
        let mut active = HashSet::new();
        for id in requested {
            let Some(target) = entries
                .iter()
                .find(|entry| entry.id.eq_ignore_ascii_case(&id))
            else {
                skipped.push(id);
                continue;
            };
            if !target.has_b42 {
                skipped.push(target.id.clone());
                continue;
            }
            plan_lua_mod_enable(
                &entries,
                &target.id,
                &mut ids,
                &mut added,
                &mut missing_requires,
                &mut active,
            );
        }
        added.clone()
    };

    let ordered = order_mod_ids_by_requires(&entries, &ids);
    let unchanged = original.len() == ordered.len()
        && original
            .iter()
            .zip(&ordered)
            .all(|(left, right)| left.eq_ignore_ascii_case(right));
    if !unchanged {
        if default_path.is_file() {
            let backup_dir = user_zomboid_directory()?
                .join(".pzmanager")
                .join("default-backups")
                .join(current_unix_timestamp()?);
            backup_file(&default_path, &backup_dir, "default.txt")?;
        }
        write_default_mod_ids(&default_path, &ordered)?;
    }

    Ok(LuaModBatchToggleResult {
        enabled,
        changed,
        added,
        missing_requires,
        dependents,
        skipped,
    })
}

#[tauri::command(async)]
fn save_lua_mod_order(mod_ids: Vec<String>) -> Result<Vec<String>, String> {
    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    if !default_path.is_file() {
        return Err("default.txt 不存在，无法排序".to_string());
    }
    let existing = read_default_mod_ids(&default_path)?;
    let mut merged: Vec<String> = Vec::new();
    for id in &mod_ids {
        let id = id.trim();
        if id.is_empty()
            || !existing.iter().any(|value| value.eq_ignore_ascii_case(id))
            || merged.iter().any(|value| value.eq_ignore_ascii_case(id))
        {
            continue;
        }
        merged.push(id.to_string());
    }
    for id in existing {
        if !merged.iter().any(|value| value.eq_ignore_ascii_case(&id)) {
            merged.push(id);
        }
    }
    let ordered = order_mod_ids_by_requires(&entries, &merged);
    let backup_dir = user_zomboid_directory()?
        .join(".pzmanager")
        .join("default-backups")
        .join(current_unix_timestamp()?);
    backup_file(&default_path, &backup_dir, "default.txt")?;
    write_default_mod_ids(&default_path, &ordered)?;
    Ok(ordered)
}

#[tauri::command(async)]
fn set_zombie_buddy_mod_decision(
    mod_id: String,
    mod_info_path: String,
    workshop_id: Option<u64>,
    jar_hash: String,
    decision: bool,
    lua_mod_id: Option<String>,
) -> Result<JarModDecisionResult, String> {
    if mod_id.trim().is_empty() || jar_hash.trim().is_empty() {
        return Err("缺少 JAR 模组审批信息".to_string());
    }
    set_jar_mod_info_enabled(&mod_info_path, decision)?;
    let mod_dir = Path::new(&mod_info_path)
        .parent()
        .ok_or_else(|| "JAR 模组路径无效".to_string())?
        .to_string_lossy()
        .into_owned();
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    upsert_jar_mod_state_entry(
        &mut state,
        JarModStateEntry {
            mod_dir,
            mod_id: mod_id.clone(),
            workshop_id,
            package_name: String::new(),
            disabled: !decision,
        },
    );
    write_jar_mod_state(&state_path, &state)?;

    let mut result = JarModDecisionResult {
        decision,
        lua_mod_enabled: false,
        lua_mod_id: None,
    };
    if !decision {
        return Ok(result);
    }

    let Some(target) = find_lua_mod_id(lua_mod_id.as_deref(), &mod_id) else {
        return Ok(result);
    };
    let entries = lua_mod_entries()?;
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let mut ids = read_default_mod_ids(&default_path)?;
    let mut added = Vec::new();
    let mut missing_requires = Vec::new();
    let mut active = HashSet::new();
    plan_lua_mod_enable(
        &entries,
        &target,
        &mut ids,
        &mut added,
        &mut missing_requires,
        &mut active,
    );
    let ordered = order_mod_ids_by_requires(&entries, &ids);
    if default_path.is_file() {
        let backup_dir = user_zomboid_directory()?
            .join(".pzmanager")
            .join("default-backups")
            .join(current_unix_timestamp()?);
        backup_file(&default_path, &backup_dir, "default.txt")?;
    }
    write_default_mod_ids(&default_path, &ordered)?;
    result.lua_mod_enabled = added.iter().any(|id| id.eq_ignore_ascii_case(&target));
    result.lua_mod_id = Some(target);
    Ok(result)
}

#[tauri::command(async)]
fn set_zombie_buddy_mods_decisions(
    updates: Vec<JarModDecisionUpdate>,
) -> Result<JarModBatchDecisionResult, String> {
    let updates = updates
        .into_iter()
        .filter(|update| !update.mod_id.trim().is_empty() && !update.jar_hash.trim().is_empty())
        .collect::<Vec<_>>();
    if updates.is_empty() {
        return Err("没有选中 JAR 模组".to_string());
    }

    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    for update in &updates {
        set_jar_mod_info_enabled(&update.mod_info_path, update.decision)?;
        let mod_dir = Path::new(&update.mod_info_path)
            .parent()
            .ok_or_else(|| "JAR 模组路径无效".to_string())?
            .to_string_lossy()
            .into_owned();
        upsert_jar_mod_state_entry(
            &mut state,
            JarModStateEntry {
                mod_dir,
                mod_id: update.mod_id.clone(),
                workshop_id: update.workshop_id,
                package_name: update.package_name.clone(),
                disabled: !update.decision,
            },
        );
    }
    write_jar_mod_state(&state_path, &state)?;

    let mut lua_targets = Vec::new();
    if updates.iter().any(|update| update.decision) {
        let entries = lua_mod_entries()?;
        for update in updates.iter().filter(|update| update.decision) {
            if let Some(target) =
                pick_lua_mod_id(&entries, update.lua_mod_id.as_deref(), &update.mod_id)
            {
                if !lua_targets
                    .iter()
                    .any(|value: &String| value.eq_ignore_ascii_case(&target))
                {
                    lua_targets.push(target);
                }
            }
        }
    }

    let lua_mods_enabled = if lua_targets.is_empty() {
        Vec::new()
    } else {
        set_lua_mods_enabled(lua_targets, true)?.changed
    };

    Ok(JarModBatchDecisionResult {
        updated: updates.len(),
        lua_mods_enabled,
    })
}

#[tauri::command(async)]
fn apply_jar_mod_file_state() -> Result<usize, String> {
    let roots = jar_scan_inputs();
    let state = read_jar_mod_state(&jar_mod_state_path()?)?;
    apply_jar_mod_state_to_roots(&roots, &state)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModPresetJarMod {
    mod_dir: String,
    mod_id: String,
    workshop_id: Option<u64>,
    package_name: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModPreset {
    id: String,
    name: String,
    lua_mod_ids: Vec<String>,
    jar_mods: Vec<ModPresetJarMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModPresetFile {
    format_version: u32,
    presets: Vec<ModPreset>,
}

impl Default for ModPresetFile {
    fn default() -> Self {
        Self {
            format_version: 1,
            presets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModPresetApplyResult {
    lua_enabled: usize,
    lua_skipped: Vec<String>,
    jar_changed: usize,
}

fn mod_presets_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("mod-presets.json"))
}

fn read_mod_presets(path: &Path) -> Result<ModPresetFile, String> {
    if !path.is_file() {
        return Ok(ModPresetFile::default());
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取模组预设失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析模组预设失败: {error}"))
}

fn write_mod_presets(path: &Path, file: &ModPresetFile) -> Result<(), String> {
    let content =
        serde_json::to_string_pretty(file).map_err(|error| format!("写入模组预设失败: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建模组预设目录失败: {error}"))?;
    }
    fs::write(path, format!("{content}\n")).map_err(|error| format!("写入模组预设失败: {error}"))
}

fn mod_dir_string(info_path: &Path) -> String {
    info_path
        .parent()
        .unwrap_or(info_path)
        .to_string_lossy()
        .into_owned()
}

fn jar_info_is_enabled(info_path: &Path) -> bool {
    info_path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("mod.info"))
}

fn preset_jar_matches(
    saved: &ModPresetJarMod,
    info_path: &Path,
    info: &JavaModInfo,
    workshop_id: Option<u64>,
) -> bool {
    saved
        .mod_dir
        .eq_ignore_ascii_case(&mod_dir_string(info_path))
        || (saved.mod_id.eq_ignore_ascii_case(&info.id)
            && saved.workshop_id == workshop_id
            && saved.package_name.eq_ignore_ascii_case(&info.package_name))
}

/// 把当前 `default.txt` 的启用顺序（保留文件里的原始顺序）和所有 JAR 模组的启用状态存为预设。
#[tauri::command(async)]
fn save_mod_preset(name: String) -> Result<ModPreset, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("预设名称不能为空".to_string());
    }
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    let lua_mod_ids = read_default_mod_ids(&default_path)?;
    let jar_mods: Vec<ModPresetJarMod> = jar_scan_candidates(&jar_scan_inputs())
        .into_iter()
        .map(|(info_path, info, workshop_id)| ModPresetJarMod {
            mod_dir: mod_dir_string(&info_path),
            mod_id: info.id,
            workshop_id,
            package_name: info.package_name,
            enabled: jar_info_is_enabled(&info_path),
        })
        .collect();

    let path = mod_presets_path()?;
    let mut file = read_mod_presets(&path)?;
    file.presets.retain(|preset| preset.name != name);
    let preset = ModPreset {
        id: current_unix_timestamp()?,
        name,
        lua_mod_ids,
        jar_mods,
    };
    file.presets.push(preset.clone());
    write_mod_presets(&path, &file)?;
    Ok(preset)
}

/// 一键切换到预设：Lua 按预设顺序重写 `default.txt`；JAR 模组中不在预设里的全部停用。
#[tauri::command(async)]
fn apply_mod_preset(id: String) -> Result<ModPresetApplyResult, String> {
    let file = read_mod_presets(&mod_presets_path()?)?;
    let preset = file
        .presets
        .iter()
        .find(|preset| preset.id == id)
        .ok_or_else(|| "找不到该模组预设".to_string())?;

    let entries = lua_mod_entries()?;
    let mut lua_ids = Vec::new();
    let mut lua_skipped = Vec::new();
    for saved in &preset.lua_mod_ids {
        match entries
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(saved))
        {
            Some(entry) if entry.has_b42 => lua_ids.push(entry.id.clone()),
            _ => lua_skipped.push(saved.clone()),
        }
    }
    let default_path = user_zomboid_directory()?.join("mods").join("default.txt");
    if default_path.is_file() {
        let backup_dir = pzmanager_directory()?
            .join("default-backups")
            .join(current_unix_timestamp()?);
        backup_file(&default_path, &backup_dir, "default.txt")?;
    }
    write_default_mod_ids(&default_path, &lua_ids)?;

    let roots = jar_scan_inputs();
    let state_path = jar_mod_state_path()?;
    let mut state = read_jar_mod_state(&state_path)?;
    for (info_path, info, workshop_id) in jar_scan_candidates(&roots) {
        let enabled = preset
            .jar_mods
            .iter()
            .find(|saved| preset_jar_matches(saved, &info_path, &info, workshop_id))
            .is_some_and(|saved| saved.enabled);
        upsert_jar_mod_state_entry(
            &mut state,
            JarModStateEntry {
                mod_dir: mod_dir_string(&info_path),
                mod_id: info.id,
                workshop_id,
                package_name: info.package_name,
                disabled: !enabled,
            },
        );
    }
    write_jar_mod_state(&state_path, &state)?;
    let jar_changed = apply_jar_mod_state_to_roots(&roots, &state)?;

    Ok(ModPresetApplyResult {
        lua_enabled: lua_ids.len(),
        lua_skipped,
        jar_changed,
    })
}

#[tauri::command(async)]
fn list_mod_presets() -> Result<Vec<ModPreset>, String> {
    Ok(read_mod_presets(&mod_presets_path()?)?.presets)
}

#[tauri::command(async)]
fn delete_mod_preset(id: String) -> Result<(), String> {
    let path = mod_presets_path()?;
    let mut file = read_mod_presets(&path)?;
    let before = file.presets.len();
    file.presets.retain(|preset| preset.id != id);
    if file.presets.len() == before {
        return Err("找不到该模组预设".to_string());
    }
    write_mod_presets(&path, &file)
}

#[tauri::command]
async fn fetch_developer_news(
    count: u32,
    language: String,
    only_gid: Option<String>,
) -> Result<news::DeveloperNewsResponse, String> {
    news::fetch_developer_news(count, language, only_gid).await
}

#[tauri::command]
async fn check_launcher_update() -> Result<update::LauncherUpdateStatus, String> {
    update::fetch_launcher_update(update::UPDATE_FEED_URL).await
}
