use super::{
    apply_file_transaction, apply_jar_mod_state_to_roots, apply_launch_loader_at_paths,
    build_fast_launch_cache, class_annotations, compose_title_background_at,
    configure_viewpoint_three_d_at_paths, create_fast_launch_backup_at, delete_shaderpack_at,
    discover_mod_info_files, fast_launch_cache_matches, fast_launch_cache_mods_at,
    force_zombie_buddy_policy_at_paths, game_path_from_library, import_shaderpack_to,
    install_locations_from_libraries, install_zombie_buddy_at_paths,
    is_builtin_background_path, jar_scan_candidates, jar_scan_inputs,
    library_paths_from_config, list_viewpoint_shaderpacks_at, list_viewpoint_shots_at,
    steam_screenshots_directory_at,
    load_app_settings_at, merge_lua_mod_entry, mod_inner_event, mod_jar_path,
    mod_loader_policy_agent, mod_marker_event, mod_poster_path, mod_progress_units,
    normalize_blur_strength, normalize_font_contrast, normalize_font_family,
    normalize_font_scale, normalize_mod_loader_scheme, normalize_translation_language,
    clamp_window_height, clamp_window_width, order_mod_ids_by_requires,
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
    validate_memory_limit_mb, vdf_value, version_step_for, viewpoint_three_d_status,
    workshop_path_from_library,
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
fn offers_force_install_when_workshop_classes_match_no_game_revision() {
    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("library");
    let version_root = library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(PROJECT_ZOMBOID_APP_ID)
        .join("3805285544")
        .join("mods")
        .join("PZ_Optimization")
        .join("42");
    let classes = version_root.join("pzopt-classes");
    fs::create_dir_all(classes.join("pzopt")).unwrap();
    fs::write(classes.join("pzopt-files.txt"), "pzopt/Test.class\n").unwrap();
    fs::write(classes.join("pzopt").join("Test.class"), b"class").unwrap();
    fs::write(
        classes.join("pzopt").join("build-info.properties"),
        "revision=4a0e9546ec\nbuilt=42\n",
    )
    .unwrap();
    fs::write(version_root.join("install.ps1"), "# fixture\n").unwrap();

    // 模拟 b42.22：游戏 revision 与 Workshop 里的构建不一致。
    let game = temp.path().join("game");
    fs::create_dir_all(&game).unwrap();
    let jar = fs::File::create(game.join("projectzomboid.jar")).unwrap();
    let mut zip = ZipWriter::new(jar);
    zip.start_file("zombie/GitVersion.class", SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"revision e013dbb529").unwrap();
    zip.finish().unwrap();

    let libraries = [library];
    let status = pz_optimization_status(Some(&game), &libraries);
    assert!(!status.available);
    assert!(status.force_install_available);
    assert!(status.installer_path.is_some());
    assert_eq!(status.source_path, None);
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
        aggressive_memory_clean: true,
        wallpaper_effect: None,
        steam_relay_mode: Some("relay".to_string()),
        steam_relay_base: Some("https://mirror.example/steam".to_string()),
        steam_proxy: Some("socks5://127.0.0.1:1080".to_string()),
        version_store_path: None,
        version_parallel: true,
        version_include_mods: true,
        version_system_drive_notice_dismissed: false,
        privacy_consented_version: None,
    };

    save_app_settings_at(&path, &settings).unwrap();

    assert_eq!(load_app_settings_at(&path).unwrap(), settings);
}

#[test]
fn mod_step_progress_is_weighted_per_mod() {
    // 第 2 个模组（index 1）、共 3 个：起点是整条进度的 1/3。
    let marker = mod_marker_event(1, 3, "Beta");
    assert_eq!(marker.phase, "mods");
    assert_eq!((marker.processed_bytes, marker.total_bytes), (1000, 3000));
    assert_eq!(marker.current_path.as_deref(), Some("Beta（2/3）"));

    // 模组内部 50% → 整条进度的 1.5/3，而不是 50%。
    let inner = mod_inner_event(
        1,
        3,
        "Beta",
        crate::versioning::ProgressEvent {
            phase: "copying".to_string(),
            processed_files: 5,
            total_files: 10,
            processed_bytes: 50,
            total_bytes: 100,
            current_path: Some("file.bin".to_string()),
        },
    );
    assert_eq!(inner.phase, "mods");
    assert_eq!((inner.processed_bytes, inner.total_bytes), (1500, 3000));
    assert_eq!(inner.current_path.as_deref(), Some("Beta（2/3）"));

    // 内层进度被夹在 0–1，不会跳回上一个模组、也不会越过下一个。
    let overshoot = mod_inner_event(
        1,
        3,
        "Beta",
        crate::versioning::ProgressEvent {
            phase: "copying".to_string(),
            processed_files: 99,
            total_files: 10,
            processed_bytes: 999,
            total_bytes: 100,
            current_path: None,
        },
    );
    assert_eq!(overshoot.processed_bytes, 2000);
    assert_eq!(mod_progress_units(3, 0.0, 3), (3000, 3000));
}

#[test]
fn maps_version_phases_to_operation_steps() {
    // 游戏大版本：计算哈希 → 复制/保存变化 → 保存模组版本
    assert_eq!(version_step_for("snapshot", "preparing"), 1);
    assert_eq!(version_step_for("snapshot", "hashing"), 1);
    assert_eq!(version_step_for("snapshot", "copying"), 2);
    assert_eq!(version_step_for("snapshot", "mods"), 3);
    assert_eq!(version_step_for("capture", "delta"), 2);
    // 还原：检查 → 还原（含 PZ Optimization）→ 清理 → 还原模组版本
    assert_eq!(version_step_for("restore", "hashing"), 1);
    assert_eq!(version_step_for("restore", "pzopt"), 2);
    assert_eq!(version_step_for("restore", "restoring"), 2);
    assert_eq!(version_step_for("restore", "cleaning"), 3);
    assert_eq!(version_step_for("restore", "mods"), 4);
    // 单个模组
    assert_eq!(version_step_for("modSnapshot", "copying"), 2);
    assert_eq!(version_step_for("modCapture", "delta"), 2);
    assert_eq!(version_step_for("modRestore", "cleaning"), 3);
    assert_eq!(version_step_for("verify", "hashing"), 1);
    // 结束态不推进步骤
    assert_eq!(version_step_for("capture", "done"), 0);
    assert_eq!(version_step_for("capture", "error"), 0);
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
    // 窗口尺寸低于下限 / 超过防御上限时钳位保存（DPI 缩放下拖到最小尺寸附近
    // 的逻辑尺寸可能略低于 840×560，直接拒绝会让这次调整「不被记住」）。
    assert_eq!(clamp_window_width(WINDOW_WIDTH_MIN - 1), WINDOW_WIDTH_MIN);
    assert_eq!(clamp_window_width(WINDOW_WIDTH_MIN), WINDOW_WIDTH_MIN);
    assert_eq!(clamp_window_height(559), 560);
    assert_eq!(clamp_window_height(560), 560);
    assert_eq!(clamp_window_width(10001), 10000);
}

fn scanned_jar(id: &str, decision: Option<bool>, disabled: bool) -> ScannedJarMod {
    ScannedJarMod {
        id: id.to_string(),
        info_id: id.to_string(),
        mod_info_path: format!("H:/mods/{id}/mod.info"),
        mod_root: format!("H:/mods/{id}"),
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

#[test]
fn steam_screenshots_path_is_userdata_accountid_760_remote_108600() {
    let path = steam_screenshots_directory_at(Path::new("C:/Program Files (x86)/Steam"), "1222587902");
    let expected = Path::new("C:/Program Files (x86)/Steam")
        .join("userdata")
        .join("1222587902")
        .join("760")
        .join("remote")
        .join("108600")
        .join("screenshots");
    assert_eq!(path, expected);
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
fn keeps_loaders_ahead_of_a_manually_moved_order() {
    // 手动拖动/上下移只提交可排序的模组，加载器可能被挤到后面；写回时必须回到最前，
    // 否则 ZombieBuddy 会排在普通模组之后加载，整套依赖都断掉。
    let mut loader = lua_mod("ZombieBuddy", "ZombieBuddy");
    loader.is_loader = true;
    let entries = vec![loader, lua_mod("ModManager", "Mod Manager"), lua_mod("Other", "Other")];

    let ordered = order_mod_ids_by_requires(
        &entries,
        &[
            "Other".to_string(),
            "ModManager".to_string(),
            "ZombieBuddy".to_string(),
        ],
    );

    assert_eq!(ordered, ["ZombieBuddy", "ModManager", "Other"]);
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
