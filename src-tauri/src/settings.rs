//! 应用设置：归一化、settings.json 读写与设置命令。
//!
//! 通过 `lib.rs` 的 `pub(crate) use settings::*;` 保持原有调用路径。

use crate::*;
use std::sync::Mutex;
use tauri::Manager;
/// 设置页「通用 › 翻译语言」的取值：和前端 TRANSLATION_LANGUAGE_OPTIONS 一一对应。
/// 翻译链路（`news_translation::normalize_language`）其实认更多语言，但设置里只暴露这 6 种，
/// 免得下拉框显示的语言和实际拿去翻译的语言对不上。
pub(crate) const TRANSLATION_LANGUAGES: [&str; 6] = ["zh-CN", "zh-TW", "en", "ja", "ko", "ru"];

pub(crate) const DEFAULT_TRANSLATION_LANGUAGE: &str = "zh-CN";

/// 大小写不敏感地归一化到上面的取值；不在列表里返回 None。
pub(crate) fn normalize_translation_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    TRANSLATION_LANGUAGES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}

/// 「通用 › 翻译引擎」的取值：auto = 默认链（Google → 必应 → MyMemory）；
/// 其余为手动指定的优先引擎，失败后仍按默认顺序回退其余引擎。
/// relay（中转服务器的 Argos 离线翻译）已从可选引擎下线：旧配置里的 relay
/// 归一化时不在列表内，自动回落默认引擎；管线里的 relay 代码保留但不再被选中。
pub(crate) const TRANSLATION_ENGINES: [&str; 4] = ["auto", "bing", "google", "mymemory"];

/// 没写过 / 写坏时的默认引擎：Google（2026-11-06 起由「默认链首选必应」改为 Google 优先）。
pub(crate) const DEFAULT_TRANSLATION_ENGINE: &str = "google";

/// 大小写不敏感地归一化到上面的取值；不在列表里返回 None。
pub(crate) fn normalize_translation_engine(engine: &str) -> Option<String> {
    let trimmed = engine.trim();
    TRANSLATION_ENGINES
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
        .map(|candidate| (*candidate).to_string())
}

/// 翻译管线读的当前引擎偏好；读设置失败 / 坏值按默认引擎处理。
pub(crate) fn configured_translation_engine() -> String {
    app_settings_path()
        .and_then(|path| load_app_settings_at(&path))
        .ok()
        .and_then(|settings| settings.translation_engine)
        .and_then(|value| normalize_translation_engine(&value))
        .unwrap_or_else(|| DEFAULT_TRANSLATION_ENGINE.to_string())
}

/// 字体对比度：0（保持现有配色）到 100（浅色面上的文字压到近黑）。
/// 用来处理"文字与背景、尤其是亚克力面板叠加后的底色太接近"。设置页「通用 › 字体对比度」。
pub(crate) const FONT_CONTRAST_MAX: u8 = 100;

pub(crate) fn normalize_font_contrast(value: u8) -> u8 {
    value.min(FONT_CONTRAST_MAX)
}

/// 模糊度：0（关掉模糊）到 200（设计基准的两倍），100 = 设计基准。
/// 设置页「通用 › 模糊度」，前端换算成 `--pz-blur-scale` 作用到全站毛玻璃 / 亚克力。
pub(crate) const BLUR_STRENGTH_MAX: u8 = 200;

pub(crate) const BLUR_STRENGTH_DEFAULT: u8 = 100;

pub(crate) fn normalize_blur_strength(value: u8) -> u8 {
    value.min(BLUR_STRENGTH_MAX)
}

/// 界面字号缩放（百分比）：80–130，100 = 设计基准。
/// 前端换算成 `--pz-font-scale` 作用到根 font-size，全站 rem 字号随之缩放。设置页「通用 › 界面字号」。
pub(crate) const FONT_SCALE_MIN: u8 = 80;

pub(crate) const FONT_SCALE_MAX: u8 = 130;

pub(crate) const FONT_SCALE_DEFAULT: u8 = 100;

pub(crate) fn normalize_font_scale(value: u8) -> u8 {
    value.clamp(FONT_SCALE_MIN, FONT_SCALE_MAX)
}

/// 界面字体：存完整的 CSS font-family 列表；空 / 缺失 = 回落 App.css 的默认字体栈。
pub(crate) const FONT_FAMILY_MAX_LEN: usize = 200;

pub(crate) fn normalize_font_family(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(FONT_FAMILY_MAX_LEN).collect())
}

/// 覆盖层背景色：存 `#RRGGBB`；空 / 缺失 = 覆盖层默认纯红。
/// 只接受 3 位或 6 位十六进制（`#` 可省），统一规范成大写 `#RRGGBB`，
/// 这样状态文件里永远是一个确定格式，DLL 解析失败时会保留原色而不是变黑。
pub(crate) fn normalize_overlay_background_color(value: &str) -> Option<String> {
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
pub(crate) const DEFAULT_OVERLAY_BACKGROUND_COLOR: &str = "#FF0000";

/// 启动器窗口尺寸（逻辑像素）：下限与两份 tauri conf 的 minWidth/minHeight 一致；
/// 上限只是防御性的，防止手改 settings.json 写进离谱数值。
pub(crate) const WINDOW_WIDTH_MIN: u32 = 840;

pub(crate) const WINDOW_HEIGHT_MIN: u32 = 560;

pub(crate) const WINDOW_SIZE_MAX: u32 = 10000;

/// 把记忆的窗口尺寸钳位到合法区间（最小 840×560）：
/// DPI 缩放下拖到最小尺寸附近时，逻辑尺寸可能略低于下限——
/// 之前直接拒绝（None → 不落盘），导致这类调整「不会被记住」；现在钳位保存。
pub(crate) fn clamp_window_width(value: u32) -> u32 {
    value.clamp(WINDOW_WIDTH_MIN, WINDOW_SIZE_MAX)
}

pub(crate) fn clamp_window_height(value: u32) -> u32 {
    value.clamp(WINDOW_HEIGHT_MIN, WINDOW_SIZE_MAX)
}

pub(crate) fn user_zomboid_directory() -> Result<PathBuf, String> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|profile| profile.join("Zomboid"))
        .ok_or_else(|| "找不到当前 Windows 用户目录".to_string())
}

pub(crate) fn pzmanager_directory() -> Result<PathBuf, String> {
    Ok(user_zomboid_directory()?.join(".pzmanager"))
}

pub(crate) fn app_settings_path() -> Result<PathBuf, String> {
    Ok(pzmanager_directory()?.join("settings.json"))
}

pub(crate) fn load_app_settings_at(path: &Path) -> Result<AppSettings, String> {
    if !path.is_file() {
        // `AppSettings::default()` 来自 derive，布尔字段全是 false；这里把 serde 里
        // 「缺失 = 默认开启」的字段补上，否则全新安装会和默认值对不上：
        // - 「启动时保留 JAR 模组启用状态」
        // - 版本保存多线程 / 保存版本时同时备份模组（版本页的默认开关应为开）
        let mut settings = AppSettings::default();
        settings.jar_mod_use_disk_state_on_startup = default_jar_mod_use_disk_state_on_startup();
        settings.version_parallel = crate::default_version_parallel_enabled();
        settings.version_include_mods = crate::default_version_parallel_enabled();
        return Ok(settings);
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取应用设置失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析应用设置失败: {error}"))
}

/// settings.json 的全部读写都过这把锁：约 20 个命令各自「读全文 → 改一个字段 → 写全文」，
/// 同步命令跑在主线程、异步命令跑在 tokio worker，没有这把锁就会互相覆盖丢字段。
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// 串行地「加载 → 修改 → 保存」一份应用设置。
/// 所有改设置字段的命令都应走这里，而不是各自 load/save。
pub(crate) fn update_app_settings(
    update: impl FnOnce(&mut AppSettings),
) -> Result<AppSettings, String> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    update(&mut settings);
    save_app_settings_at(&path, &settings)?;
    Ok(settings)
}

pub(crate) fn save_app_settings_at(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败: {error}"))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化应用设置失败: {error}"))?;
    // 先写临时文件再改名：写一半崩溃/断电不会留下解析失败的 settings.json——
    // 那会让所有 save_*/get_app_settings/启动/版本操作全部报错，只能手删文件自救。
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, format!("{content}\n"))
        .map_err(|error| format!("保存应用设置失败: {error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("保存应用设置失败: {error}"))
}

#[tauri::command]
pub(crate) fn get_app_settings() -> Result<AppSettings, String> {
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
    // 翻译引擎：坏值回落默认（Google 优先 → 必应 → MyMemory）。
    settings.translation_engine = Some(
        settings
            .translation_engine
            .as_deref()
            .and_then(normalize_translation_engine)
            .unwrap_or_else(|| DEFAULT_TRANSLATION_ENGINE.to_string()),
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
    // 启动参数：去掉首尾空白与空条目，保证存进去的就是可用的参数列表。
    settings.launch_arguments = settings
        .launch_arguments
        .iter()
        .map(|argument| argument.trim().to_string())
        .filter(|argument| !argument.is_empty())
        .collect();
    Ok(settings)
}

/// 「启动详情 › 启动参数」：整体替换（前端传完整列表），立即生效并落盘。
#[tauri::command]
pub(crate) fn save_launch_arguments(arguments: Vec<String>) -> Result<Vec<String>, String> {
    update_app_settings(|settings| {
        settings.launch_arguments = arguments
            .iter()
            .map(|argument| argument.trim().to_string())
            .filter(|argument| !argument.is_empty())
            .collect();
    })?;
    Ok(load_app_settings_at(&app_settings_path()?)?.launch_arguments)
}

/// 「实验性 › 自动加入服务器」：保存开关 / 目标地址 / 密码。
/// 开启时目标必须是 `主机:端口`（校验失败整次拒绝，不留半开状态）；
/// 关闭时把目标与密码一起清空（明文不留盘），并顺手清掉残留的一次性配置。
#[tauri::command]
pub(crate) fn save_auto_join_settings(
    enabled: bool,
    target: String,
    password: String,
) -> Result<(), String> {
    let normalized_target = if enabled {
        Some(normalize_auto_join_target(&target).ok_or_else(|| {
            "服务器地址格式不对：应为 主机:端口，例如 1.2.3.4:16261".to_string()
        })?)
    } else {
        None
    };
    let normalized_password = if enabled {
        let trimmed = password.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    } else {
        None
    };
    update_app_settings(|settings| {
        settings.auto_join_enabled = enabled;
        settings.auto_join_target = normalized_target;
        settings.auto_join_password = normalized_password;
    })?;
    if !enabled {
        // 旧钩子机制的一次性配置已随机制退役（见 auto_join.rs）；关闭时顺手清掉残留。
        let ini = user_zomboid_directory()
            .ok()
            .map(|dir| dir.join("Lua").join("pzm-autojoin.ini"));
        if ini.as_deref().is_some_and(|path| path.is_file())
            && fs::remove_file(ini.unwrap()).is_ok()
        {
            launcher_log::info("自动加入：已关闭并清理残留的一次性配置。");
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn save_developer_mode(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.developer_mode = enabled)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_shaderpack_override_enabled(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.shaderpack_override_enabled = enabled)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_launch_from_game_files(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.launch_from_game_files = enabled)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_pause_music_on_launch(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.pause_music_on_launch = enabled)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_steam_account_hidden(hidden: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.steam_account_hidden = hidden)?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_translation_language(language: String) -> Result<(), String> {
    let normalized = normalize_translation_language(&language)
        .ok_or_else(|| format!("不支持的翻译语言: {language}"))?;
    update_app_settings(|settings| settings.translation_language = Some(normalized))?;
    Ok(())
}

/// 「通用 › 翻译引擎」：只落盘，之后的翻译请求立即生效（已翻好的内容保持原结果）。
#[tauri::command]
pub(crate) fn save_translation_engine(engine: String) -> Result<(), String> {
    let normalized = normalize_translation_engine(&engine)
        .ok_or_else(|| format!("不支持的翻译引擎: {engine}"))?;
    update_app_settings(|settings| settings.translation_engine = Some(normalized))?;
    // 用户明确切换引擎后，之前失败留下的静默期作废：立刻按新选择重试。
    crate::news_translation::clear_bing_backoff();
    crate::news_translation::clear_google_backoff();
    Ok(())
}

#[tauri::command]
pub(crate) fn save_font_contrast(value: u8) -> Result<(), String> {
    update_app_settings(|settings| settings.font_contrast = Some(normalize_font_contrast(value)))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_blur_strength(value: u8) -> Result<(), String> {
    update_app_settings(|settings| settings.blur_strength = Some(normalize_blur_strength(value)))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_font_scale(value: u8) -> Result<(), String> {
    update_app_settings(|settings| settings.font_scale = Some(normalize_font_scale(value)))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_font_family(value: String) -> Result<(), String> {
    update_app_settings(|settings| settings.font_family = normalize_font_family(&value))?;
    Ok(())
}

/// 覆盖层背景色（设置 › 通用 › 外观）：存 `#RRGGBB`，写空串 = 恢复默认纯红。
#[tauri::command]
pub(crate) fn save_overlay_background_color(value: String) -> Result<(), String> {
    update_app_settings(|settings| {
        settings.overlay_background_color = normalize_overlay_background_color(&value);
    })?;
    Ok(())
}

/// 记住当前窗口尺寸（前端在拖动结束后防抖调用）。
///
/// 尺寸由后端自己读取（物理像素 ÷ 缩放系数 = 逻辑像素），前端就不需要
/// inner-size / scale-factor 两个窗口权限；最大化时不记忆（那不是用户想要的固定尺寸）。
#[tauri::command]
pub(crate) fn save_window_size(app: tauri::AppHandle) -> Result<(u32, u32), String> {

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
        .map_err(|error| format!("读取窗口尺寸失败: {error}"))?;
    let width = (size.width as f64 / factor).round() as u32;
    let height = (size.height as f64 / factor).round() as u32;
    // 拖动过程的中间态也钳位到合法区间保存（DPI 缩放下最小尺寸附近可能略低于下限）。
    let width = clamp_window_width(width);
    let height = clamp_window_height(height);

    update_app_settings(|settings| {
        settings.window_width = Some(width);
        settings.window_height = Some(height);
    })?;
    Ok((width, height))
}


/// 恢复默认窗口尺寸：立即生效，同时清掉记住的尺寸（下次启动回到 conf 默认值）。
#[tauri::command]
pub(crate) fn restore_default_window_size(app: tauri::AppHandle) -> Result<(), String> {
    
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口。".to_string())?;
    window
        .set_size(tauri::LogicalSize::new(WINDOW_WIDTH_MIN, WINDOW_HEIGHT_MIN))
        .map_err(|error| format!("设置窗口尺寸失败: {error}"))?;

    update_app_settings(|settings| {
        settings.window_width = None;
        settings.window_height = None;
    })?;
    Ok(())
}

#[tauri::command]
pub(crate) fn save_mod_loader_scheme(scheme: String) -> Result<(), String> {
    let normalized = normalize_mod_loader_scheme(&scheme)
        .ok_or_else(|| format!("不支持的模组加载器方案: {scheme}"))?;
    update_app_settings(|settings| settings.mod_loader_scheme = Some(normalized))?;
    Ok(())
}

/// 「通用 › 外观 › 原生窗口标题栏」：切换系统原生标题栏（decorations），立即生效并落盘。
/// 自定义标题栏在个别设备 / WebView 上拖不动时用它兜底；开启后前端把自绘标题栏
/// 两侧的 LOGO 与窗口按钮滑出隐藏（动画在 CSS），中间的导航保留。
#[tauri::command]
pub(crate) fn save_native_titlebar(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "未找到主窗口。".to_string())?;
    // 先应用再落盘：切换失败时设置保持原值，界面和实际状态不会错位。
    window
        .set_decorations(enabled)
        .map_err(|error| format!("切换原生标题栏失败: {error}"))?;
    update_app_settings(|settings| settings.native_titlebar = enabled)?;
    Ok(())
}

/// 「模组加载器 › 启动时保留 JAR 模组启用状态」：只落盘，下一次启动 / 自动扫描时生效。
#[tauri::command]
pub(crate) fn save_jar_mod_use_disk_state_on_startup(enabled: bool) -> Result<(), String> {
    update_app_settings(|settings| settings.jar_mod_use_disk_state_on_startup = enabled)?;
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn save_memory_limit(memory_mb: u64) -> Result<(), String> {
    validate_memory_limit_mb(memory_mb, system_memory_mb()?)?;
    let steam_configs = current_steam_config_paths();
    if steam_configs.is_empty() {
        return Err("找不到 Steam localconfig.vdf".to_string());
    }
    // Steam 启动项里的 agent 按当前方案处理：方案 = 不使用加载器时**摘掉**，
    // 否则改一次内存上限就会把 -agentlib:zbNative 偷偷写回 Steam 配置。
    // 方案读取放在 update_app_settings 的锁内（读改写一体的旧写法在这里等价，
    // 但统一走锁，避免 async 命令与主线程的同步 save_* 竞争）。
    let policy_agent = {
        let path = app_settings_path()?;
        let _guard = SETTINGS_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let settings = load_app_settings_at(&path)?;
        settings
            .mod_loader_scheme
            .clone()
            .unwrap_or_else(|| DEFAULT_MOD_LOADER_SCHEME.to_string())
        // guard 在此 drop
    };
    patch_steam_launch_options_at_paths(
        &steam_configs,
        Some(memory_mb),
        &[],
        normalize_mod_loader_scheme(&policy_agent)
            .as_deref()
            .and_then(mod_loader_policy_agent),
    )?;
    // 同一份内存上限也写进游戏目录的 JSON / BAT，避免那里残留的旧 -Xmx 覆盖设置。
    force_zombie_buddy_policy(Some(memory_mb))?;
    update_app_settings(|settings| settings.memory_limit_mb = Some(memory_mb))?;
    Ok(())
}

/// 记录用户同意隐私协议时的协议版本号（首次启动与版本更新后都会要求重新同意）。
#[tauri::command]
pub(crate) fn save_privacy_consent(version: String) -> Result<(), String> {
    let trimmed = version.trim().to_string();
    if trimmed.is_empty() {
        return Err("隐私协议版本号不能为空".to_string());
    }
    update_app_settings(|settings| settings.privacy_consented_version = Some(trimmed))?;
    Ok(())
}

/// 撤销隐私协议同意：清空记录，下次启动会重新弹出同意框。
#[tauri::command]
pub(crate) fn clear_privacy_consent() -> Result<(), String> {
    update_app_settings(|settings| settings.privacy_consented_version = None)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LauncherUserDataClearResult {
    pub(crate) path: String,
    pub(crate) removed_entries: usize,
    pub(crate) freed_bytes: u64,
}

/// 递归统计条目占用（只用于清除前的提示，不是精确的磁盘回收量）。
fn directory_usage_bytes(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };
    if metadata.is_file() {
        return metadata.len();
    }
    if !metadata.is_dir() {
        return 0;
    }
    fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| directory_usage_bytes(&entry.path()))
                .sum()
        })
        .unwrap_or(0)
}

/// 开发者选项 › 清除启动器用户数据：删除 `%USERPROFILE%\Zomboid\.pzmanager` 下的
/// 全部启动器数据（设置、JAR 模组状态、模组预设、日志、背景图、反馈记录、
/// 极速启动备份与版本备份），不影响游戏本体、存档和模组文件。
#[tauri::command(async)]
pub(crate) fn clear_launcher_user_data() -> Result<LauncherUserDataClearResult, String> {
    // 原子地占住版本操作互斥标记：删除可能持续数秒（版本备份上 GB），
    // 只在删除前检查一次的话，期间启动的版本操作会把自己的存储目录删到一半。
    // 占住之后新版本操作会被 spawn_version_operation 直接拒绝，删完再释放。
    if crate::VERSION_OPERATION_ACTIVE
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err("请等当前版本操作完成后再清除用户数据。".to_string());
    }
    let result = clear_launcher_user_data_inner();
    // 无论成败都要释放标记：spawn_version_operation 完成时同样清它，语义一致。
    crate::VERSION_OPERATION_ACTIVE.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

fn clear_launcher_user_data_inner() -> Result<LauncherUserDataClearResult, String> {
    let directory = pzmanager_directory()?;
    let mut removed_entries = 0;
    let mut freed_bytes = 0;
    if directory.is_dir() {
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("读取启动器数据目录失败: {error}"))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("读取启动器数据目录失败: {error}"))?;
            let path = entry.path();
            freed_bytes += directory_usage_bytes(&path);
            if path.is_dir() {
                fs::remove_dir_all(&path)
                    .map_err(|error| format!("删除 {} 失败: {error}", path.display()))?;
            } else {
                fs::remove_file(&path)
                    .map_err(|error| format!("删除 {} 失败: {error}", path.display()))?;
            }
            removed_entries += 1;
        }
    }
    // 目录本身保留：后续写日志 / 设置不用再处理目录不存在的情况。
    fs::create_dir_all(&directory)
        .map_err(|error| format!("重建启动器数据目录失败: {error}"))?;
    Ok(LauncherUserDataClearResult {
        path: directory.to_string_lossy().into_owned(),
        removed_entries,
        freed_bytes,
    })
}
