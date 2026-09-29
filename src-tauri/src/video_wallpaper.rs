// DotWallpaper 动态壁纸模块（含视频壁纸）
//
// 实现方案：置底窗口 + WebView 渲染 —— 为每个显示器创建一个全屏 WebviewWindow，
// 通过 Progman/WorkerW 挂载链塞到桌面图标之下，前端播放页按 kind 渲染内容。
// 后续 GIF/网页类型只需扩展前端 kind 分支，本模块窗口挂载逻辑完全复用。
//
// 关于闪屏（本模块最贵的一课 —— **改窗口相关代码前先读这段**）：
//   **唯一有效原则：就位之后一次都不碰。** `SetParent` / `SetWindowPos` / `show()`
//   只要被**调用**（哪怕参数与当前状态完全相同），Windows 就会重建该窗口的绘制
//   表面并触发整窗重绘 —— 正在播放的视频就会可见地闪一下。所以"先判断是否幂等、
//   相同就跳过"是**不够的**，必须做到"绝不调用"。
//
//   已踩过的四个成因（每一个都单独造成过可见闪屏）：
//     ① 早期每 4s 无条件执行 SetParent + SetWindowPos；
//     ② 巡检用"父窗口 == 本次找到的 WorkerW"判定脱挂 —— 这要求挂载那次与巡检这次
//        两次独立查找返回同一个句柄，不一致就**每轮**重挂；
//     ③ 巡检调用的 `attach_to_desktop` 内部用的是带 `WM_SPAWN_WORKERW`(0x052C)
//        兜底的查找，一旦重挂就会**重排整个桌面**；
//     ④ 巡检线程随"停止 → 再启用"被重复创建，多个线程各自判定、各自可能碰窗口。
//
//   现做法：`SETTLED` 登记 + `all_settled` 整轮短路 + 只看父窗口句柄是否有效 +
//   `allow_spawn` 开关（巡检只读查找）+ 巡检线程只启动一次（`WATCHER_STARTED`）。
//   机制细节见 `memory/video-wallpaper.md`。

use std::collections::HashSet;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetParent, GetWindowLongPtrW,
    GetWindowRect, IsWindow, IsWindowVisible, SendMessageTimeoutW, SetParent, SetWindowLongPtrW,
    SetWindowPos, GWL_EXSTYLE, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOREDRAW, SWP_NOSENDCHANGING, SWP_NOSIZE, WS_EX_NOACTIVATE,
};

/// 视频扩展名判定**只有一份**（在 `wallpaper.rs`），这里原样再导出。
///
/// 曾在此文件与 `wallpaper.rs` 各存一份 4 元素数组 —— 那正是"一处放行、
/// 另一处拦截"这类不一致的温床（拖入只认图片的 bug 就是这么来的）。
/// 口径见 `wallpaper.rs` 的 `SUPPORTED_VIDEO_EXTS`：**放行集**（4 个）用于
/// 扫描 / 删除 / 设置，**导入集**（图片 + mp4/webm）只用于拖入。
pub use crate::wallpaper::is_supported_video_ext;

/// 动态壁纸窗口 label 前缀（多显示器：wallpaper-video-0/1/2...）
const VIDEO_WINDOW_LABEL_PREFIX: &str = "wallpaper-video";
/// 播放页 view 参数：前端 App.vue 据此分流渲染（不渲染主界面）
const VIDEO_VIEW: &str = "index.html?view=video";
/// 置底窗口重挂检测间隔（explorer 重启重建 WorkerW 后自动恢复）
const RELINK_INTERVAL_SECS: u64 = 4;

/// 下发给播放页的**状态事件名**。
///
/// 换视频 / 静音 / 暂停 / 恢复 全部走这一条事件、携带完整状态快照。
/// 曾用只带 `path` 的 `video-wallpaper-set`：那样每加一种可变项就得再加一个
/// 事件名与一个前端 listener，漏一个就是"静默不生效"（这个模块已经因为
/// 漏监听踩过一次坑，界面还完全无报错）。统一成状态快照后，播放页只有
/// **一个收敛入口**，新增字段不会漏。
const VIDEO_STATE_EVENT: &str = "video-wallpaper-state";

// ---------- 全局状态 ----------

#[derive(Clone)]
struct VideoWallpaperState {
    enabled: bool,
    path: String,
    /// 选中的显示器索引（`available_monitors()` 的下标）；空表示"全部显示器"。
    /// 用 Vec 保存以便持久化与顺序稳定比较。
    monitors: Vec<usize>,
    /// 静音播放。这是**用户偏好**而非临时状态：换视频、改播放范围、停止动态壁纸
    /// 都保留它。持久化落在前端 localStorage（`dot-wallpaper-video-muted`），
    /// 每次 `set_video_wallpaper` 都会把偏好带过来 —— 后端状态是进程内的，
    /// 应用重启后回到默认值，而播放页读的是后端状态，不带就会"上次静音了、这次又有声音"。
    muted: bool,
    /// 是否暂停（冻结在当前帧，恢复时从原处继续）。
    /// **不持久化**：重启后本就没有动态壁纸，存它没有意义。
    /// 切换视频会复位为 false（换了片子就该开始播），只改播放范围则保持。
    paused: bool,
}

fn state() -> &'static Mutex<VideoWallpaperState> {
    static STATE: OnceLock<Mutex<VideoWallpaperState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(VideoWallpaperState {
            enabled: false,
            path: String::new(),
            monitors: Vec::new(),
            muted: false,
            paused: false,
        })
    })
}

/// 巡检线程**是否已启动过**。
///
/// 它是一个"只启动一次"的闸门，**不是"当前是否在运行"的开关** ——
/// 线程随应用存活、不再停止：`stop_video_wallpaper` 会清空 `watch_hwnds()`，
/// 于是巡检每轮在 `hwnds.is_empty()` 处直接短路，代价可忽略。
///
/// 为什么不随"停止"而退出：曾用"停止时置 false、启用时置 true"的写法，
/// 而旧线程还在 `sleep` 里 —— 它醒来看到 true 就以为该继续，**于是旧线程不退出**，
/// 每经历一次"停止 → 再启用"就多一个巡检线程。多个线程各自判定 `all_settled`、
/// 各自可能去碰窗口，正是"周期性闪屏"的隐患。
static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);
/// 当前各置底窗口句柄（重挂线程轮询读取；每显示器一个）
static WATCH_HWNDS: OnceLock<Mutex<Vec<isize>>> = OnceLock::new();
fn watch_hwnds() -> &'static Mutex<Vec<isize>> {
    WATCH_HWNDS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 已"就位"的窗口集合：一旦某窗口完成首次挂载 + 几何设置，就记入本表。
/// 重挂线程对已就位窗口**只读校验、绝不写入** —— 这是消除残留闪屏的关键。
///
/// 为什么需要它：`SetParent`/`SetWindowPos`/`ShowWindow` 只要被调用（哪怕参数
/// 与当前状态完全相同），Windows 都会重建该窗口的绘制表面并触发整窗重绘，
/// 正在播放的视频就会可见地闪一下。所以"幂等调用"本身不够，
/// 必须做到"**就位后一次都不碰**"。
static SETTLED: OnceLock<Mutex<HashSet<isize>>> = OnceLock::new();
fn settled() -> &'static Mutex<HashSet<isize>> {
    SETTLED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 标记窗口已就位（首次挂载成功后调用）
fn mark_settled(hwnd: HWND) {
    settled().lock().unwrap().insert(hwnd.0 as isize);
}

/// 窗口是否已就位
fn is_settled(hwnd: HWND) -> bool {
    settled().lock().unwrap().contains(&(hwnd.0 as isize))
}

/// 清除就位标记（窗口重建 / 停止动态壁纸时调用，使新窗口能重新挂载）
fn unmark_settled(hwnd: isize) {
    settled().lock().unwrap().remove(&hwnd);
}

/// 清空全部就位标记
fn clear_settled() {
    settled().lock().unwrap().clear();
}

/// 动态壁纸状态快照。
///
/// 同时用于两处：`get_video_wallpaper_state` 的返回值，以及
/// `VIDEO_STATE_EVENT` 事件负载。**刻意共用一个类型** —— 播放页挂载时的
/// "拉取兜底"与之后的"事件推送"因此可以走同一个收敛函数，
/// 不会出现两条路径字段不一致（少一个字段就少一处生效）。
#[derive(serde::Serialize, Clone)]
pub struct VideoWallpaperStateOut {
    pub enabled: bool,
    pub path: String,
    /// 当前生效的显示器索引（空 = 全部）
    pub monitors: Vec<usize>,
    /// 静音播放
    pub muted: bool,
    /// 是否暂停
    pub paused: bool,
}

/// 显示器描述（供前端渲染选择列表）
#[derive(serde::Serialize, Clone)]
pub struct MonitorInfo {
    /// `available_monitors()` 中的下标，作为稳定标识
    pub index: usize,
    /// 展示名（含分辨率与主屏标记）。
    /// 保留字段是为了兼容旧前端；新前端请用 `label` + `resolution` + `scale` 自行组合。
    pub name: String,
    /// **通俗展示名**（不含分辨率/缩放，也不含方位），如"DELL U2720Q""显示器 2"。
    /// 前端直接显示即可，不需要再解析分隔符。
    /// **刻意不带"（主）"这类标记**：方位统一由 `position_hint` 表达，
    /// 否则主屏与其它屏的后缀风格不一致，下拉里长短不齐。
    pub label: String,
    /// 分辨率文本，如"3840×2160"（已含 × 号，前端直接显示）
    pub resolution: String,
    /// 相对主屏的方位，**可直接展示的短词**："主屏"/"左侧"/"右侧"/"上方"/"下方"。
    /// 前端拼成 `<label> · <position_hint>` 即可，无需解析字符串。仅单屏时为 None。
    pub position_hint: Option<String>,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub is_primary: bool,
}

// ---------- Win32 挂载链 ----------
// 各窗口的父窗口（WorkerW）在挂载后记录，供几何换算与重挂线程复用。

/// 将 UTF-8 字符串转为 UTF-16 宽字符 C 字符串（供 Win32 使用）
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// EnumWindows 回调上下文：收集找到的 WorkerW + 目标类名
struct EnumCtx {
    found: *mut Option<HWND>,
    def_view: PCWSTR,
}

/// 查找桌面图标层之下的背景 WorkerW：
/// 1) EnumWindows 直接找"类名 WorkerW 且含 SHELLDLL_DefView"的图标层，取其父；
/// 2) Win11 退化：Progman 下不含 DefView 的可见 WorkerW 即壁纸层；
/// 3) 仍找不到才 SendMessageW 0x052C（WM_SPAWN_WORKERW）触发生成，再枚举一次。
///
/// 修复要点：**Progman 找不到也必须继续尝试**。历史实现在 FindWindowW("Progman")
/// 失败时直接 return None，导致窗口完全无法挂到桌面层（浮在普通顶层），
/// 表现为"多显示器有些屏根本不显示壁纸"。
fn find_desktop_workerw() -> Option<HWND> {
    /// EnumWindows 回调：命中"含 SHELLDLL_DefView 的 WorkerW"时记录其父并停止枚举
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut EnumCtx);
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]);
            if class == "WorkerW" {
                let Ok(def_view) = FindWindowExW(hwnd, None, ctx.def_view, PCWSTR::null()) else {
                    return BOOL(1);
                };
                if !def_view.0.is_null() {
                    let Ok(parent) = GetParent(hwnd) else {
                        return BOOL(1);
                    };
                    let target = if !parent.0.is_null() { parent } else { hwnd };
                    *ctx.found = Some(target);
                    return BOOL(0); // 找到即停止枚举
                }
            }
        }
        BOOL(1)
    }

    /// 执行一次全量枚举，返回命中结果
    unsafe fn enum_for_defview(ctx: &mut EnumCtx) {
        let _ = EnumWindows(Some(enum_proc), LPARAM((ctx) as *mut EnumCtx as isize));
    }

    let progman_name = wide("Progman");
    let def_view_name = wide("SHELLDLL_DefView");
    let workerw_name = wide("WorkerW");

    // Progman 可能取不到（部分 shell 环境/时机），用 Option 承载并在后续分支兜底
    let progman: Option<HWND> = unsafe {
        match FindWindowW(PCWSTR(progman_name.as_ptr()), PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    };

    unsafe {
        // ---- 第 1 步：直接枚举（不发送任何消息，避免桌面重排 → 闪屏）----
        let mut found: Option<HWND> = None;
        let mut ctx = EnumCtx {
            found: &mut found,
            def_view: PCWSTR(def_view_name.as_ptr()),
        };
        enum_for_defview(&mut ctx);
        if found.is_some() {
            return found;
        }

        // ---- 第 2 步：Progman 下的可见 WorkerW（不含 DefView）即壁纸层 ----
        if let Some(progman) = progman {
            if let Ok(worker) =
                FindWindowExW(progman, None, PCWSTR(workerw_name.as_ptr()), PCWSTR::null())
            {
                if !worker.0.is_null() && IsWindowVisible(worker).as_bool() {
                    let has_def_view = match FindWindowExW(
                        worker,
                        None,
                        PCWSTR(def_view_name.as_ptr()),
                        PCWSTR::null(),
                    ) {
                        Ok(h) => !h.0.is_null(),
                        Err(_) => false,
                    };
                    if !has_def_view {
                        return Some(worker);
                    }
                }
            }

            // ---- 第 3 步：最后手段，发送 WM_SPAWN_WORKERW 生成桌面层后重试 ----
            // 该消息会触发桌面重排（可能瞬闪），故仅在前两步都失败时使用。
            let _ = SendMessageTimeoutW(
                progman,
                0x052C,
                windows::Win32::Foundation::WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                500,
                None,
            );
            let mut found2: Option<HWND> = None;
            let mut ctx2 = EnumCtx {
                found: &mut found2,
                def_view: PCWSTR(def_view_name.as_ptr()),
            };
            enum_for_defview(&mut ctx2);
            if found2.is_some() {
                return found2;
            }
            // 生成后 Progman 下可能出现新的可见 WorkerW，再查一次
            if let Ok(worker) =
                FindWindowExW(progman, None, PCWSTR(workerw_name.as_ptr()), PCWSTR::null())
            {
                if !worker.0.is_null() && IsWindowVisible(worker).as_bool() {
                    return Some(worker);
                }
            }
        }

        None
    }
}

/// 将窗口挂到桌面图标之下，返回实际父窗口（用于几何换算）。
///
/// 关键：**只在父窗口确实变化时才 SetParent**。
/// 对一个正在播放视频的窗口重复 SetParent 会重建绘制表面并整窗重绘，
/// 是此前"周期性闪屏"的主因。
///
/// `allow_spawn` 区分两条调用路径，**绝不能混用**：
/// - `true`：首次挂载（`on_page_load`）。此时桌面层可能还不存在，
///   允许 `find_desktop_workerw()` 发 `WM_SPAWN_WORKERW` 把它生出来。
/// - `false`：巡检路径。**必须走只读查找** —— 那条消息会让整个桌面重排，
///   正在播放的视频会整屏闪一下，而巡检每 4s 跑一次。
///   历史缺陷：巡检调用的也是本函数，而本函数内部一律用带 0x052C 兜底的
///   `find_desktop_workerw()` —— 注释里写着"巡检绝不发 0x052C"，代码却做不到。
fn attach_to_desktop(hwnd: HWND, allow_spawn: bool) -> Option<HWND> {
    unsafe {
        let worker = if allow_spawn {
            find_desktop_workerw()?
        } else {
            find_desktop_workerw_readonly()?
        };
        let cur_parent = GetParent(hwnd).unwrap_or(HWND::default());
        if cur_parent == worker {
            return Some(worker); // 已挂在目标父窗口下，无需重排
        }
        let _ = SetParent(hwnd, worker);
        // 挂载后沉到 WorkerW 客户区最底部（图标层之下）；SWP_NOREDRAW 抑制同步重绘。
        // 用 HWND_BOTTOM 而非 TOP：壁纸必须在桌面图标下面，TOP 会遮住图标。
        let _ = SetWindowPos(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM,
            0,
            0,
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSENDCHANGING | SWP_NOMOVE | SWP_NOSIZE | SWP_NOREDRAW,
        );
        Some(worker)
    }
}

/// 目标几何：**全部为物理像素**（SetWindowPos 对子窗口直接使用物理像素，
/// 与父窗口同处一个客户区坐标系，不能除以 DPI 缩放）。
///
/// 历史 bug：早期把宽高除以 scale 变成"逻辑值"，而 GetWindowRect 返回的是物理
/// 像素，导致 apply_geo_if_changed 里的相等判断**永远不成立**，每轮重挂线程
/// 都真的执行一次 SetWindowPos → 视频绘制表面重建 → 用户看到的周期性闪屏。
#[derive(Clone, Copy, PartialEq, Debug)]
struct Geo {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// 计算某个显示器铺满其所在区域所需的窗口几何（**物理像素**）。
///
/// 多显示器核心：不能统一用主屏尺寸。每个显示器有自己的物理位置与 DPI。
///
/// 坐标系说明（这是历史上闪屏/错位的根源，务必保持）：
/// - Tauri 的 `Monitor::position()` / `size()` 返回的是**物理像素**。
/// - 窗口一旦 SetParent 到 WorkerW，`SetWindowPos` 的坐标/尺寸就是
///   **父窗口客户区下的物理像素**（同 DPI 感知上下文），不再经过逻辑换算。
/// - 因此这里只做"减去父窗口屏幕原点"的平移，**绝不能除以 scale_factor**：
///   一除就会与 GetWindowRect 的物理值永远不等，触发每轮无条件重排 → 闪屏。
fn monitor_geo(app: &tauri::AppHandle, mon: &tauri::window::Monitor, parent: HWND) -> Option<Geo> {
    let _ = app; // 保留参数以便未来做混合 DPI 的窗口级 DPI 感知设置
    // WorkerW 客户区在屏幕上的原点（多屏时通常覆盖整个虚拟桌面，起点可能为负）
    let (px, py) = unsafe {
        let mut r = RECT::default();
        if GetWindowRect(parent, &mut r).is_ok() {
            (r.left, r.top)
        } else {
            (0, 0)
        }
    };

    let pos = mon.position();
    let size = mon.size();
    // 物理像素直接相减得到父窗口客户区坐标下的位置与尺寸
    let x = pos.x - px;
    let y = pos.y - py;
    let w = size.width as i32;
    let h = size.height as i32;
    if w <= 0 || h <= 0 {
        return None;
    }
    Some(Geo { x, y, w, h })
}

/// 仅当几何不一致时才调整窗口（避免每轮无谓 SetWindowPos 造成闪屏）。
///
/// 关键修复：`GetWindowRect` 返回**屏幕物理像素**，`geo` 也是物理像素，
/// 比较前先把窗口矩形换算成"父窗口客户区坐标"再比对，二者量纲一致才能正确短路。
/// 位置也参与比较（早期只比尺寸且除以了缩放，判断恒真 → 每 4s 重排一次 → 闪屏）。
fn apply_geo_if_changed(hwnd: HWND, geo: Geo, parent: HWND) {
    unsafe {
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_ok() {
            // 父窗口屏幕原点：把窗口屏幕坐标换成父客户区坐标
            let (px, py) = {
                let mut pr = RECT::default();
                if GetWindowRect(parent, &mut pr).is_ok() {
                    (pr.left, pr.top)
                } else {
                    (0, 0)
                }
            };
            let cur_x = wr.left - px;
            let cur_y = wr.top - py;
            let cur_w = wr.right - wr.left;
            let cur_h = wr.bottom - wr.top;
            if cur_x == geo.x && cur_y == geo.y && cur_w == geo.w && cur_h == geo.h {
                return; // 已完全就位，短路，不触碰窗口 → 视频表面不重建
            }
        }
        // HWND_BOTTOM：壁纸层必须沉到 WorkerW 客户区最底部，位于桌面图标(SHELLDLL_DefView)
        // 之下。用 HWND_TOP 会把视频盖到图标上面，遮挡桌面图标。
        let _ = SetWindowPos(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM,
            geo.x,
            geo.y,
            geo.w,
            geo.h,
            SWP_NOACTIVATE | SWP_NOSENDCHANGING | SWP_NOREDRAW,
        );
    }
}

/// 置为不抢焦点（WS_EX_NOACTIVATE）：动态壁纸窗口不打断用户操作
fn set_no_activate(hwnd: HWND) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let want = (ex as u32) | WS_EX_NOACTIVATE.0;
        if ex as u32 != want {
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want as isize);
        }
    }
}

/// 巡检专用的 **只读** WorkerW 查找：绝不发送 `WM_SPAWN_WORKERW`。
///
/// `find_desktop_workerw()` 的兜底第 3 步会发送 0x052C 触发生成桌面层，
/// 而该消息会让整个桌面重排 —— 正在播放的视频会整屏闪一下。
/// 巡检每 4s 跑一次，绝不能带这种副作用；找不到就返回 None，
/// 由调用方按"父窗口非空 = 仍挂在桌面层"保守处理，等下一轮再判断。
fn find_desktop_workerw_readonly() -> Option<HWND> {
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut EnumCtx);
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        if len > 0 && String::from_utf16_lossy(&buf[..len as usize]) == "WorkerW" {
            let Ok(def_view) = FindWindowExW(hwnd, None, ctx.def_view, PCWSTR::null()) else {
                return BOOL(1);
            };
            if !def_view.0.is_null() {
                let Ok(parent) = GetParent(hwnd) else {
                    return BOOL(1);
                };
                *ctx.found = Some(if !parent.0.is_null() { parent } else { hwnd });
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let def_view_name = wide("SHELLDLL_DefView");
    unsafe {
        let mut found: Option<HWND> = None;
        let mut ctx = EnumCtx {
            found: &mut found,
            def_view: PCWSTR(def_view_name.as_ptr()),
        };
        let _ = EnumWindows(Some(enum_proc), LPARAM((&mut ctx) as *mut EnumCtx as isize));
        if found.is_some() {
            return found;
        }
    }
    // 退化查法：Progman 下不含 DefView 的可见 WorkerW（同样只读，不发消息）
    let progman_name = wide("Progman");
    let workerw_name = wide("WorkerW");
    unsafe {
        let progman = match FindWindowW(PCWSTR(progman_name.as_ptr()), PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => h,
            _ => return None,
        };
        if let Ok(worker) =
            FindWindowExW(progman, None, PCWSTR(workerw_name.as_ptr()), PCWSTR::null())
        {
            if !worker.0.is_null() && IsWindowVisible(worker).as_bool() {
                let has_def_view = match FindWindowExW(
                    worker,
                    None,
                    PCWSTR(def_view_name.as_ptr()),
                    PCWSTR::null(),
                ) {
                    Ok(h) => !h.0.is_null(),
                    Err(_) => false,
                };
                if !has_def_view {
                    return Some(worker);
                }
            }
        }
    }
    None
}

/// 启动后台巡检线程：只做**只读校验**，发现窗口真的脱离桌面层（explorer 重启
/// 重建 WorkerW）才重新挂载；已就位且父窗口正常的窗口**一次都不触碰**。
///
/// 这是消除残留闪屏的核心：任何 `SetParent`/`SetWindowPos`/`show()` 调用都会让
/// Windows 重建该窗口绘制表面，正在播放的视频就会闪。因此巡检必须"零副作用"。
///
/// **全应用只会有一个巡检线程**：闸门是 `WATCHER_STARTED`，启动过就永远直接返回。
/// 本函数可以被反复调用（每次 `set_video_wallpaper` 都会调），这是安全的。
fn spawn_relink_watcher(app: tauri::AppHandle) {
    if WATCHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(RELINK_INTERVAL_SECS));
        // 没有终止条件：线程随应用存活。停止动态壁纸后 `watch_hwnds()` 被清空，
        // 下面这行会直接短路，所以空转的代价可忽略（见 `WATCHER_STARTED` 的注释）。
        let hwnds: Vec<isize> = watch_hwnds().lock().unwrap().clone();
        if hwnds.is_empty() {
            continue;
        }
        let monitors = app.available_monitors().unwrap_or_default();

        // 全部窗口都已就位且句柄有效 → 跳过所有探测（最省，且绝无副作用）。
        // 注意：热插拔补齐不在此 gate 内，见循环末尾。
        let all_settled = hwnds
            .iter()
            .all(|h| *h != 0 && is_settled(HWND(*h as *mut c_void)));

        if !all_settled {
            for (i, h) in hwnds.iter().enumerate() {
                if *h == 0 {
                    continue;
                }
                let hwnd = HWND(*h as *mut c_void);
                if !unsafe { IsWindow(hwnd) }.as_bool() {
                    // 窗口没了（explorer 重启时父窗口被销毁会连带销毁子窗口）：
                    // 清掉就位标记，**并把登记句柄置 0** —— 否则下面的热插拔补齐
                    // 会因为"记录里还有一个非 0 句柄"而认为窗口存在，永远不重建，
                    // 也就是"explorer 重启后动态壁纸再也不回来"。
                    unmark_settled(*h);
                    let mut v = watch_hwnds().lock().unwrap();
                    if i < v.len() {
                        v[i] = 0;
                    }
                    continue;
                }

                // ---- 只读校验：**唯一可靠且零副作用的信号是"父窗口句柄还有效"** ----
                // 刻意**不**比对"父窗口 == 本次找到的 WorkerW"。那个比较要求
                // 两次独立查找（挂载时那次、巡检这次）返回同一个句柄，一旦不一致
                // （explorer 重建过、机器上同时存在多个 WorkerW）就会**每轮**都判定
                // "脱挂"并重挂 → SetParent + SetWindowPos 每 4s 一次 → 周期性闪屏。
                // 父窗口被销毁时句柄自然失效，那才是真正需要重挂的时机。
                let cur_parent = unsafe { GetParent(hwnd).unwrap_or(HWND::default()) };
                let still_attached =
                    !cur_parent.0.is_null() && unsafe { IsWindow(cur_parent) }.as_bool();

                if still_attached {
                    // 已就位：不做任何写操作。仅当尺寸真的被外部改坏时才纠正。
                    if is_settled(hwnd) {
                        continue;
                    }
                    // 未标记就位（首次巡检）：补一次几何后标记。
                    // 几何基准用**真实父窗口**，而不是再去找一次 WorkerW ——
                    // 少一次查找就少一次"两次结果不一致"的机会。
                    if let Some(mon) = monitors.get(i) {
                        if let Some(geo) = monitor_geo(&app, mon, cur_parent) {
                            apply_geo_if_changed(hwnd, geo, cur_parent);
                        }
                        mark_settled(hwnd);
                    }
                    continue;
                }

                // ---- 父窗口确实失效（explorer 重启等）→ 重新挂载 ----
                // `allow_spawn = false`：巡检**绝不能**发 WM_SPAWN_WORKERW，
                // 那条消息会让整个桌面重排 → 正在播放的视频整屏闪一下。
                unmark_settled(*h);
                let Some(parent) = attach_to_desktop(hwnd, false) else {
                    continue;
                };
                if let Some(mon) = monitors.get(i) {
                    if let Some(geo) = monitor_geo(&app, mon, parent) {
                        apply_geo_if_changed(hwnd, geo, parent);
                    }
                }
                mark_settled(hwnd);
            }
        }

        // 热插拔补齐：仅对**选中且在播放**的显示器补建窗口（每轮都检查，
        // 不受上面 all_settled gate 影响，保证新显示器接入后能自动铺上）。
        let cur = state().lock().unwrap().clone();
        if cur.enabled && !cur.path.is_empty() {
            for mi in targets_of(&cur, monitors.len()) {
                let label = window_label_for(mi);
                // **读实时登记表，不用上面的 `hwnds` 快照** ——
                // 快照是进入本轮时克隆的，而校验环节可能刚把死窗口的句柄置 0；
                // 用快照会漏掉这次置 0，导致该重建的窗口本轮不重建。
                let recorded = watch_hwnds().lock().unwrap().get(mi).copied().unwrap_or(0);
                let missing = app.get_webview_window(&label).is_none() || recorded == 0;
                if !missing {
                    continue;
                }
                match app.get_webview_window(&label) {
                    // Tauri 侧没有这个窗口 → 直接建
                    None => {
                        let _ = create_video_window(&app, mi);
                    }
                    // Tauri 侧还留着窗口对象：**先确认它的原生窗口是否还活着**。
                    // 直接问句柄，比"登记句柄为 0"可靠 —— 登记为 0 也可能只是
                    // `on_page_load` 还没跑到（窗口刚建完），那种情况绝不能关掉重建，
                    // 否则会退化成每 4s 关一次建一次 → 一直闪。
                    Some(win) => {
                        let alive = win
                            .hwnd()
                            .map(|h| unsafe { IsWindow(HWND(h.0)) }.as_bool())
                            .unwrap_or(false);
                        if !alive {
                            // 原生窗口已被销毁（explorer 重启会连带销毁子窗口）：
                            // 必须先 close() 释放 label，否则 create_video_window 会因
                            // label 重名而失败 —— 表现为"重启 explorer 后动态壁纸再也
                            // 回不来"，且每轮都白试一次。
                            let _ = win.close();
                            let _ = create_video_window(&app, mi);
                        }
                    }
                }
            }
        }
    });
}

/// 计算当前应当铺壁纸的显示器下标列表。
///
/// - `state.monitors` 为空 → 全部显示器
/// - 非空 → 仅其中仍然存在的下标（显示器拔出后自动失效，避免索引越界）
///
/// 约定：**窗口 label 的序号 == 显示器下标**，二者一一对应，不做重映射，
/// 这样巡检/补齐/关闭都不需要额外的映射表。
fn targets_of(st: &VideoWallpaperState, monitor_count: usize) -> Vec<usize> {
    if st.monitors.is_empty() {
        (0..monitor_count).collect()
    } else {
        let mut v: Vec<usize> = st
            .monitors
            .iter()
            .copied()
            .filter(|i| *i < monitor_count)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// 计算各显示器对应的窗口 label
fn window_label_for(index: usize) -> String {
    format!("{VIDEO_WINDOW_LABEL_PREFIX}-{index}")
}

/// 清除所有动态壁纸窗口（含已拔出显示器留下的孤儿窗口）
fn close_all_video_windows(app: &tauri::AppHandle) {
    // 以"已记录的句柄数"与"显示器数"的较大者为上界遍历，
    // 避免显示器被拔掉后其窗口成为无法回收的孤儿。
    let recorded = watch_hwnds().lock().unwrap().len();
    let monitor_count = app
        .available_monitors()
        .map(|m| m.len())
        .unwrap_or(1)
        .max(1);
    let upper = recorded.max(monitor_count);
    for i in 0..upper {
        if let Some(win) = app.get_webview_window(&window_label_for(i)) {
            let _ = win.close();
        }
    }
    // 清理注册表与就位标记，保证下次启用能重新挂载
    watch_hwnds().lock().unwrap().clear();
    clear_settled();
}

/// 状态 → 可序列化快照（事件负载与查询返回值共用同一个形状）
fn state_out(s: &VideoWallpaperState) -> VideoWallpaperStateOut {
    VideoWallpaperStateOut {
        enabled: s.enabled,
        path: s.path.clone(),
        monitors: s.monitors.clone(),
        muted: s.muted,
        paused: s.paused,
    }
}

/// 把当前状态广播给**所有已存在**的动态壁纸窗口。
///
/// 只 `emit`、**绝不重建窗口** —— `SetParent`/`SetWindowPos`/`show()` 都会让
/// Windows 重建绘制表面，正在播放的视频会可见地闪一下（详见文件头关于闪屏的说明）。
/// 因此"换视频 / 静音 / 暂停"这三种变更一律走本函数，而不是关掉窗口再建。
///
/// 注意：事件可能早于播放页的 listener 注册（窗口刚建、页面还没加载完），
/// 所以播放页挂载时还会主动拉一次 `get_video_wallpaper_state` 兜底。
fn emit_state_to_windows(app: &tauri::AppHandle) {
    let out = state_out(&state().lock().unwrap().clone());
    // 上界取"已记录句柄数"与"显示器数"的较大者：显示器被拔掉后其窗口可能尚未回收，
    // 遍历宽一点无害（`get_webview_window` 对已销毁的 label 返回 None）。
    let recorded = watch_hwnds().lock().unwrap().len();
    let monitor_count = app
        .available_monitors()
        .map(|m| m.len())
        .unwrap_or(1)
        .max(1);
    for i in 0..recorded.max(monitor_count) {
        if let Some(win) = app.get_webview_window(&window_label_for(i)) {
            let _ = win.emit(VIDEO_STATE_EVENT, out.clone());
        }
    }
}

// ---------- Tauri 命令 ----------

/// 将本地视频设为桌面动态壁纸。
///
/// `monitors` 为要铺壁纸的显示器下标列表（对应 `list_monitors()` 的 `index`）：
/// - 传 `None` 或空数组 → 覆盖**全部**显示器
/// - 传具体下标 → 只在选中的显示器上铺，未选中的显示器其窗口会被**关闭**
///
/// 这样用户可以在不重启应用的前提下随时收窄/扩大生效范围。
///
/// `muted` 为静音偏好：传 `Some` 就采用（前端把持久化偏好带过来），
/// 传 `None` 则沿用上一次的值 —— 只切播放范围时不会把静音状态弄丢。
#[tauri::command]
pub async fn set_video_wallpaper(
    path: String,
    monitors: Option<Vec<usize>>,
    muted: Option<bool>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err(format!("视频文件不存在：{path}"));
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    if !is_supported_video_ext(ext) {
        return Err(format!("不支持的视频格式：{ext}（支持 mp4/webm/mkv/mov）"));
    }
    // 视频所在目录加入 asset scope，前端 convertFileSrc 可加载
    if let Some(dir) = p.parent() {
        crate::ensure_asset_scope(&app, &dir.to_path_buf());
    }

    // 规范化选择：排序去重（保留空 = 全部 的语义）
    let mut sel: Vec<usize> = monitors.unwrap_or_default();
    sel.sort_unstable();
    sel.dedup();

    // 写入状态（供巡检线程与热插拔补齐使用）。
    // 先取旧状态：muted / paused 的取舍取决于"这次是不是换了片子"。
    let prev = state().lock().unwrap().clone();
    let path_changed = prev.path != path;
    *state().lock().unwrap() = VideoWallpaperState {
        enabled: true,
        path: path.clone(),
        monitors: sel.clone(),
        muted: muted.unwrap_or(prev.muted),
        // 换视频 → 复位暂停：换了片子就该开始播，否则用户会以为新视频也坏了；
        // 只改播放范围 → 保持暂停（用户是暂停着在调整范围，不该被偷偷恢复）。
        paused: if path_changed { false } else { prev.paused },
    };

    let monitor_count = app.available_monitors().map(|m| m.len()).unwrap_or(0).max(1);
    // 按"窗口 label 序号 == 显示器下标"约定，算出应存在的窗口集合
    let want: Vec<usize> = {
        let st = state().lock().unwrap().clone();
        targets_of(&st, monitor_count)
    };

    // 1) 关掉本轮不再需要的窗口（用户收窄了选择范围）
    let recorded = watch_hwnds().lock().unwrap().len();
    let upper = recorded.max(monitor_count);
    for i in 0..upper {
        if want.contains(&i) {
            continue;
        }
        // 先取出旧句柄：close() 后窗口销毁，但 SETTLED 里若留着旧 HWND，
        // 后续万一被系统复用会误判"已就位"。这里显式解除登记。
        let old_hwnd = {
            let v = watch_hwnds().lock().unwrap();
            v.get(i).copied().unwrap_or(0)
        };
        if old_hwnd != 0 {
            unmark_settled(old_hwnd);
        }
        if let Some(win) = app.get_webview_window(&window_label_for(i)) {
            let _ = win.close();
        }
        let mut v = watch_hwnds().lock().unwrap();
        if i < v.len() {
            v[i] = 0;
        }
    }

    // 2) 对目标显示器：缺失的补建窗口；**已存在的一律不重建**（重建会闪屏）
    for &mi in &want {
        if app.get_webview_window(&window_label_for(mi)).is_none() {
            create_video_window(&app, mi)?;
        }
    }

    // 3) 广播完整状态：已存在的窗口借此换源 / 更新静音与暂停；
    //    刚补建的窗口也可能在页面加载完成前错过事件，由播放页挂载时的拉取兜底。
    emit_state_to_windows(&app);

    // 无条件调用：内部有"只启动一次"的闸门（`WATCHER_STARTED`）。
    // 不要在这里自己判"是否已在运行" —— 那是两份会漂移的规则，且原来那种
    // `if !RUNNING { spawn }` + 内部 `swap` 的组合正好漏掉了"旧线程还没退出"的窗口期。
    spawn_relink_watcher(app.clone());
    Ok(())
}

/// 静音开关：只影响桌面动态壁纸的音频，**不重建窗口**（重建会闪一下）
#[tauri::command]
pub async fn set_video_wallpaper_muted(
    muted: bool,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state().lock().unwrap().muted = muted;
    // 不做"值没变就跳过"的短路：幂等广播代价极低，
    // 而一旦状态因任何原因漂移，短路会让它永远修不回来。
    emit_state_to_windows(&app);
    Ok(())
}

/// 暂停 / 恢复动态壁纸。
///
/// 暂停 = 冻结当前帧（不销毁窗口、不回退到静态壁纸），恢复从原处继续。
/// 与 `stop_video_wallpaper` 的区别：停止会销毁置底窗口，桌面回到静态壁纸。
#[tauri::command]
pub async fn set_video_wallpaper_paused(
    paused: bool,
    app: tauri::AppHandle,
) -> Result<(), String> {
    state().lock().unwrap().paused = paused;
    emit_state_to_windows(&app);
    Ok(())
}

/// 列出当前所有显示器（供前端渲染"选择显示器"列表）
///
/// 展示名策略：Windows 的 `Monitor::name()` 常是 `\\.\DISPLAY1` 这类**设备路径**，
/// 对用户毫无意义（甚至可能是空的）。因此这里优先用 EDID 型号名，
/// 拿不到就退化为"显示器 N"。
///
/// **名称风格必须统一**：每块屏一律是 `<名称> · <方位>`（方位见 `position_hint`），
/// 主屏也不例外。曾经给主屏加"（主）"后缀、并且不给它方位后缀 ——
/// 同一份下拉里主屏那行比别人短一截、后缀风格还不同，视觉上长短不齐。
/// 分辨率/缩放/相对位置拆成独立字段交给前端组合，避免前端解析字符串。
#[tauri::command]
pub async fn list_monitors(app: tauri::AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("枚举显示器失败：{e}"))?;
    let primary = app.primary_monitor().ok().flatten();
    let primary_name = primary.as_ref().and_then(|m| m.name().cloned());
    let primary_pos = primary.as_ref().map(|m| (m.position().x, m.position().y));

    // 主屏中心点，用于判断其它屏在主屏的哪个方向
    let primary_center = primary.as_ref().map(|m| {
        (
            m.position().x + m.size().width as i32 / 2,
            m.position().y + m.size().height as i32 / 2,
        )
    });

    let multi = monitors.len() > 1;

    let list = monitors
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let size = m.size();
            let scale = m.scale_factor();
            let is_primary = match (&primary_name, primary_pos) {
                (Some(pn), _) if Some(pn) == m.name() => true,
                (None, Some(pp)) => (m.position().x, m.position().y) == pp,
                _ => false,
            };

            // ---- 通俗展示名 ----
            // 排除设备路径（\\.\DISPLAY1）与空串：这类名字给用户看没有信息量。
            // 真实的 EDID 型号名（如 "DELL U2720Q"）才值得展示。
            let raw = m.name().map(|s| s.trim()).unwrap_or("");
            let is_device_path = raw.is_empty() || raw.starts_with("\\\\") || {
                // 形如 \\.\DISPLAY1 / DISPLAY1 这类纯设备标识
                let upper = raw.to_ascii_uppercase();
                upper.starts_with("DISPLAY") && raw.len() <= 10
            };
            let model = if is_device_path { "" } else { raw };

            // 名称里**不带任何"主屏"标记**：方位统一由 position_hint 表达
            //（主屏就是"主屏"），避免出现"（主）"与" · 左侧"两种后缀风格并存。
            // 拿不到型号时一律按序号，不再给主屏"主显示器"特例 ——
            // 那个名字自带"主"的语义，会与方位后缀" · 主屏"重复。
            let label = if model.is_empty() {
                format!("显示器 {}", i + 1)
            } else {
                model.to_string()
            };

            // ---- 相对位置 ----
            // 多屏时用户是靠"在左边/在上面"来指认物理屏幕的，比型号更好用。
            // 方位词是**可直接展示**的短词，不带"主屏"前缀 ——
            // 前端直接拼成 `<名称> · <方位>`，不需要再解析字符串。
            let position_hint = if !multi {
                None
            } else if is_primary {
                Some("主屏".to_string())
            } else if let Some((pcx, pcy)) = primary_center {
                let cx = m.position().x + size.width as i32 / 2;
                let cy = m.position().y + size.height as i32 / 2;
                let dx = cx - pcx;
                let dy = cy - pcy;
                // 以位移较大的轴判定主方向
                let hint = if dx.abs() >= dy.abs() {
                    if dx < 0 { "左侧" } else { "右侧" }
                } else if dy < 0 {
                    "上方"
                } else {
                    "下方"
                };
                Some(hint.to_string())
            } else {
                None
            };

            let resolution = format!("{}×{}", size.width, size.height);
            let scale_text = if (scale - 1.0).abs() > 0.01 {
                format!(" · {}%", (scale * 100.0).round() as i32)
            } else {
                String::new()
            };

            MonitorInfo {
                index: i,
                // 兼容字段：保持与旧版一致的完整字符串
                name: format!("{label} · {resolution}{scale_text}"),
                label,
                resolution,
                position_hint,
                width: size.width,
                height: size.height,
                scale,
                is_primary,
            }
        })
        .collect();
    Ok(list)
}

/// 创建第 index 个显示器上的动态壁纸窗口
fn create_video_window(app: &tauri::AppHandle, index: usize) -> Result<(), String> {
    let label = window_label_for(index);
    let label_for_cb = label.clone();
    let app_for_cb = app.clone();
    let index_for_cb = index;

    // 关于"有声音时能否自动播放"：
    //   置底窗口永远不会获得用户手势（它挂在桌面图标层下、且带 WS_EX_NOACTIVATE），
    //   所以"取消静音"后能不能继续出声，取决于 Chromium 的自动播放策略。
    //   这里**不需要**自己加 `--autoplay-policy=no-user-gesture-required`：
    //   wry 的 `WebViewAttributes::autoplay` 默认为 true，且 Tauri 从不覆写它，
    //   因此默认参数里已经带了这条 flag。**别去调 `additional_browser_args`** ——
    //   那个方法是"整体替换"而不是"追加"，会把 wry 的默认参数
    //   （`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`
    //   以及这条 autoplay flag）一起抹掉。
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(VIDEO_VIEW.into()))
        .title("DotWallpaper 动态壁纸")
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(false)
        .visible(false)
        .on_page_load(move |window, _payload| {
            if let Ok(tauri_hwnd) = window.hwnd() {
                let hwnd = HWND(tauri_hwnd.0);
                set_no_activate(hwnd);
                // 先挂载拿到 WorkerW，再按"本窗口对应的显示器"摆放。
                // `allow_spawn = true`：这是首次挂载，桌面层可能还不存在，
                // 允许发送 WM_SPAWN_WORKERW 把它生出来（只此一处允许）。
                if let Some(parent) = attach_to_desktop(hwnd, true) {
                    let monitors = app_for_cb.available_monitors().unwrap_or_default();
                    if let Some(mon) = monitors.get(index_for_cb) {
                        if let Some(geo) = monitor_geo(&app_for_cb, mon, parent) {
                            apply_geo_if_changed(hwnd, geo, parent);
                        }
                    }
                    // 挂载 + 摆位都完成后标记就位，巡检线程此后不再触碰本窗口
                    mark_settled(hwnd);
                }
                // 记录句柄（按 label 序号位置写入，供巡检线程按序号取用）
                if let Some(idx) = label_for_cb
                    .rsplit('-')
                    .next()
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    let mut v = watch_hwnds().lock().unwrap();
                    if v.len() <= idx {
                        v.resize(idx + 1, 0);
                    }
                    v[idx] = hwnd.0 as isize;
                }
                // 只在**尚未挂到桌面层**时才 show()。
                // 已挂到 WorkerW 后再调 show() 会让 Windows 重建绘制表面 → 视频闪一下；
                // 且挂载后的窗口本就随桌面层显示，无需再 show。
                let already_attached = unsafe {
                    !GetParent(hwnd).unwrap_or(HWND::default()).0.is_null()
                };
                if !already_attached {
                    let _ = window.show();
                }
            }
        });

    let win = builder
        .build()
        .map_err(|e| format!("创建动态壁纸窗口失败（显示器 {index}）：{e}"))?;
    // 事件可能早于前端 listener 注册：播放页挂载时会主动拉取状态兜底。
    // 这里仍然发一次 —— 热插拔补建的新窗口正是靠它立刻拿到路径，
    // 不必等页面自己拉（两条路都通，谁先到都不影响结果）。
    let _ = win.emit(VIDEO_STATE_EVENT, state_out(&state().lock().unwrap().clone()));
    Ok(())
}

/// 停止动态壁纸：销毁所有置底窗口并复位状态
///
/// 复位时**保留 `muted`** —— 它是用户偏好（"我不想让桌面出声"），
/// 不该因为停了一次动态壁纸就被忘掉；`paused` 则复位（下次启用应当正常播放）。
///
/// **不停止巡检线程**：它随应用存活，`close_all_video_windows()` 清空句柄后
/// 每轮都会在 `hwnds.is_empty()` 处短路。曾在这里把巡检标志置 false 想"停掉它"，
/// 结果是"停止 → 再启用"每来一次就多一个巡检线程（详见 `WATCHER_STARTED`）。
#[tauri::command]
pub async fn stop_video_wallpaper(app: tauri::AppHandle) -> Result<(), String> {
    {
        let mut s = state().lock().unwrap();
        let muted = s.muted;
        *s = VideoWallpaperState {
            enabled: false,
            path: String::new(),
            monitors: Vec::new(),
            muted,
            paused: false,
        };
    }
    close_all_video_windows(&app);
    Ok(())
}

/// 查询当前动态壁纸状态（播放页挂载与主界面恢复时拉取）
#[tauri::command]
pub async fn get_video_wallpaper_state() -> Result<VideoWallpaperStateOut, String> {
    Ok(state_out(&state().lock().unwrap().clone()))
}
