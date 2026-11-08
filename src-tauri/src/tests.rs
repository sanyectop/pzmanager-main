use super::{
    adopt_jar_mod_state_from_roots, apply_file_transaction, apply_jar_mod_state_to_roots,
    apply_launch_loader_at_paths,
    build_fast_launch_cache, class_annotations, class_references, compose_title_background_at,
    configure_viewpoint_three_d_at_paths, create_fast_launch_backup_at, delete_shaderpack_at,
    discover_mod_info_files, fast_launch_cache_matches, fast_launch_cache_mods_at,
    force_zombie_buddy_policy_at_paths, game_path_from_library, import_shaderpack_to,
    install_locations_from_libraries, install_zombie_buddy_at_paths,
    jar_mod_injection_matches,
    is_builtin_background_path, jar_scan_candidates, jar_scan_inputs,
    library_paths_from_config, list_viewpoint_shaderpacks_at, list_viewpoint_shots_at,
    steam_screenshots_directory_at,
    load_app_settings_at, merge_lua_mod_entry, mod_inner_event, mod_jar_path,
    mod_loader_policy_agent, mod_marker_event, mod_poster_path, mod_progress_units,
    normalize_blur_strength, normalize_font_contrast, normalize_font_family,
    normalize_font_scale, normalize_mod_loader_scheme, normalize_translation_engine,
    normalize_translation_language, DEFAULT_TRANSLATION_ENGINE,
    clamp_window_height, clamp_window_width, order_mod_ids_by_requires,
    normalize_overlay_background_color,
    parse_default_mod_ids, parse_java_mod_info, parse_lua_mod_info, parse_lua_sorting_rules,
    patch_batch_content, patch_batch_content_with_memory, patch_batch_with_agent,
    patch_json_launcher_content, patch_json_launcher_content_with_memory,
    patch_json_launcher_with_agent, patch_steam_launch_options_content,
    steam_config_has_both, steam_launch_value_has_both,
    patch_steam_launch_options_content_with_memory, pick_lua_mod_id, plan_lua_mod_enable,
    quoted_key, replace_vdf_value,
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

use super::{
    apply_shaderpack_overrides_at_dir, candidate_pack_id, classify_key, default_lang_stems,
    normalize_property_value, observed_minecraft_pack_ids, parse_lang_content,
    parse_shaders_properties, read_pack_lang, read_pack_shader_meta, read_shaders_properties_from_pack,
    read_shaderpack_overrides_at, remove_shaderpack_override, resolve_pack_id,
    restore_viewpoint_live_backup_at, scan_source_options, upsert_shaderpack_override,
    write_shaderpack_overrides_at, write_viewpoint_live_backup_at, LiveKeyClass,
    PropertiesDocument, ShaderpackOverrideEntry, SHADERPACK_OVERRIDES_FORMAT,
};
use std::collections::BTreeMap;

#[test]
fn steam_launch_options_status_reads_the_effective_last_duplicate() {
    // 真实用户文件里出现过两条重复的 LaunchOptions（一条补丁写的、一条 Steam 里设的窗口参数）：
    // VDF 同名键后者生效，状态判定必须看最后一条，否则安装后依旧显示缺少。
    let block = |options: &str| {
        concat!(
            "\"UserLocalConfigStore\"\n",
            "{\n",
            "  \"apps\"\n",
            "  {\n",
            "    \"108600\"\n",
            "    {\n",
        )
        .to_string()
            + options
            + "\n    }\n  }\n}\n"
    };
    let with_agent =
        "      \"LaunchOptions\" \"-agentlib:zbNative=policy=allow-all -Xmx12288m\"";
    let without_agent =
        "      \"LaunchOptions\" \"-screen-width 1920 -screen-height 1080\"";

    // 只有前一条有 agent：生效值（最后一条）没配 → 未配置
    assert!(!steam_config_has_both(&block(&format!(
        "{with_agent}\n{without_agent}"
    ))));
    // 最后一条有 agent = 生效值已配置
    assert!(steam_config_has_both(&block(&format!(
        "{without_agent}\n{with_agent}"
    ))));
    // 现行写入格式已不含 `--` 分隔符（B42 Zulu 25 拒绝），单条带 agent 也算已配置
    assert!(steam_config_has_both(&block(with_agent)));
    assert!(steam_launch_value_has_both(
        "-agentlib:zbNative=policy=allow-all -Xmx12288m"
    ));
    assert!(!steam_launch_value_has_both("-screen-width 1920"));
}

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
        translation_engine: Some("bing".to_string()),
        font_contrast: Some(45),
        blur_strength: Some(120),
        font_family: Some("\"Microsoft YaHei UI\", sans-serif".to_string()),
        font_scale: Some(110),
        window_width: Some(970),
        window_height: Some(600),
        native_titlebar: true,
        launch_arguments: vec!["-agentlib:zbNative".to_string(), "--".to_string()],
        overlay_background_color: Some("#1E90FF".to_string()),
        mod_loader_scheme: Some("none".to_string()),
        fast_launch: false,
        jar_mod_use_disk_state_on_startup: true,
        developer_mode: true,
        steam_account_hidden: true,
        silent_memory_cleanup: true,
        silent_memory_cleanup_threshold: 85,
        clean_memory_on_launch: true,
        pause_music_on_launch: true,
        shaderpack_override_enabled: false,
        aggressive_memory_clean: true,
        wallpaper_effect: None,
        launch_music: None,
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
fn jar_mod_disk_state_defaults_to_enabled_for_new_or_old_settings() {
    let temp = tempfile::tempdir().unwrap();
    // 没有设置文件（新安装）。
    let missing = load_app_settings_at(&temp.path().join("settings.json")).unwrap();
    assert!(missing.jar_mod_use_disk_state_on_startup);

    // 老设置文件（没有这个字段）。
    let path = temp.path().join("legacy-settings.json");
    fs::write(&path, "{}\n").unwrap();
    let legacy = load_app_settings_at(&path).unwrap();
    assert!(legacy.jar_mod_use_disk_state_on_startup);

    // 用户显式关闭后要能读回 false。
    let mut settings = legacy;
    settings.jar_mod_use_disk_state_on_startup = false;
    save_app_settings_at(&path, &settings).unwrap();
    assert!(!load_app_settings_at(&path).unwrap().jar_mod_use_disk_state_on_startup);
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
        class_references: Vec::new(),
        decision,
    }
}

/// 注入项反查：崩溃诊断点名的类 → 拥有它的模组（弹窗底部列表的数据源）。
#[test]
fn injection_item_lookup_matches_all_patch_sources() {
    let mut mod_info = scanned_jar("DGV", Some(true), false);
    mod_info.class_references = vec![
        "zombie.util.list.PZArrayList".to_string(),
        "zombie.network.fields.hit.HitInfo".to_string(),
    ];
    mod_info.patch_classes = vec!["dgveyes/Patch_Foo".to_string()];
    mod_info.lua_classes = vec!["dgveyes/DGVEReach".to_string()];
    mod_info.patch_methods = vec![JavaPatchMethod {
        annotation: "Inject".to_string(),
        method_name: "onProneToggled".to_string(),
    }];

    // 游戏类引用命中，点分/斜杠/大小写三种写法等价。
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "zombie.util.list.PZArrayList"),
        vec!["zombie.util.list.PZArrayList"]
    );
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "zombie/util/list/PZArrayList"),
        vec!["zombie.util.list.PZArrayList"]
    );
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "  PZARRAYLIST "),
        vec!["zombie.util.list.PZArrayList"]
    );
    // 补丁类 / 暴露类（内部名，斜杠形式）同样能反查到。
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "dgveyes.Patch_Foo"),
        vec!["dgveyes/Patch_Foo"]
    );
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "DGVEReach"),
        vec!["dgveyes/DGVEReach"]
    );
    // 注入方法名也在范围内。
    assert_eq!(
        jar_mod_injection_matches(&mod_info, "onProneToggled"),
        vec!["onProneToggled"]
    );
    // 不相关 / 空查询不命中。
    assert!(jar_mod_injection_matches(&mod_info, "zombie.ai.StateMachine").is_empty());
    assert!(jar_mod_injection_matches(&mod_info, "   ").is_empty());
}

#[test]
fn builds_fast_launch_cache_from_approved_and_enabled_mods_only() {    let scanned = vec![
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
fn normalizes_translation_engine_setting() {
    assert_eq!(normalize_translation_engine("auto").as_deref(), Some("auto"));
    assert_eq!(normalize_translation_engine("Bing").as_deref(), Some("bing"));
    assert_eq!(
        normalize_translation_engine(" Google ").as_deref(),
        Some("google")
    );
    assert_eq!(
        normalize_translation_engine("MyMemory").as_deref(),
        Some("mymemory")
    );
    assert_eq!(normalize_translation_engine("RELAY").as_deref(), Some("relay"));
    // 设置页没提供的引擎和空值交给默认值（11-06 起默认 Google，见 news_translation 的引擎链）。
    assert_eq!(normalize_translation_engine("yandex"), None);
    assert_eq!(normalize_translation_engine(""), None);
    assert_eq!(DEFAULT_TRANSLATION_ENGINE, "google");
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

/// 最小可解析 classfile：常量池里塞任意 Utf8 条目，供常量池扫描类测试用。
fn classfile_with_utf8_entries(values: &[&str]) -> Vec<u8> {
    fn utf8(bytes: &mut Vec<u8>, value: &str) {
        bytes.push(1);
        bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0xcafebabe_u32.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&52_u16.to_be_bytes());
    // 常量池：#1 类名 → #2 Class(#1)，#3 父类名 → #4 Class(#3)，随后是测试用的 Utf8。
    // count = 条目数 + 1（index 0 保留槽），和 annotated_class_bytes 的算法一致。
    bytes.extend_from_slice(&((5 + values.len()) as u16).to_be_bytes());
    utf8(&mut bytes, "viewpoint/Patch_Foo");
    bytes.push(7);
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    utf8(&mut bytes, "java/lang/Object");
    bytes.push(7);
    bytes.extend_from_slice(&3_u16.to_be_bytes());
    for value in values {
        utf8(&mut bytes, value);
    }
    bytes.extend_from_slice(&0x0021_u16.to_be_bytes());
    bytes.extend_from_slice(&2_u16.to_be_bytes());
    bytes.extend_from_slice(&4_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u16.to_be_bytes());
    bytes
}

#[test]
fn extracts_game_class_references_from_classfile() {
    let class_bytes = classfile_with_utf8_entries(&[
        // 字段描述符：42.22 移除的 PZArrayList 就是这样被 DGVViewpointEyes 引用的。
        "Lzombie/util/list/PZArrayList;",
        // 方法描述符 + 泛型签名（同一条 Utf8 里两段类名都要收）。
        "(Lzombie/network/fields/hit/HitInfo;)V",
        "Lzombie/util/list/PZArrayList<Lzombie/network/fields/hit/HitInfo;>;",
        // @Patch 注解值是点分形式。
        "zombie.gameStates.GameLoadingState",
        // fmod 命名空间同样算游戏类。
        "fmod.fmod.FMODSoundEmitter",
        // 排除项：模组自己的注解 / 路径段 / 更长名字的尾巴 / 普通散文。
        "Lme/zed_0xff/zombie_buddy/Exposer$LuaClass;",
        "media/lua/client/zombie.foo",
        "me.zombie.utils.Helper",
        "the zombie. end",
    ]);

    let references = class_references(&class_bytes);

    assert_eq!(
        references,
        vec![
            "fmod.fmod.FMODSoundEmitter",
            "zombie.gameStates.GameLoadingState",
            "zombie.network.fields.hit.HitInfo",
            "zombie.util.list.PZArrayList",
        ]
    );
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
        patched.contains(r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -novid""#)
    );

    let (patched_again, changed_again) = patch_steam_launch_options_content(&patched).unwrap();
    assert!(!changed_again);
    assert_eq!(patched_again, patched);
}

#[test]
fn strips_bare_double_dash_from_existing_launch_options() {
    // B42 的 Zulu 25 JRE 直接拒绝 `--`（"Unrecognized option: --"，游戏 1-2 秒内秒退）。
    // 旧版本启动器曾自动往 agent 后面插 `--`；现在不再插入，并且会把存量 `--` 洗掉，
    // 用户下次启动时 vdf 自动痊愈，不用手动去 Steam 里清理。
    let content = r#""UserLocalConfigStore"
{
"Apps"
{
    "108600"
    {
        "LaunchOptions" "-agentlib:zbNative=policy=allow-all -Xmx10240m --"
    }
}
}"#;

    let (patched, changed) = patch_steam_launch_options_content_with_memory(
        content,
        Some(12288),
        &["-agentlib:zbNative".to_string(), "--".to_string()],
        Some(ZOMBIE_BUDDY_POLICY_AGENT),
    )
    .unwrap();

    assert!(changed);
    let options_line = patched
        .lines()
        .find(|line| line.contains("LaunchOptions"))
        .unwrap();
    assert!(!options_line.contains("--"), "不应残留任何 --：{options_line}");
    assert!(options_line.contains("-Xmx12288m"));
    assert!(options_line.contains("-agentlib:zbNative=policy=allow-all"));
}

#[test]
fn does_not_add_separator_after_an_existing_agent() {
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
    let options_line = patched
        .lines()
        .find(|line| line.contains("LaunchOptions"))
        .unwrap();
    assert!(options_line.contains(r#""-agentlib:zbNative=policy=allow-all -novid""#));
    assert!(!options_line.contains("--"));
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
        Some(ZOMBIE_BUDDY_POLICY_AGENT),
    )
    .unwrap();

    assert!(changed);
    assert!(patched.contains(
        r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -Xmx8192m -novid -debug""#
    ));

    let (patched_again, changed_again) = patch_steam_launch_options_content_with_memory(
        &patched,
        Some(16384),
        &[],
        Some(ZOMBIE_BUDDY_POLICY_AGENT),
    )
    .unwrap();
    assert!(changed_again);
    assert!(patched_again
        .contains(r#""-agentlib:zbNative=policy=allow-all -Xmx16384m -novid -debug""#));
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
        .contains("-agentlib:zbNative=policy=allow-all"));
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
fn adopts_disk_state_as_baseline_without_touching_mod_info() {
    let temp = tempfile::tempdir().unwrap();
    let mods_root = temp.path().join("mods");
    let mod_root = mods_root.join("Example");
    fs::create_dir_all(&mod_root).unwrap();
    // 磁盘上是禁用态（只有 mod.info.disabled），记录里却写着启用。
    fs::write(
        mod_root.join("mod.info.disabled"),
        "name=Example Mod\nid=Example\njavaJarFile=Example.jar\njavaPkgName=example.patch\n",
    )
    .unwrap();
    fs::write(mod_root.join("Example.jar"), b"jar").unwrap();
    let mut state = JarModStateFile {
        format_version: 1,
        mods: vec![JarModStateEntry {
            mod_dir: mod_root.to_string_lossy().into_owned(),
            mod_id: "Example".to_string(),
            workshop_id: None,
            package_name: "example.patch".to_string(),
            disabled: false,
        }],
    };

    let changed = adopt_jar_mod_state_from_roots(std::slice::from_ref(&mods_root), &mut state);

    assert_eq!(changed, 1);
    assert!(state.mods[0].disabled);
    // 以磁盘为基准：文件保持原样，不能被改回启用。
    assert!(!mod_root.join("mod.info").exists());
    assert!(mod_root.join("mod.info.disabled").exists());
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

/// LaunchOptions 值里带转义引号（Steam 里写过带引号的路径参数）时，
/// 按值重写不能把原文尾巴拼回闭引号后面 —— 那会让整个 localconfig.vdf 从这行起解析错乱。
#[test]
fn replaces_vdf_value_respecting_escaped_quotes() {
    let line = r#"					"LaunchOptions" "\"C:\my dir\a.jar\" -novid""#;
    let patched =
        replace_vdf_value(line, "-agentlib:zbNative=policy=allow-all -- -novid").unwrap();
    assert_eq!(
        patched,
        r#"					"LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""#
    );

    // 无转义的普通值行为不变。
    let plain = "\t\t\"LaunchOptions\" \"-novid\"";
    let patched = replace_vdf_value(plain, "-Xmx4096m").unwrap();
    assert_eq!(patched, "\t\t\"LaunchOptions\" \"-Xmx4096m\"");
    // 行尾的其它键值对不能被吃掉（old.txt 那种多键行）。
    let multi = "\"LaunchOptions\" \"-novid\" \"SomeKey\" \"1\"";
    let patched = replace_vdf_value(multi, "-debug").unwrap();
    assert_eq!(patched, "\"LaunchOptions\" \"-debug\" \"SomeKey\" \"1\"");
}

/// Steam 写出的 localconfig.vdf 存在 "apps"（全小写）拼写：
/// 首次插入 108600 条目时必须同样能找到节点，否则报"找不到 Apps 节点"。
#[test]
fn matches_app_and_launch_options_keys_case_insensitively() {
    assert!(quoted_key("\t\t\"apps\"", "Apps"));
    assert!(quoted_key("\t\t\"Apps\"", "apps"));
    assert!(quoted_key("\t\t\t\"LaunchOptions\" \"-novid\"", "launchoptions"));
    assert!(!quoted_key("\t\t\"app\"", "Apps"));
}

/// 方案 = 「不使用加载器」时，Steam 启动项里的加载器 agent 必须被摘掉，
/// 而不是每次改内存上限/启动时又被写回去。
#[test]
fn none_scheme_strips_loader_agent_from_steam_launch_options() {
    let content = r#""UserLocalConfigStore"
{
"apps"
{
    "108600"
    {
        "LaunchOptions" "-agentlib:zbNative=policy=allow-all -Xmx8192m -- -novid"
    }
}
}"#;

    let (patched, changed) =
        patch_steam_launch_options_content_with_memory(&content, Some(4096), &[], None).unwrap();

    assert!(changed);
    // agent 被摘掉、内存照写、其余参数保留；apps（小写）节点也能被识别。
    assert!(!patched.contains("zbNative"));
    assert!(patched.contains(r#""LaunchOptions" "-Xmx4096m -novid""#));
}

/// 旧版本把含转义引号的 LaunchOptions 行写坏（闭引号后残留原文尾巴）时，
/// 新版在下次写入时整行重建，残留垃圾随之清除——用户无需手动去 Steam 清理。
#[test]
fn repairs_corrupted_launch_options_line_on_rewrite() {
    // 旧 bug 的产物形状：正确值闭引号之后紧跟原值的残余（转义引号被截断处开始）。
    let corrupted = concat!(
        r#""UserLocalConfigStore""#,
        "\n{\n\"apps\"\n{\n    \"108600\"\n    {\n",
        r#"        "LaunchOptions" "-agentlib:zbNative=policy=allow-all -- -novid""C:\my dir\a.jar\" -novid""#,
        "\n    }\n}\n}\n",
    );

    let (patched, changed) =
        patch_steam_launch_options_content(&corrupted).unwrap();

    assert!(changed);
    assert!(patched.contains(
        r#""LaunchOptions" "-agentlib:zbNative=policy=allow-all -novid""#
    ));
    assert!(!patched.contains("--"));
    assert!(!patched.contains("my dir"));
}

// ---------------------------------------------------------------------------
// 光影包配置解析与启动覆写（shaderpack_config.rs，思路文档 §7 测试清单）
// ---------------------------------------------------------------------------

#[test]
fn classifies_viewpoint_live_keys_per_document() {
    assert_eq!(
        classify_key("minecraft.minecraft-BSL_v8_4.SHADOW"),
        LiveKeyClass::McParam {
            pack_id: "minecraft-BSL_v8_4".to_string(),
            param: "SHADOW".to_string()
        }
    );
    // packId 可能包含点 / 下划线 / 连字符，必须按最后一个点拆（思路文档 §1）。
    assert_eq!(
        classify_key("minecraft.minecraft-Bliss_v2_1_2__Chocapic13_Shaders_edit_.TAA"),
        LiveKeyClass::McParam {
            pack_id: "minecraft-Bliss_v2_1_2__Chocapic13_Shaders_edit_".to_string(),
            param: "TAA".to_string()
        }
    );
    // minecraftProfile. 不是 minecraft. 的子情况，要先判断。
    assert_eq!(
        classify_key("minecraftProfile.minecraft-BSL_v8_4"),
        LiveKeyClass::McProfile {
            pack_id: "minecraft-BSL_v8_4".to_string()
        }
    );
    assert_eq!(classify_key("camera.motion"), LiveKeyClass::Viewpoint);
    assert_eq!(classify_key("graphics.pack"), LiveKeyClass::Viewpoint);
    assert_eq!(classify_key("pack.default.SHADOW"), LiveKeyClass::PackParam);
    assert_eq!(classify_key("minecraft.about"), LiveKeyClass::McUnknown);
}

#[test]
fn properties_document_preserves_comments_order_and_newlines() {
    let content = "# header\r\n\r\ncamera.motion=High\r\nminecraft.minecraft-BSL_v8_4.SHADOW=true\r\n! disabled\r\nminecraft.minecraft-BSL_v8_4.TAA=false\r\n";
    let mut doc = PropertiesDocument::parse(content);
    assert_eq!(
        doc.get("minecraft.minecraft-BSL_v8_4.SHADOW").as_deref(),
        Some("true")
    );
    assert!(doc.set("minecraft.minecraft-BSL_v8_4.SHADOW", "false").unwrap());

    let rendered = doc.render();
    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines[0], "# header");
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "camera.motion=High");
    assert_eq!(lines[3], "minecraft.minecraft-BSL_v8_4.SHADOW=false");
    assert_eq!(lines[4], "! disabled");
    assert_eq!(lines[5], "minecraft.minecraft-BSL_v8_4.TAA=false");
    // 换行风格（CRLF）保留
    assert!(rendered.ends_with("\r\n"));

    // LF 风格同样保留
    let lf = PropertiesDocument::parse("a=1\nb=2\n");
    assert_eq!(lf.render(), "a=1\nb=2\n");
    // 无尾换行也保留
    assert_eq!(PropertiesDocument::parse("a=1").render(), "a=1");
    // BOM 去掉（写回无 BOM，思路文档 §4.3）
    assert_eq!(PropertiesDocument::parse("\u{feff}a=1\n").render(), "a=1\n");
}

#[test]
fn properties_document_inserts_new_keys_by_section() {
    let mut doc = PropertiesDocument::parse(
        "camera.motion=High\nminecraft.minecraft-AAA.OPTION_ONE=1\nminecraft.minecraft-BBB.OTHER=2\nminecraftProfile.minecraft-BBB=HIGH\n",
    );
    doc.set("minecraft.minecraft-AAA.OPTION_TWO", "x").unwrap();
    doc.set("minecraft.minecraft-CCC.NEW", "y").unwrap();
    doc.set("minecraftProfile.minecraft-AAA", "LOW").unwrap();

    let rendered = doc.render();
    let lines: Vec<&str> = rendered.lines().collect();
    // 新参数插到同 packId 最后一行之后
    assert_eq!(lines[2], "minecraft.minecraft-AAA.OPTION_TWO=x");
    // 新 pack 插到最后一个 minecraft. 行之后
    assert_eq!(lines[4], "minecraft.minecraft-CCC.NEW=y");
    // profile 键插到最后一个 minecraftProfile 行之后
    assert_eq!(lines[6], "minecraftProfile.minecraft-AAA=LOW");
}

#[test]
fn properties_document_handles_duplicate_keys() {
    let mut doc = PropertiesDocument::parse("k=a\n# keep\nk=b\n");
    // 重复键逻辑值取最后一次出现
    assert_eq!(doc.get("k").as_deref(), Some("b"));
    // 覆写改最后一行
    doc.set("k", "c").unwrap();
    assert_eq!(doc.render(), "k=a\n# keep\nk=c\n");
    // remove 删所有重复行，注释保留
    assert_eq!(doc.remove("k"), 2);
    assert_eq!(doc.render(), "# keep\n");
}

#[test]
fn normalizes_override_values() {
    assert_eq!(normalize_property_value("TRUE").unwrap(), "true");
    assert_eq!(normalize_property_value("False").unwrap(), "false");
    // 数字 / 枚举保留原字符串（0.60 不变 0.6，大小写敏感）
    assert_eq!(normalize_property_value("0.60").unwrap(), "0.60");
    assert_eq!(normalize_property_value("High").unwrap(), "High");
    assert!(normalize_property_value("a\nb").is_err());

    // 前导空格转义 + 反斜杠双写后能原样读回
    let mut doc = PropertiesDocument::parse("");
    doc.set("k", " spaced").unwrap();
    assert_eq!(doc.get("k").as_deref(), Some(" spaced"));
    doc.set("p", "C:\\path").unwrap();
    assert_eq!(doc.get("p").as_deref(), Some("C:\\path"));
    // 值中的空格与逗号原样保留
    doc.set("m", "Nearest, mipmapped").unwrap();
    assert_eq!(doc.get("m").as_deref(), Some("Nearest, mipmapped"));
    // 空值
    doc.set("e", "").unwrap();
    assert_eq!(doc.get("e").as_deref(), Some(""));
}

#[test]
fn derives_pack_ids_from_file_names() {
    assert_eq!(candidate_pack_id("BSL v8.4.zip"), "minecraft-BSL_v8_4");
    assert_eq!(candidate_pack_id("BSL_v8_4.zip"), "minecraft-BSL_v8_4");
    assert_eq!(
        candidate_pack_id("Bliss v2.1.2 (Chocapic13 Shaders edit).zip"),
        "minecraft-Bliss_v2_1_2__Chocapic13_Shaders_edit_"
    );
}

#[test]
fn resolve_pack_id_prefers_observed_keys() {
    let doc = PropertiesDocument::parse("minecraft.minecraft-BSL_v8_4.SHADOW=true\n");
    assert_eq!(
        resolve_pack_id("BSL v8.4.zip", &doc),
        ("minecraft-BSL_v8_4".to_string(), true)
    );
    let plain = PropertiesDocument::parse("camera.motion=High\n");
    assert_eq!(
        resolve_pack_id("BSL v8.4.zip", &plain),
        ("minecraft-BSL_v8_4".to_string(), false)
    );
    assert_eq!(
        observed_minecraft_pack_ids(&doc),
        vec!["minecraft-BSL_v8_4".to_string()]
    );
}

#[test]
fn parses_shaders_properties_options_profiles_and_sliders() {
    let meta = parse_shaders_properties(
        "# BSL sample\nSHADOWS=true\nshadowMapResolution=2048.0\nsliders=shadowMapResolution SUN_ANGLE\nprofile.LOW=SHADOWS=false !TAA shadowMapResolution=1024.0\nprofile.HIGH=SHADOWS=true TAA shadowMapResolution=4096.0\nscreen=<empty> SHADOWS\nscreen.SHADOW=SHADOWS shadowMapResolution\nprogram.composite.enabled=true\ntexture.noise=texture/noise.png\n",
    );
    let names: Vec<&str> = meta.options.iter().map(|option| option.name.as_str()).collect();
    assert_eq!(names, vec!["SHADOWS", "shadowMapResolution", "TAA"]);

    let shadows = meta.options.iter().find(|o| o.name == "SHADOWS").unwrap();
    assert_eq!(shadows.default_value.as_deref(), Some("true"));
    assert_eq!(shadows.values, vec!["true", "false"]);
    assert!(!shadows.slider);

    let smr = meta
        .options
        .iter()
        .find(|o| o.name == "shadowMapResolution")
        .unwrap();
    assert!(smr.slider);
    assert_eq!(smr.values, vec!["2048.0", "1024.0", "4096.0"]);

    // 只出现在预设里的参数也收进来；`opt` → true，`!opt` → false
    let taa = meta.options.iter().find(|o| o.name == "TAA").unwrap();
    assert_eq!(taa.default_value, None);
    // LOW 的 !TAA 先出现，所以 false 在前
    assert_eq!(taa.values, vec!["false", "true"]);

    let high = meta.profiles.iter().find(|p| p.name == "HIGH").unwrap();
    let taa_token = high.tokens.iter().find(|t| t.option == "TAA").unwrap();
    assert_eq!(taa_token.value, "true");
    let low = meta.profiles.iter().find(|p| p.name == "LOW").unwrap();
    let taa_low = low.tokens.iter().find(|t| t.option == "TAA").unwrap();
    assert_eq!(taa_low.value, "false");

    // 引擎命名空间与 screen 不当作参数
    assert!(!names.contains(&"program.composite.enabled"));
    assert!(!names.iter().any(|name| name.starts_with("screen")));
    assert!(!names.contains(&"texture.noise"));
}

#[test]
fn folds_java_properties_continuations_for_screens() {
    // BSL 主页的真实形态：分类按钮全在 `\` 续行上。
    // Iris 用 OrderBackedProperties（java.util.Properties 子类）读 shaders.properties，
    // 续行是 Java load 语义：尾随奇数个反斜杠折叠下一行（剥前导空白）。
    let meta = parse_shaders_properties(concat!(
        "SHADOWS=true\n",
        "screen=<empty> <profile> SHADOWS \\\n",
        "  [LIGHTING] [MATERIAL]\n",
        "screen.LIGHTING=SHADOWS\n",
        "screen.MATERIAL=SHADOWS\n",
        "sliders=SHADOWS\n",
    ));
    let main = &meta.screens[0];
    let kinds: Vec<&str> = main.tokens.iter().map(|token| token.kind.as_str()).collect();
    assert_eq!(kinds, vec!["empty", "profile", "option", "screen", "screen"]);
    assert!(meta.screens.iter().any(|screen| screen.name == "LIGHTING"));
    assert!(meta.screens.iter().any(|screen| screen.name == "MATERIAL"));
}

#[test]
fn parses_java_properties_escapes_and_whitespace_separators() {
    // java.util.Properties.load 语义：key 也能被空白 / `:` 终止；完整反转义
    let doc = PropertiesDocument::parse("key1 value1\nkey2\\:name=v2\nkey3=C:\\\\path\n");
    assert_eq!(doc.get("key1").as_deref(), Some("value1"));
    assert_eq!(doc.get("key2:name").as_deref(), Some("v2"));
    assert_eq!(doc.get("key3").as_deref(), Some("C:\\path"));
    // \uXXXX 反转义
    let unicode = PropertiesDocument::parse("k=\\u9634\\u5f71\n");
    assert_eq!(unicode.get("k").as_deref(), Some("阴影"));
}

#[test]
fn unknown_screen_tokens_are_skipped_like_iris() {
    // Iris `OptionMenuContainer` 对每个 token 单独 try/catch：引用未检测到的参数
    // 只跳过该元素并记日志，菜单照常构建（photon 就带 3 个这种幻影 token）。
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"screen=REAL NOT_DEFINED_ANYWHERE\n").unwrap();
    archive
        .start_file("shaders/test.fsh", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"#define REAL // on\n").unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    assert_eq!(meta.screens.len(), 1);
    assert_eq!(
        meta.screens[0].tokens,
        vec![super::ShaderScreenToken {
            kind: "option".to_string(),
            name: Some("REAL".to_string())
        }]
    );
}

#[test]
fn profile_token_without_profiles_is_skipped_like_iris() {
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    // `<profile>` 但包里没有任何 profile.* 定义 → 该元素被跳过，其余照常
    archive.write_all(b"screen=<profile> REAL\n").unwrap();
    archive
        .start_file("shaders/test.fsh", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"#define REAL // on\n").unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    assert_eq!(meta.screens.len(), 1);
    assert_eq!(
        meta.screens[0].tokens,
        vec![super::ShaderScreenToken {
            kind: "option".to_string(),
            name: Some("REAL".to_string())
        }]
    );
}

#[test]
fn all_unknown_tokens_still_yield_a_forced_flat_screen() {
    // 强制分页兜底：screen= 全引用不存在的参数 → 过滤后所有页都空 →
    // 必须合成"全部参数"单页，绝不允许落到空状态。
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"screen=GHOST_ONE GHOST_TWO
").unwrap();
    archive
        .start_file("shaders/test.fsh", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"#define SSR_ENABLED // on
").unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    assert_eq!(meta.options.len(), 1);
    assert_eq!(meta.screens.len(), 1);
    assert_eq!(
        meta.screens[0].tokens,
        vec![super::ShaderScreenToken {
            kind: "option".to_string(),
            name: Some("SSR_ENABLED".to_string())
        }]
    );
}

#[test]
fn source_only_packs_chunk_into_button_pages_when_overflowing() {
    // 强制分页形态：没有自带布局且参数超过一页容量时，主页变成"第 N 页"按钮页
    //（与截图中 Bliss / photon 的分类按钮页同款），点进去才是该页的参数网格。
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/test.fsh", SimpleFileOptions::default())
        .unwrap();
    let mut defines = String::new();
    for index in 0..13 {
        defines.push_str(&format!("#define OPTION_{index} // on
"));
    }
    archive.write_all(defines.as_bytes()).unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    assert_eq!(meta.options.len(), 13);
    assert_eq!(meta.screens.len(), 3);
    // 主页：两个"第 N 页"按钮
    assert_eq!(
        meta.screens[0].tokens,
        vec![
            super::ShaderScreenToken { kind: "screen".to_string(), name: Some("auto_page_1".to_string()) },
            super::ShaderScreenToken { kind: "screen".to_string(), name: Some("auto_page_2".to_string()) },
        ]
    );
    assert_eq!(meta.screens[1].label.as_deref(), Some("第 1 页"));
    assert_eq!(meta.screens[1].tokens.len(), 10);
    assert_eq!(meta.screens[2].label.as_deref(), Some("第 2 页"));
    assert_eq!(meta.screens[2].tokens.len(), 3);
}

#[test]
fn source_only_packs_get_a_flat_screen() {
    // EclipseShaders 这类没有 shaders.properties 的纯源码包：
    // 选项能扫出来，布局合成一份"全部参数"单页，而不是空界面。
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/test.fsh", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"#define SSR_ENABLED // on\n").unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    assert_eq!(meta.options.len(), 1);
    assert_eq!(meta.screens.len(), 1);
    assert_eq!(
        meta.screens[0].tokens,
        vec![super::ShaderScreenToken {
            kind: "option".to_string(),
            name: Some("SSR_ENABLED".to_string())
        }]
    );
}

/// 本地真实 Steam 配置抽查（默认忽略，只读不写）：拿真实的 localconfig.vdf 走一遍
/// "补丁 → 状态判定" 回环，确认修复后安装闭环成立、用户自设参数不丢。
#[test]
#[ignore]
fn real_world_steam_config_round_trips_status() {
    let steam_root = std::path::Path::new("c:/program files (x86)/steam");
    let Some(userdata) = steam_root.join("userdata").read_dir().ok() else {
        return;
    };
    let mut checked = 0;
    for entry in userdata.flatten() {
        let config = entry.path().join("config").join("localconfig.vdf");
        let Ok(content) = fs::read_to_string(&config) else {
            continue;
        };
        let (patched, changed) = patch_steam_launch_options_content(&content).unwrap();
        // 安装后状态必须判定为已配置（agent 已写入生效的那条 LaunchOptions）
        assert!(
            steam_config_has_both(&patched),
            "{} 补丁后状态仍是缺少",
            config.display()
        );
        // 用户自设的参数（如 -screen-width）不能被清掉
        if content.contains("screen-width") {
            assert!(patched.contains("screen-width"), "{} 丢了用户的窗口参数", config.display());
        }
        let _ = changed;
        checked += 1;
    }
    assert!(checked > 0, "没有找到任何 localconfig.vdf");
}

/// 诊断指定真实包的解析结果（默认忽略）：选项数 / 分页数 / 各分页 token 数。
#[test]
#[ignore]
fn real_world_diagnose_two_packs() {
    for path in [
        r"C:\Users\SanyeSAMA\Zomboid\viewpoint-shaderpacks\EclipseShaders.zip",
        r"C:\Users\SanyeSAMA\Zomboid\viewpoint-shaderpacks\photon_v1.0a.zip",
    ] {
        let path = std::path::Path::new(path);
        if !path.is_file() {
            continue;
        }
        // properties 层独立诊断：screen= / screen.* 是否被解析出来
        if let Ok(Some(content)) = read_shaders_properties_from_pack(path) {
            let parsed = parse_shaders_properties(&content);
            println!(
                "  properties 层：选项 {} 个，分页 {} 个（{:?}）",
                parsed.options.len(),
                parsed.screens.len(),
                parsed.screens.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()
            );
        }
        // 预检：properties 层的 screen token 里哪些不在（合并后）选项集合里
        if let (Ok(Some(content)), Ok(meta)) = (
            read_shaders_properties_from_pack(path),
            read_pack_shader_meta(path),
        ) {
            let parsed = parse_shaders_properties(&content);
            let known: std::collections::HashSet<&str> =
                meta.options.iter().map(|option| option.name.as_str()).collect();
            let mut unresolved: Vec<String> = Vec::new();
            for screen in &parsed.screens {
                for token in &screen.tokens {
                    if token.kind == "option" {
                        if let Some(name) = &token.name {
                            if !known.contains(name.as_str())
                                && !unresolved.iter().any(|probe| probe == name)
                            {
                                unresolved.push(name.clone());
                            }
                        }
                    }
                }
            }
            if !unresolved.is_empty() {
                println!("  校验不过的 token：{unresolved:?}");
            }
        }
        match read_pack_shader_meta(path) {
            Ok(meta) => {
                println!(
                    "{}: 选项 {} 个，分页 {} 个（{:?}）",
                    path.file_name().unwrap().to_string_lossy(),
                    meta.options.len(),
                    meta.screens.len(),
                    meta.screens
                        .iter()
                        .map(|screen| screen.name.as_str())
                        .collect::<Vec<_>>()
                );
                // 找出 screen 里引用了但选项集合里没有的 token（触发 Iris 回退的原因）
                let known: std::collections::HashSet<&str> =
                    meta.options.iter().map(|option| option.name.as_str()).collect();
                let mut unresolved: Vec<&str> = Vec::new();
                for screen in &meta.screens {
                    for token in &screen.tokens {
                        if token.kind == "option" {
                            if let Some(name) = &token.name {
                                if !known.contains(name.as_str())
                                    && !unresolved.contains(&name.as_str())
                                {
                                    unresolved.push(name.as_str());
                                }
                            }
                        }
                    }
                }
                if !unresolved.is_empty() {
                    println!("  screen 引用但未检测到的参数：{unresolved:?}");
                }
            }
            Err(error) => println!("{}: 解析失败：{error}", path.file_name().unwrap().to_string_lossy()),
        }
    }
}

/// 本地真实包语言覆盖率抽查（默认忽略）：每个包统计参数总数 / 有显示名 / 有说明的数量，
/// 用来定位"标签没应用"是解析问题还是包本身没翻译。
#[test]
#[ignore]
fn real_world_shaderpacks_report_lang_coverage() {
    let dir = std::path::Path::new("H:\\.minecraft\\versions\\1.20.1-Fabric机械美化\\shaderpacks");
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("zip") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(meta) = read_pack_shader_meta(&path) else {
            continue;
        };
        // 与命令同一套语言选择：默认中文 → en_US 键级回退
        let pack_lang = read_pack_lang(&path, &["zh_CN".to_string(), "zh_TW".to_string()]);
        let Ok(Some(pack_lang)) = pack_lang else {
            println!("{name}: (没有 lang 文件)");
            continue;
        };
        let mut labels = pack_lang.labels.clone();
        if !pack_lang.chosen.eq_ignore_ascii_case("en_US") {
            if let Ok(Some(base)) = read_pack_lang(&path, &["en_US".to_string()]) {
                labels = super::ShaderLangLabels::merged_lang_fallback(&base.labels, labels);
            }
        }
        let total = meta.options.len();
        let labelled = meta
            .options
            .iter()
            .filter(|option| labels.option_label(&option.name).is_some())
            .count();
        let commented = meta
            .options
            .iter()
            .filter(|option| labels.option_comment(&option.name).is_some())
            .count();
        let without_label: Vec<&String> = meta
            .options
            .iter()
            .filter(|option| labels.option_label(&option.name).is_none())
            .map(|option| &option.name)
            .collect();
        println!(
            "{name}: chosen={} 参数 {total}，有显示名 {labelled}，有说明 {commented}；无显示名前 15 个：{:?}",
            pack_lang.chosen,
            &without_label[..without_label.len().min(15)]
        );
        // 用户可见层：screen 布局实际引用的参数里，还有多少没拿到语言文本
        let on_screen: Vec<String> = meta
            .screens
            .iter()
            .flat_map(|screen| &screen.tokens)
            .filter_map(|token| token.name.clone())
            .collect();
        let visible: Vec<String> = on_screen
            .into_iter()
            .filter(|name| meta.options.iter().any(|option| &option.name == name))
            .collect();
        let visible_total = visible.len();
        let visible_unlabelled: Vec<&String> = visible
            .iter()
            .filter(|name| labels.option_label(name).is_none())
            .collect();
        println!(
            "{name}: 界面可见参数 {visible_total}，其中无显示名 {} 个 {:?}",
            visible_unlabelled.len(),
            &visible_unlabelled[..visible_unlabelled.len().min(15)]
        );
    }
}

/// 本地真实包抽查（依赖个人机器上的目录，默认忽略）：
/// `cargo test --lib -- --ignored real_world`
#[test]
#[ignore]
fn real_world_shaderpacks_keep_their_screen_layouts() {
    let dir = std::path::Path::new("H:\\.minecraft\\versions\\1.20.1-Fabric机械美化\\shaderpacks");
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("zip") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Ok(meta) = read_pack_shader_meta(&path) else {
            // 没有 shaders.properties 的包（Bliss-Shader-Unstable / photon-voxy-support）本来就会失败
            continue;
        };
        // 有 screen 布局的包（BSL / Bliss / Alpha Piscium / Complementary）：解析后必须保留子屏分页
        if name.starts_with("BSL") || name.starts_with("Bliss") || name.starts_with("Alpha Piscium") || name.starts_with("Complementary") {
            assert!(
                meta.screens.len() > 1,
                "{name} 的子屏丢了：只剩 {} 个 screen",
                meta.screens.len()
            );
        }
    }
}

#[test]
fn parses_screens_like_optifine_layout() {    let meta = parse_shaders_properties(concat!(
        "SHADOWS=true\n",
        "shadowMapResolution=2048\n",
        "TAA=true\n",
        "screen.columns=4\n",
        "screen=<empty> <profile> SHADOWS [DETAIL]\n",
        "screen.DETAIL=shadowMapResolution TAA\n",
        "screen.DETAIL.columns=3\n",
    ));

    // 主页 + 子屏（按出现顺序）
    assert_eq!(meta.screens.len(), 2);
    let main = &meta.screens[0];
    assert_eq!(main.name, "screen");
    // screen.columns 设主页列数
    assert_eq!(main.columns, 4);
    // <empty> / <profile> / 参数 / [子屏] 四种 token；shadowMapResolution 与 TAA 已被
    // screen.DETAIL 引用，不重复追加主页
    assert_eq!(
        main.tokens,
        vec![
            super::ShaderScreenToken { kind: "empty".to_string(), name: None },
            super::ShaderScreenToken { kind: "profile".to_string(), name: None },
            super::ShaderScreenToken { kind: "option".to_string(), name: Some("SHADOWS".to_string()) },
            super::ShaderScreenToken { kind: "screen".to_string(), name: Some("DETAIL".to_string()) },
        ]
    );
    let detail = &meta.screens[1];
    assert_eq!(detail.name, "DETAIL");
    // screen.<name>.columns 设子屏列数
    assert_eq!(detail.columns, 3);
    assert_eq!(detail.tokens.len(), 2);

    // 没有 screen= → 合成一份"全部参数"的单页主页
    let flat = parse_shaders_properties("A=1\nB=2\n");
    assert_eq!(flat.screens.len(), 1);
    assert_eq!(flat.screens[0].tokens.len(), 2);
}

#[test]
fn reads_shaders_properties_from_zip() {
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/gbuffers.frag", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"void main() {}").unwrap();
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"SHADOWS=true\nsliders=SHADOWS\n").unwrap();
    archive.finish().unwrap();

    let content = read_shaders_properties_from_pack(&pack).unwrap().unwrap();
    let meta = parse_shaders_properties(&content);
    assert_eq!(meta.options.len(), 1);
    assert_eq!(meta.options[0].name, "SHADOWS");
    assert!(meta.options[0].slider);

    // 没有 shaders.properties 的包（如 Viewpoint 内置包）返回 None
    let plain = temp.path().join("plain.zip");
    let file = fs::File::create(&plain).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("readme.txt", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"nothing").unwrap();
    archive.finish().unwrap();
    assert!(read_shaders_properties_from_pack(&plain).unwrap().is_none());
}

#[test]
fn applies_shaderpack_overrides_only_to_minecraft_keys() {
    let temp = tempfile::tempdir().unwrap();
    let zomboid = temp.path();
    fs::create_dir_all(zomboid.join("viewpoint-shaderpacks")).unwrap();
    let live = zomboid.join("viewpoint-live.properties");
    let original = concat!(
        "# Viewpoint live settings\r\n",
        "camera.motion=High\r\n",
        "keys.cameraTrace=\r\n",
        "models.filter=Nearest, mipmapped\r\n",
        "graphics.pack=vivid\r\n",
        "minecraft.minecraft-BSL_v8_4.SHADOW=true\r\n",
        "minecraftProfile.minecraft-BSL_v8_4=LOW\r\n",
        "minecraft.minecraft-Other_pack.OLD=yes\r\n",
    );
    fs::write(&live, original).unwrap();

    // 包里带 shaders.properties，用于参数白名单校验
    let pack = zomboid.join("viewpoint-shaderpacks").join("BSL v8.4.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(b"SHADOW=true\nSHADOW_DISTANCE=256.0\nTAA=true\n")
        .unwrap();
    archive.finish().unwrap();

    let overrides = vec![ShaderpackOverrideEntry {
        pack_name: "BSL v8.4.zip".to_string(),
        profile: Some("HIGH".to_string()),
        values: BTreeMap::from([
            ("SHADOW".to_string(), "false".to_string()),
            ("SHADOW_DISTANCE".to_string(), "512.0".to_string()),
            ("NOT_IN_PACK".to_string(), "1".to_string()),
        ]),
    }];
    let outcome = apply_shaderpack_overrides_at_dir(zomboid, &overrides, false).unwrap();
    assert!(outcome.applied);
    assert_eq!(outcome.packs, 1);
    // profile + SHADOW + SHADOW_DISTANCE；白名单外的 NOT_IN_PACK 被跳过
    assert_eq!(outcome.keys, 3);

    let written = fs::read_to_string(&live).unwrap();
    // Viewpoint 自身键一字未动（思路文档 §1）
    assert!(written.contains("camera.motion=High\r\n"));
    assert!(written.contains("keys.cameraTrace=\r\n"));
    assert!(written.contains("models.filter=Nearest, mipmapped\r\n"));
    assert!(written.contains("graphics.pack=vivid\r\n"));
    assert!(written.contains("minecraft.minecraft-Other_pack.OLD=yes\r\n"));
    // 目标键：既有键原位改，profile 与新参数按区域插入
    assert!(written.contains("minecraft.minecraft-BSL_v8_4.SHADOW=false\r\n"));
    assert!(written.contains("minecraftProfile.minecraft-BSL_v8_4=HIGH\r\n"));
    assert!(written.contains("minecraft.minecraft-BSL_v8_4.SHADOW_DISTANCE=512.0\r\n"));
    assert!(!written.contains("NOT_IN_PACK"));
    // 注释与换行风格保留
    assert!(written.starts_with("# Viewpoint live settings\r\n"));

    // 备份与清单已生成
    let backup_dir = zomboid.join(".pzmanager").join("viewpoint-live-backups");
    assert!(backup_dir.join("manifest.json").is_file());
    assert_eq!(
        fs::read_dir(&backup_dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("viewpoint-live.properties.bak."))
            .count(),
        1
    );

    // 回读校验通过：文档 get 取到新值
    let doc = PropertiesDocument::parse(&written);
    assert_eq!(
        doc.get("minecraft.minecraft-BSL_v8_4.SHADOW_DISTANCE").as_deref(),
        Some("512.0")
    );
}

#[test]
fn applies_overrides_creates_and_restores_missing_live_file() {
    let temp = tempfile::tempdir().unwrap();
    let zomboid = temp.path();
    let overrides = vec![ShaderpackOverrideEntry {
        pack_name: "BSL v8.4.zip".to_string(),
        profile: None,
        values: BTreeMap::from([("SHADOW".to_string(), "true".to_string())]),
    }];
    let outcome = apply_shaderpack_overrides_at_dir(zomboid, &overrides, false).unwrap();
    assert!(outcome.applied);

    let live = zomboid.join("viewpoint-live.properties");
    assert!(live.is_file());
    assert!(fs::read_to_string(&live)
        .unwrap()
        .contains("minecraft.minecraft-BSL_v8_4.SHADOW=true"));

    // 整文件还原：原本不存在 → 删除覆写生成的文件
    let message = restore_viewpoint_live_backup_at(zomboid).unwrap().unwrap();
    assert!(message.contains("移除"));
    assert!(!live.exists());
    // 没有待还原的备份 → None（还原是空操作）
    assert!(restore_viewpoint_live_backup_at(zomboid).unwrap().is_none());
}

#[test]
fn restores_whole_file_after_game_rewrites_it() {
    let temp = tempfile::tempdir().unwrap();
    let zomboid = temp.path();
    let live = zomboid.join("viewpoint-live.properties");
    fs::write(&live, "camera.motion=High\n").unwrap();
    write_viewpoint_live_backup_at(zomboid).unwrap();

    // 模拟游戏内改动（Viewpoint 在 Delete 菜单保存时重写整个文件）
    fs::write(
        &live,
        "camera.motion=Right\nminecraft.minecraft-BSL_v8_4.SHADOW=false\n",
    )
    .unwrap();

    restore_viewpoint_live_backup_at(zomboid).unwrap().unwrap();
    assert_eq!(fs::read_to_string(&live).unwrap(), "camera.motion=High\n");
    // 还原后清单清掉，再次还原是空操作
    assert!(restore_viewpoint_live_backup_at(zomboid).unwrap().is_none());
}

#[test]
fn skips_override_apply_while_game_running() {
    let temp = tempfile::tempdir().unwrap();
    let overrides = vec![ShaderpackOverrideEntry {
        pack_name: "a.zip".to_string(),
        profile: None,
        values: BTreeMap::from([("P".to_string(), "1".to_string())]),
    }];
    let outcome = apply_shaderpack_overrides_at_dir(temp.path(), &overrides, true).unwrap();
    assert!(!outcome.applied);
    assert!(outcome.reason.contains("运行"));
    assert!(!temp.path().join("viewpoint-live.properties").exists());
}

#[test]
fn apply_restores_leftover_backup_before_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let zomboid = temp.path();
    let live = zomboid.join("viewpoint-live.properties");
    fs::write(&live, "camera.motion=High\n").unwrap();
    write_viewpoint_live_backup_at(zomboid).unwrap();
    // 上次启动器中途退出的残留：覆写值还在、没来得及还原
    fs::write(
        &live,
        "camera.motion=High\nminecraft.minecraft-BSL_v8_4.SHADOW=true\n",
    )
    .unwrap();

    let overrides = vec![ShaderpackOverrideEntry {
        pack_name: "BSL v8.4.zip".to_string(),
        profile: None,
        values: BTreeMap::from([("SHADOW".to_string(), "false".to_string())]),
    }];
    let outcome = apply_shaderpack_overrides_at_dir(zomboid, &overrides, false).unwrap();
    assert!(outcome.applied);

    // 先恢复了遗留备份再覆写：camera 行不变，minecraft 键存在且是新值
    let doc = PropertiesDocument::parse(&fs::read_to_string(&live).unwrap());
    assert_eq!(doc.get("camera.motion").as_deref(), Some("High"));
    assert_eq!(
        doc.get("minecraft.minecraft-BSL_v8_4.SHADOW").as_deref(),
        Some("false")
    );
}

#[test]
fn keeps_only_recent_viewpoint_live_backups() {
    let temp = tempfile::tempdir().unwrap();
    let zomboid = temp.path();
    let live = zomboid.join("viewpoint-live.properties");
    fs::write(&live, "k=v\n").unwrap();
    for _ in 0..7 {
        write_viewpoint_live_backup_at(zomboid).unwrap();
        // 纳秒时间戳做文件名，稍等保证不重名
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let backup_dir = zomboid.join(".pzmanager").join("viewpoint-live-backups");
    let backups: Vec<_> = fs::read_dir(&backup_dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("viewpoint-live.properties.bak.")
        })
        .collect();
    assert_eq!(backups.len(), 5);
    assert!(backup_dir.join("manifest.json").is_file());
}

#[test]
fn shaderpack_overrides_round_trip() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shaderpack-overrides.json");
    // 缺失 = 默认空配置
    let mut state = read_shaderpack_overrides_at(&path).unwrap();
    assert!(state.packs.is_empty());
    assert_eq!(state.format_version, SHADERPACK_OVERRIDES_FORMAT);

    upsert_shaderpack_override(
        &mut state,
        "a.zip",
        Some(" HIGH ".to_string()),
        BTreeMap::from([
            ("P".to_string(), "1".to_string()),
            ("".to_string(), "x".to_string()),
        ]),
    );
    upsert_shaderpack_override(&mut state, "b.zip", None, BTreeMap::new());
    write_shaderpack_overrides_at(&path, &state).unwrap();

    let loaded = read_shaderpack_overrides_at(&path).unwrap();
    assert_eq!(loaded.packs.len(), 2);
    assert_eq!(loaded.packs[0].profile.as_deref(), Some("HIGH"));
    assert!(!loaded.packs[0].values.contains_key(""));
    assert_eq!(loaded.packs[1].pack_name, "b.zip");

    let mut trimmed = loaded;
    assert!(remove_shaderpack_override(&mut trimmed, "a.zip"));
    assert!(!remove_shaderpack_override(&mut trimmed, "missing.zip"));
    assert_eq!(trimmed.packs.len(), 1);
}

#[test]
fn default_lang_stems_prefer_chinese() {
    // 弹窗默认中文（简体 → 繁体），没有中文时由 read_pack_lang 回退 en_US / 第一个
    assert_eq!(
        default_lang_stems(),
        vec!["zh_CN".to_string(), "zh_TW".to_string()]
    );
}

#[test]
fn parses_lang_content_keys_and_values() {
    let labels = parse_lang_content(
        "# comment\noption.SHADOWS=Shadows\noption.SHADOWS.comment=Enable shadows.\nvalue.SHADOWS.true=ON\nvalue.true=On\n",
    );
    assert_eq!(labels.option_label("SHADOWS").as_deref(), Some("Shadows"));
    assert_eq!(
        labels.option_comment("SHADOWS").as_deref(),
        Some("Enable shadows.")
    );
    assert_eq!(labels.value_label("SHADOWS", "true", 0).as_deref(), Some("ON"));
    // 通用 value.<VALUE> 兜底（lang 里常配 value.true=On / value.false=Off）
    assert_eq!(labels.value_label("OTHER_OPT", "true", 0).as_deref(), Some("On"));
    assert_eq!(labels.value_label("OTHER_OPT", "false", 1), None);
    // 按序号的键形态（Bliss：value.RESOURCEPACK_SKY.0=不使用）
    let indexed = parse_lang_content("value.STYLE.0=Original\nvalue.STYLE.1=Classic\n");
    assert_eq!(indexed.value_label("STYLE", "0", 0).as_deref(), Some("Original"));
    assert_eq!(indexed.value_label("STYLE", "1", 1).as_deref(), Some("Classic"));
    assert_eq!(indexed.value_label("STYLE", "9", 1).as_deref(), Some("Classic"));
    // 键级语言回退：chosen 缺的键用 base 补，chosen 有的键覆盖 base
    let base = parse_lang_content("option.A=Base A\noption.B=Base B\n");
    let chosen = parse_lang_content("option.A=中文 A\n");
    let merged = super::ShaderLangLabels::merged_lang_fallback(&base, chosen);
    assert_eq!(merged.option_label("A").as_deref(), Some("中文 A"));
    assert_eq!(merged.option_label("B").as_deref(), Some("Base B"));
    // 值里的冒号不被当分隔符（第一个分隔符是 =）
    let colon = parse_lang_content("option.X=A: B\n");
    assert_eq!(colon.option_label("X").as_deref(), Some("A: B"));
}

#[test]
fn reads_lang_labels_preferring_user_language() {
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("BSL_v8.4.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(b"SHADOWS=true\nshadowMapResolution=2048.0\nsliders=shadowMapResolution\n")
        .unwrap();
    archive
        .start_file("shaders/lang/en_US.lang", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(
            concat!(
                "option.SHADOWS=Shadows\n",
                "option.SHADOWS.comment=Enable shadow casting.\n",
                "value.SHADOWS.true=ON\n",
                "option.shadowMapResolution=Shadow resolution\n",
                "value.shadowMapResolution.2048=2K\n",
            )
            .as_bytes(),
        )
        .unwrap();
    archive
        .start_file("shaders/lang/zh_CN.lang", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(
            concat!(
                "option.SHADOWS=阴影\n",
                "value.SHADOWS.true=开\n",
                "profile.HIGH=高画质\n",
                // lang 键大小写本应敏感，但有些包写得很随意：小写键兜底
                "option.shadows.comment=阴影开关说明\n",
            )
            .as_bytes(),
        )
        .unwrap();
    archive.finish().unwrap();

    // 用户语言 zh_CN 优先；小写键兜底也能命中
    let zh = read_pack_lang(&pack, &["zh_CN".to_string(), "en_US".to_string()])
        .unwrap()
        .unwrap();
    assert_eq!(zh.chosen, "zh_CN");
    // 包里可用语言按出现顺序去重
    assert_eq!(zh.available, vec!["en_US".to_string(), "zh_CN".to_string()]);
    assert_eq!(zh.labels.option_label("SHADOWS").as_deref(), Some("阴影"));
    assert_eq!(
        zh.labels.option_comment("SHADOWS").as_deref(),
        Some("阴影开关说明")
    );
    assert_eq!(zh.labels.value_label("SHADOWS", "true", 0).as_deref(), Some("开"));
    assert_eq!(zh.labels.profile_label("HIGH").as_deref(), Some("高画质"));
    // zh 没有这个键，也没有通用 value.false → None（前端回退原值）
    assert_eq!(zh.labels.value_label("SHADOWS", "false", 1), None);

    // 没配用户语言 → en_US 兜底；数值尾零变体（2048.0 → 2048）能命中 lang 里的 2048
    let en = read_pack_lang(&pack, &["ja_JP".to_string()])
        .unwrap()
        .unwrap();
    assert_eq!(en.chosen, "en_US");
    assert_eq!(en.labels.option_label("SHADOWS").as_deref(), Some("Shadows"));
    assert_eq!(en.labels.value_label("SHADOWS", "true", 0).as_deref(), Some("ON"));
    assert_eq!(
        en.labels.value_label("shadowMapResolution", "2048.0", 1).as_deref(),
        Some("2K")
    );
    assert_eq!(en.labels.value_label("shadowMapResolution", "4096", 3), None);

    // 没有任何 lang 文件的包 → None
    let plain = temp.path().join("plain.zip");
    let file = fs::File::create(&plain).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"SHADOWS=true\n").unwrap();
    archive.finish().unwrap();
    assert!(read_pack_lang(&plain, &["zh_CN".to_string()])
        .unwrap()
        .is_none());
}

#[test]
fn lang_labels_fall_back_to_first_available_language() {
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/lang/fr_FR.lang", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"option.SHADOWS=Ombres\n").unwrap();
    archive.finish().unwrap();

    // 候选语言都不存在（没有 en_US）→ 取目录里第一个 .lang
    let labels = read_pack_lang(&pack, &["zh_CN".to_string()])
        .unwrap()
        .unwrap();
    assert_eq!(labels.chosen, "fr_FR");
    assert_eq!(labels.labels.option_label("SHADOWS").as_deref(), Some("Ombres"));
}

#[test]
fn scans_source_options_like_viewpoint_iris_options() {
    // 照搬 Viewpoint `IrisOptions` 的三类声明（正则语义一致）
    let options = scan_source_options(concat!(
        "#define SHADOWS // 阴影开关\n",
        "//  中间夹一行普通注释\n",
        "//#define TAA // 默认关闭\n",
        "#  define SPACED // # 与 define 之间允许空白\n",
        "#define SHADOW_QUALITY 1.0 // 质量 [0.5 1.0 1.5]\n",
        "#define BLOOM true // [true false]\n",
        "#define PI 3.14\n",
        "#define NOT_OPTION 5 // 没有方括号\n",
        "//#define COMMENTED_VALUE 2 // [1 2]\n",
        "const int shadowMapResolution = 2048; // 分辨率 [1024 2048 4096]\n",
        "const float SUN_ANGLE = 30.0; //[0.0 30.0 60.0]\n",
        "const bool OLD_LIGHTING = true; // [true false]\n",
        "const vec3 NOT_OPTION2 = vec3(1.0);\n",
    ));

    let find = |name: &str| options.iter().find(|option| option.name == name).unwrap();

    // TOGGLE：默认开；注释掉的默认关；# 与 define 之间允许空白
    assert_eq!(find("SHADOWS").default_value, "true");
    assert_eq!(find("SHADOWS").comment.as_deref(), Some("阴影开关"));
    assert_eq!(find("TAA").default_value, "false");
    assert_eq!(find("SPACED").default_value, "true");
    // 普通宏 / 无方括号 / 注释掉的带值声明 / 非 const 类型 → 都不是选项
    assert!(!options.iter().any(|option| option.name == "PI"));
    assert!(!options.iter().any(|option| option.name == "NOT_OPTION"));
    assert!(!options.iter().any(|option| option.name == "COMMENTED_VALUE"));
    assert!(!options.iter().any(|option| option.name == "NOT_OPTION2"));

    // VALUE：默认值 + 方括号允许值
    let quality = find("SHADOW_QUALITY");
    assert_eq!(quality.default_value, "1.0");
    assert_eq!(quality.allowed, vec!["0.5", "1.0", "1.5"]);
    assert_eq!(quality.comment.as_deref(), Some("质量"));
    let bloom = find("BLOOM");
    assert_eq!(bloom.default_value, "true");
    assert_eq!(bloom.allowed, vec!["true", "false"]);

    // CONST：int / float / bool 三种类型
    let resolution = find("shadowMapResolution");
    assert_eq!(resolution.default_value, "2048");
    assert_eq!(resolution.allowed, vec!["1024", "2048", "4096"]);
    let angle = find("SUN_ANGLE");
    assert_eq!(angle.default_value, "30.0");
    assert_eq!(angle.allowed, vec!["0.0", "30.0", "60.0"]);
    assert_eq!(find("OLD_LIGHTING").default_value, "true");
}

#[test]
fn read_pack_shader_meta_merges_source_scan_and_properties() {
    let temp = tempfile::tempdir().unwrap();
    let pack = temp.path().join("pack.zip");
    let file = fs::File::create(&pack).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("shaders/shaders.properties", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(concat!(
            "profile.LOW=SHADOWS=false shadowMapResolution=1024\n",
            "profile.HIGH=SHADOWS=true shadowMapResolution=4096\n",
            "sliders=shadowMapResolution\n",
            "PROPERTY_ONLY_OPTION=3\n",
        ).as_bytes())
        .unwrap();
    // 子目录（shaders/lib）里的文件同样要扫到
    archive
        .start_file("shaders/lib/settings.glsl", SimpleFileOptions::default())
        .unwrap();
    archive
        .write_all(concat!(
            "#define SHADOWS // 阴影\n",
            "const int shadowMapResolution = 2048; // [1024 2048 4096]\n",
        ).as_bytes())
        .unwrap();
    archive.finish().unwrap();

    let meta = read_pack_shader_meta(&pack).unwrap();
    let names: Vec<&str> = meta.options.iter().map(|option| option.name.as_str()).collect();
    // 源码声明的在前，properties 独有的在后
    assert_eq!(names, vec!["SHADOWS", "shadowMapResolution", "PROPERTY_ONLY_OPTION"]);

    // sliders= 对源码扫出的选项同样生效；默认值与允许值来自源码
    let shadows = meta.options.iter().find(|o| o.name == "SHADOWS").unwrap();
    assert!(!shadows.slider);
    assert_eq!(shadows.default_value.as_deref(), Some("true"));
    assert_eq!(shadows.values, vec!["true", "false"]);
    assert_eq!(shadows.comment.as_deref(), Some("阴影"));
    let resolution = meta.options.iter().find(|o| o.name == "shadowMapResolution").unwrap();
    assert!(resolution.slider);
    assert_eq!(resolution.default_value.as_deref(), Some("2048"));
    // 默认值已在方括号允许值里 → 保持包声明的顺序
    assert_eq!(resolution.values, vec!["1024", "2048", "4096"]);
    // properties 独有的默认值声明照常收进来
    let property_only = meta.options.iter().find(|o| o.name == "PROPERTY_ONLY_OPTION").unwrap();
    assert_eq!(property_only.default_value.as_deref(), Some("3"));
    // 预设来自 shaders.properties，token 里出现过的取值并进选项
    assert_eq!(meta.profiles.len(), 2);
    assert_eq!(meta.profiles[0].name, "LOW");
    assert!(resolution.values.contains(&"4096".to_string()));

    // 既没有 shaders.properties 也没有源码声明的包 → 报错（内置包 / 非 MC 光影包）
    let plain = temp.path().join("plain.zip");
    let file = fs::File::create(&plain).unwrap();
    let mut archive = ZipWriter::new(file);
    archive
        .start_file("readme.txt", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"nothing").unwrap();
    archive.finish().unwrap();
    assert!(read_pack_shader_meta(&plain).is_err());
}



/// 本机体检（默认忽略）：启动路径上的检测必须都是"有界"的。
/// 2026-11-08 启动卡死的根因是 `project_zomboid_running()`（枚举窗口 + 同步取标题）
/// 被无响应窗口拖住 —— 它已从启动路径移除，这里只校验替代方案（进程快照）够快。
#[test]
#[ignore]
fn real_world_launch_guard_timings() {
    // 三处检测都必须是"有界"的：tasklist 在本机实测 60–110 秒，已全部换成进程快照。
    let quick = std::time::Instant::now();
    let running = super::game_running_quick();
    let quick_elapsed = quick.elapsed();
    println!("game_running_quick() = {running}, 耗时 {quick_elapsed:?}");

    let t1 = std::time::Instant::now();
    let steam = super::steam_process_running();
    let steam_elapsed = t1.elapsed();
    println!("steam_process_running() = {steam}, 耗时 {steam_elapsed:?}");

    let t2 = std::time::Instant::now();
    let slow = super::project_zomboid_running();
    let slow_elapsed = t2.elapsed();
    println!("project_zomboid_running() = {slow}, 耗时 {slow_elapsed:?}");

    for (label, elapsed) in [
        ("game_running_quick", quick_elapsed),
        ("steam_process_running", steam_elapsed),
        ("project_zomboid_running", slow_elapsed),
    ] {
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "{label} 必须先有界（实测 {elapsed:?}）：它会被启动/版本操作路径同步调用"
        );
    }
}
