//! 应用设置：归一化、settings.json 读写与设置命令。
//!
//! 通过 `lib.rs` 的 `pub(crate) use settings::*;` 保持原有调用路径。

use crate::*;
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

pub(crate) fn normalize_window_width(value: u32) -> Option<u32> {
    (WINDOW_WIDTH_MIN..=WINDOW_SIZE_MAX)
        .contains(&value)
        .then_some(value)
}

pub(crate) fn normalize_window_height(value: u32) -> Option<u32> {
    (WINDOW_HEIGHT_MIN..=WINDOW_SIZE_MAX)
        .contains(&value)
        .then_some(value)
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
        return Ok(AppSettings::default());
    }
    let content = fs::read_to_string(path).map_err(|error| format!("读取应用设置失败: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("解析应用设置失败: {error}"))
}

pub(crate) fn save_app_settings_at(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建应用设置目录失败: {error}"))?;
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("序列化应用设置失败: {error}"))?;
    fs::write(path, format!("{content}\n")).map_err(|error| format!("保存应用设置失败: {error}"))
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
pub(crate) fn save_developer_mode(enabled: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.developer_mode = enabled;
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_steam_account_hidden(hidden: bool) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.steam_account_hidden = hidden;
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_translation_language(language: String) -> Result<(), String> {
    let normalized = normalize_translation_language(&language)
        .ok_or_else(|| format!("不支持的翻译语言: {language}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.translation_language = Some(normalized);
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_font_contrast(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_contrast = Some(normalize_font_contrast(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_blur_strength(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.blur_strength = Some(normalize_blur_strength(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_font_scale(value: u8) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_scale = Some(normalize_font_scale(value));
    save_app_settings_at(&path, &settings)
}

#[tauri::command]
pub(crate) fn save_font_family(value: String) -> Result<(), String> {
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.font_family = normalize_font_family(&value);
    save_app_settings_at(&path, &settings)
}

/// 覆盖层背景色（设置 › 通用）：存 `#RRGGBB`，写空串 = 恢复默认纯红。
#[tauri::command]
pub(crate) fn save_overlay_background_color(value: String) -> Result<(), String> {
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
pub(crate) fn restore_default_window_size(app: tauri::AppHandle) -> Result<(), String> {
    
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
pub(crate) fn save_mod_loader_scheme(scheme: String) -> Result<(), String> {
    let normalized = normalize_mod_loader_scheme(&scheme)
        .ok_or_else(|| format!("不支持的模组加载器方案: {scheme}"))?;
    let path = app_settings_path()?;
    let mut settings = load_app_settings_at(&path)?;
    settings.mod_loader_scheme = Some(normalized);
    save_app_settings_at(&path, &settings)
}

#[tauri::command(async)]
pub(crate) fn save_memory_limit(memory_mb: u64) -> Result<(), String> {
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
