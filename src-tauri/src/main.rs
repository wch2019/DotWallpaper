// DotWallpaper 壁纸工具 - Tauri 后端入口
// 通过 Win32 SystemParametersInfoW 设置/获取桌面壁纸
// 本地壁纸源 + 拖入图片保存

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod thumbs;
mod wallpaper;

use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager;

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
fn get_wallpaper_style() -> Result<wallpaper::DesktopStyle, String> {
    wallpaper::get_desktop_wallpaper_style()
}

/// 设置桌面壁纸展示样式（写入注册表并立即刷新桌面生效）
#[tauri::command]
fn set_desktop_style(style: u32, tile: bool) -> Result<(), String> {
    wallpaper::set_desktop_wallpaper_style(style, tile)
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
fn delete_wallpaper(path: String, app: tauri::AppHandle) -> Result<(), String> {
    // 先删除源文件：系统路径 / 不存在 / 不支持类型会在这一步被拒绝
    wallpaper::delete_wallpaper_file(&path)?;
    // 源文件删除成功后再清理缩略图缓存（尽力而为）
    if let Ok(cache) = thumbs::cache_dir(&app) {
        thumbs::delete_thumb(&path, &cache);
    }
    Ok(())
}

/// 将目录动态加入 asset protocol scope，使前端 convertFileSrc 可预览该目录图片
fn ensure_asset_scope(app: &tauri::AppHandle, dir: &PathBuf) {
    if let Err(e) = app.asset_protocol_scope().allow_directory(dir, true) {
        eprintln!("[warn] asset scope 添加失败 {}: {e}", dir.display());
    }
}

/// 将本地壁纸路径设置为桌面壁纸。
///
/// `path` 必须为本地文件系统路径。
/// `dir` 可选：拖入图片下载保存的目标目录；为空时使用默认图片目录。
#[tauri::command]
fn set_wallpaper(
    path: String,
    dir: Option<String>,
    app: tauri::AppHandle,
) -> Result<SetWallpaperResult, String> {
    let save_dir = resolve_save_dir(dir);
    ensure_asset_scope(&app, &save_dir);

    // 本地文件路径：直接设置
    wallpaper::set_wallpaper_win32(&path)?;
    Ok(SetWallpaperResult { path })
}

/// 获取当前桌面壁纸路径
#[tauri::command]
fn get_current_wallpaper() -> Result<String, String> {
    wallpaper::get_current_wallpaper_win32()
}

/// 扫描壁纸目录并生成/复用缩略图，返回列表条目（原图路径 + 缩略图路径）
///
/// `directory` 为可选的自定义壁纸目录；传 Some 时只扫描该目录，留空则用预设目录。
#[tauri::command]
fn list_local_wallpapers(
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
    let paths = wallpaper::scan_local_wallpapers(directory)?;
    let cache = thumbs::cache_dir(&app).ok();
    Ok(thumbs::make_entries(&app, paths, cache.as_deref()))
}

/// 扫描 Windows 自带系统壁纸目录（C:\Windows\Web\Wallpaper，含子目录）并生成缩略图，
/// 仅供"系统壁纸"选项卡只读展示（只生成缩略图，不提供删除/写源目录）。
#[tauri::command]
fn list_system_wallpapers(app: tauri::AppHandle) -> Result<Vec<thumbs::WallpaperEntry>, String> {
    let paths = wallpaper::scan_system_wallpapers()?;
    let cache = thumbs::cache_dir(&app).ok();
    Ok(thumbs::make_entries(&app, paths, cache.as_deref()))
}

/// 弹出系统目录选择框，返回用户选择的目录路径（取消时返回 None）
#[tauri::command]
fn pick_wallpaper_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string().replace('/', "\\")))
}

/// 将原生拖放事件给出的本地文件路径保存到壁纸目录，返回保存成功与跳过列表
#[tauri::command]
fn save_dropped_paths(
    paths: Vec<String>,
    dir: Option<String>,
    app: tauri::AppHandle,
) -> Result<SaveDroppedPathsResult, String> {
    let save_dir = resolve_save_dir(dir);
    ensure_asset_scope(&app, &save_dir);
    let (saved, skipped) = copy_dropped_files(&paths, &save_dir)?;
    Ok(SaveDroppedPathsResult { saved, skipped })
}

/// 将外部拖入的本地图片文件复制到壁纸目录（保留原名，重名自动加序号）
fn copy_dropped_files(paths: &[String], save_dir: &std::path::Path) -> Result<(Vec<String>, Vec<String>), String> {
    std::fs::create_dir_all(save_dir)
        .map_err(|e| format!("创建目录失败 {}: {e}", save_dir.display()))?;

    let mut saved: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    for p in paths {
        let src = std::path::Path::new(p);
        let Some(name) = src.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let ext = name.rsplit('.').next().unwrap_or_default();
        if !wallpaper::is_supported_image_ext(ext) {
            skipped.push(format!("{name}: 不支持的格式（仅 JPG/PNG/BMP/WebP）"));
            continue;
        }

        // 目标已存在同名时自动追加序号
        let mut dest = save_dir.join(&name);
        let mut idx = 1u32;
        while dest.exists() {
            let stem = src
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "image".to_string());
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
fn resolve_save_dir(dir: Option<String>) -> PathBuf {
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 启动时即把缩略图缓存目录加入 asset protocol scope，
            // 保证 WebView 可通过 asset/convertFileSrc 加载缩略图。
            if let Ok(cache) = thumbs::cache_dir(app.handle()) {
                if let Err(e) = std::fs::create_dir_all(&cache) {
                    eprintln!("[warn] 创建缩略图缓存目录失败 {}: {e}", cache.display());
                }
                ensure_asset_scope(app.handle(), &cache);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_wallpaper,
            get_current_wallpaper,
            list_local_wallpapers,
            list_system_wallpapers,
            pick_wallpaper_directory,
            save_dropped_paths,
            delete_wallpaper,
            get_wallpaper_style,
            set_desktop_style,
            get_desktop_screen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
