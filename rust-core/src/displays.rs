// 显示器枚举与稳定标识。
// 内置屏 / 有 EDID 序列号的外接屏使用跨拔插稳定的 ID；无序列号退回临时 ID。
// 运行时再映射回当前 CGDirectDisplayID。全部 CoreGraphics 调用线程安全。

use objc2_app_kit::NSScreen;
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayIsActive, CGDisplayIsAsleep, CGDisplayIsBuiltin,
    CGDisplayIsInMirrorSet, CGDisplayIsMain, CGDisplayPixelsHigh, CGDisplayPixelsWide,
    CGDisplaySerialNumber, CGDisplayVendorNumber, CGGetActiveDisplayList,
};
use objc2_foundation::{MainThreadMarker, NSString};

use crate::types::DisplayInfo;

pub fn active_displays() -> Vec<CGDirectDisplayID> {
    let mut capacity = 8usize;
    loop {
        let mut ids = vec![0u32; capacity];
        let mut count: u32 = 0;
        let err = unsafe { CGGetActiveDisplayList(capacity as u32, ids.as_mut_ptr(), &mut count) };
        if err.0 != 0 {
            return Vec::new();
        }
        if count as usize > capacity {
            capacity = count as usize;
            continue;
        }
        ids.truncate(count as usize);
        return ids
            .into_iter()
            .filter(|id| CGDisplayIsActive(*id))
            .collect();
    }
}

/// 稳定显示器 ID：内置屏固定为 builtin；有 EDID 序列号的外接屏用厂商+序列号
/// （跨拔插/重启稳定）；无序列号的退化为 cgdisplay 临时 ID（仅本次连接内有效）。
pub fn stable_id(display: CGDirectDisplayID) -> String {
    stable_id_of(display).0
}

/// 返回 (显示器 ID, 是否临时 ID)。无序列号时 ID 只在本次连接内有效。
pub fn stable_id_of(display: CGDirectDisplayID) -> (String, bool) {
    id_of_parts(
        CGDisplayIsBuiltin(display),
        CGDisplayVendorNumber(display),
        CGDisplaySerialNumber(display),
        display,
    )
}

/// ID 组装规则单独拆出：与 CoreGraphics 的查询解耦后才能离线单测
pub(crate) fn id_of_parts(
    builtin: bool,
    vendor: u32,
    serial: u32,
    display: CGDirectDisplayID,
) -> (String, bool) {
    if builtin {
        return ("builtin".to_string(), false);
    }
    if serial != 0 {
        return (format!("disp-{vendor:x}-{serial:x}"), false);
    }
    (format!("cgdisplay-{display}"), true)
}

/// 稳定 ID -> 当前活动桌面的 CGDisplayID
pub fn cg_id_for_stable(stable: &str) -> Option<CGDirectDisplayID> {
    active_displays()
        .into_iter()
        .find(|id| stable_id(*id) == stable)
}

pub fn logical_bounds(display: CGDirectDisplayID) -> (f64, f64, f64, f64) {
    let b = CGDisplayBounds(display);
    (b.origin.x, b.origin.y, b.size.width, b.size.height)
}

pub fn is_asleep(display: CGDirectDisplayID) -> bool {
    CGDisplayIsAsleep(display)
}

pub fn is_main(display: CGDirectDisplayID) -> bool {
    CGDisplayIsMain(display)
}

/// 在 AppKit 主线程上读取 NSScreen 的 backingScaleFactor。
pub fn enumerate_on_main(mtm: &MainThreadMarker) -> Vec<DisplayInfo> {
    let screens = NSScreen::screens(*mtm);
    let mut scale_by_display = std::collections::HashMap::<CGDirectDisplayID, f64>::new();
    for i in 0..screens.count() {
        let screen = screens.objectAtIndex(i);
        let Some(number) = screen
            .deviceDescription()
            .objectForKey(&NSString::from_str("NSScreenNumber"))
        else {
            continue;
        };
        let display_id: CGDirectDisplayID = unsafe { objc2::msg_send![&*number, unsignedIntValue] };
        let scale = screen.backingScaleFactor();
        if scale.is_finite() && scale > 0.0 {
            scale_by_display.insert(display_id, scale);
        }
    }
    enumerate_with_scales(&scale_by_display)
}

/// 非 AppKit 调用方的保底枚举。正常 UI 命令通过 `enumerate_on_main`，
/// 这里保留 CoreGraphics 路径避免启动早期或测试环境因主线程标记缺失而失败。
pub fn enumerate() -> Vec<DisplayInfo> {
    enumerate_with_scales(&std::collections::HashMap::new())
}

/// 线程安全的枚举入口：尝试主线程，失败回退到 CG-only 路径。
pub fn enumerate_on_main_safe() -> Vec<DisplayInfo> {
    if let Ok(result) = crate::runtime::on_main(enumerate_on_main) {
        return result;
    }
    enumerate()
}

fn enumerate_with_scales(
    scale_by_display: &std::collections::HashMap<CGDirectDisplayID, f64>,
) -> Vec<DisplayInfo> {
    let mut displays = active_displays();
    displays.sort_by_key(|id| (!is_main(*id), stable_id(*id)));
    let total = displays.len();
    displays
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let bounds = logical_bounds(*id);
            let mode_pixels = || {
                objc2_core_graphics::CGDisplayCopyDisplayMode(*id)
                    .map(|mode| {
                        (
                            objc2_core_graphics::CGDisplayMode::pixel_width(Some(&mode)) as u32,
                            objc2_core_graphics::CGDisplayMode::pixel_height(Some(&mode)) as u32,
                        )
                    })
                    .filter(|(w, h)| *w > 0 && *h > 0)
                    .unwrap_or((
                        CGDisplayPixelsWide(*id) as u32,
                        CGDisplayPixelsHigh(*id) as u32,
                    ))
            };
            let scale = scale_by_display.get(id).copied().unwrap_or_else(|| {
                let (mode_w, _) = mode_pixels();
                if bounds.2 > 0.0 {
                    mode_w as f64 / bounds.2
                } else {
                    1.0
                }
            });
            let (pixel_w, pixel_h) = if scale_by_display.contains_key(id) {
                (
                    (bounds.2 * scale).round().max(1.0) as u32,
                    (bounds.3 * scale).round().max(1.0) as u32,
                )
            } else {
                mode_pixels()
            };
            let primary = is_main(*id);
            let (stable, temporary) = stable_id_of(*id);
            DisplayInfo {
                id: stable,
                name: if primary {
                    "主显示器".to_string()
                } else {
                    format!("显示器 {}（共 {total} 块）", i + 1)
                },
                logical_bounds: bounds,
                pixel_width: pixel_w,
                pixel_height: pixel_h,
                scale_factor: scale,
                primary,
                mirrored: CGDisplayIsInMirrorSet(*id),
                temporary,
            }
        })
        .collect()
}
