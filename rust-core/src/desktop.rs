// macOS 桌面静态壁纸与屏幕工具（仅主线程访问 AppKit）。
// - 设置：NSWorkspace.setDesktopImageURL（fill=比例缩放+允许裁剪；fit=比例缩放）
// - 真值读取：desktopImageURLForScreen
// - 目录/文件选择：原生 NSOpenPanel

use std::path::Path;

use objc2::rc::Retained;
use objc2_app_kit::{
    NSModalResponseOK, NSOpenPanel, NSScreen, NSWorkspace, NSWorkspaceDesktopImageAllowClippingKey,
    NSWorkspaceDesktopImageScalingKey,
};
use objc2_foundation::{MainThreadMarker, NSMutableDictionary, NSNumber, NSString, NSURL};

use crate::displays;
use crate::types::FitMode;

pub fn ns_url_for_path(path: &str) -> Retained<NSURL> {
    let s = NSString::from_str(path);
    NSURL::fileURLWithPath_isDirectory(&s, false)
}

/// 稳定显示器 ID -> NSScreen
pub fn screen_for_stable_id(mtm: &MainThreadMarker, stable: &str) -> Option<Retained<NSScreen>> {
    let screens = NSScreen::screens(*mtm);
    for i in 0..screens.count() {
        let screen = screens.objectAtIndex(i);
        let Some(number) = screen
            .deviceDescription()
            .objectForKey(&NSString::from_str("NSScreenNumber"))
        else {
            continue;
        };
        let cgid: u32 = unsafe { objc2::msg_send![&*number, unsignedIntValue] };
        if displays::stable_id(cgid) == stable {
            return Some(screen);
        }
    }
    None
}

/// 系统读取的当前静态壁纸路径；读不到时返回 None。
pub fn current_static_wallpaper(_mtm: &MainThreadMarker, screen: &NSScreen) -> Option<String> {
    let workspace = NSWorkspace::sharedWorkspace();
    workspace
        .desktopImageURLForScreen(screen)
        .and_then(|url| url.path())
        .map(|p| p.to_string())
}

/// 设置静态图片壁纸。同步结果以 AppKit 返回的 BOOL/NSError 为准。
pub fn set_static_wallpaper(
    _mtm: &MainThreadMarker,
    screen: &NSScreen,
    path: &str,
    fit: FitMode,
) -> Result<(), String> {
    let canonical = Path::new(path)
        .canonicalize()
        .map_err(|e| format!("路径规范化失败 {path}: {e}"))?;
    let url = ns_url_for_path(&canonical.to_string_lossy());

    // options 字典：NSWorkspaceImageScalingKey=3（比例缩放），fill 额外允许裁剪。
    // 接成 `Retained` 才有所有权：`new` 返回 +1 引用，之前每次换壁纸都漏一个字典。
    let options: Retained<NSMutableDictionary> =
        unsafe { objc2::msg_send![objc2::class!(NSMutableDictionary), new] };
    let scaling: Retained<NSNumber> = NSNumber::numberWithInt(3);
    let clipping: Retained<NSNumber> = NSNumber::numberWithBool(true);
    let workspace = NSWorkspace::sharedWorkspace();
    let mut error: *mut objc2_foundation::NSError = std::ptr::null_mut();
    let success: bool = unsafe {
        let _: () = objc2::msg_send![
            &*options,
            setObject: &*scaling,
            forKey: NSWorkspaceDesktopImageScalingKey
        ];
        if fit == FitMode::Fill {
            let _: () = objc2::msg_send![
                &*options,
                setObject: &*clipping,
                forKey: NSWorkspaceDesktopImageAllowClippingKey
            ];
        }
        objc2::msg_send![
            &*workspace,
            setDesktopImageURL: &*url,
            forScreen: screen,
            options: &*options,
            error: &mut error
        ]
    };
    if !success {
        let msg = if error.is_null() {
            "未知错误".to_string()
        } else {
            let e = unsafe { &*error };
            e.to_string()
        };
        return Err(format!("设置静态壁纸失败: {msg}"));
    }
    Ok(())
}

fn run_open_panel(mtm: &MainThreadMarker, directories: bool, multiple: bool) -> Vec<String> {
    let panel = NSOpenPanel::openPanel(*mtm);
    panel.setCanChooseDirectories(directories);
    panel.setCanChooseFiles(!directories);
    panel.setAllowsMultipleSelection(multiple);
    panel.setCanCreateDirectories(directories);
    let response = panel.runModal();
    if response != NSModalResponseOK {
        return Vec::new();
    }
    let urls = panel.URLs();
    (0..urls.count())
        .filter_map(|i| urls.objectAtIndex(i).path())
        .map(|p| p.to_string())
        .collect()
}

/// 原生目录选择器；取消时返回 None。
pub fn pick_directory(mtm: &MainThreadMarker) -> Option<String> {
    run_open_panel(mtm, true, false).into_iter().next()
}

/// 原生多文件选择器（导入用）。
pub fn pick_files(mtm: &MainThreadMarker) -> Vec<String> {
    run_open_panel(mtm, false, true)
}
