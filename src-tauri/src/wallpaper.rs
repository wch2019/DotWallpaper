// 壁纸相关后端实现：
// - 设置/获取桌面壁纸：Win32 SystemParametersInfoW
// - 扫描本地壁纸目录（Windows 自带壁纸目录 + 用户图片文件夹）

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LPARAM, WPARAM, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_READ, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, SystemParametersInfoW, HWND_BROADCAST, SMTO_ABORTIFHUNG,
    SPI_GETDESKWALLPAPER, SPI_SETDESKWALLPAPER, SPIF_UPDATEINIFILE, WM_SETTINGCHANGE,
};

/// 支持的壁纸图片扩展名
const SUPPORTED_EXTS: [&str; 5] = ["jpg", "jpeg", "png", "bmp", "webp"];

/// **放行集**：目录扫描 / 删除 / 设为动态壁纸认的视频容器（含不能播放的）。
///
/// **放行 ≠ 可播放**：WebView2 只能可靠渲染 mp4 / webm，mkv / mov 仍会被扫描进来、
/// 出现在列表里，由前端按 kind 标记为"不支持的格式"。
/// 之所以把不能播的一起放行，是因为这些文件**已经在用户机器上了** ——
/// 应用不该假装看不见（用户手动放进目录却"找不到"，比标个角标更让人困惑）。
///
/// 拖入导入的口径更严，见 [`is_importable_wallpaper_ext`]。
const SUPPORTED_VIDEO_EXTS: [&str; 4] = ["mp4", "webm", "mkv", "mov"];

/// **可播放集**：WebView2 能稳定解码的视频容器。
///
/// 与前端 `stores/wallpaper/types.ts` 的 `WEBVIEW_PLAYABLE_EXTS` **一一对应**
/// （前端用它决定"动态壁纸 / 不支持的格式"角标与放大按钮是否可用）。
/// **两处必须同步改**，否则会出现"前端说能播、后端不给导"这类自相矛盾。
const PLAYABLE_VIDEO_EXTS: [&str; 2] = ["mp4", "webm"];

/// 是否受支持的壁纸图片扩展名（大小写不敏感）
pub fn is_supported_image_ext(ext: &str) -> bool {
    SUPPORTED_EXTS.iter().any(|s| s.eq_ignore_ascii_case(ext))
}

/// 是否受支持的动态壁纸视频扩展名（大小写不敏感）
pub fn is_supported_video_ext(ext: &str) -> bool {
    SUPPORTED_VIDEO_EXTS.iter().any(|s| s.eq_ignore_ascii_case(ext))
}

/// 是否为 WebView2 可稳定播放的视频扩展名（大小写不敏感）
pub fn is_playable_video_ext(ext: &str) -> bool {
    PLAYABLE_VIDEO_EXTS.iter().any(|s| s.eq_ignore_ascii_case(ext))
}

/// 是否受支持的壁纸文件扩展名：**图片 + 视频**（大小写不敏感）。
///
/// 这是"这个文件算不算壁纸"的**唯一权威判定**，供目录扫描与删除共用
/// （视频由前端按 kind 分流为动态壁纸）。只用 `is_supported_image_ext` 的地方，
/// 应当是有意排除视频的图片专用流程。
pub fn is_supported_wallpaper_ext(ext: &str) -> bool {
    is_supported_image_ext(ext) || is_supported_video_ext(ext)
}

/// 拖入导入的判定：图片 + **可播放的**视频（大小写不敏感）。
///
/// 与 [`is_supported_wallpaper_ext`] **故意不同**：扫描要"所见即所得"，
/// 拖入则主动拒收注定播不了的文件 —— 收下一个 mkv，用户点开只会看到黑屏，
/// 不如当场告诉他为什么没收（用户明确要求）。
/// 因此 `mkv` / `mov` 是"能被扫描到、能被删除，但拖不进来"的。
pub fn is_importable_wallpaper_ext(ext: &str) -> bool {
    is_supported_image_ext(ext) || is_playable_video_ext(ext)
}

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

    // 不带 SPIF_SENDCHANGE：该标志会同步向所有顶层窗口广播 WM_SETTINGCHANGE，
    // 并等待各窗口处理完才返回（explorer 繁忙或存在挂起窗口时最长可阻塞数秒），
    // 导致前端 invoke 迟迟不 resolve、界面一直转圈。壁纸本身在调用时即已生效，
    // 广播改由后台线程异步补发（见 notify_desktop_changed）。
    unsafe {
        SystemParametersInfoW(
            SPI_SETDESKWALLPAPER,
            0,
            Some(path_utf16.as_mut_ptr() as *mut c_void),
            SPIF_UPDATEINIFILE,
        )
        .map_err(|e| format!("设置壁纸失败 (Win32 错误: {e})"))?;
    }

    notify_desktop_changed();

    Ok(())
}

/// 后台异步广播 WM_SETTINGCHANGE("Desktop")，通知资源管理器刷新桌面。
///
/// 与 SPIF_SENDCHANGE 等价，但放在独立线程执行，并用 SMTO_ABORTIFHUNG +
/// 500ms 超时避免被挂起窗口拖住；广播失败不影响壁纸已生效的事实。
fn notify_desktop_changed() {
    std::thread::spawn(|| {
        let mut desktop: Vec<u16> = "Desktop".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(desktop.as_mut_ptr() as isize),
                SMTO_ABORTIFHUNG,
                500,
                None,
            );
        }
    });
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

/// 扫描壁纸目录，返回全部壁纸文件路径列表。
///
/// 不设硬性条数上限：大批量图片由前端分页 + 缩略图后台渐进生成承载。
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

/// 递归遍历目录，收集全部支持的壁纸文件（图片 + 动态壁纸视频）
/// 递归遍历目录（含子目录），返回受支持的壁纸文件路径（原生分隔符未规范化）。
///
/// `pub(crate)`：必应那边要在**本地壁纸目录**里找已下载的壁纸
/// （见 `bing::scan_downloaded`），复用同一个遍历实现而不是各写一份。
pub(crate) fn walk_dir(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut found: Vec<String> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

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
                if is_supported_wallpaper_ext(ext) {
                    if let Some(p) = path.to_str() {
                        found.push(p.to_string());
                    }
                }
            }
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

/// 设置桌面壁纸展示样式（写入 HKCU\Control Panel\Desktop 并立即刷新桌面生效）
///
/// - `style`：Windows WallpaperStyle 值（0=居中 6=适应 10=填充 22=拉伸）
/// - `tile`：是否平铺（TileWallpaper=1，平铺优先于 style）
pub fn set_desktop_wallpaper_style(style: u32, tile: bool) -> Result<(), String> {
    if !matches!(style, 0 | 6 | 10 | 22) {
        return Err("不支持的壁纸样式".into());
    }
    let sub_wide: Vec<u16> = r"Control Panel\Desktop"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut key: HKEY = HKEY(std::ptr::null_mut());
        let open = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub_wide.as_ptr()),
            0,
            KEY_READ | KEY_SET_VALUE,
            &mut key,
        );
        if open != WIN32_ERROR(0) {
            return Err(format!("打开桌面设置注册表失败: {}", open.0));
        }
        let r1 = set_reg_str(key, "WallpaperStyle", &style.to_string());
        let r2 = set_reg_str(key, "TileWallpaper", if tile { "1" } else { "0" });
        let _ = RegCloseKey(key);
        r1?;
        r2?;
    }
    // 注意：此处不再重新应用当前壁纸。
    // 调用方（设为壁纸流程）写入样式后总会紧跟设置新壁纸，一次
    // SystemParametersInfoW(SPI_SETDESKWALLPAPER) 即按新样式生效；
    // 若在函数内先重应用旧壁纸，会多一次 SPI_SENDCHANGE 同步广播，
    // 主线程需等待 explorer 应用完才返回，曾导致窗口长时间无响应。
    Ok(())
}

/// 写入注册表 REG_SZ 字符串值（值以 NUL 结尾）
fn set_reg_str(key: HKEY, name: &str, value: &str) -> Result<(), String> {
    let name_wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let value_wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
    // REG_SZ 数据以字节切片传入（windows crate 按切片长度自动计算 cbData）
    let data: &[u8] = unsafe {
        std::slice::from_raw_parts(
            value_wide.as_ptr() as *const u8,
            value_wide.len() * 2,
        )
    };
    unsafe {
        let rc = RegSetValueExW(
            key,
            PCWSTR(name_wide.as_ptr()),
            0,
            REG_SZ,
            Some(data),
        );
        if rc != WIN32_ERROR(0) {
            return Err(format!("写入注册表 {name} 失败: {}", rc.0));
        }
    }
    Ok(())
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
        if open != WIN32_ERROR(0) {
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

        if query != WIN32_ERROR(0) || typ != REG_SZ {
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

/// 把文件移入**回收站**（可撤销），语义等同于资源管理器里按 Delete。
///
/// 壁纸目录通常是用户自己长期积攒的图，误删后不该无法挽回，因此不走
/// `std::fs::remove_file`（那是永久抹除）。这里调用 shell 的
/// `SHFileOperationW` + `FOF_ALLOWUNDO`，文件进入回收站，用户可自行还原。
///
/// 几个必须注意的点：
/// - `pFrom` 要求**绝对路径**，且必须以**双 NUL** 结尾（单 NUL 结尾会被当成
///   多文件列表的第一项而继续读下去）。
/// - `SHFileOperationW` **不接受 `\\?\` 前缀**的 verbatim 路径，传进去会失败，
///   所以这里刻意不用 `std::fs::canonicalize`（它返回的正是 verbatim 形式）。
/// - 该 API 不弹 UI（`FOF_SILENT | FOF_NOERRORUI`），错误码是 shell 的旧式
///   非 Win32 错误码。**返回码不完全可靠**（见下方结果判定），仅用于日志。
#[cfg(windows)]
fn move_to_recycle_bin(p: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{BOOL, HWND};
    use windows::Win32::UI::Shell::{
        SHFileOperationW, SHFILEOPSTRUCTW, FO_DELETE, FOF_ALLOWUNDO, FOF_NOERRORUI,
        FOF_NOCONFIRMATION, FOF_SILENT,
    };

    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("解析当前目录失败：{e}"))?
            .join(p)
    };
    // 防御：调用方若传入 verbatim 路径（\\?\C:\... / \\?\UNC\server\share），
    // 先还原成 shell 能识别的普通形式
    let abs = {
        let s = abs.to_string_lossy();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => match rest.strip_prefix(r"UNC\") {
                Some(unc) => PathBuf::from(format!(r"\\{unc}")),
                None => PathBuf::from(rest),
            },
            None => abs,
        }
    };

    // 先确认文件确实存在：下面的结果判定以"源文件消失"为成功依据，
    // 若调用前文件就不在，就无法区分"被我们移走"和"本来就没有"。
    if !abs.exists() {
        return Err("文件不存在或已被移动".into());
    }

    let mut from: Vec<u16> = abs.as_os_str().encode_wide().collect();
    from.push(0);
    from.push(0);

    let mut op = SHFILEOPSTRUCTW {
        hwnd: HWND::default(),
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        pTo: PCWSTR::null(),
        // 该字段在 windows-rs 中声明为 u16，而 FILEOPERATION_FLAGS 是 u32 新类型
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI).0 as u16,
        fAnyOperationsAborted: BOOL(0),
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: PCWSTR::null(),
    };

    // SAFETY: `from` 在本次调用期间保持存活且以双 NUL 结尾；其余字段均为
    // 空指针 / 默认值，符合 SHFILEOPSTRUCTW 对单文件删除的要求。
    let rc = unsafe { SHFileOperationW(&mut op) };
    let aborted = op.fAnyOperationsAborted.as_bool();

    // **结果以文件系统状态为准，而不是返回码。**
    // 实测部分卷（本机 D:）上，文件已被正确移入回收站，`SHFileOperationW`
    // 却仍返回 0x2（ERROR_FILE_NOT_FOUND）。若只看返回码，前端会误报"删除失败"，
    // 于是把已经不在磁盘上的条目继续留在列表里、缩略图缓存也不清理。
    // 因此只要源文件确实消失了就算成功，异常返回码仅记录日志备查。
    if !abs.exists() {
        if rc != 0 {
            eprintln!(
                "[warn] SHFileOperationW 返回 0x{rc:X}，但文件已移入回收站：{}",
                abs.display()
            );
        }
        return Ok(());
    }

    if rc != 0 {
        return Err(format!("移入回收站失败（shell 错误码 0x{rc:X}）"));
    }
    if aborted {
        return Err("移入回收站被取消".into());
    }
    Err("移入回收站失败：文件仍存在于原位置".into())
}

/// 非 Windows 平台的兜底（本项目只发布 Windows 版，此分支仅保证可编译）
#[cfg(not(windows))]
fn move_to_recycle_bin(p: &Path) -> Result<(), String> {
    std::fs::remove_file(p).map_err(|e| format!("删除失败：{e}"))
}

/// 删除壁纸文件：**移入回收站**而非永久抹除（仅限支持的图片/视频扩展名）
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
    // 列表已放行视频，删除同步放行（否则列表里的视频无法删除）；
    // 系统目录拦截在最前，只读约束不受影响
    if !is_supported_wallpaper_ext(&ext) {
        return Err("不支持的壁纸文件类型".into());
    }

    move_to_recycle_bin(&p)
}

// ============================================================================
// 壁纸模糊遮罩效果（MVP）
// ============================================================================

/// 壁纸模糊遮罩效果参数（预览与设为壁纸共用）
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct WallpaperEffect {
    /// 是否启用效果
    pub enabled: bool,
    /// 高斯模糊强度（sigma，0~30）
    pub blur: u32,
    /// 遮罩不透明度（百分比，0~80）
    pub opacity: u32,
    /// 遮罩颜色（十六进制 #RRGGBB，默认黑色）
    pub color: String,
}

/// 解析 #RRGGBB 颜色字符串，非法时回退黑色
fn parse_hex_color(s: &str) -> [u8; 3] {
    let t = s.trim().trim_start_matches('#');
    if t.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&t[0..2], 16),
            u8::from_str_radix(&t[2..4], 16),
            u8::from_str_radix(&t[4..6], 16),
        ) {
            return [r, g, b];
        }
    }
    [0, 0, 0]
}

/// FNV-1a 64 位稳定哈希（用于效果缓存文件名；进程间稳定，不随随机种子变化）
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 将原图按主屏尺寸合成为"模糊 + 遮罩"壁纸图，返回合成图路径。
///
/// 处理流程（不修改原图）：
/// 1. 解码原图（jpg/png/bmp/webp）
/// 2. 按主屏物理分辨率等比裁剪填充（覆盖全屏，与"填充"样式一致）
/// 3. 高斯模糊（blur=0 时跳过）
/// 4. 叠加半透明遮罩（opacity=0 时跳过，颜色取 effect.color）
/// 5. JPEG 编码写入 app_cache_dir/effects/，同参数幂等复用，返回绝对路径
pub fn compose_wallpaper_effect(
    app: &tauri::AppHandle,
    path: &str,
    effect: WallpaperEffect,
) -> Result<String, String> {
    use tauri::Manager;

    // 1. 解码原图
    let img = image::open(path).map_err(|e| format!("读取壁纸失败：{e}"))?;
    let img = img.to_rgba8();
    if img.width() == 0 || img.height() == 0 {
        return Err("壁纸尺寸异常".into());
    }

    // 2. 主屏物理分辨率（封顶 3840 长边，避免超大屏/高 DPI 合成过慢）
    let mon = app
        .primary_monitor()
        .map_err(|e| format!("读取屏幕信息失败：{e}"))?
        .ok_or_else(|| "未检测到显示器".to_string())?;
    let size = mon.size();
    let mut tw = size.width;
    let mut th = size.height;
    let longest = tw.max(th);
    if longest > 3840 {
        let k = 3840.0 / longest as f32;
        tw = (tw as f32 * k).round() as u32;
        th = (th as f32 * k).round() as u32;
    }
    if tw == 0 || th == 0 {
        return Err("屏幕分辨率异常".into());
    }

    // 等比放大覆盖 + 居中裁剪（fill 语义）
    let scale = (tw as f32 / img.width() as f32).max(th as f32 / img.height() as f32);
    let nw = (img.width() as f32 * scale).round() as u32;
    let nh = (img.height() as f32 * scale).round() as u32;
    let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
    let x = (nw - tw) / 2;
    let y = (nh - th) / 2;
    let cropped = image::imageops::crop_imm(&resized, x, y, tw, th).to_image();

    // 3. 高斯模糊
    let mut out = if effect.blur > 0 {
        image::imageops::blur(&cropped, effect.blur.min(30) as f32)
    } else {
        cropped
    };

    // 4. 遮罩叠加（逐像素混合，等价于 color alpha 混合）
    if effect.opacity > 0 {
        let [cr, cg, cb] = parse_hex_color(&effect.color);
        let a = (effect.opacity.min(80).min(100) * 255 / 100) as u32;
        let keep = 255u32 - a;
        for p in out.pixels_mut() {
            p[0] = ((p[0] as u32 * keep + cr as u32 * a) / 255) as u8;
            p[1] = ((p[1] as u32 * keep + cg as u32 * a) / 255) as u8;
            p[2] = ((p[2] as u32 * keep + cb as u32 * a) / 255) as u8;
            p[3] = 255;
        }
    }

    // 5. 写缓存（幂等复用）
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("读取缓存目录失败：{e}"))?
        .join("effects");
    std::fs::create_dir_all(&cache).map_err(|e| format!("创建效果缓存目录失败：{e}"))?;
    let hash = fnv1a64(path.as_bytes());
    let color_tag = effect.color.trim().trim_start_matches('#').to_ascii_lowercase();
    let name = format!("effect_{hash:x}_{}_{}_{}.jpg", effect.blur, effect.opacity, color_tag);
    let out_path = cache.join(&name);

    if !out_path.exists() {
        let rgb = image::DynamicImage::ImageRgba8(out).to_rgb8();
        let file = std::fs::File::create(&out_path)
            .map_err(|e| format!("创建效果缓存文件失败：{e}"))?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 90);
        enc.encode(
            &rgb,
            tw,
            th,
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("合成壁纸编码失败：{e}"))?;
    }

    Ok(out_path.to_string_lossy().replace('/', "\\"))
}
