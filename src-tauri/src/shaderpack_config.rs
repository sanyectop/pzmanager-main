//! Viewpoint 光影包配置解析与启动覆写。
//!
//! 覆写对象是 `%USERPROFILE%\Zomboid\viewpoint-live.properties`（Viewpoint 的实时设置总表，
//! 游戏内 Delete 菜单修改后由 Viewpoint 写回）；光影包 zip 里的 `shaders.properties`
//! 只作为参数元数据（参数名 / 预设 / 滑块，只读不写回）。
//!
//! 实现遵循《Project Viewpoint 配置文件解析/覆写思路文档》：
//! - 行级编辑：保留注释、顺序与换行风格，不用 Properties.store 全量重写；
//! - 只碰 `minecraft.<packId>.*` / `minecraftProfile.<packId>`，绝不改 Viewpoint 自身键
//!   （`camera.*` / `vam.*` / `gunplay.*` / `keys.*` / `pack.*` / `graphics.*`）；
//! - 重复键逻辑值取最后一行，覆写改最后一行；
//! - 新键按区域插入（同 packId 之后 → 最后一个 minecraft. 行之后 → 文件末尾）；
//! - 备份 + 原子替换 + 回读校验，只在游戏关闭时写入；
//! - 游戏进程退出后整文件还原（备份清单驱动，保留最近 5 份）。
//!
//! 通过 `lib.rs` 的 `pub(crate) use shaderpack_config::*;` 保持原有调用路径。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use crate::*;

const MINECRAFT_KEY_PREFIX: &str = "minecraft.";
const MINECRAFT_PROFILE_KEY_PREFIX: &str = "minecraftProfile.";
const PACK_KEY_PREFIX: &str = "pack.";

pub(crate) const VIEWPOINT_LIVE_PROPERTIES_FILE: &str = "viewpoint-live.properties";
pub(crate) const SHADERPACK_OVERRIDES_FILE: &str = "shaderpack-overrides.json";
pub(crate) const SHADERPACK_OVERRIDES_FORMAT: u32 = 1;
pub(crate) const VIEWPOINT_LIVE_BACKUP_DIR: &str = "viewpoint-live-backups";
pub(crate) const VIEWPOINT_LIVE_BACKUP_MANIFEST: &str = "manifest.json";
/// 思路文档 §8：备份建议保留最近 5 份，自动清理旧备份。
pub(crate) const VIEWPOINT_LIVE_BACKUP_MAX: usize = 5;

// ---------------------------------------------------------------------------
// 行级属性文档（思路文档 §2）
// ---------------------------------------------------------------------------

/// 行级属性文档：保留原始行、注释、顺序与换行风格。
pub(crate) struct PropertiesDocument {
    lines: Vec<String>,
    newline: &'static str,
    trailing_newline: bool,
}

impl PropertiesDocument {
    /// 逐行解析：探测换行风格，去掉 BOM；空行 / `#` / `!` 开头视为注释行。
    pub(crate) fn parse(content: &str) -> Self {
        let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };
        let content = content.strip_prefix('\u{feff}').unwrap_or(content);
        let trailing_newline = content.ends_with('\n');
        let mut lines: Vec<String> = content
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        if trailing_newline {
            // split('\n') 会在结尾多出一个空段，它代表"文件以换行结束"而不是一行。
            lines.pop();
        }
        Self {
            lines,
            newline,
            trailing_newline,
        }
    }

    /// 按原换行风格拼回文本（写回无 BOM，思路文档 §4.3）。
    pub(crate) fn render(&self) -> String {
        let mut out = self.lines.join(self.newline);
        if self.trailing_newline {
            out.push_str(self.newline);
        }
        out
    }

    /// 逻辑值：重复键取最后一次出现（思路文档 §2.2）。
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        self.entry_lines(key)
            .last()
            .and_then(|&index| parse_entry_line(&self.lines[index]).map(|(_, value)| value))
    }

    /// 覆写键值：存在则改最后一行，不存在则按区域插入（思路文档 §3.1）。
    /// 返回是否真的改变了文档（写入相同值 = 没改）。
    pub(crate) fn set(&mut self, key: &str, value: &str) -> Result<bool, String> {
        if key.is_empty() || key.contains('\n') || key.contains('\r') {
            return Err(format!("非法的属性键: {key:?}"));
        }
        let normalized = normalize_property_value(value)?;
        let line = format!("{key}={}", escape_value_for_line(&normalized));
        if let Some(&last) = self.entry_lines(key).last() {
            if self.lines[last] == line {
                return Ok(false);
            }
            self.lines[last] = line;
            return Ok(true);
        }
        let position = self.insert_position(key);
        self.lines.insert(position, line);
        Ok(true)
    }

    /// 删除键的所有行（思路文档 §3.1 默认：删除所有重复行，保持文件干净）。返回删除的行数。
    /// 文档模型的必备操作（覆写流程目前只用 set），测试覆盖。
    #[allow(dead_code)]
    pub(crate) fn remove(&mut self, key: &str) -> usize {
        let before = self.lines.len();
        self.lines
            .retain(|line| !parse_entry_line(line).is_some_and(|(entry_key, _)| entry_key == key));
        before - self.lines.len()
    }

    /// 该键出现的所有行号（升序）。
    fn entry_lines(&self, key: &str) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, line)| {
                parse_entry_line(line).is_some_and(|(entry_key, _)| entry_key == key)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// 新键的插入位置（思路文档 §3.2）：
    /// - `minecraft.<packId>.<PARAM>`：同 packId 最后一行之后；pack 没出现过 → 最后一个
    ///   `minecraft.` 行之后；连 minecraft 都没有 → 文件末尾；
    /// - `minecraftProfile.<packId>`：最后一个 minecraftProfile 行之后；
    /// - 其他未知键：文件末尾。
    fn insert_position(&self, key: &str) -> usize {
        if key.starts_with(MINECRAFT_PROFILE_KEY_PREFIX) {
            return self
                .last_line_position(|entry_key| {
                    entry_key.starts_with(MINECRAFT_PROFILE_KEY_PREFIX)
                })
                .unwrap_or(self.lines.len());
        }
        if let Some(rest) = key.strip_prefix(MINECRAFT_KEY_PREFIX) {
            // packId 可能包含点、下划线、连字符，必须按最后一个点拆（思路文档 §1）。
            if let Some(pos) = rest.rfind('.') {
                let pack_line_prefix =
                    format!("{MINECRAFT_KEY_PREFIX}{}", &rest[..pos + 1]);
                if let Some(position) =
                    self.last_line_position(|entry_key| entry_key.starts_with(&pack_line_prefix))
                {
                    return position;
                }
            }
            return self
                .last_line_position(|entry_key| entry_key.starts_with(MINECRAFT_KEY_PREFIX))
                .unwrap_or(self.lines.len());
        }
        self.lines.len()
    }

    /// 满足条件的最后一个键值行的「之后」位置（即插入点）。
    fn last_line_position(&self, predicate: impl Fn(&str) -> bool) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| {
                parse_entry_line(line).is_some_and(|(key, _)| predicate(&key))
            })
            .map(|(index, _)| index + 1)
    }
}

/// 解析一行键值。输入必须是**逻辑行**（shaders.properties 先过 `logical_property_lines`
/// 折叠续行；live 文件没有续行，逐行即可）。语义照搬 java.util.Properties.load
/// （Iris 的 OrderBackedProperties 就是 Properties 子类）：
/// - `#` / `!` 注释行跳过；
/// - key 终止于第一个未转义的 空白 / `=` / `:`；
/// - key 后跳过空白，若紧跟 `=` 或 `:` 吃掉一个再跳过空白；
/// - 转义反转义：`\\`、`\ `、`\=`、`\:`、`\t`、`\n`、`\r`、`\f`、`\uXXXX`，其余 `\X` 取字面。
fn parse_entry_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    // key 终止于第一个未转义的 空白 / = / :（都没有 = 整行是 key，值为空）
    let key_end = bytes
        .iter()
        .enumerate()
        .find(|(index, byte)| {
            !is_escaped(bytes, *index)
                && (**byte == b'=' || **byte == b':' || byte.is_ascii_whitespace())
        })
        .map(|(index, _)| index)
        .unwrap_or(trimmed.len());
    let key = unescape_properties(&trimmed[..key_end]);
    if key.is_empty() {
        return None;
    }
    // key 后：跳过空白；若紧跟 = 或 : 吃掉一个，再跳过空白（Java load 的分隔符规则）
    let rest = trimmed[key_end..].trim_start();
    let rest = rest
        .strip_prefix('=')
        .or_else(|| rest.strip_prefix(':'))
        .map(|value| value.trim_start())
        .unwrap_or(rest);
    Some((key, unescape_properties(rest)))
}

/// 把物理行折叠成逻辑行（java.util.Properties.load 语义，Iris 的 ShaderProperties 走同一套）：
/// - 行首非空白字符为 `#` / `!` 的是注释行（注释不参与续行）；
/// - 尾随**奇数个**反斜杠 = 续行（`\\` 是字面反斜杠不续），下一行的前导空白剥掉后拼接；
/// - 空行跳过。
fn logical_property_lines(content: &str) -> Vec<String> {
    let mut logical: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut continuing = false;
    for line in content.lines() {
        if continuing {
            current.push_str(line.trim_start());
        } else {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            current.push_str(trimmed);
        }
        let trailing_backslashes = current.chars().rev().take_while(|c| *c == '\\').count();
        if trailing_backslashes % 2 == 1 {
            current.pop();
            continuing = true;
        } else {
            logical.push(std::mem::take(&mut current));
            continuing = false;
        }
    }
    if !current.is_empty() {
        logical.push(current);
    }
    logical
}

fn is_escaped(bytes: &[u8], index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

/// 完整的 Java Properties 反转义（loadConvert）：`\\`、`\ `、`\=`、`\:`、`\t`、`\n`、
/// `\r`、`\f`、`\uXXXX`，其余 `\X` 取字面；孤立 `\` 保留。
fn unescape_properties(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(current) = chars.next() {
        if current != '\\' {
            out.push(current);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(escaped) => out.push(escaped),
                    None => out.push_str(&hex),
                }
            }
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// 写回值规范化（思路文档 §2.3）：禁止换行；布尔统一小写；数字 / 枚举保留原字符串。
/// 返回的是"逻辑值"（未做行内转义），校验与存储都用它。
pub(crate) fn normalize_property_value(value: &str) -> Result<String, String> {
    if value.contains('\n') || value.contains('\r') {
        return Err(format!("覆写值不能包含换行: {value:?}"));
    }
    if value.eq_ignore_ascii_case("true") {
        return Ok("true".to_string());
    }
    if value.eq_ignore_ascii_case("false") {
        return Ok("false".to_string());
    }
    Ok(value.to_string())
}

/// 行内转义（思路文档 §2.3）：反斜杠双写；前导空格用 `\` 保护。
/// 内部 / 尾随空格、逗号等无需转义。
fn escape_value_for_line(value: &str) -> String {
    let doubled = value.replace('\\', "\\\\");
    if doubled.starts_with(' ') {
        format!("\\{doubled}")
    } else {
        doubled
    }
}

// ---------------------------------------------------------------------------
// 键分类与 packId（思路文档 §1 / §6）
// ---------------------------------------------------------------------------

/// live 文件键的分类（思路文档 §6：先判 `minecraftProfile.`，再判 `minecraft.`）。
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum LiveKeyClass {
    /// `minecraftProfile.<packId>`
    McProfile { pack_id: String },
    /// `minecraft.<packId>.<PARAM>`
    McParam { pack_id: String, param: String },
    /// `minecraft.` 前缀但拆不出 packId / 参数（元数据、调试项，只读）。
    McUnknown,
    /// `pack.<preset>.*` 等 Viewpoint 内置光影键。
    PackParam,
    /// `camera.*` / `vam.*` / `gunplay.*` / `keys.*` / `graphics.*` 等 Viewpoint 自身键。
    Viewpoint,
}

pub(crate) fn classify_key(key: &str) -> LiveKeyClass {
    if let Some(pack_id) = key.strip_prefix(MINECRAFT_PROFILE_KEY_PREFIX) {
        return LiveKeyClass::McProfile {
            pack_id: pack_id.to_string(),
        };
    }
    if let Some(rest) = key.strip_prefix(MINECRAFT_KEY_PREFIX) {
        return match rest.rfind('.') {
            Some(pos) if pos > 0 => LiveKeyClass::McParam {
                pack_id: rest[..pos].to_string(),
                param: rest[pos + 1..].to_string(),
            },
            _ => LiveKeyClass::McUnknown,
        };
    }
    if key.starts_with(PACK_KEY_PREFIX) {
        return LiveKeyClass::PackParam;
    }
    LiveKeyClass::Viewpoint
}

/// live 文件里已出现过的 MC 光影包 packId（首次出现顺序，去重）。
pub(crate) fn observed_minecraft_pack_ids(document: &PropertiesDocument) -> Vec<String> {
    let mut pack_ids: Vec<String> = Vec::new();
    for line in &document.lines {
        let Some((key, _)) = parse_entry_line(line) else {
            continue;
        };
        let pack_id = match classify_key(&key) {
            LiveKeyClass::McParam { pack_id, .. } | LiveKeyClass::McProfile { pack_id } => pack_id,
            _ => continue,
        };
        if !pack_ids.contains(&pack_id) {
            pack_ids.push(pack_id);
        }
    }
    pack_ids
}

/// 从光影包 zip 文件名推导 packId：`"minecraft-" + 去 .zip 后非 [A-Za-z0-9_-] 字符替换为 "_"`。
/// 依据思路文档示例：`BSL v8.4` → `minecraft-BSL_v8_4`；
/// `Bliss v2.1.2 (Chocapic13 Shaders edit)` → `minecraft-Bliss_v2_1_2__Chocapic13_Shaders_edit_`。
pub(crate) fn candidate_pack_id(pack_file_name: &str) -> String {
    let stem = Path::new(pack_file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(pack_file_name);
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("minecraft-{sanitized}")
}

/// 解析某光影包的 packId：优先与 live 文件里已出现的 packId 反查对上（文件为准），
/// 推导值只作兜底。返回 `(packId, 是否与已观测键对上)`；
/// 推导值若不正确，Viewpoint 只是忽略这些键，还原时一并清除，无副作用。
pub(crate) fn resolve_pack_id(
    pack_file_name: &str,
    document: &PropertiesDocument,
) -> (String, bool) {
    let candidate = candidate_pack_id(pack_file_name);
    let matched = observed_minecraft_pack_ids(document)
        .iter()
        .any(|pack_id| *pack_id == candidate);
    (candidate, matched)
}

// ---------------------------------------------------------------------------
// shaders.properties 只读元数据（思路文档 §5）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderOptionMeta {
    pub name: String,
    pub default_value: Option<String>,
    /// `sliders=` 里列出的参数，游戏内是滑块。
    pub slider: bool,
    /// 从默认值与各预设里观测到的取值（去重保序）。
    pub values: Vec<String>,
    /// 源码声明里 `//` 注释方括号外的说明文字（lang 文件的说明优先于它）。
    pub comment: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderProfileToken {
    pub option: String,
    pub value: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderProfileMeta {
    pub name: String,
    /// lang 文件里的预设显示名（`profile.<name>`）；没有 = None。
    pub label: Option<String>,
    /// 预设展开后的参数值；`opt` → true，`!opt` → false。
    pub tokens: Vec<ShaderProfileToken>,
}

/// 分页界面里的一个格子（OptiFine / Iris 的 screen 布局 token）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderScreenToken {
    /// "option"（参数）| "screen"（`[子屏]` 引用）| "empty"（`<empty>`）| "profile"（`<profile>` 预设按钮）。
    pub kind: String,
    /// option 名或子屏名；empty 没有。
    pub name: Option<String>,
}

/// 一个分页界面（主页 = `screen=`，子页 = `screen.<name>=`）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderScreenInfo {
    /// 主页固定名 "screen"，子页为 `screen.<name>` 的 name。
    pub name: String,
    /// lang 的 `screen.<name>` 显示名（主页通常没有）。
    pub label: Option<String>,
    /// `screen.<name>.columns`，OptiFine 默认 2。
    pub columns: u32,
    pub tokens: Vec<ShaderScreenToken>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ShadersPropertiesMeta {
    pub options: Vec<ShaderOptionMeta>,
    pub profiles: Vec<ShaderProfileMeta>,
    /// `sliders=` 里列出的原始参数名（合并源码扫描结果时要再套一遍滑块标记）。
    pub sliders: Vec<String>,
    /// 分页界面；第一个是主页。没有 `screen=` 时合成一份"全部参数"主页。
    pub screens: Vec<ShaderScreenInfo>,
}

/// 解析 shaders.properties：`profile.*` 预设、`sliders` 滑块、顶层无点键 = 参数默认值。
/// `screen*` / `program.*` / `texture.*` 等带点的引擎配置一律不当作参数。
pub(crate) fn parse_shaders_properties(content: &str) -> ShadersPropertiesMeta {
    let mut names: Vec<String> = Vec::new();
    let mut defaults: HashMap<String, String> = HashMap::new();
    let mut values: HashMap<String, Vec<String>> = HashMap::new();
    let mut sliders: Vec<String> = Vec::new();
    let mut profiles: Vec<ShaderProfileMeta> = Vec::new();
    // screen 布局：主页（`screen=`）与子屏（`screen.<name>=`），`.columns` 记列数
    let mut main_tokens: Option<Vec<String>> = None;
    let mut main_columns: u32 = DEFAULT_SCREEN_COLUMNS;
    let mut sub_screens: Vec<(String, u32, Vec<String>)> = Vec::new();

    // Iris 用 OrderBackedProperties（java.util.Properties 子类）读 shaders.properties：
    // 先按 load 语义折叠 `\` 续行，再逐个逻辑行分发指令（screen= 一类长布局都靠续行折行）
    for line in logical_property_lines(content) {
        let Some((key, value)) = parse_entry_line(&line) else {
            continue;
        };
        if key == "sliders" {
            sliders = value.split_whitespace().map(str::to_string).collect();
            continue;
        }
        if key == "screen" {
            main_tokens = Some(value.split_whitespace().map(str::to_string).collect());
            continue;
        }
        if let Some(rest) = key.strip_prefix("screen.") {
            // screen.<name>.columns=<n>（或 screen.columns 设主页列数）
            if rest == "columns" {
                if let Ok(columns) = value.trim().parse::<u32>() {
                    main_columns = columns.max(1);
                }
                continue;
            }
            if let Some(name) = rest.strip_suffix(".columns") {
                if let Ok(columns) = value.trim().parse::<u32>() {
                    match sub_screens.iter_mut().find(|(screen, ..)| screen == name) {
                        Some((_, columns_slot, _)) => *columns_slot = columns.max(1),
                        None => sub_screens.push((name.to_string(), columns.max(1), Vec::new())),
                    }
                }
                continue;
            }
            let tokens = value.split_whitespace().map(str::to_string).collect();
            match sub_screens.iter_mut().find(|(screen, ..)| screen == rest) {
                Some((_, _, tokens_slot)) => *tokens_slot = tokens,
                None => sub_screens.push((rest.to_string(), DEFAULT_SCREEN_COLUMNS, tokens)),
            }
            continue;
        }
        if let Some(profile_name) = key.strip_prefix("profile.") {
            if profile_name.is_empty() {
                continue;
            }
            // 预设 token 里的 `profile.<名>` 是**继承**另一条预设（如 Alpha Piscium 的
            // `profile.Medium=profile.Low …`）：展开被继承预设的参数值，同名参数后者覆盖
            let mut tokens: Vec<ShaderProfileToken> = Vec::new();
            for token in parse_profile_tokens(&value) {
                if let Some(inherited) = token.option.strip_prefix("profile.") {
                    if let Some(base) = profiles.iter().find(|profile| profile.name == inherited) {
                        tokens.extend(base.tokens.iter().cloned());
                    }
                    continue;
                }
                tokens.push(token);
            }
            for token in &tokens {
                push_option_value(&mut names, &mut values, &token.option, &token.value);
            }
            profiles.push(ShaderProfileMeta {
                name: profile_name.to_string(),
                label: None,
                tokens,
            });
            continue;
        }
        // 其余带点的键（program.* / texture.* / uniform.* / 参数值标签等）不是参数本体。
        if key.contains('.') {
            continue;
        }
        push_option_value(&mut names, &mut values, &key, &value);
        defaults.insert(key, value);
    }

    let options: Vec<ShaderOptionMeta> = names
        .into_iter()
        .map(|name| ShaderOptionMeta {
            slider: sliders.iter().any(|slider| *slider == name),
            default_value: defaults.get(&name).cloned(),
            values: values.remove(&name).unwrap_or_default(),
            comment: None,
            name,
        })
        .collect();

    let option_names: Vec<String> = options.iter().map(|option| option.name.clone()).collect();
    let screens = build_screens(main_tokens, main_columns, sub_screens, &option_names);

    ShadersPropertiesMeta {
        options,
        profiles,
        sliders,
        screens,
    }
}

/// OptiFine 的 screen 默认列数。
const DEFAULT_SCREEN_COLUMNS: u32 = 2;

/// 扁平主页：全部参数单页铺开（Iris 菜单构建失败时"手动创建菜单"的兜底形态）。
fn flat_screen(option_names: &[String]) -> ShaderScreenInfo {
    ShaderScreenInfo {
        name: "screen".to_string(),
        label: None,
        columns: DEFAULT_SCREEN_COLUMNS,
        tokens: option_names
            .iter()
            .map(|name| ShaderScreenToken {
                kind: "option".to_string(),
                name: Some(name.clone()),
            })
            .collect(),
    }
}

/// 没有自带 screen 布局的包（EclipseShaders 类纯源码包）也要长成截图里的分页形态：
/// 主页是一页"第 N 页"按钮，点进去才是该页的参数网格。参数不超过一页容量时直接单页。
/// 页容量与前端网格一致（2 列 × 5 行）。
const SYNTHESIZED_PAGE_CAPACITY: usize = 10;

fn synthesize_paged_screens(option_names: &[String]) -> Vec<ShaderScreenInfo> {
    if option_names.len() <= SYNTHESIZED_PAGE_CAPACITY {
        return vec![flat_screen(option_names)];
    }
    let mut screens = vec![ShaderScreenInfo {
        name: "screen".to_string(),
        label: None,
        columns: DEFAULT_SCREEN_COLUMNS,
        tokens: Vec::new(),
    }];
    for (index, chunk) in option_names.chunks(SYNTHESIZED_PAGE_CAPACITY).enumerate() {
        let name = format!("auto_page_{}", index + 1);
        screens[0].tokens.push(ShaderScreenToken {
            kind: "screen".to_string(),
            name: Some(name.clone()),
        });
        screens.push(ShaderScreenInfo {
            name,
            label: Some(format!("第 {} 页", index + 1)),
            columns: DEFAULT_SCREEN_COLUMNS,
            tokens: chunk
                .iter()
                .map(|name| ShaderScreenToken {
                    kind: "option".to_string(),
                    name: Some(name.clone()),
                })
                .collect(),
        });
    }
    screens
}

/// 照搬 Iris `OptionMenuContainer` 的逐元素解析：token 解析不了（引用未检测到的参数、
/// `<profile>` 但包没有任何预设）就**跳过该元素**并记日志——Iris 对每个 token 单独
/// try/catch `IllegalArgumentException`，绝不因此废弃整个菜单。返回跳过的 token 清单。
fn filter_unresolved_screen_tokens(
    screens: &mut [ShaderScreenInfo],
    option_names: &std::collections::HashSet<String>,
    has_profiles: bool,
) -> Vec<String> {
    let mut dropped: Vec<String> = Vec::new();
    for screen in screens.iter_mut() {
        let mut kept = Vec::with_capacity(screen.tokens.len());
        for token in screen.tokens.drain(..) {
            let resolvable = match token.kind.as_str() {
                "empty" => true,
                "profile" => has_profiles,
                // `[子屏]` 链接 Iris 在构建期不校验目标是否存在
                "screen" => true,
                _ => token
                    .name
                    .as_deref()
                    .is_some_and(|name| option_names.contains(name)),
            };
            if resolvable {
                kept.push(token);
                continue;
            }
            let entry = match &token.name {
                Some(name) => format!("{}/{}", token.kind, name),
                None => format!("{}/<无名>", token.kind),
            };
            if !dropped.contains(&entry) {
                dropped.push(entry);
            }
        }
        screen.tokens = kept;
    }
    dropped
}

/// 解析单个 screen token：`<empty>` / `<profile>` / `[子屏]` / 参数名。
fn parse_screen_token(token: &str) -> ShaderScreenToken {
    if token == "<empty>" {
        return ShaderScreenToken {
            kind: "empty".to_string(),
            name: None,
        };
    }
    if token == "<profile>" {
        return ShaderScreenToken {
            kind: "profile".to_string(),
            name: None,
        };
    }
    if let Some(name) = token.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
        return ShaderScreenToken {
            kind: "screen".to_string(),
            name: Some(name.to_string()),
        };
    }
    ShaderScreenToken {
        kind: "option".to_string(),
        name: Some(token.to_string()),
    }
}

/// 组装分页界面：主页（没有 `screen=` 就合成一份"全部参数"）、子屏按出现顺序排在后面；
/// 没被任何 screen 引用到的参数追加到主页末尾（OptiFine 的兜底行为，保证都能被看到）。
fn build_screens(
    main_tokens: Option<Vec<String>>,
    main_columns: u32,
    sub_screens: Vec<(String, u32, Vec<String>)>,
    option_names: &[String],
) -> Vec<ShaderScreenInfo> {
    let Some(raw_main) = main_tokens else {
        // 没有 screen=：强制分页形态（参数多时主页是"第 N 页"按钮）
        return synthesize_paged_screens(option_names);
    };

    let mut screens = vec![ShaderScreenInfo {
        name: "screen".to_string(),
        label: None,
        columns: main_columns,
        tokens: raw_main.iter().map(|token| parse_screen_token(token)).collect(),
    }];
    for (name, columns, tokens) in sub_screens {
        screens.push(ShaderScreenInfo {
            name,
            label: None,
            columns,
            tokens: tokens.iter().map(|token| parse_screen_token(token)).collect(),
        });
    }

    // 注意：screen 布局里没列出的参数不做追加——Iris 的 GUI 只渲染布局里列出的元素，
    // 包作者没列的（MC_NORMAL_MAP / FSH / VSH 等内部 define）就该保持隐藏。
    screens
}

/// 预设值 token：`opt=value` / `opt`（开）/ `!opt`（关）。
fn parse_profile_tokens(value: &str) -> Vec<ShaderProfileToken> {
    value
        .split_whitespace()
        .filter_map(|token| {
            if let Some(option) = token.strip_prefix('!') {
                (!option.is_empty()).then(|| ShaderProfileToken {
                    option: option.to_string(),
                    value: "false".to_string(),
                })
            } else if let Some((option, option_value)) = token.split_once('=') {
                (!option.is_empty()).then(|| ShaderProfileToken {
                    option: option.to_string(),
                    value: option_value.to_string(),
                })
            } else {
                Some(ShaderProfileToken {
                    option: token.to_string(),
                    value: "true".to_string(),
                })
            }
        })
        .collect()
}

fn push_option_value(
    names: &mut Vec<String>,
    values: &mut HashMap<String, Vec<String>>,
    name: &str,
    value: &str,
) {
    if !names.iter().any(|existing| existing == name) {
        names.push(name.to_string());
    }
    let bucket = values.entry(name.to_string()).or_default();
    if !bucket.iter().any(|existing| existing == value) {
        bucket.push(value.to_string());
    }
}

// ---------------------------------------------------------------------------
// 源码选项扫描（照搬 Viewpoint `IrisOptions` 的检测正则）
// ---------------------------------------------------------------------------

/// Viewpoint `IrisOptions` 扫描的源码扩展名。
const SOURCE_EXTENSIONS: [&str; 7] = ["vsh", "fsh", "gsh", "csh", "tcs", "tes", "glsl"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceOptionKind {
    /// `#define NAME // 注释`（`//#define` = 注释掉，默认关）。
    Toggle,
    /// `#define NAME 值 // 注释 [允许值...]`。
    Value,
    /// `const int|float|bool NAME = 值; // 注释 [允许值...]`。
    Const,
}

#[derive(Debug, Clone)]
pub(crate) struct SourceOption {
    pub name: String,
    pub kind: SourceOptionKind,
    pub default_value: String,
    /// 方括号注释里的允许值（TOGGLE 没有，游戏内取值就是 true/false）。
    pub allowed: Vec<String>,
    /// `//` 注释里方括号外的说明文字。
    pub comment: Option<String>,
}

/// 逐行扫描一个源码文件，找出选项声明（未匹配的行忽略）。
pub(crate) fn scan_source_options(content: &str) -> Vec<SourceOption> {
    content.lines().filter_map(parse_source_option_line).collect()
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// 取出一个 `\w+` 词，返回 (词, 剩余文本)。
fn take_word(text: &str) -> Option<(&str, &str)> {
    let end = text
        .bytes()
        .position(|byte| !is_word_byte(byte))
        .unwrap_or(text.len());
    if end == 0 {
        None
    } else {
        Some((&text[..end], &text[end..]))
    }
}

/// 解析 `//` 之后的注释：`说明文字 [允许值1 允许值2]`。
/// 返回 (说明文字, 允许值)；没有方括号时 allowed 为空。
fn parse_bracket_comment(comment: &str) -> (Option<String>, Vec<String>) {
    let Some(open) = comment.find('[') else {
        let text = comment.trim();
        return (
            (!text.is_empty()).then(|| text.to_string()),
            Vec::new(),
        );
    };
    let text = comment[..open].trim();
    let inner = &comment[open + 1..];
    let allowed = inner
        .find(']')
        .map(|close| inner[..close].split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    ((!text.is_empty()).then(|| text.to_string()), allowed)
}

/// 解析一行源码里的选项声明，正则语义与 Viewpoint `IrisOptions` 一致：
/// - `^\s*(//+)?\s*#\s*define\s+(\w+)\s*(//.*)?$` → TOGGLE（注释掉 = 默认 false）；
/// - `^\s*#\s*define\s+(\w+)\s+([^\s/]+)\s*//[^\[]*\[([^\]]*)\].*$` → VALUE；
/// - `^\s*const\s+(?:int|float|bool)\s+(\w+)\s*=\s*([^;]+?)\s*;\s*//[^\[]*\[([^\]]*)\].*$` → CONST。
fn parse_source_option_line(line: &str) -> Option<SourceOption> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("const") {
        return parse_const_line(trimmed);
    }

    // `//` 前缀（可以多于两个斜杠）= 被注释掉的开关声明
    let (commented_out, body) = match trimmed.strip_prefix("//") {
        Some(rest) => (true, rest.trim_start_matches('/').trim_start()),
        None => (false, trimmed),
    };
    let body = body.strip_prefix('#')?.trim_start();
    let rest = body.strip_prefix("define")?;
    // `#\s*define`：# 与 define 之间可以有空白，define 后必须是空白或行尾
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let (name, tail) = take_word(rest.trim_start())?;
    let tail = tail.trim_start();

    // TOGGLE：`[#]define NAME` 或 `[#]define NAME // 注释`
    if tail.is_empty() {
        return Some(toggle_option(name, commented_out, None));
    }
    if let Some(comment) = tail.strip_prefix("//") {
        let (comment_text, _) = parse_bracket_comment(comment);
        return Some(toggle_option(name, commented_out, comment_text));
    }

    // 带值的形态只认未注释的（regex 没有 `(//+)?` 前缀）
    if commented_out {
        return None;
    }
    // 值 = [^\s/]+
    let value_end = tail
        .bytes()
        .position(|byte| byte.is_ascii_whitespace() || byte == b'/')
        .unwrap_or(tail.len());
    let value = &tail[..value_end];
    let after_value = tail[value_end..].trim_start();
    let comment = after_value.strip_prefix("//")?;
    let (comment_text, allowed) = parse_bracket_comment(comment);
    // 带值但注释里没有 [允许值] → 普通宏，不是可调选项
    if allowed.is_empty() {
        return None;
    }
    Some(SourceOption {
        name: name.to_string(),
        kind: SourceOptionKind::Value,
        default_value: value.to_string(),
        allowed,
        comment: comment_text,
    })
}

fn toggle_option(name: &str, commented_out: bool, comment: Option<String>) -> SourceOption {
    SourceOption {
        name: name.to_string(),
        kind: SourceOptionKind::Toggle,
        default_value: if commented_out { "false" } else { "true" }.to_string(),
        allowed: Vec::new(),
        comment,
    }
}

fn parse_const_line(trimmed: &str) -> Option<SourceOption> {
    let rest = trimmed.strip_prefix("const")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim_start();
    let (type_token, rest) = take_word(rest)?;
    if !matches!(type_token, "int" | "float" | "bool") {
        return None;
    }
    let (name, rest) = take_word(rest.trim_start())?;
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    // 值到第一个 ';'（惰性匹配 + 去尾空白）
    let semi = rest.find(';')?;
    let value = rest[..semi].trim_end();
    if value.is_empty() {
        return None;
    }
    let comment = rest[semi + 1..].trim_start().strip_prefix("//")?;
    let (comment_text, allowed) = parse_bracket_comment(comment);
    if allowed.is_empty() {
        return None;
    }
    Some(SourceOption {
        name: name.to_string(),
        kind: SourceOptionKind::Const,
        default_value: value.to_string(),
        allowed,
        comment: comment_text,
    })
}

/// 从光影包 zip 里读 `shaders/shaders.properties`（思路文档 §5：zip 内需要解压读取）。
/// 兜底取路径最短的 `shaders.properties`；包里没有时返回 `Ok(None)`（如 Viewpoint 内置包）。
/// 生产路径已由 `read_pack_shader_meta` 的定位逻辑取代（它还要拿包根），保留给测试与复用。
#[allow(dead_code)]
pub(crate) fn read_shaders_properties_from_pack(pack_path: &Path) -> Result<Option<String>, String> {
    let file = fs::File::open(pack_path)
        .map_err(|error| format!("打开光影包 {} 失败: {error}", pack_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("解析光影包 {} 失败: {error}", pack_path.display()))?;

    let mut candidate: Option<(usize, String)> = None;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| format!("遍历光影包 {} 失败: {error}", pack_path.display()))?
            .name()
            .to_string();
        if !name.ends_with("shaders.properties") || !name.contains("shaders/") {
            continue;
        }
        if name == "shaders/shaders.properties" {
            candidate = Some((index, name));
            break;
        }
        match &candidate {
            Some((_, current)) if current.len() <= name.len() => {}
            _ => candidate = Some((index, name)),
        }
    }
    let Some((index, _)) = candidate else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    archive
        .by_index(index)
        .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?;
    Ok(Some(decode_properties_bytes(&bytes)))
}

/// 编码检测（思路文档 §4.3）：优先 UTF-8，失败按 Latin-1 逐字节映射。
fn decode_properties_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&byte| byte as char).collect(),
    }
}

/// 读 live 文件文本（缺失返回 None；编码同上）。
pub(crate) fn read_live_properties_at(zomboid_dir: &Path) -> Result<Option<String>, String> {
    let live_path = viewpoint_live_path_at(zomboid_dir);
    let bytes = match fs::read(&live_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 {} 失败: {error}", live_path.display())),
    };
    Ok(Some(decode_properties_bytes(&bytes)))
}

/// 照搬 Viewpoint `IrisOptions` 的选项检测：
/// 扫描 zip 内 `shaders/` 下所有源码文件（.vsh/.fsh/.gsh/.csh/.tcs/.tes/.glsl）里的
/// `#define` / `const` 选项声明（真实光影包的选项几乎全在这里），再合并
/// `shaders.properties` 的 profiles / `sliders` / 顶层默认值声明。
/// 两者都拿不到任何选项时返回 Err（内置包 / 非 MC 光影包）。
pub(crate) fn read_pack_shader_meta(pack_path: &Path) -> Result<ShadersPropertiesMeta, String> {
    // 定位 shaders.properties：优先标准路径 `shaders/`，兜底一层顶层目录包裹
    // （`Wrapper/shaders/…`，OptiFine / Iris 都支持）——包根由它的位置决定
    let file = fs::File::open(pack_path)
        .map_err(|error| format!("打开光影包 {} 失败: {error}", pack_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("解析光影包 {} 失败: {error}", pack_path.display()))?;

    let mut properties_entry: Option<(usize, String)> = None;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| format!("遍历光影包 {} 失败: {error}", pack_path.display()))?
            .name()
            .to_string();
        if name == "shaders/shaders.properties" {
            properties_entry = Some((index, "shaders/".to_string()));
            break;
        }
        if properties_entry.is_none() {
            if let Some(wrapper) = name.strip_suffix("/shaders/shaders.properties") {
                if !wrapper.contains('/') {
                    properties_entry = Some((index, format!("{wrapper}/shaders/")));
                }
            }
        }
    }

    let mut properties_content: Option<String> = None;
    let mut base = "shaders/".to_string();
    if let Some((index, found_base)) = properties_entry {
        let mut bytes = Vec::new();
        archive
            .by_index(index)
            .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?
            .read_to_end(&mut bytes)
            .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?;
        properties_content = Some(decode_properties_bytes(&bytes));
        base = found_base;
    }

    // 扫描包根下的源码声明
    let mut scanned_any_source = false;
    let mut names: Vec<String> = Vec::new();
    let mut options: HashMap<String, ShaderOptionMeta> = HashMap::new();

    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| format!("遍历光影包 {} 失败: {error}", pack_path.display()))?
            .name()
            .to_string();
        let Some(rest) = name.strip_prefix(&base) else {
            continue;
        };
        let is_source = rest
            .rsplit('.')
            .next()
            .is_some_and(|ext| SOURCE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
        if !is_source {
            continue;
        }
        let mut bytes = Vec::new();
        archive
            .by_index(index)
            .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?
            .read_to_end(&mut bytes)
            .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?;
        scanned_any_source = true;
        for source_option in scan_source_options(&decode_properties_bytes(&bytes)) {
            merge_source_option(&mut names, &mut options, source_option);
        }
    }

    let properties = properties_content
        .as_deref()
        .map(parse_shaders_properties)
        .unwrap_or_default();
    if properties_content.is_none() && !scanned_any_source {
        return Err(format!(
            "{} 里没有 shaders.properties（可能是 Viewpoint 内置包或非 MC 光影包）",
            pack_path.display()
        ));
    }

    // shaders.properties 的顶层默认值声明：已扫到的选项补默认值/取值，没扫到的追加
    for option in properties.options {
        match options.get_mut(&option.name) {
            Some(existing) => {
                if existing.default_value.is_none() {
                    existing.default_value = option.default_value.clone();
                }
                for value in &option.values {
                    if !existing.values.iter().any(|probe| probe == value) {
                        existing.values.push(value.clone());
                    }
                }
                existing.slider |= option.slider;
            }
            None => {
                names.push(option.name.clone());
                options.insert(option.name.clone(), option);
            }
        }
    }

    let mut meta = ShadersPropertiesMeta {
        options: names
            .into_iter()
            .filter_map(|name| options.remove(&name))
            .collect(),
        profiles: properties.profiles,
        sliders: properties.sliders,
        screens: properties.screens,
    };
    // `sliders=` 列表对源码扫描出的选项同样生效
    for option in &mut meta.options {
        if meta.sliders.iter().any(|slider| *slider == option.name) {
            option.slider = true;
        }
    }
    // Iris 的行为（OptionMenuContainer 逐元素 try/catch）：解析不了的 token 跳过并记日志，
    // 菜单照常构建。注意：布局里没列出的参数（MC_NORMAL_MAP / FSH / VSH 等内部 define）
    // 保持隐藏——Iris 的 GUI 只渲染布局里列出的元素。
    let option_names: Vec<String> = meta.options.iter().map(|option| option.name.clone()).collect();
    let option_set: std::collections::HashSet<String> = option_names.iter().cloned().collect();
    let dropped = filter_unresolved_screen_tokens(&mut meta.screens, &option_set, !meta.profiles.is_empty());
    if !dropped.is_empty() {
        launcher_log::warn(&format!(
            "光影包解析：screen 布局里 {} 个 token 引用了不存在的参数（同 Iris 会抛异常），已跳过：{}",
            dropped.len(),
            dropped.join("、")
        ));
    }
    // 分页设计强制兜底（两步）：
    // ① Eclipse 这类没有 shaders.properties 的纯源码包，parse 层没机会合成布局 → 补单页；
    // ② 布局存在但过滤后所有页都空了（screen= 全引用不存在的参数）→ 同样补单页，
    //    绝不让弹窗落到"无可调参数"的空状态。
    if meta.screens.is_empty() || meta.screens.iter().all(|screen| screen.tokens.is_empty()) {
        meta.screens = synthesize_paged_screens(&option_names);
    }
    Ok(meta)
}

/// 合并一个源码声明的选项：首个声明定默认值/说明，允许值做并集。
fn merge_source_option(
    names: &mut Vec<String>,
    options: &mut HashMap<String, ShaderOptionMeta>,
    source: SourceOption,
) {
    if let Some(existing) = options.get_mut(&source.name) {
        for value in source.allowed {
            if !existing.values.iter().any(|existing_value| *existing_value == value) {
                existing.values.push(value);
            }
        }
        return;
    }
    // TOGGLE 的取值就是 true/false，默认值排前面；VALUE/CONST 默认值也进可选值
    let mut values = source.allowed.clone();
    if source.kind == SourceOptionKind::Toggle {
        let other = if source.default_value == "true" { "false" } else { "true" };
        values.insert(0, other.to_string());
        values.insert(0, source.default_value.clone());
    } else if !values.iter().any(|value| *value == source.default_value) {
        values.insert(0, source.default_value.clone());
    }
    names.push(source.name.clone());
    options.insert(
        source.name.clone(),
        ShaderOptionMeta {
            name: source.name,
            default_value: Some(source.default_value),
            slider: false,
            values,
            comment: source.comment,
        },
    );
}

// ---------------------------------------------------------------------------
// shaders/lang 本地化标签（把参数名 / 取值替换成用户能看懂的描述）
// ---------------------------------------------------------------------------

/// 一个光影包 `shaders/lang/*.lang` 的标签集（OptiFine / Iris 通用格式）。
#[derive(Debug, Default, Clone)]
pub(crate) struct ShaderLangLabels {
    entries: HashMap<String, String>,
    /// key 的小写索引 → 原始 key（lang 键大小写敏感，但有些包写得随意，做不敏感兜底）。
    lower_index: HashMap<String, String>,
}

impl ShaderLangLabels {
    fn lookup(&self, key: &str) -> Option<&str> {
        self.entries
            .get(key)
            .map(String::as_str)
            .or_else(|| {
                self.lower_index
                    .get(&key.to_lowercase())
                    .and_then(|original| self.entries.get(original))
                    .map(String::as_str)
            })
    }

    /// `option.<NAME>` 参数显示名。
    pub(crate) fn option_label(&self, name: &str) -> Option<String> {
        self.lookup(&format!("option.{name}")).map(str::to_string)
    }

    /// `option.<NAME>.comment` 参数说明。
    pub(crate) fn option_comment(&self, name: &str) -> Option<String> {
        self.lookup(&format!("option.{name}.comment"))
            .map(str::to_string)
    }

    /// `profile.<NAME>` 预设显示名。
    pub(crate) fn profile_label(&self, name: &str) -> Option<String> {
        self.lookup(&format!("profile.{name}")).map(str::to_string)
    }

    /// `screen.<NAME>` 分页界面显示名。
    pub(crate) fn screen_label(&self, name: &str) -> Option<String> {
        self.lookup(&format!("screen.{name}")).map(str::to_string)
    }

    /// 取值显示名，兼容真实光影包的两种键形态（思路来自 OptiFine GUI）：
    /// 1. 按值：`value.<NAME>.<VALUE>`（BSL：`value.shadowMapResolution.512=Very Low (512)`）；
    /// 2. 按序号：`value.<NAME>.<INDEX>`（Bliss：`value.RESOURCEPACK_SKY.0=不使用`，INDEX 是允许值列表下标）；
    /// 3. 通用 `value.<VALUE>` 兜底（`value.true=On`）。
    /// 数值先原样匹配再去掉小数尾零（shaders.properties 写 `2048.0`，lang 里常写 `2048`）。
    pub(crate) fn value_label(&self, name: &str, value: &str, index: usize) -> Option<String> {
        let variants = numeric_value_variants(value);
        for candidate in &variants {
            if let Some(label) = self.lookup(&format!("value.{name}.{candidate}")) {
                return Some(label.to_string());
            }
        }
        if let Some(label) = self.lookup(&format!("value.{name}.{index}")) {
            return Some(label.to_string());
        }
        for candidate in &variants {
            if let Some(label) = self.lookup(&format!("value.{candidate}")) {
                return Some(label.to_string());
            }
        }
        None
    }

    /// MC 语言的键级回退语义：切换语言里缺失的键用 en_US 兜底（chosen 覆盖 base）。
    pub(crate) fn merged_lang_fallback(base: &ShaderLangLabels, chosen: ShaderLangLabels) -> ShaderLangLabels {
        let mut merged = chosen;
        for (key, value) in &base.entries {
            merged
                .entries
                .entry(key.clone())
                .or_insert_with(|| value.clone());
            merged
                .lower_index
                .entry(key.to_lowercase())
                .or_insert_with(|| key.clone());
        }
        merged
    }
}

/// 数值匹配变体：`2048.0` → [`2048.0`, `2048`]；`0.60` → [`0.60`, `0.6`]；非数值原样。
fn numeric_value_variants(value: &str) -> Vec<String> {
    let mut variants = vec![value.to_string()];
    if value.contains('.') {
        let trimmed = value.trim_end_matches('0').trim_end_matches('.');
        if !trimmed.is_empty() {
            variants.push(trimmed.to_string());
        }
    }
    variants
}

/// 解析 .lang 内容：`key=value` 行，`#` 注释；重复键后者覆盖。
pub(crate) fn parse_lang_content(content: &str) -> ShaderLangLabels {
    let mut labels = ShaderLangLabels::default();
    for line in content.lines() {
        let Some((key, value)) = parse_entry_line(line) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        labels
            .lower_index
            .entry(key.to_lowercase())
            .or_insert_with(|| key.clone());
        labels.entries.insert(key, value);
    }
    labels
}

/// 一个包的 lang 选择结果：选中的标签 + 实际选中语言 + 包里全部可用语言。
#[derive(Debug, Clone)]
pub(crate) struct ShaderPackLang {
    pub labels: ShaderLangLabels,
    /// 实际选中的语言（文件名字干原大小写，如 "zh_CN"）。
    pub chosen: String,
    /// 包里可用的全部语言（原大小写文件名字干，去重，首次出现顺序）。
    pub available: Vec<String>,
}

/// 读 zip 内 `shaders/lang/*.lang`：按 `preferred` 顺序选语言（如 `["zh_CN", "en_US"]`），
/// 都没有就取目录里第一个 .lang；包里没有 lang 目录返回 `Ok(None)`。
pub(crate) fn read_pack_lang(
    pack_path: &Path,
    preferred: &[String],
) -> Result<Option<ShaderPackLang>, String> {
    let file = fs::File::open(pack_path)
        .map_err(|error| format!("打开光影包 {} 失败: {error}", pack_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("解析光影包 {} 失败: {error}", pack_path.display()))?;

    // 收集 shaders/lang/ 直接子层的 .lang：(index, 小写文件名, 原大小写字干)
    let mut candidates: Vec<(usize, String, String)> = Vec::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| format!("遍历光影包 {} 失败: {error}", pack_path.display()))?
            .name()
            .to_string();
        if let Some(rest) = name.strip_prefix("shaders/lang/") {
            if rest.ends_with(".lang") && !rest.contains('/') {
                let stem = rest.trim_end_matches(".lang").to_string();
                candidates.push((index, stem.to_lowercase(), stem));
            }
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }
    let chosen_index = preferred
        .iter()
        .map(|stem| stem.to_lowercase())
        .find_map(|stem| {
            candidates
                .iter()
                .find(|(_, candidate, _)| *candidate == stem)
                .map(|(index, _, _)| *index)
        })
        .or_else(|| {
            candidates
                .iter()
                .find(|(_, candidate, _)| candidate == "en_us")
                .map(|(index, _, _)| *index)
        })
        .unwrap_or(candidates[0].0);

    let mut bytes = Vec::new();
    archive
        .by_index(chosen_index)
        .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取光影包 {} 失败: {error}", pack_path.display()))?;
    let labels = parse_lang_content(&decode_properties_bytes(&bytes));

    let mut available: Vec<String> = Vec::new();
    for (_, _, stem) in &candidates {
        if !available.iter().any(|existing| existing == stem) {
            available.push(stem.clone());
        }
    }
    let chosen = candidates
        .iter()
        .find(|(index, _, _)| *index == chosen_index)
        .map(|(_, _, stem)| stem.clone())
        .unwrap_or_default();
    Ok(Some(ShaderPackLang {
        labels,
        chosen,
        available,
    }))
}

/// 弹窗默认语言偏好：中文优先（简体 → 繁体）；没有中文时由 `read_pack_lang` 回退 en_US / 第一个。
pub(crate) fn default_lang_stems() -> Vec<String> {
    vec!["zh_CN".to_string(), "zh_TW".to_string()]
}

// ---------------------------------------------------------------------------
// 覆写配置存储：`.pzmanager/shaderpack-overrides.json`
// ---------------------------------------------------------------------------

/// 单个光影包的覆写配置。`pack_name` 是 viewpoint-shaderpacks 目录里的 zip 文件名。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ShaderpackOverrideEntry {
    pub pack_name: String,
    /// 选用的预设（shaders.properties 的 `profile.*`）；空 = 不覆写预设。
    pub profile: Option<String>,
    /// 参数覆写值（键 = shaders.properties 的参数名）。
    pub values: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ShaderpackOverridesFile {
    pub format_version: u32,
    pub packs: Vec<ShaderpackOverrideEntry>,
}

static SHADERPACK_OVERRIDES_LOCK: Mutex<()> = Mutex::new(());

fn shaderpack_overrides_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join(SHADERPACK_OVERRIDES_FILE))
}

pub(crate) fn read_shaderpack_overrides_at(path: &Path) -> Result<ShaderpackOverridesFile, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ShaderpackOverridesFile {
                format_version: SHADERPACK_OVERRIDES_FORMAT,
                packs: Vec::new(),
            })
        }
        Err(error) => return Err(format!("读取 {} 失败: {error}", path.display())),
    };
    serde_json::from_str(&content).map_err(|error| format!("解析 {} 失败: {error}", path.display()))
}

pub(crate) fn write_shaderpack_overrides_at(
    path: &Path,
    state: &ShaderpackOverridesFile,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("创建目录 {} 失败: {error}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("序列化光影包覆写失败: {error}"))?;
    write_file(path, content.as_bytes())
}

fn load_shaderpack_overrides() -> Result<ShaderpackOverridesFile, String> {
    read_shaderpack_overrides_at(&shaderpack_overrides_path()?)
}

/// 新增 / 更新一个包的覆写配置（profile 空串归一为 None，空参数名丢弃）。
pub(crate) fn upsert_shaderpack_override(
    state: &mut ShaderpackOverridesFile,
    pack_name: &str,
    profile: Option<String>,
    values: BTreeMap<String, String>,
) {
    let profile = profile
        .map(|profile| profile.trim().to_string())
        .filter(|profile| !profile.is_empty());
    let values: BTreeMap<String, String> = values
        .into_iter()
        .filter(|(name, _)| !name.trim().is_empty())
        .collect();
    let entry = ShaderpackOverrideEntry {
        pack_name: pack_name.to_string(),
        profile,
        values,
    };
    match state.packs.iter_mut().find(|entry| entry.pack_name == pack_name) {
        Some(existing) => *existing = entry,
        None => state.packs.push(entry),
    }
}

/// 移除一个包的覆写配置；返回是否真的移除了。
pub(crate) fn remove_shaderpack_override(state: &mut ShaderpackOverridesFile, pack_name: &str) -> bool {
    let before = state.packs.len();
    state.packs.retain(|entry| entry.pack_name != pack_name);
    before != state.packs.len()
}

// ---------------------------------------------------------------------------
// 备份 / 还原（思路文档 §4 + §8：备份不可省略，整文件还原）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ViewpointLiveBackupManifest {
    /// 启动前 viewpoint-live.properties 是否存在；不存在时还原 = 删除覆写产生的文件。
    pub original_existed: bool,
    /// 备份文件名（`original_existed = false` 时为 None）。
    pub backup_file: Option<String>,
    pub created_at: String,
}

pub(crate) fn viewpoint_live_path_at(zomboid_dir: &Path) -> PathBuf {
    zomboid_dir.join(VIEWPOINT_LIVE_PROPERTIES_FILE)
}

fn viewpoint_live_backup_dir_at(zomboid_dir: &Path) -> PathBuf {
    zomboid_dir.join(".pzmanager").join(VIEWPOINT_LIVE_BACKUP_DIR)
}

/// 写覆写前备份：整份原文件 + manifest，保留最近 `VIEWPOINT_LIVE_BACKUP_MAX` 份。
pub(crate) fn write_viewpoint_live_backup_at(zomboid_dir: &Path) -> Result<(), String> {
    let live_path = viewpoint_live_path_at(zomboid_dir);
    let backup_dir = viewpoint_live_backup_dir_at(zomboid_dir);
    fs::create_dir_all(&backup_dir)
        .map_err(|error| format!("创建备份目录 {} 失败: {error}", backup_dir.display()))?;
    let stamp = current_unix_timestamp()?;
    let existed = live_path.is_file();
    let backup_file = if existed {
        let name = format!("{VIEWPOINT_LIVE_PROPERTIES_FILE}.bak.{stamp}");
        fs::copy(&live_path, backup_dir.join(&name))
            .map_err(|error| format!("备份 {} 失败: {error}", live_path.display()))?;
        Some(name)
    } else {
        None
    };
    let manifest = ViewpointLiveBackupManifest {
        original_existed: existed,
        backup_file,
        created_at: stamp,
    };
    let manifest_path = backup_dir.join(VIEWPOINT_LIVE_BACKUP_MANIFEST);
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("序列化备份清单失败: {error}"))?;
    write_file(&manifest_path, content.as_bytes())?;
    prune_viewpoint_live_backups_at(&backup_dir);
    Ok(())
}

/// 只保留最近 `VIEWPOINT_LIVE_BACKUP_MAX` 份备份（文件名带纳秒时间戳，字典序 = 时间序）。
fn prune_viewpoint_live_backups_at(backup_dir: &Path) {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return;
    };
    let prefix = format!("{VIEWPOINT_LIVE_PROPERTIES_FILE}.bak.");
    let mut backups: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix))
        .collect();
    backups.sort();
    while backups.len() > VIEWPOINT_LIVE_BACKUP_MAX {
        let oldest = backups.remove(0);
        let _ = fs::remove_file(backup_dir.join(oldest));
    }
}

/// 恢复上次覆写前的 viewpoint-live.properties（整文件还原）。
/// 返回 `Ok(Some(消息))` = 已恢复；`Ok(None)` = 没有待恢复的备份。
pub(crate) fn restore_viewpoint_live_backup_at(zomboid_dir: &Path) -> Result<Option<String>, String> {
    let live_path = viewpoint_live_path_at(zomboid_dir);
    let backup_dir = viewpoint_live_backup_dir_at(zomboid_dir);
    let manifest_path = backup_dir.join(VIEWPOINT_LIVE_BACKUP_MANIFEST);
    let content = match fs::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取 {} 失败: {error}", manifest_path.display())),
    };
    let manifest: ViewpointLiveBackupManifest = match serde_json::from_str(&content) {
        Ok(manifest) => manifest,
        Err(error) => {
            // 清单损坏就挪到一边并报错：还原做不了，但不能一直卡住后续覆写。
            let quarantined = format!("{VIEWPOINT_LIVE_BACKUP_MANIFEST}.corrupt.{}", current_unix_timestamp()?);
            let _ = fs::rename(&manifest_path, backup_dir.join(&quarantined));
            return Err(format!(
                "解析 {} 失败: {error}（清单已改名为 {quarantined}，请检查备份目录）",
                manifest_path.display()
            ));
        }
    };

    if manifest.original_existed {
        let Some(backup_file) = &manifest.backup_file else {
            return Err("备份清单缺少备份文件名，无法还原".to_string());
        };
        let backup_path = backup_dir.join(backup_file);
        let bytes = fs::read(&backup_path)
            .map_err(|error| format!("读取备份 {} 失败: {error}", backup_path.display()))?;
        write_file(&live_path, &bytes)?;
    } else if live_path.exists() {
        fs::remove_file(&live_path)
            .map_err(|error| format!("移除 {} 失败: {error}", live_path.display()))?;
    }
    let _ = fs::remove_file(&manifest_path);
    Ok(Some(if manifest.original_existed {
        format!("已还原 {}。", live_path.display())
    } else {
        format!("已移除覆写生成的 {}（启动前不存在）。", live_path.display())
    }))
}

// ---------------------------------------------------------------------------
// 启动覆写
// ---------------------------------------------------------------------------

/// 一次启动覆写的结果（调用方据此记日志 / 决定是否启动还原线程）。
#[derive(Debug, Clone)]
pub(crate) struct ShaderOverrideApplyOutcome {
    pub applied: bool,
    pub reason: String,
    pub packs: usize,
    pub keys: usize,
}

/// 把覆写配置行级写进 viewpoint-live.properties（思路文档 §4 安全写入流程）。
/// `game_running` 由调用方注入以便测试；只在游戏关闭时写入。
pub(crate) fn apply_shaderpack_overrides_at_dir(
    zomboid_dir: &Path,
    overrides: &[ShaderpackOverrideEntry],
    game_running: bool,
) -> Result<ShaderOverrideApplyOutcome, String> {
    if overrides.is_empty() {
        return Ok(ShaderOverrideApplyOutcome {
            applied: false,
            reason: "未配置光影包覆写".to_string(),
            packs: 0,
            keys: 0,
        });
    }
    if game_running {
        return Ok(ShaderOverrideApplyOutcome {
            applied: false,
            reason: "游戏正在运行，Viewpoint 退出时会重写配置，已跳过覆写".to_string(),
            packs: 0,
            keys: 0,
        });
    }

    // 上次启动器中途退出留下的备份：先恢复原状，再基于原文件覆写。
    if let Some(message) = restore_viewpoint_live_backup_at(zomboid_dir)? {
        launcher_log::info(&format!("光影包覆写：检测到未还原的备份，{message}"));
    }

    let live_path = viewpoint_live_path_at(zomboid_dir);
    let original = read_live_properties_at(zomboid_dir)?;
    let mut document = PropertiesDocument::parse(original.as_deref().unwrap_or(""));

    let packs_dir = zomboid_dir.join("viewpoint-shaderpacks");
    let mut touched: Vec<(String, String)> = Vec::new();
    let mut changed_keys = 0usize;
    let mut applied_packs = 0usize;

    for entry in overrides {
        let (pack_id, matched) = resolve_pack_id(&entry.pack_name, &document);
        if !matched {
            launcher_log::warn(&format!(
                "光影包覆写：{} 的 packId {} 是按文件名推导的，live 文件里还没有它的键；游戏内打开一次该包设置即可让 Viewpoint 写入真实键。",
                entry.pack_name, pack_id
            ));
        }
        // 参数白名单（思路文档 §5）：包能解析出选项元数据时只写其中存在的参数
        //（检测方式与弹窗一致：源码 #define/const 扫描 + shaders.properties）。
        let whitelist: Option<Vec<String>> =
            read_pack_shader_meta(&packs_dir.join(&entry.pack_name))
                .ok()
                .map(|meta| {
                    meta.options
                        .into_iter()
                        .map(|option| option.name)
                        .collect()
                });

        let mut pack_keys = 0usize;
        if let Some(profile) = &entry.profile {
            let key = format!("{MINECRAFT_PROFILE_KEY_PREFIX}{pack_id}");
            let normalized = normalize_property_value(profile)?;
            if document.set(&key, profile)? {
                changed_keys += 1;
            }
            touched.push((key, normalized));
            pack_keys += 1;
        }
        for (param, value) in &entry.values {
            if let Some(names) = &whitelist {
                if !names.iter().any(|name| name == param) {
                    launcher_log::warn(&format!(
                        "光影包覆写：{} 的参数 {} 不在 shaders.properties 中，已跳过",
                        entry.pack_name, param
                    ));
                    continue;
                }
            }
            let key = format!("{MINECRAFT_KEY_PREFIX}{pack_id}.{param}");
            let normalized = normalize_property_value(value)?;
            if document.set(&key, value)? {
                changed_keys += 1;
            }
            touched.push((key, normalized));
            pack_keys += 1;
        }
        if pack_keys > 0 {
            applied_packs += 1;
        }
    }

    if changed_keys == 0 {
        return Ok(ShaderOverrideApplyOutcome {
            applied: false,
            reason: "覆写值与当前文件一致，无需修改".to_string(),
            packs: 0,
            keys: 0,
        });
    }

    // 备份 → 原子替换 → 回读校验（思路文档 §4.2/4.7/4.9）。
    write_viewpoint_live_backup_at(zomboid_dir)?;
    let rendered = document.render();
    apply_file_transaction(vec![(live_path.clone(), rendered.as_bytes().to_vec())])?;

    let verify_errors: Vec<String> = {
        let written = read_live_properties_at(zomboid_dir)?;
        let verify = PropertiesDocument::parse(written.as_deref().unwrap_or(""));
        touched
            .iter()
            .filter(|(key, expected)| verify.get(key).as_deref() != Some(expected.as_str()))
            .map(|(key, expected)| format!("{key} 应为 {expected:?}"))
            .collect()
    };
    if !verify_errors.is_empty() {
        if let Some(message) = restore_viewpoint_live_backup_at(zomboid_dir)? {
            launcher_log::warn(&format!("光影包覆写：{message}"));
        }
        return Err(format!(
            "回读校验失败（{}），已用备份还原",
            verify_errors.join("；")
        ));
    }

    Ok(ShaderOverrideApplyOutcome {
        applied: true,
        reason: String::new(),
        packs: applied_packs,
        keys: changed_keys,
    })
}

/// 启动命令里调用：读真实目录与覆写文件执行覆写；记日志，失败不阻断启动。
/// 返回是否真的写入了（决定要不要启动还原监听线程）。
pub(crate) fn apply_shaderpack_overrides_for_launch() -> Result<bool, String> {
    let zomboid_dir = user_zomboid_directory()?;
    let state = load_shaderpack_overrides()?;
    let outcome = apply_shaderpack_overrides_at_dir(&zomboid_dir, &state.packs, project_zomboid_running());
    match &outcome {
        Ok(outcome) if outcome.applied => {
            launcher_log::info(&format!(
                "光影包覆写：已写入 {} 个光影包共 {} 项，游戏退出后自动还原。",
                outcome.packs, outcome.keys
            ));
            Ok(true)
        }
        Ok(outcome) if outcome.reason == "未配置光影包覆写" => Ok(false),
        Ok(outcome) => {
            launcher_log::info(&format!("光影包覆写：{}。", outcome.reason));
            Ok(false)
        }
        Err(error) => Err(error.clone()),
    }
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

/// 弹窗数据：包元数据 + live 当前值 + 已存覆写。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderpackConfigInfo {
    pub pack_name: String,
    /// 推导 / 反查出的 packId（写入键用）。
    pub pack_id: String,
    /// packId 是否与 live 文件里已出现的键对上；false = 推导值，UI 上提示用户游戏内验证。
    pub pack_id_matched: bool,
    /// 实际使用的 lang 文件名字干（如 "zh_CN"）；包里没有 lang = None。
    pub lang: Option<String>,
    /// 包里可用的全部语言（文件名字干，去重）。
    pub available_langs: Vec<String>,
    /// 分页界面（Iris 同款布局）；第一个是主页。
    pub screens: Vec<ShaderScreenInfo>,
    pub profiles: Vec<ShaderProfileMeta>,
    pub options: Vec<ShaderOptionInfo>,
    /// live 文件里当前的 `minecraftProfile.<packId>`。
    pub live_profile: Option<String>,
    pub saved: Option<ShaderpackOverrideEntry>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderOptionInfo {
    pub name: String,
    /// lang 文件里的显示名（`option.<name>`）；没有 = None（前端回退参数名）。
    pub label: Option<String>,
    /// lang 文件里的说明（`option.<name>.comment`）。
    pub comment: Option<String>,
    pub default_value: Option<String>,
    pub slider: bool,
    pub values: Vec<String>,
    /// 有本地化标签的观测取值（`value.<name>.<value>` / 通用 `value.<value>`）。
    pub value_labels: Vec<ShaderValueLabel>,
    /// live 文件里的当前值（没有 = 该参数还没在游戏里调过）。
    pub live_value: Option<String>,
}

/// 一个取值的本地化显示名。
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShaderValueLabel {
    pub value: String,
    pub label: String,
}

fn is_valid_pack_file_name(pack_name: &str) -> bool {
    !pack_name.is_empty()
        && !pack_name.contains('\\')
        && !pack_name.contains('/')
        && !pack_name.contains("..")
}

#[tauri::command(async)]
pub(crate) fn read_shaderpack_config(
    pack_name: String,
    lang: Option<String>,
) -> Result<ShaderpackConfigInfo, String> {
    if !is_valid_pack_file_name(&pack_name) {
        return Err("非法的光影包文件名".to_string());
    }
    let zomboid_dir = user_zomboid_directory()?;
    let pack_path = zomboid_dir.join("viewpoint-shaderpacks").join(&pack_name);
    // 选项检测照搬 Viewpoint `IrisOptions`：源码 #define/const 扫描 + shaders.properties 合并
    let ShadersPropertiesMeta {
        options,
        profiles,
        screens,
        ..
    } = read_pack_shader_meta(&pack_path)?;

    // 描述语言：弹窗里切换指定的优先；否则默认中文（zh_CN → zh_TW），没有中文回退 en_US / 第一个
    let lang_preference: Vec<String> = lang
        .as_deref()
        .map(|stem| vec![stem.to_string()])
        .unwrap_or_else(default_lang_stems);
    let pack_lang = read_pack_lang(&pack_path, &lang_preference)?;
    let mut lang_labels = pack_lang
        .as_ref()
        .map(|pack_lang| pack_lang.labels.clone())
        .unwrap_or_default();
    // MC 的键级语言回退：切换语言缺失的键用 en_US 补上（本身选的就是 en_US 时不重复读）
    let chosen = pack_lang.as_ref().map(|pack_lang| pack_lang.chosen.clone());
    if chosen
        .as_deref()
        .is_some_and(|stem| !stem.eq_ignore_ascii_case("en_US"))
    {
        if let Ok(Some(base)) = read_pack_lang(&pack_path, &["en_US".to_string()]) {
            lang_labels = ShaderLangLabels::merged_lang_fallback(&base.labels, lang_labels);
        }
    }
    let available_langs = pack_lang
        .as_ref()
        .map(|pack_lang| pack_lang.available.clone())
        .unwrap_or_default();

    let original = read_live_properties_at(&zomboid_dir)?;
    let document = PropertiesDocument::parse(original.as_deref().unwrap_or(""));
    let (pack_id, matched) = resolve_pack_id(&pack_name, &document);
    let option_infos = options
        .into_iter()
        .map(|option| {
            let live_value = document.get(&format!("{MINECRAFT_KEY_PREFIX}{pack_id}.{}", option.name));
            // 取值标签带允许值列表下标，兼容按值（BSL）/ 按序号（Bliss）两种 lang 键形态
            let value_labels = option
                .values
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    lang_labels
                        .value_label(&option.name, value, index)
                        .map(|label| ShaderValueLabel {
                            value: value.clone(),
                            label,
                        })
                })
                .collect();
            let ShaderOptionMeta {
                name,
                default_value,
                slider,
                values,
                comment,
            } = option;
            ShaderOptionInfo {
                label: lang_labels.option_label(&name),
                // lang 文件的说明优先，其次源码声明里的 `// 注释`
                comment: lang_labels.option_comment(&name).or(comment),
                name,
                default_value,
                slider,
                values,
                value_labels,
                live_value,
            }
        })
        .collect();
    let mut profiles = profiles;
    for profile in &mut profiles {
        profile.label = lang_labels.profile_label(&profile.name);
    }
    let mut screens = screens;
    for screen in &mut screens {
        // lang 里有 screen.<name> 就用翻译；没有（如合成的"第 N 页"）保留后端给的标签
        screen.label = lang_labels
            .screen_label(&screen.name)
            .or_else(|| screen.label.clone());
    }
    let live_profile = document.get(&format!("{MINECRAFT_PROFILE_KEY_PREFIX}{pack_id}"));
    let saved = load_shaderpack_overrides()?
        .packs
        .into_iter()
        .find(|entry| entry.pack_name == pack_name);

    Ok(ShaderpackConfigInfo {
        pack_name,
        pack_id,
        pack_id_matched: matched,
        lang: chosen,
        available_langs,
        profiles,
        options: option_infos,
        screens,
        live_profile,
        saved,
    })
}

#[tauri::command]
pub(crate) fn get_shaderpack_overrides() -> Result<Vec<ShaderpackOverrideEntry>, String> {
    Ok(load_shaderpack_overrides()?.packs)
}

#[tauri::command(async)]
pub(crate) fn save_shaderpack_override(
    pack_name: String,
    profile: Option<String>,
    values: BTreeMap<String, String>,
) -> Result<(), String> {
    if !is_valid_pack_file_name(&pack_name) {
        return Err("非法的光影包文件名".to_string());
    }
    let _guard = SHADERPACK_OVERRIDES_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = shaderpack_overrides_path()?;
    let mut state = read_shaderpack_overrides_at(&path)?;
    upsert_shaderpack_override(&mut state, &pack_name, profile, values);
    write_shaderpack_overrides_at(&path, &state)
}

#[tauri::command(async)]
pub(crate) fn clear_shaderpack_override(pack_name: String) -> Result<(), String> {
    let _guard = SHADERPACK_OVERRIDES_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = shaderpack_overrides_path()?;
    let mut state = read_shaderpack_overrides_at(&path)?;
    remove_shaderpack_override(&mut state, &pack_name);
    write_shaderpack_overrides_at(&path, &state)
}
