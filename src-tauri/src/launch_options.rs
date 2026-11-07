//! 启动参数 / Steam 启动选项的纯解析与改写。
//!
//! 这些函数不依赖 Tauri 状态，方便单测；`lib.rs` 通过 `pub(crate) use` 保持原有调用路径。

use crate::*;

pub(crate) fn parse_quoted_tokens(line: &str) -> Vec<String> {
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

pub(crate) fn parse_vdf_pairs(content: &str) -> Vec<(String, String)> {
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

pub(crate) fn vdf_value(content: &str, key: &str) -> Option<String> {
    parse_vdf_pairs(content)
        .into_iter()
        .find_map(|(pair_key, value)| (pair_key == key).then_some(value))
}

pub(crate) fn contains_zombie_buddy_agent(value: &str) -> bool {
    value.split_whitespace().any(is_zombie_buddy_agent_token)
}

pub(crate) fn is_zombie_buddy_agent_token(token: &str) -> bool {
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
pub(crate) fn is_fast_launch_agent_token(token: &str) -> bool {
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
pub(crate) fn normalize_fast_launch_agent_tokens(tokens: &mut Vec<String>, agent_path: Option<&str>) -> bool {
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
pub(crate) fn fast_launch_agent_for_batch(agent_path: Option<&str>) -> Option<&str> {
    agent_path.filter(|path| !path.chars().any(char::is_whitespace))
}

/// 方案不使用加载器时，把启动参数里的加载器 agent 摘掉：
/// 前端默认启动参数带 `-agentlib:zbNative`，不摘就等于背着方案偷偷挂上。
/// 返回 true 表示确实摘掉了参数。
pub(crate) fn strip_loader_agent_tokens(tokens: &mut Vec<String>) -> bool {
    let original_length = tokens.len();
    tokens.retain(|token| !is_zombie_buddy_agent_token(token.trim()));
    tokens.len() != original_length
}

/// 把选定加载器的 agent 参数归一化成 `policy_agent`：已有的只保留一条且值正确，
/// 缺失时插到最前（必须在其它 agent 之前，否则 write-once 的策略会被先到的那条锁死）。
/// 识别"这行是不是加载器 agent"仍由各加载器自己的判定函数负责（见 `is_zombie_buddy_agent_token`）。
pub(crate) fn normalize_loader_agent_tokens(tokens: &mut Vec<String>, policy_agent: &str) -> bool {
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

/// LaunchOptions 是否已配置好 ZombieBuddy 加载器（agent 存在即视为已配置）。
///
/// 旧版还要求 agent 之后必须跟 `--` 分隔符；2026-11-07 起写入时会把裸 `--` 一律剥掉
/// （B42 自带的 Zulu 25 JRE 报 "Unrecognized option: --" 直接拒绝创建 JVM，见
/// `patch_steam_launch_options_value_with_memory`），这条检查没跟着改，导致"安装成功
/// 但状态永远显示缺少 Steam LaunchOptions"。现在放宽为只验 agent 本身。
pub(crate) fn steam_launch_value_has_both(value: &str) -> bool {
    contains_zombie_buddy_agent(value)
}

/// 按当前加载器方案改写 Steam LaunchOptions 值：
/// `policy_agent = Some(agent)` 归一化 agent（方案启用加载器）；
/// `None`（方案 = 不使用加载器）则**摘掉**已有的 agent——否则每次改内存上限/启动
/// 都会把 `-agentlib:zbNative` 偷偷写回 Steam 配置，用户选的"不使用加载器"形同虚设。
pub(crate) fn patch_steam_launch_options_value_with_memory(
    value: &str,
    memory_mb: Option<u64>,
    extra_args: &[String],
    policy_agent: Option<&str>,
) -> (String, bool) {
    let mut tokens: Vec<String> = value.split_whitespace().map(str::to_string).collect();
    let original = tokens.clone();

    match policy_agent {
        Some(agent) => {
            normalize_loader_agent_tokens(&mut tokens, agent);
        }
        None => {
            strip_loader_agent_tokens(&mut tokens);
        }
    }
    // 裸 `--` 一律剥掉：B42 自带的 Zulu 25 JRE 直接报 "Unrecognized option: --" 拒绝创建
    // JVM（实测 2026-11-07，游戏 1-2 秒内秒退且不落任何日志）。旧版本曾在这里给 agent
    // 后面自动补 `--`，但 11-01 pzopt 写的启动项没有 `--`，ZB agent 照常工作——它从来
    // 不是必需品；`--` 只会把后面的 JVM 参数误当成主类参数。
    tokens.retain(|token| token != "--");

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
            // 内存是 JVM 参数，紧跟在 agent 之后（没有 agent 就放最前），
            // 保持「JVM 参数在前、游戏参数在后」的次序。
            let insert_at = tokens
                .iter()
                .position(|token| is_zombie_buddy_agent_token(token))
                .map(|index| index + 1)
                .unwrap_or(0);
            tokens.insert(insert_at, target);
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

pub(crate) fn is_memory_arg(arg: &str) -> bool {
    arg.trim_start()
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("-xmx"))
}

pub(crate) fn memory_arg(memory_mb: u64) -> String {
    format!("-Xmx{memory_mb}m")
}

pub(crate) fn patch_json_launcher_content_with_memory(
    content: &str,
    memory_mb: Option<u64>,
) -> Result<(String, bool), String> {
    // 历史调用点与测试沿用这个签名：默认按 ZombieBuddy 方案处理。
    patch_json_launcher_with_agent(content, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

/// `policy_agent` = `None`（「模组加载器方案」选了不使用加载器）时只改内存，
/// 完全不往 `ProjectZomboid64.json` 里塞 agent 参数。
/// `fast_launch_agent` = `Some(DLL 绝对路径)` 时额外写一条 `-agentpath:<DLL>`（极速启动）。
pub(crate) fn patch_json_launcher_with_agent(
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

pub(crate) fn patch_json_launcher_content(content: &str) -> Result<(String, bool), String> {
    patch_json_launcher_content_with_memory(content, None)
}

pub(crate) fn patch_batch_content_with_memory(content: &str, memory_mb: Option<u64>) -> (String, bool) {
    // 历史调用点与测试沿用这个签名：默认按 ZombieBuddy 方案处理。
    patch_batch_with_agent(content, memory_mb, Some(ZOMBIE_BUDDY_POLICY_AGENT), None)
}

/// `policy_agent` = `None`（「模组加载器方案」选了不使用加载器）时只改内存，
/// 不往 `ProjectZomboid64.bat` 里塞 agent 参数。
/// `fast_launch_agent` = `Some(DLL 绝对路径)` 时额外写一条 `-agentpath:<DLL>`（极速启动）。
pub(crate) fn patch_batch_with_agent(
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

pub(crate) fn patch_batch_content(content: &str) -> (String, bool) {
    patch_batch_content_with_memory(content, None)
}

pub(crate) fn validate_memory_limit_mb(memory_mb: u64, total_memory_mb: u64) -> Result<(), String> {
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

pub(crate) fn format_memory_mb(memory_mb: u64) -> String {
    if memory_mb % 1024 == 0 {
        format!("{} GB", memory_mb / 1024)
    } else {
        format!("{:.1} GB", memory_mb as f64 / 1024.0)
    }
}

pub(crate) fn quoted_key(line: &str, key: &str) -> bool {
    // Steam 自己解析 VDF 时键名不区分大小写（实测文件里 "Apps" 与 "apps" 两种拼写都存在），
    // 这里也必须不区分，否则手机/老版本写出的全小写文件会走不到结构节点。
    let tokens = parse_quoted_tokens(line);
    tokens
        .first()
        .is_some_and(|value| value.eq_ignore_ascii_case(key))
}

pub(crate) fn brace_delta(line: &str) -> i32 {
    line.chars().fold(0, |delta, character| match character {
        '{' => delta + 1,
        '}' => delta - 1,
        _ => delta,
    })
}

pub(crate) fn line_indentation(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// 从 `start`（指向开引号）开始找闭引号的位置（返回值指向闭引号本身）。
/// 感知 `\"` / `\\` 转义：值里含转义引号时，按「第 N 个引号」数会截错位置，
/// 拼回去就会在闭引号后残留原文尾巴 —— 整个 localconfig.vdf 从那行起解析错乱。
/// 按 ASCII 字节走是安全的：UTF-8 多字节序列不会包含 0x22 / 0x5C。
fn closing_quote_index(line: &str, start: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

pub(crate) fn replace_vdf_value(line: &str, value: &str) -> Option<String> {
    let first_quote = line.find('"')?;
    let key_end = closing_quote_index(line, first_quote)?;
    let value_start = line[key_end + 1..].find('"')? + key_end + 1;
    let value_end = closing_quote_index(line, value_start)?;
    let escaped_value = value.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!(
        "{}{}{}",
        &line[..value_start + 1],
        escaped_value,
        &line[value_end..]
    ))
}

pub(crate) fn patch_steam_launch_options_content_with_memory(
    content: &str,
    memory_mb: Option<u64>,
    extra_args: &[String],
    policy_agent: Option<&str>,
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
                let tokens_on_line = parse_quoted_tokens(&line);
                let indentation = line_indentation(&line);
                let current_value = tokens_on_line
                    .get(1)
                    .cloned()
                    .unwrap_or_default();
                let (options, options_changed) = patch_steam_launch_options_value_with_memory(
                    &current_value,
                    memory_mb,
                    extra_args,
                    policy_agent,
                );
                // 历史版本在值含转义引号时会把这行写坏（闭引号后残留原文尾巴）。
                // VDF 的 LaunchOptions 一行只应有「键 + 值」两组引号，多出来的都是
                // 残留垃圾：整行重建成规范形式，用户无需手动去 Steam 里清理。
                if tokens_on_line.len() > 2 {
                    output.push(format!("{indentation}\"LaunchOptions\" \"{options}\""));
                    launch_options_found = true;
                    changed = true;
                } else if !options_changed {
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
                    patch_steam_launch_options_value_with_memory("", memory_mb, extra_args, policy_agent)
                        .0;
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
                    let launch_options = patch_steam_launch_options_value_with_memory(
                        "",
                        memory_mb,
                        extra_args,
                        policy_agent,
                    )
                    .0;
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

pub(crate) fn patch_steam_launch_options_content(content: &str) -> Result<(String, bool), String> {
    // 安装/配置流程只在 ZombieBuddy 方案下发生，按启用加载器处理。
    patch_steam_launch_options_content_with_memory(content, None, &[], Some(ZOMBIE_BUDDY_POLICY_AGENT))
}
