// 改造计划 §4 自动化测试（可离线运行的部分）。
// 依赖真实屏幕/播放器的用例（UUID 屏幕映射、会话状态迁移、真机验收）不在单测范围。

#[cfg(test)]
mod tests {
    use crate::displays;
    use crate::media;
    use crate::settings::{self, AppSettings};
    use crate::thumbs;
    use crate::types::{
        DisplayInfo, DisplayWallpaperState, FitMode, MediaItem, MediaKind, Phase,
        WallpaperAssignment,
    };
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// settings 为进程级全局，触及其测试需串行执行
    fn serialize() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dotwallpaper-test-{tag}-{:?}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn media_extension_classification() {
        for ext in ["jpg", "JPG", "jpeg", "png", "bmp", "webp", "heic"] {
            assert_eq!(media::kind_of_ext(ext), Some(MediaKind::Image), "{ext}");
        }
        for ext in ["mp4", "MOV", "mov"] {
            assert_eq!(media::kind_of_ext(ext), Some(MediaKind::Video), "{ext}");
        }
        // 首版明确拒绝：GIF、网页、m4v 等
        for ext in ["gif", "html", "m4v", "txt", ""] {
            assert_eq!(media::kind_of_ext(ext), None, "{ext}");
        }
    }

    #[test]
    fn apply_validation_enforces_kind_and_boundary() {
        let _guard = serialize();
        let lib = scratch_dir("lib");
        let inside = lib.join("photo.JPG");
        std::fs::write(&inside, b"fake").unwrap();
        let video = lib.join("clip.mp4");
        std::fs::write(&video, b"fake").unwrap();
        let gif = lib.join("anim.gif");
        std::fs::write(&gif, b"fake").unwrap();

        settings::init_for_test(
            PathBuf::from("unused.json"),
            AppSettings {
                library_dir: lib.to_string_lossy().to_string(),
                ..Default::default()
            },
        );

        let ok = media::validate_for_apply(&inside.to_string_lossy(), MediaKind::Image);
        assert!(ok.is_ok(), "{ok:?}");
        // 声明类型与实际扩展名不符
        assert!(media::validate_for_apply(&inside.to_string_lossy(), MediaKind::Video).is_err());
        assert!(media::validate_for_apply(&video.to_string_lossy(), MediaKind::Image).is_err());
        // GIF 拒绝；不存在的路径拒绝；目录外文件（系统文件）拒绝
        assert!(media::validate_for_apply(&gif.to_string_lossy(), MediaKind::Image).is_err());
        assert!(media::validate_for_apply("/no/such.png", MediaKind::Image).is_err());
        assert!(media::validate_for_apply("/etc/hosts", MediaKind::Image).is_err());
    }

    #[test]
    fn delete_only_within_library_dir() {
        let _guard = serialize();
        let lib = scratch_dir("del");
        let inside = lib.join("a.png");
        std::fs::write(&inside, b"x").unwrap();
        settings::init_for_test(
            PathBuf::from("unused.json"),
            AppSettings {
                library_dir: lib.to_string_lossy().to_string(),
                ..Default::default()
            },
        );
        assert!(media::delete("/etc/hosts").is_err());
        assert!(media::delete(&format!("{}/../escape.png", lib.display())).is_err());
        assert!(media::delete(&inside.to_string_lossy()).is_ok());
        assert!(!inside.exists());
    }

    #[test]
    fn settings_roundtrip_corruption_and_version() {
        let _guard = serialize();
        let dir = scratch_dir("settings");
        let path = dir.join("settings.json");
        settings::init_for_test(
            path.clone(),
            AppSettings {
                library_dir: dir.to_string_lossy().to_string(),
                ..Default::default()
            },
        );
        let a = WallpaperAssignment {
            display_id: "builtin".into(),
            path: "/tmp/x.jpg".into(),
            kind: MediaKind::Image,
            fit_mode: FitMode::Fit,
            muted: false,
        };
        settings::record_assignment(&a);
        let reloaded = settings::load_from(&path);
        assert_eq!(reloaded.assignments.get("builtin"), Some(&a));
        // 保存是「同目录临时文件 + rename」：rename 成功意味着磁盘上任意时刻的 settings.json
        // 都是完整可读的。这里钉住临时文件名与「不残留」这条约定，改名会让清理逻辑静默失效。
        assert!(!path.with_extension("json.tmp").exists());
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"builtin\""));

        // 损坏配置：回退默认
        std::fs::write(&path, b"{ not json").unwrap();
        let fallback = settings::load_from(&path);
        assert_eq!(fallback, AppSettings::default());
        assert!(path.with_extension("json.bak").exists());

        // 未知版本：回退默认
        std::fs::write(
            &path,
            br#"{"version":999,"libraryDir":"/x","defaultFitMode":"fill","defaultMuted":true,"assignments":{},"pausedDisplays":[]}"#,
        )
        .unwrap();
        assert_eq!(settings::load_from(&path), AppSettings::default());
    }

    #[test]
    fn thumb_cache_key_follows_file_content() {
        let dir = scratch_dir("thumbkey");
        let src = dir.join("shot.png");
        std::fs::write(&src, b"v1").unwrap();
        let src = src.to_string_lossy().to_string();
        let k1 = thumbs::hashed_file_name(&src, "jpg");
        assert_eq!(
            k1,
            thumbs::hashed_file_name(&src, "jpg"),
            "同内容必须命中同一缓存"
        );

        std::fs::write(dir.join("shot.png"), b"v2-replaced").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("shot.png"), b"v3-replaced").unwrap();
        let k2 = thumbs::hashed_file_name(&src, "jpg");
        assert_ne!(k1, k2, "文件被替换后旧缓存必须失效");
        // 前 16 位是路径哈希：同一源文件的全部变体共享前缀
        assert_eq!(&k1[..16], &k2[..16]);
        // 源文件已消失时不得 panic（删除流程按前缀清理）
        assert!(thumbs::hashed_file_name("/no/such/file.png", "jpg").ends_with(".jpg"));
    }

    #[test]
    fn delete_thumb_removes_stale_variants_of_replaced_file() {
        let dir = scratch_dir("thumbdel");
        let cache = scratch_dir("thumbdel-cache");
        let src = dir.join("clip.mp4");
        std::fs::write(&src, b"a").unwrap();
        let n1 = thumbs::hashed_file_name(&src.to_string_lossy(), "jpg");
        std::fs::write(&src, b"bb").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(&src, b"ccc").unwrap();
        let n2 = thumbs::hashed_file_name(&src.to_string_lossy(), "jpg");
        let other = "ffffffffffffffff-0000000000000000.jpg";
        for n in [&n1, &n2, other] {
            std::fs::write(cache.join(n), b"x").unwrap();
        }
        thumbs::delete_thumb(&src.to_string_lossy(), &cache);
        assert!(!cache.join(&n1).exists(), "旧内容变体也应清掉");
        assert!(!cache.join(&n2).exists());
        assert!(cache.join(other).exists(), "不得误删其他文件的缓存");
    }

    #[test]
    fn image_size_reads_properties_and_thumbnail_round_trip() {
        // 尺寸现在是查属性字典的 "PixelWidth"/"PixelHeight"（换取不再整幅解码），键名或
        // CFNumber 类型判断写错都会**静默**返回 None，小图判断随之失真——用仓库自带的
        // 512×512 图标做正反向断言。
        let png = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../xcode/WallpaperEngineApp/Resources/Assets.xcassets/AppIcon.appiconset/icon_512x512.png");
        assert_eq!(
            crate::cfmedia::image_size(&png.to_string_lossy()),
            Some((512, 512)),
            "读到的应是真实像素尺寸"
        );
        let dir = scratch_dir("cfmedia");
        let jpg = dir.join("thumb.jpg");
        crate::cfmedia::make_thumbnail_jpeg(&png.to_string_lossy(), &jpg.to_string_lossy(), 64)
            .unwrap();
        assert_eq!(
            crate::cfmedia::image_size(&jpg.to_string_lossy()),
            Some((64, 64))
        );
        // 读不懂的文件：不 panic，返回 None（库里混着各种文件是常态）
        assert_eq!(
            crate::cfmedia::image_size(&dir.join("nope").to_string_lossy()),
            None
        );
    }

    #[test]
    fn legacy_display_ids_migrate_on_load() {
        let dir = scratch_dir("migrate");
        let path = dir.join("settings.json");
        // v1 的外接屏 ID 形如 disp-<vendor>-<model>-<serial>，model 段内嵌了易变的 CGDisplayID
        std::fs::write(
            &path,
            br#"{"version":1,"libraryDir":"/lib","defaultFitMode":"fill","defaultMuted":true,
"assignments":{"disp-4a8b-906d7-1010101":{"displayId":"disp-4a8b-906d7-1010101","path":"/lib/a.jpg","kind":"image","fitMode":"fill","muted":true}},
"pausedDisplays":["disp-4a8b-906d7-1010101","builtin"]}"#,
        )
        .unwrap();
        let s = settings::load_from(&path);
        assert_eq!(s.version, settings::SETTINGS_VERSION);
        let a = s
            .assignments
            .get("disp-4a8b-1010101")
            .expect("旧 ID 应迁移为 disp-<vendor>-<serial>");
        assert_eq!(a.display_id, "disp-4a8b-1010101");
        assert!(!s.assignments.contains_key("disp-4a8b-906d7-1010101"));
        // 非 disp 前缀（builtin / 临时 ID）原样保留
        assert!(s.paused_displays.contains(&"builtin".to_string()));
        assert!(s.paused_displays.contains(&"disp-4a8b-1010101".to_string()));
        // 迁移必须幂等：迁移后的 v2 文件再载入不应再被改写
        std::fs::write(&path, serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(settings::load_from(&path), s);
    }

    #[test]
    fn settings_update_reports_disk_failure_and_rolls_back() {
        let _guard = serialize();
        let dir = scratch_dir("settings-durable");
        let blocked_parent = dir.join("not-a-directory");
        std::fs::write(&blocked_parent, b"file").unwrap();
        let original = AppSettings::default();
        settings::init_for_test(blocked_parent.join("settings.json"), original.clone());
        let error = settings::update_checked(|s| s.onboarding_completed = true)
            .expect_err("cannot persist settings under a regular file");
        assert!(error.contains("配置目录"));
        assert_eq!(settings::get(), original);

        let valid_path = dir.join("settings.json");
        settings::init_for_test(valid_path.clone(), original);
        let updated = settings::update_checked(|s| s.onboarding_completed = true).unwrap();
        assert!(updated.onboarding_completed);
        assert!(settings::load_from(&valid_path).onboarding_completed);
    }

    #[test]
    fn library_selection_persists_only_valid_directories() {
        let _guard = serialize();
        let root = scratch_dir("library-selection");
        let config = root.join("config.json");
        let library = root.join("chosen");
        std::fs::create_dir_all(&library).unwrap();
        settings::init_for_test(config.clone(), AppSettings::default());

        let selected = settings::set_library_directory(&library.to_string_lossy()).unwrap();
        assert_eq!(selected, library.canonicalize().unwrap().to_string_lossy());
        assert_eq!(settings::get().library_dir, selected);
        assert_eq!(settings::load_from(&config).library_dir, selected);

        let file = root.join("not-a-directory");
        std::fs::write(&file, b"file").unwrap();
        assert!(settings::set_library_directory(&file.to_string_lossy()).is_err());
        assert_eq!(settings::get().library_dir, selected);
        assert_eq!(settings::load_from(&config).library_dir, selected);
    }

    #[test]
    fn scan_ignores_symlinks_instead_of_recursing_or_listing_them() {
        let dir = scratch_dir("scanlink").canonicalize().unwrap();
        let real = dir.join("real.jpg");
        std::fs::write(&real, b"x").unwrap();
        let outside = scratch_dir("scanlink-outside").canonicalize().unwrap();
        let secret = outside.join("secret.jpg");
        std::fs::write(&secret, b"y").unwrap();
        std::os::unix::fs::symlink(&secret, dir.join("out.jpg")).unwrap();
        std::os::unix::fs::symlink(dir.join("real.jpg"), dir.join("alias.jpg")).unwrap();
        // 自我引用的目录软链：跟随符号链接的旧实现会把它当成子目录无限递归（栈只进不出）
        std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();

        let found = media::scan(&dir.to_string_lossy()).unwrap();
        let real_str = real.to_string_lossy().to_string();
        // 只认库内真实文件：软链不入列——应用/删除侧本来就按 canonical 边界判定，
        // 列出来只会得到「看得见点不动」的死条目。
        assert_eq!(
            found.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
            vec![real_str.as_str()]
        );
    }

    #[test]
    fn scan_distinguishes_missing_dir_from_empty_dir() {
        let dir = scratch_dir("scan");
        // 未选择目录：空列表（前端显示"选择文件夹"引导，而不是错误）
        assert!(media::scan("   ").unwrap().is_empty());
        // 目录存在但无媒体：仍是空列表
        assert!(media::scan(&dir.to_string_lossy()).unwrap().is_empty());
        // 目录被删除/移动：必须报错，前端才能给"读取失败 + 重试"
        assert!(media::scan(&dir.join("gone").to_string_lossy()).is_err());
        // 指向文件而非目录：同样报错
        let not_dir = dir.join("notafolder.txt");
        std::fs::write(&not_dir, b"x").unwrap();
        assert!(media::scan(&not_dir.to_string_lossy()).is_err());
        // 递归收集受支持文件，忽略不支持的扩展名
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.png"), b"x").unwrap();
        std::fs::write(sub.join("b.MOV"), b"x").unwrap();
        let found = media::scan(&dir.to_string_lossy()).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|(_, k)| *k == MediaKind::Video));
    }

    #[test]
    fn media_default_order_is_case_insensitive_and_mtime_is_reported() {
        let dir = scratch_dir("order");
        for name in ["b.png", "A.png", "c.MOV"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        // 按字节序会把大写排到前面（B.png < a.png），默认序必须忽略大小写才是人类预期
        let names: Vec<String> = media::scan(&dir.to_string_lossy())
            .unwrap()
            .into_iter()
            .map(|(p, _)| {
                PathBuf::from(p)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["A.png", "b.png", "c.MOV"], "{names:?}");

        let fresh = dir.join("A.png").to_string_lossy().to_string();
        assert!(thumbs::mtime_of(&fresh) > 0, "刚写入的文件应带修改时间");
        assert_eq!(
            thumbs::mtime_of("/no/such/file.png"),
            0,
            "取不到时间按未知处理"
        );
        // 出口字段名必须仍是 camelCase mtime（前端排序读它）
        let item = MediaItem {
            path: "/lib/A.png".into(),
            name: "A.png".into(),
            kind: MediaKind::Image,
            thumb: "/cache/a.jpg".into(),
            mtime: 1_700_000_000,
        };
        assert_eq!(
            serde_json::to_value(&item).unwrap(),
            serde_json::json!({
                "path": "/lib/A.png",
                "name": "A.png",
                "kind": "image",
                "thumb": "/cache/a.jpg",
                "mtime": 1_700_000_000,
            })
        );
    }

    #[test]
    fn wire_format_field_consistency() {
        // 前端契约：camelCase 字段 + snake_case 枚举值
        let a = WallpaperAssignment {
            display_id: "d1".into(),
            path: "/m.mp4".into(),
            kind: MediaKind::Video,
            fit_mode: FitMode::Fill,
            muted: true,
        };
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "displayId": "d1",
                "path": "/m.mp4",
                "kind": "video",
                "fitMode": "fill",
                "muted": true,
            })
        );
        let state = DisplayWallpaperState {
            display_id: "d1".into(),
            phase: Phase::Paused,
            assignment: Some(a),
            error: None,
        };
        let text = serde_json::to_string(&state).unwrap();
        let back: DisplayWallpaperState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.phase, Phase::Paused);
        assert!(text.contains("\"phase\":\"paused\""));
        // 非法阶段必须解析失败（前后端枚举一致性护栏）
        assert!(serde_json::from_str::<DisplayWallpaperState>(
            r#"{"displayId":"d1","phase":"bogus","assignment":null,"error":null}"#
        )
        .is_err());
        // 显示器契约：temporary 必须随 JSON 出口，前端才能标注「临时 ID」
        let d = DisplayInfo {
            id: "disp-10ac-8801".into(),
            name: "Display".into(),
            logical_bounds: (0.0, 0.0, 1920.0, 1080.0),
            pixel_width: 3840,
            pixel_height: 2160,
            scale_factor: 2.0,
            primary: true,
            mirrored: false,
            temporary: false,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "disp-10ac-8801",
                "name": "Display",
                "logicalBounds": [0.0, 0.0, 1920.0, 1080.0],
                "pixelWidth": 3840,
                "pixelHeight": 2160,
                "scaleFactor": 2.0,
                "primary": true,
                "mirrored": false,
                "temporary": false,
            })
        );
    }

    #[test]
    fn settings_wire_contract_both_directions() {
        // vite 不做类型检查：设置的双向契约只能用 serde 测试钉住。
        // 写入方向 —— 前端 updateSettings() 发出的字面 JSON（参数名 settings 对应
        // invoke 的第二参数键，结构体字段必须是 camelCase 的 defaultFitMode/defaultMuted）
        let s: crate::FrontendSettings =
            serde_json::from_str(r#"{"defaultFitMode":"fit","defaultMuted":false}"#).unwrap();
        assert_eq!(s.default_fit_mode, FitMode::Fit);
        assert!(!s.default_muted);
        // 旧字段名必须解析失败，而不是静默按默认值保存
        assert!(serde_json::from_str::<crate::FrontendSettings>(
            r#"{"fitMode":"fit","muted":false}"#
        )
        .is_err());
        // 库目录不属于这个载荷：它只能由 pick_library_directory 改。
        // 曾因每次保存都回写前端的 libraryDir，在快照未回来/失败时把空目录落盘，
        // 下一次扫描直接报「无法读取壁纸目录」；带目录的旧载荷现在必须显式失败。
        assert!(serde_json::from_str::<crate::FrontendSettings>(
            r#"{"libraryDir":"/Users/me/Pictures","defaultFitMode":"fit","defaultMuted":false}"#
        )
        .is_err());

        // 读取方向 —— get_app_snapshot 出口，前端用 snapshot.defaultFitMode 初始化界面
        let snap = crate::AppSnapshot {
            library_dir: "/lib".into(),
            default_fit_mode: FitMode::Fill,
            default_muted: true,
            onboarding_completed: false,
            displays: vec![],
            states: vec![],
        };
        assert_eq!(
            serde_json::to_value(&snap).unwrap(),
            serde_json::json!({
                "libraryDir": "/lib",
                "defaultFitMode": "fill",
                "defaultMuted": true,
                "onboardingCompleted": false,
                "displays": [],
                "states": [],
            })
        );
    }

    #[test]
    fn stable_display_id_rules() {
        // 内置屏固定 ID
        assert_eq!(
            displays::id_of_parts(true, 0, 0, 123),
            ("builtin".to_string(), false)
        );
        // 外接屏只由厂商+序列号决定：CGDisplayID 变化（拔插/重启）不影响，配置才能对上
        assert_eq!(
            displays::id_of_parts(false, 0x10ac, 0x8801, 4),
            ("disp-10ac-8801".to_string(), false)
        );
        assert_eq!(
            displays::id_of_parts(false, 0x10ac, 0x8801, 9),
            ("disp-10ac-8801".to_string(), false)
        );
        // 无 EDID 序列号：退回临时 ID，且必须标记为临时供界面提示
        let (id, temporary) = displays::id_of_parts(false, 0x0610, 0, 7);
        assert_eq!(id, "cgdisplay-7");
        assert!(temporary);
    }

    /// 原生 WKWebView bridge 的 action 名必须同时存在于 Vue api.ts 与 Swift dispatcher，
    /// 否则界面会发出请求但原生层只会返回「未知操作」。
    #[test]
    fn native_bridge_actions_match_frontend() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("项目根目录")
            .to_path_buf();
        let api_ts =
            std::fs::read_to_string(root.join("ui/src/lib/api.ts")).expect("前端 bridge 文件可读");
        let swift = std::fs::read_to_string(
            root.join("xcode/WallpaperEngineApp/Sources/App/ContentView.swift"),
        )
        .expect("Swift bridge 文件可读");

        let mut frontend = Vec::new();
        for line in api_ts.lines() {
            let Some(index) = line.find("invokeNative<") else {
                continue;
            };
            let call = &line[index..];
            let Some(generic_end) = call.find('>') else {
                continue;
            };
            let call = &call[generic_end + 1..];
            let Some(open) = call.find("(\"") else {
                continue;
            };
            let after = &call[open + 2..];
            let close = after.find('\"').expect("action 名引号未闭合");
            frontend.push(after[..close].to_string());
        }

        let swift_actions: Vec<String> = swift
            .lines()
            .filter_map(|line| line.trim().strip_prefix("case \""))
            .filter_map(|line| line.split_once('"').map(|(name, _)| name.to_string()))
            .collect();

        assert_eq!(frontend.len(), 10, "前端 bridge action 数量发生变化");
        assert_eq!(swift_actions.len(), 10, "Swift bridge action 数量发生变化");
        for action in &frontend {
            assert!(
                swift_actions.iter().any(|candidate| candidate == action),
                "前端调用了 Swift 未注册的 bridge action: {action}"
            );
        }
        for action in &swift_actions {
            assert!(
                frontend.iter().any(|candidate| candidate == action),
                "Swift 注册了前端未调用的 bridge action: {action}"
            );
        }
    }

    /// 路径一律由调用方传入或由原生宿主提供，后端源码里不许出现手写的家目录/系统目录。
    /// 这类字面量抄错不会编译失败，只会在另一台机器上静默写错位置（曾把 settings.json
    /// 落进壁纸库、也曾经把 identifier 手抄一遍，需要另一条测试来交叉校验）。
    /// 现在改成扫描源码本身：新增写死路径 = 测试红，而不是等下一次换机器才发现。
    #[test]
    fn no_handwritten_absolute_paths_in_backend_sources() {
        // 这些片段只能出现在解析器返回值或调用方参数里，不许出现在源码（含字符串常量）里
        const FORBIDDEN: [&str; 2] = ["/Users/", "/System/Library"];
        let src_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let files: Vec<PathBuf> = std::fs::read_dir(&src_dir)
            .expect("src 目录")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "rs"))
            .collect();
        assert!(files.len() > 5, "扫描到的源文件太少，路径大概不对");
        for file in files {
            // 本测试自己的夹具里有被禁止的字面量，跳过
            if file.file_name().is_some_and(|n| n == "unit_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("源码可读");
            for (idx, line) in text.lines().enumerate() {
                // 只看代码：注释里提到某个目录名是说明，不是把路径写死
                let code = line.split("//").next().unwrap_or("");
                for needle in FORBIDDEN {
                    assert!(
                        !code.contains(needle),
                        "{}:{} 出现写死的路径片段 {needle}",
                        file.file_name().unwrap_or_default().to_string_lossy(),
                        idx + 1
                    );
                }
            }
        }
    }
}
