// 壁纸相关后端实现：
// - 设置/获取桌面壁纸：Win32 SystemParametersInfoW
// - 扫描本地壁纸目录（Windows 自带壁纸目录 + 用户图片文件夹）

use std::ffi::c_void;
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER,
    SPIF_SENDCHANGE, SPIF_UPDATEINIFILE,
};

/// 支持的壁纸图片扩展名
const SUPPORTED_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "bmp"];

/// Windows 自带系统壁纸目录（只读展示，禁止删除/写入）
const SYSTEM_WALLPAPER_DIR: &str = r"C:\Windows\Web\Wallpaper";

/// 本地壁纸目录列表（用户图片文件夹）
///
/// Windows 自带壁纸目录已拆分为独立的"系统壁纸"源（scan_system_wallpapers），
/// 不再混入本地列表，避免两个选项卡内容重复。
fn wallpaper_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(home) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(&home).join("Pictures"));
    }

    dirs
}

/// 通过 Win32 SystemParametersInfoW(SPI_SETDESKWALLPAPER) 设置桌面壁纸
pub fn set_wallpaper_win32(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("壁纸路径为空".into());
    }

    // 将路径转换为以 \0 结尾的 UTF-16 缓冲区
    let mut path_utf16: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(path_utf16.as_mut_ptr() as *mut c_void),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        )
        .map_err(|e| format!("设置壁纸失败 (Win32 错误: {e})"))?;
    }

    Ok(())
}

/// 通过 Win32 SystemParametersInfoW(SPI_GETDESKWALLPAPER) 获取当前桌面壁纸路径
pub fn get_current_wallpaper_win32() -> Result<String, String> {
    let mut buffer = [0u16; 2048];

    unsafe {
        SystemParametersInfoW(
            SPI_GETDESKWALLPAPER,
            buffer.len() as u32,
            Some(buffer.as_mut_ptr() as *mut c_void),
            Default::default(),
        )
        .map_err(|e| format!("获取当前壁纸失败 (Win32 错误: {e})"))?;
    }

    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let path = String::from_utf16_lossy(&buffer[..len]);

    if path.is_empty() {
        return Err("未获取到当前壁纸路径（可能使用了幻灯片模式）".into());
    }

    Ok(path)
}

/// 扫描壁纸目录，返回壁纸文件路径列表（上限 200，防止用户图片文件夹过大）。
///
/// 传入 `custom_dir`（Some 且非空）时只扫描该目录；否则使用预设目录。
pub fn scan_local_wallpapers(custom_dir: Option<String>) -> Result<Vec<String>, String> {
    let mut results: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 自定义目录优先：仅扫描用户指定目录
    if let Some(dir) = custom_dir {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            let dir = PathBuf::from(trimmed);
            if !dir.is_dir() {
                return Err(format!("目录不存在或不可访问：{trimmed}"));
            }
            if let Ok(entries) = walk_dir(&dir) {
                for path in entries {
                    if results.len() >= 200 {
                        break;
                    }
                    let normalized = path.replace('/', "\\");
                    if seen.insert(normalized.clone()) {
                        results.push(normalized);
                    }
                }
            }
            return Ok(results);
        }
    }

    for dir in wallpaper_dirs() {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(entries) = walk_dir(&dir) {
            for path in entries {
                if results.len() >= 200 {
                    break;
                }
                let normalized = path.replace('/', "\\");
                if seen.insert(normalized.clone()) {
                    results.push(normalized);
                }
            }
        }
    }

    Ok(results)
}

/// 扫描 Windows 自带系统壁纸目录（含子目录），返回壁纸文件路径列表。
///
/// 仅供"系统壁纸"选项卡只读展示；调用方不得对返回路径执行删除/写入。
pub fn scan_system_wallpapers() -> Result<Vec<String>, String> {
    let dir = PathBuf::from(SYSTEM_WALLPAPER_DIR);
    if !dir.is_dir() {
        return Err(format!("系统壁纸目录不存在：{SYSTEM_WALLPAPER_DIR}"));
    }

    let mut results: Vec<String> = Vec::new();
    if let Ok(entries) = walk_dir(&dir) {
        for path in entries {
            if results.len() >= 200 {
                break;
            }
            results.push(path.replace('/', "\\"));
        }
    }
    Ok(results)
}

/// 判断路径是否位于 C:\Windows 系统目录下（大小写不敏感）。
/// 用于系统壁纸只读约束：任何删除/写操作前必须拦截。
fn is_under_windows_dir(path: &std::path::Path) -> bool {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let norm = abs.canonicalize().unwrap_or(abs);
    let s = norm.to_string_lossy().replace('/', "\\").to_lowercase();
    s.starts_with("c:\\windows") || s.starts_with("c:\\windows\\")
}

/// 递归遍历目录，收集支持的图片文件（最多 200 个）
fn walk_dir(dir: &PathBuf) -> std::io::Result<Vec<String>> {
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![dir.clone()];

    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if SUPPORTED_EXTS.iter().any(|s| s.eq_ignore_ascii_case(ext)) {
                    if let Some(p) = path.to_str() {
                        found.push(p.to_string());
                    }
                }
            }
            if found.len() >= 200 {
                break;
            }
        }
        if found.len() >= 200 {
            break;
        }
    }

    Ok(found)
}

/// 桌面壁纸展示样式（对应 Windows 个性化设置中的壁纸模式）
#[derive(serde::Serialize, Clone, Copy)]
pub struct DesktopStyle {
    /// WallpaperStyle 注册表值：0=居中 6=适应 10=填充 22=拉伸
    pub style: u32,
    /// TileWallpaper 是否为 1（平铺优先）
    pub tile: bool,
}

/// 读取当前桌面壁纸展示样式（HKCU\Control Panel\Desktop）
pub fn get_desktop_wallpaper_style() -> Result<DesktopStyle, String> {
    let style = reg_str_value(r"Control Panel\Desktop", "WallpaperStyle")
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(10);
    let tile = reg_str_value(r"Control Panel\Desktop", "TileWallpaper")
        .map(|s| s.trim() == "1")
        .unwrap_or(false);
    Ok(DesktopStyle { style, tile })
}

/// 读取注册表 REG_SZ 字符串值
fn reg_str_value(subkey: &str, value: &str) -> Option<String> {
    let sub_wide: Vec<u16> = subkey.encode_utf16().chain(Some(0)).collect();
    let val_wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();

    unsafe {
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub_wide.as_ptr()),
            0,
            KEY_READ,
            &mut key,
        );
        if open.is_err() {
            return None;
        }

        let mut buf = [0u16; 32];
        let mut size = (buf.len() * 2) as u32;
        let mut typ: REG_VALUE_TYPE = REG_VALUE_TYPE(0);
        let query = RegQueryValueExW(
            key,
            PCWSTR(val_wide.as_ptr()),
            None,
            Some(&mut typ),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        if query.is_err() || typ != REG_SZ {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// 主屏幕元信息（逻辑分辨率 + 缩放比），用于按真实电脑屏幕效果预览
#[derive(serde::Serialize, Clone, Copy)]
pub struct ScreenMeta {
    /// 逻辑宽度（物理像素 ÷ 缩放比）
    pub logical_width: f64,
    /// 逻辑高度
    pub logical_height: f64,
    /// 缩放比（如 1.25 / 1.5 / 2.0）
    pub scale_factor: f64,
}

/// 读取当前主监视器的逻辑分辨率与缩放比
pub fn get_primary_screen_meta(app: &tauri::AppHandle) -> Result<ScreenMeta, String> {
    let mon = app
        .primary_monitor()
        .map_err(|e| format!("读取屏幕信息失败：{e}"))?
        .ok_or_else(|| "未检测到显示器".to_string())?;
    let scale = mon.scale_factor();
    if scale <= 0.0 {
        return Err("屏幕缩放比异常".into());
    }
    let w = mon.size().width as f64 / scale;
    let h = mon.size().height as f64 / scale;
    if w <= 0.0 || h <= 0.0 {
        return Err("屏幕分辨率异常".into());
    }
    Ok(ScreenMeta {
        logical_width: w,
        logical_height: h,
        scale_factor: scale,
    })
}

/// 从本地磁盘永久删除壁纸文件（仅限支持的图片扩展名）
///
/// 安全约束：C:\Windows 等系统路径下的壁纸一律只读，禁止删除。
pub fn delete_wallpaper_file(path: &str) -> Result<(), String> {
    let p = PathBuf::from(path);
    if is_under_windows_dir(&p) {
        return Err("系统壁纸只读，禁止删除 Windows 系统目录下的文件".into());
    }
    if p.is_dir() {
        return Err("不能删除目录".into());
    }
    if !p.exists() {
        return Err("文件不存在或已被移动".into());
    }

    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let allowed: Vec<String> = SUPPORTED_EXTS
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once("webp".to_string()))
        .collect();
    if !allowed.iter().any(|s| s.eq_ignore_ascii_case(&ext)) {
        return Err("不支持的壁纸文件类型".into());
    }

    std::fs::remove_file(&p).map_err(|e| format!("删除失败：{e}"))?;
    Ok(())
}
