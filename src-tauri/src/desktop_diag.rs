// 动态壁纸「一键诊断」—— **应用内原生实现，不依赖 PowerShell**
//
// 为什么不用脚本：仓库里原本有个 `scripts/diag-video-window.ps1`，但它是把
// `video_wallpaper.rs` 的挂载逻辑**照着重写的一份副本**。2026-10-19 就吃过亏：
// 脚本断言"app 会挂到那个 WorkerW"，而实际代码早已因为 `GetParent` 返回 NULL
// 放弃了那条分支 —— 用户发来的诊断反而是误导。
//
// 所以这里**直接调用真实函数**（`find_wallpaper_layer` / `real_parent` /
// `is_attached_to_desktop`）：诊断说的永远等于程序做的，不可能再跑偏。
//
// 原生实现还顺带解决三件事：不闪命令行窗口；不受 PowerShell 执行策略 / AppLocker
// 限制（公司电脑常见）；输出可以直接排版成「人话结论 + 明细」。
//
// **本模块必须保持只读**：不 `SetParent`、不 `SetWindowPos`、不发 0x052C、不 `show()`。
// 任何写操作都会重建绘制表面 → 用户正在看的视频会闪一下。
// 查找挂载点用的是 `find_wallpaper_layer(false)`（只读版），与巡检同一条路径。

use std::fmt::Write as _;

use serde::Serialize;
use tauri::Manager;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetDesktopWindow,
    GetParent, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    IsWindow, IsWindowVisible, WindowFromPoint, GWL_STYLE,
};

use crate::video_wallpaper::{
    find_wallpaper_layer, is_attached_to_desktop, real_parent, window_label_for,
};

/// 诊断结果（前端渲染用）
#[derive(Serialize)]
pub struct DiagReport {
    /// 总体结论等级：`ok` / `warn` / `fail` —— 前端据此给标题上色
    pub level: String,
    /// **人话结论**，一条一行，每条自带等级
    pub summary: Vec<DiagItem>,
    /// 完整明细：给「复制」与「保存为文件」用，也是发给开发者排障的原始材料
    pub detail: String,
}

/// 一条结论
#[derive(Serialize)]
pub struct DiagItem {
    /// `ok` / `warn` / `fail`
    pub level: String,
    pub text: String,
}

fn sev_text(sev: u8) -> &'static str {
    match sev {
        0 => "ok",
        1 => "warn",
        _ => "fail",
    }
}

/// 追加一条结论，并同步抬高总体等级
fn push_item(items: &mut Vec<DiagItem>, total: &mut u8, sev: u8, text: String) {
    if sev > *total {
        *total = sev;
    }
    items.push(DiagItem {
        level: sev_text(sev).to_string(),
        text,
    });
}

// ---------- 小工具 ----------

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn hx(h: HWND) -> String {
    if h.0.is_null() {
        "(null)".to_string()
    } else {
        format!("0x{:X}", h.0 as usize)
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn title_of(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn rect_of(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    if unsafe { GetWindowRect(h, &mut r) }.is_ok() {
        Some(r)
    } else {
        None
    }
}

fn rect_text(r: &RECT) -> String {
    format!(
        "({}, {}) {}×{}",
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top
    )
}

fn rect_text_or(h: HWND) -> String {
    rect_of(h).map(|r| rect_text(&r)).unwrap_or_else(|| "(?)".into())
}

fn visible(h: HWND) -> bool {
    unsafe { IsWindowVisible(h) }.as_bool()
}

unsafe extern "system" fn collect_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let v = &mut *(lparam.0 as *mut Vec<HWND>);
    if v.len() < 8192 {
        v.push(hwnd);
    }
    BOOL(1)
}

/// 全部顶层窗口（z 序，最上层在前）
fn top_level_windows() -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_proc), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

/// 全部后代窗口（`EnumChildWindows` 是**递归**的，不只是直接子窗口）
fn descendants(h: HWND) -> Vec<HWND> {
    let mut v: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumChildWindows(h, Some(collect_proc), LPARAM(&mut v as *mut Vec<HWND> as isize));
    }
    v
}

/// 直接子窗口里有没有指定类名（`class` 传 `PCWSTR::null()` 表示任意类名）
fn direct_children(parent: HWND, class: PCWSTR) -> Vec<HWND> {
    let mut v = Vec::new();
    let mut cur = match unsafe { FindWindowExW(parent, None, class, PCWSTR::null()) } {
        Ok(h) => h,
        Err(_) => return v,
    };
    // 防御性上界：真实桌面上不可能超过这个数
    for _ in 0..64 {
        if cur.0.is_null() {
            break;
        }
        v.push(cur);
        cur = match unsafe { FindWindowExW(parent, cur, class, PCWSTR::null()) } {
            Ok(h) => h,
            Err(_) => break,
        };
    }
    v
}

fn has_direct_child(parent: HWND, class: PCWSTR) -> bool {
    direct_children(parent, class).iter().any(|h| !h.0.is_null())
}

// ---------- 采集 ----------

/// 单个置底窗口的体检结果（用于拼结论）
struct WinFact {
    index: usize,
    attached: bool,
    shown: bool,
    geo_ok: bool,
}

pub fn collect(app: &tauri::AppHandle) -> DiagReport {
    let mut summary: Vec<DiagItem> = Vec::new();
    let mut d = String::new();
    // 总体严重度：0=ok 1=warn 2=fail
    let mut sev = 0u8;

    let _ = writeln!(d, "=== DotWallpaper 动态壁纸诊断 ===");
    let _ = writeln!(
        d,
        "版本 {}    进程 PID {}",
        app.package_info().version,
        std::process::id()
    );

    // ---------- 显示器 ----------
    let monitors = app.available_monitors().unwrap_or_default();
    let _ = writeln!(d, "\n--- 显示器（物理像素）---");
    if monitors.is_empty() {
        let _ = writeln!(d, "  (枚举不到任何显示器)");
    }
    for (i, m) in monitors.iter().enumerate() {
        let p = m.position();
        let s = m.size();
        let _ = writeln!(
            d,
            "  [{i}] {}  {}×{}  @{}%  pos=({}, {})",
            m.name().map(|n| n.to_string()).unwrap_or_else(|| "(无名)".into()),
            s.width,
            s.height,
            (m.scale_factor() * 100.0).round() as i32,
            p.x,
            p.y
        );
    }

    // ---------- 桌面层 ----------
    let progman_name = wide("Progman");
    let def_view_name = wide("SHELLDLL_DefView");
    let workerw_name = wide("WorkerW");
    let def_view = PCWSTR(def_view_name.as_ptr());
    let workerw = PCWSTR(workerw_name.as_ptr());

    let progman = unsafe { FindWindowW(PCWSTR(progman_name.as_ptr()), PCWSTR::null()) }
        .ok()
        .filter(|h| !h.0.is_null());

    let _ = writeln!(d, "\n--- 桌面层（Progman / WorkerW）---");
    match progman {
        Some(p) => {
            let _ = writeln!(
                d,
                "  Progman {} rect={} visible={}",
                hx(p),
                rect_text_or(p),
                visible(p)
            );
            let kids = direct_children(p, PCWSTR::null());
            if kids.is_empty() {
                let _ = writeln!(d, "    直接子窗口: (无)");
            } else {
                for (k, c) in kids.iter().enumerate().take(12) {
                    let _ = writeln!(
                        d,
                        "    子窗口[{k}] class=\"{}\" {} rect={} visible={}",
                        class_of(*c),
                        hx(*c),
                        rect_text_or(*c),
                        visible(*c)
                    );
                }
            }
        }
        None => {
            let _ = writeln!(d, "  Progman: 找不到（桌面 shell 可能没起来）");
        }
    }

    let tops = top_level_windows();
    let top_workers: Vec<HWND> = tops
        .iter()
        .copied()
        .filter(|h| class_of(*h) == "WorkerW")
        .collect();
    let _ = writeln!(d, "  顶层 WorkerW 共 {} 个:", top_workers.len());
    for w in &top_workers {
        let _ = writeln!(
            d,
            "    {} rect={} visible={} 含DefView={}",
            hx(*w),
            rect_text_or(*w),
            visible(*w),
            has_direct_child(*w, def_view)
        );
    }

    // ---------- 布局判定 + 挂载点（与 video_wallpaper 的查找顺序一致）----------
    // 只做判定，**不改任何东西**。
    let mut icon_layer: Option<HWND> = None;
    let mut layout = "";

    // Win10：图标层是一个**可见的顶层** WorkerW
    for w in &top_workers {
        if visible(*w) && has_direct_child(*w, def_view) {
            icon_layer = Some(*w);
            layout = "Win10（图标层是顶层 WorkerW，壁纸层是它之后的兄弟 WorkerW）";
            break;
        }
    }
    // Win11：图标层在 Progman 之下
    if icon_layer.is_none() {
        if let Some(p) = progman {
            if has_direct_child(p, def_view) {
                layout = "Win11（SHELLDLL_DefView 是 Progman 的直接子窗口）";
            } else {
                for w in direct_children(p, workerw) {
                    if visible(w) && has_direct_child(w, def_view) {
                        icon_layer = Some(w);
                        layout = "Win11（图标层是 Progman 的子 WorkerW）";
                        break;
                    }
                }
            }
        }
    }

    let target = find_wallpaper_layer(false);
    let _ = writeln!(d, "\n--- 挂载点判定（只读，与程序实际使用的同一函数）---");
    let _ = writeln!(
        d,
        "  系统布局: {}",
        if layout.is_empty() { "(无法判定)" } else { layout }
    );
    let _ = writeln!(
        d,
        "  图标层  : {}",
        icon_layer.map(hx).unwrap_or_else(|| "(未找到)".into())
    );
    match target {
        Some(t) => {
            let _ = writeln!(
                d,
                "  壁纸层  : {} class=\"{}\" rect={} visible={}  ← 会挂到这里",
                hx(t),
                class_of(t),
                rect_text_or(t),
                visible(t)
            );
        }
        None => {
            let _ = writeln!(d, "  壁纸层  : 找不到 —— 无法挂载");
        }
    }

    // ---------- 我们的置底窗口 ----------
    let _ = writeln!(d, "\n--- 动态壁纸窗口 ---");
    let mut facts: Vec<WinFact> = Vec::new();
    let desktop = unsafe { GetDesktopWindow() };

    let scan = monitors.len().max(1);
    for i in 0..scan {
        let label = window_label_for(i);
        let Some(win) = app.get_webview_window(&label) else {
            continue;
        };
        let h = match win.hwnd() {
            Ok(t) => HWND(t.0),
            Err(e) => {
                let _ = writeln!(d, "  [{i}] {label}: 取窗口句柄失败（{e}）");
                continue;
            }
        };
        if !unsafe { IsWindow(h) }.as_bool() {
            let _ = writeln!(d, "  [{i}] {label}: 句柄无效（窗口已销毁）");
            continue;
        }

        let style = unsafe { GetWindowLongPtrW(h, GWL_STYLE) } as u32;
        let ga = real_parent(h);
        let gp = unsafe { GetParent(h) }.ok().unwrap_or_default();
        let attached = is_attached_to_desktop(h);
        let shown = visible(h);
        let r = rect_of(h);

        let _ = writeln!(d, "  [{i}] {label}  {}", hx(h));
        let _ = writeln!(
            d,
            "      class=\"{}\" title=\"{}\"",
            class_of(h),
            title_of(h)
        );
        let _ = writeln!(
            d,
            "      rect={}  visible={}",
            r.map(|x| rect_text(&x)).unwrap_or_else(|| "(?)".into()),
            shown
        );
        let _ = writeln!(
            d,
            "      style=0x{style:X}  WS_CHILD={}  WS_POPUP={}",
            style & 0x4000_0000 != 0,
            style & 0x8000_0000 != 0
        );
        let _ = writeln!(
            d,
            "      GetAncestor(GA_PARENT)={} class=\"{}\"  （父窗口是桌面窗口? {}）",
            hx(ga),
            if ga.0.is_null() { "null".into() } else { class_of(ga) },
            ga == desktop
        );
        let _ = writeln!(
            d,
            "      GetParent()={}  ← 这类窗口恒为 null，不能用来判断挂载",
            hx(gp)
        );
        let _ = writeln!(d, "      已挂到桌面层: {}", if attached { "是" } else { "否" });

        // 期望几何 vs 实际几何（都以父窗口客户区原点为基准，物理像素）
        let mut geo_ok = false;
        if let (Some(mon), Some(rc)) = (monitors.get(i), r) {
            let (px, py) = if ga.0.is_null() {
                (0, 0)
            } else {
                rect_of(ga).map(|x| (x.left, x.top)).unwrap_or((0, 0))
            };
            let ex = mon.position().x - px;
            let ey = mon.position().y - py;
            let ew = mon.size().width as i32;
            let eh = mon.size().height as i32;
            let ax = rc.left - px;
            let ay = rc.top - py;
            let aw = rc.right - rc.left;
            let ah = rc.bottom - rc.top;
            geo_ok = ax == ex && ay == ey && aw == ew && ah == eh;
            let _ = writeln!(
                d,
                "      期望几何=({ex}, {ey}) {ew}×{eh}   实际(父坐标)=({ax}, {ay}) {aw}×{ah}   ⇒ {}",
                if geo_ok { "一致" } else { "不一致" }
            );
        }

        let kids = descendants(h);
        let _ = writeln!(d, "      后代窗口 {} 个:", kids.len());
        for (k, c) in kids.iter().enumerate().take(6) {
            let _ = writeln!(
                d,
                "        [{k}] class=\"{}\" {} rect={} visible={}",
                class_of(*c),
                hx(*c),
                rect_text_or(*c),
                visible(*c)
            );
        }
        if kids.len() > 6 {
            let _ = writeln!(d, "        …（其余 {} 个略）", kids.len() - 6);
        }

        facts.push(WinFact {
            index: i,
            attached,
            shown,
            geo_ok,
        });
    }
    if facts.is_empty() {
        let _ = writeln!(d, "  (没有动态壁纸窗口在运行)");
    }

    // ---------- 鼠标命中测试 ----------
    let _ = writeln!(d, "\n--- 鼠标命中测试（WindowFromPoint）---");
    let me = std::process::id();
    let mut hit_our_center = 0usize;
    for (i, m) in monitors.iter().enumerate() {
        let p = m.position();
        let s = m.size();
        let cx = p.x + s.width as i32 / 2;
        let cy = p.y + s.height as i32 / 2;
        let pts = [
            (cx, cy),
            (p.x + 40, p.y + 40),
            (p.x + s.width as i32 - 40, p.y + 40),
            (p.x + 40, p.y + s.height as i32 - 40),
            (p.x + s.width as i32 - 40, p.y + s.height as i32 - 40),
        ];
        for (k, (x, y)) in pts.iter().enumerate() {
            let h = unsafe { WindowFromPoint(POINT { x: *x, y: *y }) };
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(h, Some(&mut pid));
            }
            let mine = pid == me && !h.0.is_null();
            if k == 0 && mine {
                hit_our_center += 1;
            }
            let _ = writeln!(
                d,
                "  显示器[{i}] ({x}, {y}) -> class=\"{}\" {} pid={}{}",
                class_of(h),
                hx(h),
                pid,
                if mine { "  ← 我们的窗口" } else { "" }
            );
        }
    }

    // ---------- 结论 ----------
    let _ = writeln!(d, "\n--- 结论 ---");

    if facts.is_empty() {
        push_item(
            &mut summary,
            &mut sev,
            1,
            "没有正在运行的动态壁纸窗口：可能还没启用，或者已经停止了。".to_string(),
        );
    }

    for f in &facts {
        let n = f.index + 1;
        if !f.attached {
            push_item(
                &mut summary,
                &mut sev,
                2,
                format!("显示器 {n}：没能挂到桌面层 —— 这块屏不会有画面。"),
            );
        } else if !f.shown {
            push_item(
                &mut summary,
                &mut sev,
                1,
                format!("显示器 {n}：已挂到桌面层，但窗口是隐藏的（正常应在挂载成功后自动显示）。"),
            );
        } else if !f.geo_ok {
            push_item(
                &mut summary,
                &mut sev,
                1,
                format!("显示器 {n}：已挂上，但窗口大小/位置与这块屏不一致 —— 画面可能只显示一部分。"),
            );
        } else {
            push_item(
                &mut summary,
                &mut sev,
                0,
                format!("显示器 {n}：已正常挂到桌面层，画面应正常显示。"),
            );
        }
    }

    if target.is_none() {
        push_item(
            &mut summary,
            &mut sev,
            2,
            "系统里找不到可以挂载的桌面层窗口 —— 常见原因是其他动态壁纸/桌面整理软件占用，\
             或当前是远程桌面会话。"
                .to_string(),
        );
    }

    if hit_our_center > 0 && !facts.is_empty() {
        push_item(
            &mut summary,
            &mut sev,
            1,
            "鼠标点在桌面中间会点到壁纸窗口本身 —— 桌面图标可能被挡住（若你关掉了\
             「显示桌面图标」则属正常）。"
                .to_string(),
        );
    }

    DiagReport {
        level: sev_text(sev).to_string(),
        summary,
        detail: d,
    }
}

// ---------- 测试 ----------
// 这些用例跑在**真实桌面**上（本机就是真实 shell），只读、不改变任何窗口状态。
// 价值：跑的是与生产代码**同一条路径**，一旦有人改坏 `find_wallpaper_layer`
// 或窗口枚举，测试先炸，而不是等用户报"桌面没画面"。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_top_level_windows() {
        let tops = top_level_windows();
        // 正常 Windows 桌面至少有几十个顶层窗口；返回 0 说明回调签名写错了
        assert!(tops.len() > 5, "顶层窗口只枚举到 {} 个", tops.len());
    }

    #[test]
    fn finds_progman_and_walks_children() {
        let name = wide("Progman");
        let p = unsafe { FindWindowW(PCWSTR(name.as_ptr()), PCWSTR::null()) }
            .ok()
            .filter(|h| !h.0.is_null())
            .expect("找不到 Progman（桌面 shell 没起来？）");
        let kids = direct_children(p, PCWSTR::null());
        assert!(kids.len() <= 64, "子窗口数超过防御上界：{}", kids.len());
        assert_eq!(class_of(p), "Progman");
    }

    #[test]
    fn finds_a_wallpaper_layer() {
        let t = find_wallpaper_layer(false).expect("找不到壁纸层窗口（只读查找失败）");
        assert!(unsafe { IsWindow(t) }.as_bool(), "找到的壁纸层句柄无效");
        // 壁纸层必须是 Progman 或 WorkerW，否则说明查找逻辑挑错了窗口
        let cls = class_of(t);
        assert!(
            cls == "Progman" || cls == "WorkerW",
            "壁纸层类名异常：{cls}"
        );
    }
}
