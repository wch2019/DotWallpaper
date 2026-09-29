// DotWallpaper 壁纸工具 - Tauri 后端入口
// 通过 Win32 SystemParametersInfoW 设置/获取桌面壁纸
// 本地壁纸源 + 拖入图片保存

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bing;
mod thumbs;
mod wallpaper;
mod video_wallpaper;
mod video_thumbs;

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri_plugin_autostart::MacosLauncher;

/// 关闭主窗口时是否隐藏到托盘（true）还是直接退出（false）。
/// 由前端在读取 localStorage 后通过 `set_close_behavior` 同步过来。
/// 之所以把关闭决策放到 Rust 侧：主窗口 decorations=false 时唯一的关闭路径是
/// 程序调用，而前端的 onCloseRequested 监听在页面重载/HMR 期间存在丢失窗口，
/// 一旦丢失就会直接关窗退进程。这里做兜底，保证行为稳定。
static CLOSE_TO_TRAY: AtomicBool = AtomicBool::new(true);

/// 前端同步"关闭窗口行为"偏好：true=隐藏到托盘，false=直接退出
#[tauri::command]
fn set_close_behavior(hide_to_tray: bool) {
    CLOSE_TO_TRAY.store(hide_to_tray, Ordering::Relaxed);
}

/// 设置壁纸命令的统一返回：设置成功后返回实际使用的本地路径
#[derive(Serialize, Clone)]
struct SetWallpaperResult {
    path: String,
}

/// 拖入本地文件保存命令的返回：成功保存列表 + 跳过/失败原因
#[derive(Serialize, Clone)]
struct SaveDroppedPathsResult {
    saved: Vec<String>,
    skipped: Vec<String>,
}

/// 获取当前桌面壁纸展示样式（用于按真实电脑效果预览）
#[tauri::command]
async fn get_wallpaper_style() -> Result<wallpaper::DesktopStyle, String> {
    tauri::async_runtime::spawn_blocking(wallpaper::get_desktop_wallpaper_style)
        .await
        .map_err(|e| e.to_string())?
}

/// 设置桌面壁纸展示样式（写入注册表并立即刷新桌面生效）
#[tauri::command]
async fn set_desktop_style(style: u32, tile: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || wallpaper::set_desktop_wallpaper_style(style, tile))
        .await
        .map_err(|e| e.to_string())?
}

/// 获取主屏幕逻辑分辨率与缩放比（用于按真实电脑屏幕比例预览）
#[tauri::command]
fn get_desktop_screen(app: tauri::AppHandle) -> Result<wallpaper::ScreenMeta, String> {
    wallpaper::get_primary_screen_meta(&app)
}

/// 从本地磁盘永久删除壁纸文件（仅限支持的图片扩展名）
///
/// 删除原图成功后同步清理其缩略图缓存；系统壁纸只读约束不变（C:\Windows
/// 路径会先被后端拦截报错，不会误删对应缩略图）。
#[tauri::command]
async fn delete_wallpaper(path: String, app: tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        // 先删除源文件：系统路径 / 不存在 / 不支持类型会在这一步被拒绝
        wallpaper::delete_wallpaper_file(&path)?;
        // 源文件删除成功后再清理缩略图缓存（尽力而为）
        if let Ok(cache) = thumbs::cache_dir(&app) {
            thumbs::delete_thumb(&path, &cache);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 将目录动态加入 asset protocol scope，使前端 convertFileSrc 可预览该目录图片
pub(crate) fn ensure_asset_scope(app: &tauri::AppHandle, dir: &PathBuf) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("[warn] asset scope 添加失败 {}: {e}", dir.display());
    }
}

/// 将本地壁纸路径设置为桌面壁纸。
///
/// `path` 必须为本地文件系统路径。
/// `dir` 可选：拖入图片下载保存的目标目录；为空时使用默认图片目录。
#[tauri::command]
async fn set_wallpaper(
    path: String,
    dir: Option<String>,
    app: tauri::AppHandle,
) -> Result<SetWallpaperResult, String> {
    let save_dir = resolve_save_dir(dir);
    ensure_asset_scope(&app, &save_dir);

    // SystemParametersInfoW(SPI_SETDESKWALLPAPER) 为同步系统广播，会等待
    // explorer 完成壁纸应用才返回；放入 blocking 线程避免卡死窗口主线程。
    let set_path = path.clone();
    let res: Result<(), String> =
        tauri::async_runtime::spawn_blocking(move || wallpaper::set_wallpaper_win32(&set_path))
            .await
            .map_err(|e| e.to_string())?;
    res?;
    Ok(SetWallpaperResult { path })
}

/// 按模糊遮罩效果合成为壁纸并设为桌面壁纸。
///
/// 流程：合成图写入缓存 → 写入壁纸样式注册表 → SystemParametersInfoW 设置合成图。
/// 返回原图路径（当前壁纸状态仍以原图身份展示，右键再设壁纸时重新走合成）。
#[tauri::command]
async fn apply_wallpaper_effect(
    path: String,
    style: u32,
    tile: bool,
    effect: wallpaper::WallpaperEffect,
    app: tauri::AppHandle,
) -> Result<SetWallpaperResult, String> {
    let compose_path = path.clone();
    let composed = tauri::async_runtime::spawn_blocking(move || {
        wallpaper::compose_wallpaper_effect(&app, &compose_path, effect)
    })
    .await
    .map_err(|e| e.to_string())??;

    // 写样式注册表 + 设壁纸同样在 blocking 线程中完成
    let set_path = composed;
    let res: Result<(), String> =
        tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
            wallpaper::set_desktop_wallpaper_style(style, tile)?;
            wallpaper::set_wallpaper_win32(&set_path)
        })
        .await
        .map_err(|e| e.to_string())?;
    res?;

    Ok(SetWallpaperResult { path })
}

/// 获取当前桌面壁纸路径
#[tauri::command]
async fn get_current_wallpaper() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(wallpaper::get_current_wallpaper_win32)
        .await
        .map_err(|e| e.to_string())?
}

/// 扫描壁纸目录并生成/复用缩略图，返回列表条目（原图路径 + 缩略图路径）
///
/// `directory` 为可选的自定义壁纸目录；传 Some 时只扫描该目录，留空则用预设目录。
#[tauri::command]
async fn list_local_wallpapers(
    directory: Option<String>,
    app: tauri::AppHandle,
) -> Result<Vec<thumbs::WallpaperEntry>, String> {
    // 自定义目录可能尚未加入 asset scope（此前未设置/未拖入过）；小图跳过
    // 生成时 thumb 直接使用原图路径，需保证 convertFileSrc 可加载该目录。
    if let Some(dir) = &directory {
        let t = dir.trim();
        if !t.is_empty() {
            ensure_asset_scope(&app, &PathBuf::from(t));
        }
    }
    // 目录扫描 + 缩略图缓存检测为磁盘 IO，放入 blocking 线程避免卡住主线程。
    let cache = thumbs::cache_dir(&app).ok();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<thumbs::WallpaperEntry>, String> {
        let paths = wallpaper::scan_local_wallpapers(directory)?;
        Ok(thumbs::make_entries(&app, paths, cache.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 扫描 Windows 自带系统壁纸目录（C:\Windows\Web\Wallpaper，含子目录）并生成缩略图，
/// 仅供"系统壁纸"选项卡只读展示（只生成缩略图，不提供删除/写源目录）。
#[tauri::command]
async fn list_system_wallpapers(app: tauri::AppHandle) -> Result<Vec<thumbs::WallpaperEntry>, String> {
    let cache = thumbs::cache_dir(&app).ok();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<thumbs::WallpaperEntry>, String> {
        let paths = wallpaper::scan_system_wallpapers()?;
        Ok(thumbs::make_entries(&app, paths, cache.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 按给定路径列表构造列表条目（收藏页专用）。
///
/// 与 `list_local_wallpapers` 的关键区别：不扫描当前壁纸目录，只处理传入的
/// 绝对路径，因此收藏项与当前目录无关 —— 切换壁纸目录后收藏依然完整可见。
/// 路径不存在 / 非文件 / 扩展名不支持时自动过滤（外部手动删除的收藏项不展示）；
/// 每条路径所在目录动态加入 asset scope，保证跨目录的缩略图与原图可预览。
#[tauri::command]
async fn list_wallpapers_by_paths(
    paths: Vec<String>,
    app: tauri::AppHandle,
) -> Result<Vec<thumbs::WallpaperEntry>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    // 收藏项可能分散在多个目录：逐个把所在目录加入 asset scope
    let mut scoped: Vec<PathBuf> = Vec::new();
    for p in &paths {
        let Some(parent) = PathBuf::from(p).parent().map(|d| d.to_path_buf()) else {
            continue;
        };
        if scoped.contains(&parent) {
            continue;
        }
        ensure_asset_scope(&app, &parent);
        scoped.push(parent);
    }
    let cache = thumbs::cache_dir(&app).ok();
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<thumbs::WallpaperEntry>, String> {
        let valid: Vec<String> = paths
            .into_iter()
            .filter(|p| {
                let path = PathBuf::from(p);
                let ext_ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(wallpaper::is_supported_wallpaper_ext)
                    .unwrap_or(false);
                path.is_file() && ext_ok
            })
            .collect();
        Ok(thumbs::make_entries(&app, valid, cache.as_deref()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 拉取必应每日壁纸列表（在线数据，仅返回标题/日期/远程 URL，不下载原图）
///
/// 前端直接用返回的 url（原图）/ thumb（400x240 小图）作为图片地址展示，
/// 只有"设为壁纸"时才调用 download_bing_wallpaper 下载到本地。
#[tauri::command]
async fn list_bing_wallpapers() -> Result<Vec<bing::BingWallpaper>, String> {
    // 网络请求为阻塞 IO，放入 blocking 线程避免卡住主线程
    tauri::async_runtime::spawn_blocking(bing::fetch_wallpapers)
        .await
        .map_err(|e| e.to_string())?
}

/// 下载必应壁纸原图到本地缓存目录，返回本地绝对路径
///
/// 下载目录由前端传入（前端 localStorage 持久化，键 `dot-wallpaper-bing-dir`），
/// 未传或为空时退回默认 `图片目录\\BingWallpaper`。同名文件已存在时直接复用。
#[tauri::command]
async fn download_bing_wallpaper(
    url: String,
    date: String,
    dir: Option<String>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let dir = dir
        .filter(|s| !s.trim().is_empty())
        .or_else(|| bing::default_bing_dir().ok())
        .ok_or_else(|| "无法确定必应壁纸下载目录".to_string())?;
    let dir_path = PathBuf::from(dir);
    ensure_asset_scope(&app, &dir_path);
    tauri::async_runtime::spawn_blocking(move || bing::download_wallpaper(&url, &date, &dir_path))
        .await
        .map_err(|e| e.to_string())?
}

/// 弹出文件对话框选择必应壁纸下载目录，仅返回所选路径（持久化由前端 localStorage 负责）
///
/// 返回 `Ok(None)` 表示用户取消。注意：必须保持为同步命令 —— blocking_pick_folder
/// 会阻塞当前线程等待主线程事件循环返回对话框结果，放在 async 命令里会阻塞异步运行时。
#[tauri::command]
fn pick_bing_wallpaper_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // 拿不到已配置目录时退回默认目录；默认目录不存在则先建出来，
    // 否则对话框的初始定位会失效
    let default_dir = bing::default_bing_dir()
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if !default_dir.is_empty() {
        let _ = std::fs::create_dir_all(&default_dir);
    }

    let mut builder = app.dialog().file().set_title("选择必应壁纸下载目录");
    if !default_dir.is_empty() {
        builder = builder.set_directory(&PathBuf::from(&default_dir));
    }
    let res = builder.blocking_pick_folder();

    // FilePath → 字符串：Windows 下统一为反斜杠（不转义，避免出现 "\\\\"）
    Ok(res.map(|path| path.to_string().replace('/', "\\")))
}

/// 在系统资源管理器中定位文件/目录（右键"跳转到当前文件目录"）
///
/// 文件使用 explorer /select 打开所在目录并选中该项；目录则直接打开。
#[tauri::command]
fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("路径不存在：{path}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // raw_arg 不经 std 二次转义，直接交给 explorer.exe 解析，
        // 保证带空格/特殊字符的路径也能被正确识别。
        let raw = if p.is_dir() {
            format!("\"{}\"", p.to_string_lossy())
        } else {
            format!("/select,\"{}\"", p.to_string_lossy())
        };
        std::process::Command::new("explorer.exe")
            .raw_arg(&raw)
            .spawn()
            .map_err(|e| format!("打开资源管理器失败：{e}"))?;
    }
    Ok(())
}

/// 弹出系统目录选择框，返回用户选择的目录路径（取消时返回 None）
#[tauri::command]
fn pick_wallpaper_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string().replace('/', "\\")))
}

/// 将原生拖放事件给出的本地文件路径保存到壁纸目录，返回保存成功与跳过列表
///
/// 拖入的是**任意大小的文件**（视频动辄数百 MB ~ 数 GB），复制必须放到 blocking
/// 线程，否则会卡死窗口主线程。
#[tauri::command]
async fn save_dropped_paths(
    paths: Vec<String>,
    dir: Option<String>,
    app: tauri::AppHandle,
) -> Result<SaveDroppedPathsResult, String> {
    let save_dir = resolve_save_dir(dir);
    ensure_asset_scope(&app, &save_dir);
    tauri::async_runtime::spawn_blocking(move || {
        let (saved, skipped) = copy_dropped_files(&paths, &save_dir)?;
        Ok(SaveDroppedPathsResult { saved, skipped })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单次拖入展开文件夹时收集的文件数上限
///
/// 防止误把整个图片库 / 桌面拖进来导致海量复制。达到上限后停止收集并在跳过列表里说明。
const MAX_DROP_FILES: usize = 500;

/// 递归展开拖入文件夹时的最大目录深度（防止异常目录结构导致超长扫描）
const MAX_DROP_SCAN_DEPTH: usize = 6;

/// 递归展开拖入文件夹的结果
struct FolderScan {
    /// 因"是视频但 WebView2 播不了"（mkv / mov）而跳过的文件数。
    /// 其他无关文件（txt/pdf…）不计入 —— 它们本来就不是壁纸，不值一提。
    unplayable: usize,
    /// 是否**因为达到数量上限而提前停止**
    truncated: bool,
}

/// 递归收集目录下**可导入**的壁纸文件（图片 + mp4/webm）
///
/// 读目录失败的分支直接跳过（尽力而为）—— 不因为某个子目录无权限就让整次拖入失败。
fn collect_wallpaper_files(
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
    scan: &mut FolderScan,
    depth: usize,
) {
    if depth > MAX_DROP_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        // 上限按"已收下的 + 已判定不能播的"合计来算，而不是只看已收下的：
        // 否则一个装满 mkv 的目录永远不触发上限，会把整棵树走完才发现一个都导不进来。
        if out.len() + scan.unplayable >= MAX_DROP_FILES {
            scan.truncated = true;
            return;
        }
        let p = entry.path();
        if p.is_dir() {
            collect_wallpaper_files(&p, out, scan, depth + 1);
            if scan.truncated {
                return;
            }
            continue;
        }
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if wallpaper::is_importable_wallpaper_ext(ext) {
            out.push(p);
        } else if wallpaper::is_supported_video_ext(ext) {
            // 认得出是视频容器，但 WebView2 播不了 —— 单独计数，好在提示里说清楚原因
            scan.unplayable += 1;
        }
    }
}

/// 将外部拖入的本地壁纸文件复制到壁纸目录（保留原名，重名自动加序号）
///
/// 放行规则比目录扫描**更严**：图片 + mp4/webm。mkv / mov 会被当场拒收并说明原因
/// —— 收下来用户点开只会看到黑屏，不如直接告诉他为什么没收。
/// （手动放进目录的 mkv/mov 仍能被扫描到、能删除，只是拖不进来。）
///
/// 拖入**文件夹**时递归取其内所有可导入的壁纸文件（限深度、限数量）。
fn copy_dropped_files(
    paths: &[String],
    save_dir: &std::path::Path,
) -> Result<(Vec<String>, Vec<String>), String> {
    std::fs::create_dir_all(save_dir)
        .map_err(|e| format!("创建目录失败 {}: {e}", save_dir.display()))?;

    let mut saved: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // ---- 第一步：把拖入项展开成"待复制的文件"列表（文件夹递归展开）----
    let mut sources: Vec<PathBuf> = Vec::new();
    for p in paths {
        let src = PathBuf::from(p);
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.clone());

        if src.is_dir() {
            let before = sources.len();
            let mut scan = FolderScan {
                unplayable: 0,
                truncated: false,
            };
            collect_wallpaper_files(&src, &mut sources, &mut scan, 0);
            let added = sources.len() - before;

            if scan.truncated {
                skipped.push(format!(
                    "{name}: 文件太多，本次只扫描前 {MAX_DROP_FILES} 个（已导入 {added} 个）"
                ));
            } else if added == 0 && scan.unplayable == 0 {
                skipped.push(format!("{name}: 文件夹里没有壁纸文件"));
            } else if added == 0 {
                skipped.push(format!(
                    "{name}: 里面 {} 个视频不是 MP4/WebM，无法在桌面播放",
                    scan.unplayable
                ));
            } else if scan.unplayable > 0 {
                skipped.push(format!(
                    "{name}: 已导入 {added} 个；另有 {} 个视频不是 MP4/WebM，无法在桌面播放",
                    scan.unplayable
                ));
            }
            continue;
        }
        sources.push(src);
    }

    // 目标目录的规范化路径：用于跳过"拖进来的文件本来就在目标目录里"的情况
    //（例如把壁纸目录自身拖进来），否则每个文件都会被复制出一份 `_1` 副本。
    let save_dir_norm = save_dir
        .canonicalize()
        .unwrap_or_else(|_| save_dir.to_path_buf());

    // ---- 第二步：逐个复制 ----
    for src in sources {
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if src
            .canonicalize()
            .map_or(false, |abs| abs.starts_with(&save_dir_norm))
        {
            skipped.push(format!("{name}: 已在壁纸目录中"));
            continue;
        }

        // 用 Path::extension 而非 rsplit('.')：后者对"无扩展名但名字恰好叫 mp4"
        // 的文件会误判为受支持。
        let ext = src.extension().and_then(|e| e.to_str()).unwrap_or_default();
        if !wallpaper::is_importable_wallpaper_ext(ext) {
            // 分清两种拒绝原因：认得出是视频容器但播不了（要说清"为什么"），
            // 与压根不是壁纸（列白名单）。同一句话糊过去，用户会以为程序坏了。
            // 文案写成**纯原因**、不带"已跳过" —— 前端已经用
            // "跳过 N 个：<原因>" / "没有可保存的文件：<原因>" 包了一层，
            // 这里再写一遍会变成"…已跳过：…已跳过"。
            if wallpaper::is_supported_video_ext(ext) {
                skipped.push(format!("{name}: {ext} 无法在桌面播放（仅支持 MP4/WebM）"));
            } else {
                skipped.push(format!(
                    "{name}: 不支持的格式（支持 JPG/PNG/BMP/WebP 与 MP4/WebM）"
                ));
            }
            continue;
        }

        // 目标已存在同名时自动追加序号
        let mut dest = save_dir.join(&name);
        let mut idx = 1u32;
        while dest.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "wallpaper".to_string());
            let ext = src
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "jpg".to_string());
            dest = save_dir.join(format!("{stem}_{idx}.{ext}"));
            idx += 1;
        }

        match std::fs::copy(&src, &dest) {
            Ok(_) => saved.push(dest.to_string_lossy().replace('/', "\\")),
            Err(e) => skipped.push(format!("{name}: 复制失败 {e}")),
        }
    }

    Ok((saved, skipped))
}

/// 解析下载保存目录：优先用户指定目录，否则使用用户图片目录
pub(crate) fn resolve_save_dir(dir: Option<String>) -> PathBuf {
    if let Some(d) = dir {
        let t = d.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(&home).join("Pictures");
    }
    std::env::temp_dir()
}

/// 创建系统托盘图标与菜单：左键单击恢复主窗口；菜单含"显示主界面 / 退出"。
/// 关闭窗口行为（直接退出 / 隐藏到托盘）由前端读取 localStorage 配置决定。
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("DotWallpaper 壁纸工具")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .setup(|app| {
            // 启动时即把缩略图缓存目录加入 asset protocol scope，
            // 保证 WebView 可通过 asset/convertFileSrc 加载缩略图。
            if let Ok(cache) = thumbs::cache_dir(app.handle()) {
                if let Err(e) = std::fs::create_dir_all(&cache) {
                    eprintln!("[warn] 创建缩略图缓存目录失败 {}: {e}", cache.display());
                }
                ensure_asset_scope(app.handle(), &cache);
            }
            // 系统托盘：左键单击恢复主窗口，右键菜单提供"显示主界面 / 退出"。
            // 关闭窗口行为（直接退出 / 隐藏到托盘）由前端按 localStorage 配置决定。
            setup_tray(app.handle())?;
            Ok(())
        })
        // 关闭主窗口的兜底处理：按偏好隐藏到托盘，避免前端监听丢失时静默退出。
        // 真正的退出统一走托盘菜单的"退出"（app.exit(0)）。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 仅拦截主窗口；动态壁纸窗口由后端自行管理，不在此处处理
                if window.label() != "main" {
                    return;
                }
                if CLOSE_TO_TRAY.load(Ordering::Relaxed) {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    // 用户选择"关闭即退出"：放行关闭，主窗口消失后进程自然结束
                    api.prevent_close();
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_wallpaper,
            apply_wallpaper_effect,
            get_current_wallpaper,
            list_local_wallpapers,
            list_system_wallpapers,
            list_bing_wallpapers,
            download_bing_wallpaper,
            pick_bing_wallpaper_directory,
            list_wallpapers_by_paths,
            pick_wallpaper_directory,
            reveal_in_explorer,
            save_dropped_paths,
            delete_wallpaper,
            get_wallpaper_style,
            set_desktop_style,
            get_desktop_screen,
            set_close_behavior,
            video_wallpaper::set_video_wallpaper,
            video_wallpaper::set_video_wallpaper_muted,
            video_wallpaper::set_video_wallpaper_paused,
            video_wallpaper::stop_video_wallpaper,
            video_wallpaper::get_video_wallpaper_state,
            video_wallpaper::list_monitors
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
