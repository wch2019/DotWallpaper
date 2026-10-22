# DotWallpaper field diagnostic -- READ ONLY, changes nothing.
#
# !! 已被应用内诊断取代（2026-10-22）!!
# 主程序「当前壁纸 → 动态壁纸」那一行有诊断按钮，点一下就能出结论，
# 不需要用户跑命令行。**权威实现在 `src-tauri/src/desktop_diag.rs`**，
# 它直接调用生产代码的挂载逻辑；本脚本是照着逻辑重写的副本，天然会跑偏
# （2026-10-19 就发生过：脚本说"会挂到那个 WorkerW"，实际代码早已放弃）。
# 保留本文件仅作**离线备用**：主程序起不来时还能用它取一份现场。
# 改动挂载逻辑时**不必**同步本脚本，改 desktop_diag.rs 即可。
#
# Run this ON THE MACHINE WHERE THE VIDEO WALLPAPER MISBEHAVES, while the video
# wallpaper is ACTIVE (playing).
#
#   powershell -ExecutionPolicy Bypass -File diag-video-window.ps1
#
# It answers, with hard numbers:
#   1) Is our wallpaper window full screen, and where is the WebView2 child?
#   2) Is our window BELOW the desktop icon layer (SHELLDLL_DefView) in Z order?
#      ABOVE  -> the mouse can reach it. That is the "browser right-click menu
#                appears on the desktop" bug.
#   3) Which window does the mouse actually hit at sample points?
#      (WindowFromPoint is the OS's own answer to "what is under the cursor".)
#
# NOTE (2026-10-19): an un-attached wallpaper window now stays HIDDEN instead of
# being shown as a floating rectangle. So "our window exists but visible=False and
# its parent is the desktop window" is a NORMAL, EXPECTED result when attach fails
# -- it means the app is doing the right thing and the attach itself is the problem.
#
# Output is ASCII only on purpose (Windows PowerShell 5.1 reads .ps1 as ANSI).

$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class DwDiag
{
    public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
    public delegate bool MonitorProc(IntPtr hMon, IntPtr hdc, ref RECT r, IntPtr data);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [StructLayout(LayoutKind.Sequential)]
    public struct POINT { public int X, Y; }

    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc cb, IntPtr p);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetDesktopWindow();
    [DllImport("user32.dll")] public static extern long GetWindowLongPtr(IntPtr h, int idx);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string win);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string win);
    [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorProc cb, IntPtr data);
    [DllImport("shcore.dll")] public static extern int GetDpiForMonitor(IntPtr hmon, int type, out uint x, out uint y);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("shcore.dll")] public static extern int SetProcessDpiAwareness(int v);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);

    public static string Awareness = "(not set)";

    public static void MakeAware()
    {
        try { if (SetProcessDpiAwarenessContext(new IntPtr(-4))) { Awareness = "PerMonitorV2"; return; } }
        catch { }
        try { if (SetProcessDpiAwareness(2) == 0) { Awareness = "PerMonitor"; return; } }
        catch { }
        Awareness = "FAILED (rects may be DPI-virtualized!)";
    }

    public static string Cls(IntPtr h) { var sb = new StringBuilder(256); GetClassName(h, sb, 256); return sb.ToString(); }
    public static string Txt(IntPtr h) { var sb = new StringBuilder(512); GetWindowText(h, sb, 512); return sb.ToString(); }
    public static string Rc(IntPtr h) { RECT r; if (!GetWindowRect(h, out r)) return "(?)"; return string.Format("({0},{1}) {2}x{3}", r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top); }
    public static string Hx(IntPtr h) { return h == IntPtr.Zero ? "(null)" : "0x" + h.ToInt64().ToString("X"); }

    public static List<RECT> MonRects()
    {
        var l = new List<RECT>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate(IntPtr hMon, IntPtr hdc, ref RECT r, IntPtr d) {
            l.Add(r);
            return true;
        }, IntPtr.Zero);
        return l;
    }

    public static string Monitors()
    {
        var lines = new List<string>();
        EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, delegate(IntPtr hMon, IntPtr hdc, ref RECT r, IntPtr d) {
            uint dx = 0, dy = 0;
            try { GetDpiForMonitor(hMon, 0, out dx, out dy); } catch { }
            string scale = dx > 0 ? ((dx * 100) / 96).ToString() + "%" : "?";
            lines.Add(string.Format("  rect=({0},{1}) {2}x{3}   dpi={4} ({5})",
                r.Left, r.Top, r.Right - r.Left, r.Bottom - r.Top, dx, scale));
            return true;
        }, IntPtr.Zero);
        var sb = new StringBuilder();
        sb.AppendLine("=== Monitors (physical px) ===");
        if (lines.Count == 0) sb.AppendLine("  (EnumDisplayMonitors returned nothing)");
        foreach (var l in lines) sb.AppendLine(l);
        return sb.ToString();
    }

    public static string DesktopLayers()
    {
        var sb = new StringBuilder();
        sb.AppendLine("=== Desktop layer windows (top level, class Progman / WorkerW) ===");
        var found = 0;
        EnumWindows(delegate(IntPtr h, IntPtr p) {
            string c = Cls(h);
            if (c == "Progman" || c == "WorkerW")
            {
                IntPtr dv = IntPtr.Zero;
                EnumChildWindows(h, delegate(IntPtr ch, IntPtr q) {
                    if (Cls(ch) == "SHELLDLL_DefView") { dv = ch; return false; }
                    return true;
                }, IntPtr.Zero);
                IntPtr par = GetAncestor(h, 1 /* GA_PARENT */);
                sb.AppendLine(string.Format("  {0,-8} {1} rect={2} visible={3} parent={4}({5}) hasDefView={6}",
                    c, Hx(h), Rc(h), IsWindowVisible(h), Hx(par), par == IntPtr.Zero ? "null" : Cls(par), dv != IntPtr.Zero));
                found++;
            }
            return true;
        }, IntPtr.Zero);
        if (found == 0) sb.AppendLine("  (none found)");
        sb.AppendLine();
        sb.AppendLine("=== Direct children of Progman (z-order, top first) ===");
        IntPtr progman = FindWindowW("Progman", null);
        if (progman == IntPtr.Zero) { sb.AppendLine("  Progman not found"); return sb.ToString(); }
        sb.AppendLine("  Progman " + Hx(progman) + " rect=" + Rc(progman));
        IntPtr ch2 = FindWindowExW(progman, IntPtr.Zero, null, null);
        int k = 0;
        while (ch2 != IntPtr.Zero && k < 12)
        {
            sb.AppendLine(string.Format("    child[{0}] class=\"{1}\" {2} rect={3} visible={4}",
                k, Cls(ch2), Hx(ch2), Rc(ch2), IsWindowVisible(ch2)));
            ch2 = FindWindowExW(progman, ch2, null, null);
            k++;
        }
        if (k == 0) sb.AppendLine("    (none)");

        // Replicate find_wallpaper_layer() (the CURRENT algorithm) so we can see
        // EXACTLY which parent window the app would attach the video window to.
        // Step 3 (SendMessage 0x052C) is NOT executed -- it rearranges the desktop.
        sb.AppendLine();
        sb.AppendLine("=== Which parent would find_wallpaper_layer() pick? ===");

        // (1) locate the ICON layer: a *visible* top level WorkerW containing
        //     SHELLDLL_DefView (Win10 layout), or a visible WorkerW child of Progman
        //     containing it (Win11 24H2). Visibility matters: explorer restarts leave
        //     ghost SHELLDLL_DefView windows at (-32000,-317762) 0x0 behind.
        IntPtr icon = IntPtr.Zero;
        EnumWindows(delegate(IntPtr h, IntPtr p) {
            if (Cls(h) == "WorkerW" && IsWindowVisible(h) &&
                FindWindowExW(h, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero)
            { icon = h; return false; }
            return true;
        }, IntPtr.Zero);
        string iconWhere = "top level  ==> Win10 layout";
        if (icon == IntPtr.Zero)
        {
            IntPtr c = FindWindowExW(progman, IntPtr.Zero, "WorkerW", null);
            while (c != IntPtr.Zero)
            {
                if (IsWindowVisible(c) &&
                    FindWindowExW(c, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero)
                { icon = c; iconWhere = "child of Progman  ==> Win11 layout"; break; }
                c = FindWindowExW(progman, c, "WorkerW", null);
            }
        }
        if (icon == IntPtr.Zero)
        {
            sb.AppendLine("  (1) icon layer (WorkerW containing SHELLDLL_DefView) : NOT FOUND");
        }
        else
        {
            sb.AppendLine(string.Format("  (1) icon layer WorkerW = {0} rect={1} visible={2}  [{3}]",
                Hx(icon), Rc(icon), IsWindowVisible(icon), iconWhere));
            IntPtr par = GetParent(icon);
            if (par != IntPtr.Zero)
            {
                sb.AppendLine(string.Format("      GetParent() = {0} class=\"{1}\"  ==> app attaches to that PARENT",
                    Hx(par), Cls(par)));
            }
            else
            {
                // Win10: the icon layer is a TOP LEVEL window, so GetParent() returns NULL.
                // The wallpaper layer is the NEXT *visible* top level WorkerW after it
                // (created by WM_SPAWN_WORKERW), skipping anything that still contains
                // SHELLDLL_DefView. Visibility matters: real machines carry a dozen hidden
                // junk WorkerW windows (136x39 / 236x64) that must never be picked.
                IntPtr sib = FindWindowExW(IntPtr.Zero, icon, "WorkerW", null);
                int guard = 0;
                while (sib != IntPtr.Zero && guard < 16 &&
                       (!IsWindowVisible(sib) ||
                        FindWindowExW(sib, IntPtr.Zero, "SHELLDLL_DefView", null) != IntPtr.Zero))
                { sib = FindWindowExW(IntPtr.Zero, sib, "WorkerW", null); guard++; }
                if (sib != IntPtr.Zero)
                {
                    sb.AppendLine(string.Format("      GetParent() = NULL (it is a top level window)"));
                    sb.AppendLine(string.Format("      ==> app attaches to the NEXT top level WorkerW: {0} rect={1} visible={2}",
                        Hx(sib), Rc(sib), IsWindowVisible(sib)));
                }
                else
                {
                    sb.AppendLine("      GetParent() = NULL and NO sibling WorkerW follows it");
                    sb.AppendLine("      ==> app must spawn one (0x052C) before it can attach");
                }
            }
        }

        // (2) fallback: any VISIBLE WorkerW child of Progman that does NOT contain
        //     SHELLDLL_DefView -- that is the wallpaper layer on Win11 style layouts.
        IntPtr wall = IntPtr.Zero;
        IntPtr w2 = FindWindowExW(progman, IntPtr.Zero, "WorkerW", null);
        while (w2 != IntPtr.Zero)
        {
            if (IsWindowVisible(w2) && FindWindowExW(w2, IntPtr.Zero, "SHELLDLL_DefView", null) == IntPtr.Zero)
            { wall = w2; break; }
            w2 = FindWindowExW(progman, w2, "WorkerW", null);
        }
        if (wall != IntPtr.Zero)
            sb.AppendLine(string.Format("  (2) Progman child WorkerW without DefView = {0} rect={1}  ==> app falls back to IT",
                Hx(wall), Rc(wall)));
        else
            sb.AppendLine("  (2) Progman child WorkerW without DefView = none");

        if (icon == IntPtr.Zero && wall == IntPtr.Zero)
        {
            sb.AppendLine("  ==> nothing to attach to without side effects. On first attach the app");
            sb.AppendLine("      sends 0x052C (WM_SPAWN_WORKERW) and retries; the 4s watcher gets a few");
            sb.AppendLine("      more tries. If it keeps failing the window stays HIDDEN on purpose and");
            sb.AppendLine("      the panel says 'could not attach to the desktop layer'.");
            sb.AppendLine("      (Builds before 2026-10-19 showed it anyway -> the floating-image bug.)");
        }
        return sb.ToString();
    }

    // ------------------------------------------------------------------
    // Z-order walk of the WHOLE desktop tree.
    //
    // Plain EnumWindows only yields TOP LEVEL windows. Once our wallpaper window is
    // re-parented under WorkerW it is no longer top level, so EnumWindows cannot see
    // it at all -- a diagnostic built on EnumWindows silently reports "app not
    // running" exactly in the case we care about. So we walk down from the desktop
    // root instead, depth first, following GW_CHILD / GW_HWNDNEXT, which yields the
    // real Z order (index 0 = topmost).
    // ------------------------------------------------------------------
    public static List<IntPtr> ZOrderAll()
    {
        var list = new List<IntPtr>();
        IntPtr cur = GetWindow(GetDesktopWindow(), 5 /* GW_CHILD */);
        while (cur != IntPtr.Zero && list.Count < 8000)
        {
            Walk(cur, list, 0);
            cur = GetWindow(cur, 2 /* GW_HWNDNEXT */);
        }
        return list;
    }

    static void Walk(IntPtr h, List<IntPtr> list, int depth)
    {
        if (list.Count >= 8000 || depth > 8) return;
        list.Add(h);
        IntPtr c = GetWindow(h, 5 /* GW_CHILD */);
        while (c != IntPtr.Zero)
        {
            Walk(c, list, depth + 1);
            c = GetWindow(c, 2 /* GW_HWNDNEXT */);
        }
    }

    static string Slot(List<IntPtr> z, int i)
    {
        if (i < 0) return "(not found)";
        // visible matters a lot here: if the user turned desktop icons off, the
        // icon layer is hidden and the mouse passes straight through to us even
        // when our Z order is perfectly correct.
        return string.Format("index {0,-5} hwnd={1} rect={2} visible={3}", i, Hx(z[i]), Rc(z[i]), IsWindowVisible(z[i]));
    }

    public static string Scan(uint pid, bool havePid)
    {
        var sb = new StringBuilder();
        var z = ZOrderAll();

        int iDefView = -1, iList = -1, iProgman = -1;
        var defViews = new List<int>();
        var ours = new List<int>();
        for (int i = 0; i < z.Count; i++)
        {
            string c = Cls(z[i]);
            if (c == "SHELLDLL_DefView") defViews.Add(i);
            if (c == "SysListView32" && iList < 0) iList = i;
            if (c == "Progman" && iProgman < 0) iProgman = i;
            if (havePid)
            {
                uint wpid; GetWindowThreadProcessId(z[i], out wpid);
                if (wpid == pid) ours.Add(i);
            }
        }

        // A machine usually has TWO SHELLDLL_DefView windows: the real icon layer
        // (full screen, visible) and a hidden ghost left over from an explorer restart
        // (rect (-32000,-317762) 0x0, visible=False). Taking "the first one" grabs the
        // ghost and makes this report claim "desktop icons are switched off" -- which
        // sends you chasing entirely the wrong cause. Only accept a visible, non-empty one.
        foreach (int k in defViews)
        {
            RECT r;
            if (IsWindowVisible(z[k]) && GetWindowRect(z[k], out r)
                && (r.Right - r.Left) > 0 && (r.Bottom - r.Top) > 0)
            {
                iDefView = k;
                break;
            }
        }
        if (iDefView < 0 && defViews.Count > 0) iDefView = defViews[0];

        sb.AppendLine("=== Desktop window tree walked in Z order (index 0 = topmost) ===");
        sb.AppendLine("  windows walked                    = " + z.Count);
        sb.AppendLine("  Progman                           = " + Slot(z, iProgman));
        sb.AppendLine("  SHELLDLL_DefView (icon layer)     = " + Slot(z, iDefView));
        sb.AppendLine("  SysListView32    (desktop icons)  = " + Slot(z, iList));
        {
            var dv = new List<string>();
            foreach (int k in defViews) dv.Add(k + " (visible=" + IsWindowVisible(z[k]) + ")");
            sb.AppendLine("  all SHELLDLL_DefView windows      = " + (dv.Count == 0 ? "(none)" : string.Join(", ", dv.ToArray())));
        }

        if (havePid && ours.Count > 0)
        {
            var parts = new List<string>();
            foreach (int i in ours) parts.Add(i.ToString());
            sb.AppendLine("  our windows (pid " + pid + ")        = index " + string.Join(", ", parts.ToArray()));
            sb.AppendLine();
            sb.AppendLine("  --- Z ORDER VERDICT ---");
            int top = ours[0];
            if (iDefView < 0)
            {
                sb.AppendLine("  SHELLDLL_DefView not found -> cannot judge; the icon layer may be hidden.");
            }
            else if (!IsWindowVisible(z[iDefView]))
            {
                sb.AppendLine("  SHELLDLL_DefView is HIDDEN (desktop icons are switched off).");
                sb.AppendLine("  ==> The mouse reaches our window even though our Z order may be correct.");
                sb.AppendLine("  ==> Not an attach bug. The fix belongs in the frontend: swallow the menu.");
            }
            else if (top < iDefView)
            {
                sb.AppendLine("  our topmost window index " + top + " < SHELLDLL_DefView index " + iDefView);
                sb.AppendLine("  ==> ABOVE the icon layer. The mouse CAN hit our window.");
                sb.AppendLine("  ==> THIS IS AN ATTACH BUG (browser right-click menu / desktop icons hidden).");
            }
            else
            {
                sb.AppendLine("  our topmost window index " + top + " > SHELLDLL_DefView index " + iDefView);
                sb.AppendLine("  ==> BELOW the icon layer. Correct: the mouse cannot reach it.");
            }
        }
        else if (havePid)
        {
            sb.AppendLine("  our windows (pid " + pid + ")        = NONE FOUND");
            sb.AppendLine("  (the app is running but has no window in the desktop tree)");
        }

        // ---- per-window detail ----
        if (havePid && ours.Count > 0)
        {
            sb.AppendLine();
            sb.AppendLine("=== Detail of our windows ===");
            IntPtr desktop = GetDesktopWindow();
            int shown = 0;
            foreach (int i in ours)
            {
                if (shown >= 6) { sb.AppendLine("  ... (" + (ours.Count - shown) + " more omitted)"); break; }
                shown++;
                IntPtr h = z[i];
                string c = Cls(h);
                string t = Txt(h);
                if (t.Length > 40) t = t.Substring(0, 40);
                long style = GetWindowLongPtr(h, -16 /* GWL_STYLE */);
                IntPtr ga = GetAncestor(h, 1 /* GA_PARENT */);
                IntPtr gp = GetParent(h);
                sb.AppendLine("  --- z-index " + i + " ---");
                sb.AppendLine(string.Format("  hwnd={0} class=\"{1}\" title=\"{2}\"", Hx(h), c, t));
                sb.AppendLine(string.Format("  rect={0}  visible={1} minimized={2} style=0x{3:X}",
                    Rc(h), IsWindowVisible(h), IsIconic(h), style));
                sb.AppendLine(string.Format("  WS_CHILD={0}  WS_POPUP={1}",
                    (style & 0x40000000L) != 0, (style & 0x80000000L) != 0));
                sb.AppendLine(string.Format("  GetParent()={0}   GetAncestor(GA_PARENT)={1} class=\"{2}\"",
                    Hx(gp), Hx(ga), ga == IntPtr.Zero ? "null" : Cls(ga)));
                sb.AppendLine(string.Format("  parent is desktop window? {0}", ga == desktop));
                sb.AppendLine(string.Format("  parent rect = {0}", ga == IntPtr.Zero ? "(null)" : Rc(ga)));
                int n = 0;
                EnumChildWindows(h, delegate(IntPtr ch, IntPtr q) {
                    if (n < 8)
                    {
                        sb.AppendLine(string.Format("    child[{0}] class=\"{1}\" rect={2} visible={3}",
                            n, Cls(ch), Rc(ch), IsWindowVisible(ch)));
                    }
                    n++;
                    return true;
                }, IntPtr.Zero);
                sb.AppendLine(string.Format("  children total = {0}", n));
            }
        }
        return sb.ToString();
    }

    // The OS's own answer to "what is under the cursor here?".
    public static string HitTest(uint pid, bool havePid)
    {
        var sb = new StringBuilder();
        sb.AppendLine("=== WindowFromPoint probe (who does the mouse hit?) ===");
        var mons = MonRects();
        if (mons.Count == 0) { sb.AppendLine("  (no monitors)"); return sb.ToString(); }
        foreach (var m in mons)
        {
            int cx = (m.Left + m.Right) / 2, cy = (m.Top + m.Bottom) / 2;
            var pts = new POINT[5];
            pts[0] = new POINT { X = cx, Y = cy };
            pts[1] = new POINT { X = m.Left + 40, Y = m.Top + 40 };
            pts[2] = new POINT { X = m.Right - 40, Y = m.Top + 40 };
            pts[3] = new POINT { X = m.Left + 40, Y = m.Bottom - 40 };
            pts[4] = new POINT { X = m.Right - 40, Y = m.Bottom - 40 };
            sb.AppendLine(string.Format("  monitor ({0},{1}) {2}x{3}:", m.Left, m.Top, m.Right - m.Left, m.Bottom - m.Top));
            foreach (var pt in pts)
            {
                IntPtr h = WindowFromPoint(pt);
                uint wpid = 0;
                if (h != IntPtr.Zero) GetWindowThreadProcessId(h, out wpid);
                bool isOurs = havePid && wpid == pid;
                sb.AppendLine(string.Format("    ({0},{1}) -> class=\"{2}\" {3} pid={4} rect={5}{6}",
                    pt.X, pt.Y, h == IntPtr.Zero ? "?" : Cls(h), Hx(h), wpid,
                    h == IntPtr.Zero ? "(?)" : Rc(h), isOurs ? "   <== OUR WINDOW" : ""));
            }
        }
        return sb.ToString();
    }
}
'@

[DwDiag]::MakeAware()

Write-Output ""
Write-Output ("=== DPI awareness of this diagnostic = " + [DwDiag]::Awareness)
Write-Output ""

Write-Output ([DwDiag]::Monitors())
Write-Output ([DwDiag]::DesktopLayers())

$procs = @(Get-Process -Name 'dotwallpaper' -ErrorAction SilentlyContinue)
if ($procs.Count -eq 0) {
    Write-Output "=== DotWallpaper process not found (start it and turn the video wallpaper on) ==="
    Write-Output ([DwDiag]::Scan(0, $false))
} else {
    foreach ($p in $procs) {
        Write-Output ([DwDiag]::Scan([uint32]$p.Id, $true))
        Write-Output ([DwDiag]::HitTest([uint32]$p.Id, $true))
    }
}

Write-Output ""
Write-Output "=== How to read this ==="
Write-Output "  SHELLDLL_DefView visible=False"
Write-Output "      -> desktop icons are switched off; the mouse reaches us by design."
Write-Output "         Fix = swallow the context menu in the frontend."
Write-Output "  Z ORDER VERDICT says ABOVE the icon layer"
Write-Output "      -> attach bug: our window is stacked over the icons."
Write-Output "  WindowFromPoint returns our window at the corners"
Write-Output "      -> confirms, from the mouse's side, that we are reachable."
Write-Output "  Window rect == monitor rect  and  child (WebView2) rect == monitor rect"
Write-Output "      -> the wallpaper layer geometry is fine."
Write-Output "  Window rect smaller than monitor rect"
Write-Output "      -> placement failed (monitor_geo / attach path)."
Write-Output "  Window rect == monitor rect but WebView2 child smaller"
Write-Output "      -> WebView2 did not follow the window resize."
Write-Output "  parent class is not Progman/WorkerW, or parent rect != monitor rect"
Write-Output "      -> attach / coordinate-origin problem."
Write-Output "  our window is VISIBLE but its parent is the desktop window (#32769)"
Write-Output "      -> NOT attached: it is floating on the desktop as a normal window."
Write-Output "         On builds before 2026-10-19 this is the 'wallpaper image floating"
Write-Output "         on the desktop' bug; after the fix it must stay hidden instead."
Write-Output "  our window is HIDDEN and never appears"
Write-Output "      -> attach keeps failing (check the section above). The app now"
Write-Output "         refuses to show an un-attached wallpaper window on purpose."
Write-Output ""
