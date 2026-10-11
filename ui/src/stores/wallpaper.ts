// DotWallpaper 壁纸工具 - Pinia 状态仓库（主入口；类型/常量/工具/效果/来源/收藏已拆分至 wallpaper/ 子模块）
import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirmDanger, toast, toastLoading } from "../lib/naive-host";
import { useEffectState } from "./wallpaper/effect";
import { useSourceState } from "./wallpaper/sources";
import { useFavoritesState } from "./wallpaper/favorites";
import { baseName, isRemoteSrc } from "./wallpaper/utils";
import {
  DIR_STORAGE_KEY,
  BING_DIR_KEY,
  LOCAL_SORTS,
  LOCAL_SORT_KEY,
  PAGE_SIZE,
  VIDEO_ACTIVE_KEY,
  VIDEO_MUTED_KEY,
  isVideoPath,
  isWebviewPlayable,
  type WallpaperEntryData,
  type ThumbnailUpdatedPayload,
  type WallpaperItem,
  type WallpaperKind,
  type WallpaperSource,
  type BingWallpaperData,
  type DownloadedBingData,
  type BingDownloadDirInfo,
  type LocalFilter,
  type LocalSort,
} from "./wallpaper/types";

// 类型、常量与纯工具函数从子模块统一再导出，调用方 import 路径保持不变
export * from "./wallpaper/types";
export * from "./wallpaper/utils";
export * from "./wallpaper/effect";
// 后台缩略图事件监听全局只注册一次（应用单页生命周期内复用）
let thumbnailListenerRegistered = false;

/// 读取持久化的本地排序偏好。非法值（含旧版本残留、手改 localStorage）一律回退 "default"，
/// 否则下拉会拿到一个不在选项里的值、显示为空白。
function readLocalSort(): LocalSort {
  try {
    const raw = localStorage.getItem(LOCAL_SORT_KEY) ?? "";
    return (LOCAL_SORTS as readonly string[]).includes(raw) ? (raw as LocalSort) : "default";
  } catch {
    return "default";
  }
}

/// 读取持久化的动态壁纸静音偏好。默认 **false（出声）** ——
/// 用户主动把某个视频设为桌面动态壁纸时期待的是完整效果，静音是一个
/// 需要显式打开的开关，而不是"默认悄悄静音、想听还得自己找"。
function readVideoMuted(): boolean {
  try {
    return localStorage.getItem(VIDEO_MUTED_KEY) === "1";
  } catch {
    return false;
  }
}

// ---------- Pinia Store ----------
export const useWallpaperStore = defineStore("wallpaper", () => {
  // ---- 组合式子模块状态 ----
  const { wallpaperEffect, setEffect } = useEffectState();
  const {
    sourceVisibility,
    sourceOrder,
    updateSourceVisibility,
    persistSourceVisibility,
    moveSourceOrder,
    moveSourceOrderTo,
  } = useSourceState();
  const { favorites, loadFavorites, isFavorite, addFavorite, removeFavorite } =
    useFavoritesState();

  // ---- 状态 ----
  const source = ref<WallpaperSource>("local"); // 当前选项卡来源
  const currentDir = ref(""); // 自定义壁纸目录（空 = 预设目录）
  const gridItems = ref<WallpaperItem[]>([]); // 左栏当前列表
  const currentWallpaper = ref<WallpaperItem | null>(null); // 右侧当前壁纸
  const previewItem = ref<WallpaperItem | null>(null); // 正在预览的壁纸（选中态，非当前桌面）
  const desktopStyle = ref<{ style: number; tile: boolean } | null>(null); // 桌面壁纸样式
  const isApplying = ref(false); // 应用壁纸 loading
  // 必应壁纸已下载到本地的路径记录（date → 本地绝对路径）：
  // bing 列表项的 path 是远程 URL，而桌面实际使用的是下载后的本地文件，
  // 靠这份记录才能把"桌面正在显示的壁纸"对应回列表中的必应卡片（绿框）
  const bingLocalPaths = ref<Record<string, string>>({});
  // 「已下载」徽标的判定集合：**本地壁纸目录里确实存在**的必应壁纸日期。
  //
  // 与上面 bingLocalPaths 刻意分开：后者是"这个日期的文件在哪"（可能来自本次会话
  // 下载、或当前桌面壁纸的反推，位置可能已经在壁纸目录之外），服务于绿框与
  // "打开所在目录"；而徽标要回答的是"这张在「本地」选项卡里找得到吗" ——
  // 必须只认目录扫描的结果，否则点徽标跳过去会定位不到。
  const bingDownloadedDates = ref<Set<string>>(new Set());

  // 正在下载的必应壁纸 date（空串 = 没有下载在进行）。
  // 卡片按钮据此显示"下载中"并防重复点击 —— 原图数百 KB，点两下会跑两趟网络。
  const bingDownloadingDate = ref("");

  // 必应下载目录信息（设置页只读展示 + 老版本遗留目录提示）。
  // 由后端 `get_bing_download_dir_info` 填充 —— 派生规则与默认路径的真实值都在 Rust 侧。
  const bingDirInfo = ref<BingDownloadDirInfo | null>(null);

  // 必应**历史归档**的加载游标：下一个要拉的年月。
  //
  // 官方接口硬上限 16 天（`n` 被钳制在 8、`idx>8` 返回同一批），更早的靠社区归档源
  // （按月分片，约 30 条/月）。首次加载官方列表后把游标设在**最旧那条所在的月份** ——
  // 必须**含**当月而不是跳到上个月：官方只给了那个月的最后几天，前面还有大半没加载。
  // null = 已到归档起点（2016-03），不再往前拉。
  const bingArchiveCursor = ref<{ year: number; month: number } | null>(null);
  // 归档是否已经拉到底（到起点 / 连续多个月都空 / 拉取失败）。
  // 与 hasMore 是两件事：hasMore 还包含"本地已缓冲但尚未渲染"的条目。
  const bingArchiveEnd = ref(false);

  const loadingMore = ref(false);
  const allCount = ref(0);
  // 本地来源的列表加工：**类型筛选 + 名称搜索 + 排序**，三者都在分页之前作用于
  // rawEntries，因此 allCount 与"x/y"计数反映的都是加工后的数量。
  // 只作用于本地来源（其余来源类型单一或另有语义，不参与）。
  const localFilter = ref<LocalFilter>("all");
  // 名称搜索词：**不持久化** —— 一次性输入，重启后还留着会让人以为"壁纸变少了"。
  const localQuery = ref("");
  // 排序方式：持久化（属于长期偏好）
  const localSort = ref<LocalSort>(readLocalSort());
  // 本地来源原始集合中的视频数量（加工前统计）。
  // 用途：决定是否显示"图片/视频"分段控件 —— 纯图片目录给这个切换毫无意义。
  const localVideoCount = ref(0);
  const localTotalCount = ref(0);

  // 右键菜单状态
  const ctxVisible = ref(false);
  const ctxX = ref(0);
  const ctxY = ref(0);
  const ctxItem = ref<WallpaperItem | null>(null);
  const ctxReadOnly = ref(false); // 当前右键目标是否为只读来源（系统壁纸禁止删除）

  // 分页私有状态
  let loadedCount = 0;
  // 后端返回的**原始**条目（未经筛选/搜索/排序）。保留一份是为了让"改筛选条件"
  // 能纯内存重算，不必重新扫描目录。
  let rawEntries: WallpaperEntryData[] = [];
  // 当前加工后的条目（分页数据源）
  let allEntries: WallpaperEntryData[] = [];
  let loadingMoreLock = false;
  // hasMore 必须是响应式普通 ref（与 allCount 等一致，pinia 访问时解包为 boolean）：
  // 若用 computed 包普通 let 变量，let 变化不会让 computed 失效（永远 false）；
  // 若 computed getter 返回 ref，store.hasMore 拿到的又是 Ref 对象而非 boolean，
  // 模板 !store.hasMore 恒为 false，底部“已加载全部”永不显示、fill 补页判断失真。
  const hasMore = ref(false);

  // 动态壁纸状态（MVP：视频壁纸；后续 GIF/网页扩展 kind 分支）
  // monitors：生效的显示器下标；空数组 = 全部显示器
  // paused：是否暂停（冻结当前帧）；**不持久化**，停止 / 换视频都会复位
  const videoWallpaper = ref<{
    enabled: boolean;
    path: string;
    monitors: number[];
    paused: boolean;
  }>({
    enabled: false,
    path: "",
    monitors: [],
    paused: false,
  });

  // 动态壁纸静音偏好（持久化）。
  //
  // **刻意与 videoWallpaper 分开存**：前者是"用户的长期偏好"，后者是
  // "桌面此刻在播什么"。停止动态壁纸、重启应用都不该影响静音偏好 ——
  // 若把它塞进 videoWallpaper，`stopVideoWallpaper()` 与 `loadVideoWallpaperState()`
  // 每次整体重置那个对象时都会顺手把它清掉。
  //
  // 它是静音的**唯一事实来源**：后端那份只是镜像（播放页读后端状态），
  // 每次 `setVideoWallpaper` 都会把这里的值带过去。
  const videoMuted = ref(readVideoMuted());

  // 显示器选择状态：availableMonitors 来自后端 list_monitors，
  // selectedMonitors 为空数组时语义为"全部显示器"（UI 上用"全部"开关表达）
  const availableMonitors = ref<
    {
      index: number;
      /** 通俗展示名（不含分辨率/缩放/方位），直接显示即可 */
      label: string;
      /** 分辨率文本，如 "3840×2160" */
      resolution: string;
      /** 相对主屏的方位，可直接展示的短词："主屏"/"左侧"/…；单屏时为 null */
      position_hint: string | null;
      /** 兼容字段：完整字符串（含分辨率/缩放） */
      name: string;
      width: number;
      height: number;
      scale: number;
      is_primary: boolean;
    }[]
  >([]);
  const selectedMonitors = ref<number[]>([]);

  // 当前"正在预览"的显示器下标 —— **只影响预览渲染，不影响任何设置行为**。
  //
  // 它决定右侧模拟屏按哪台显示器的宽高比和分辨率来画（多屏宽高比可能不同，
  // 如 16:9 主屏 + 21:9 带鱼屏，不跟着切换就会看到错误的预览比例）。
  //
  // 为什么与设置行为解耦：
  //   - 静态壁纸走 Windows 原生全局路径（`SPI_SETDESKWALLPAPER` 只接受单张图，
  //     系统本身没有"每屏不同壁纸"的接口），必然作用于所有屏幕 —— 切这个值无意义；
  //   - 动态壁纸的播放范围由下方的 selectedMonitors 显式选择决定 —— 也不读这个值。
  // 顶部切显示器因此是一个**纯预览操作**，绝不会改动用户桌面。
  //
  // 与 selectedMonitors 的区别：
  //   - activeMonitor 是**单个**显示器，表示"我现在在看哪一台"
  //   - selectedMonitors 是**一组**显示器，表示"动态壁纸要在哪几台上播放"
  // 两者语义完全独立：可以在 2 号屏上预览，而动态壁纸播放范围是 1+2 号屏。
  const activeMonitor = ref(0);

  // 当前选中的显示器元信息（找不到时回退第一台，保证 UI 永远有可用对象）
  const activeMonitorInfo = computed(
    () =>
      availableMonitors.value.find((m) => m.index === activeMonitor.value) ??
      availableMonitors.value[0] ??
      null
  );

  // 是否有多台显示器（单台时显示器选择器与相关分支整体隐藏）
  const hasMultipleMonitors = computed(() => availableMonitors.value.length > 1);

  // ---- Getter ----
  // 右侧大预览目标：优先"正在预览"，无预览时回退当前桌面壁纸
  const previewTarget = computed<WallpaperItem | null>(
    () => previewItem.value ?? currentWallpaper.value
  );

  // 本地来源的筛选器是否显示：仅本地来源且确实存在视频时才显示
  //（纯图片目录给"图片/视频"切换毫无意义，反成噪音）
  const showLocalFilter = computed(() =>
    source.value === "local" && !!localVideoCount.value
  );

  // 本地工具条（名称搜索 + 排序）是否显示：仅本地来源且有内容时。
  // 判据用**原始**总数而不是加工后的数量 —— 否则搜索到 0 条时工具条会自己消失，
  // 用户连清空搜索词的入口都没有了。
  const showLocalTools = computed(
    () => source.value === "local" && localTotalCount.value > 0
  );

  // ---- 右键菜单 ----
  function openContextMenu(item: WallpaperItem, x: number, y: number) {
    ctxItem.value = item;
    ctxX.value = Math.min(x, window.innerWidth - 216);
    ctxY.value = Math.min(y, window.innerHeight - 220);
    ctxVisible.value = true;
    // 系统壁纸 / 收藏夹 / 必应均为只读视图：右键菜单不提供删除入口。
    // - 系统壁纸：删了会破坏 Windows 自带壁纸库
    // - 收藏夹：书签视图，删除入口仍由本地/系统来源提供，避免误删
    // - 必应：在线项本地无文件；本地缓存项（超出在线窗口的老壁纸）虽有文件，
    //   但删除入口统一留在「本地」选项卡（下载目录在本地壁纸目录下，天然可见），
    //   避免同一个文件在两个选项卡里各有一套删除入口
    ctxReadOnly.value =
      source.value === "system" ||
      source.value === "favorites" ||
      item.kind === "bing";
  }

  function closeContextMenu() {
    ctxVisible.value = false;
    ctxItem.value = null;
    ctxReadOnly.value = false;
  }

  // ---- 后台缩略图渐进更新 ----
  // 后端列表命令不再等待全量缩略图生成：缺失项由后台线程池渐进生成，
  // 每完成一张推送 "thumbnail-updated" { path, thumb }。命中当前列表条目时
  // 仅更新该条 thumb（响应式触发对应 <img> :src 重算加载新图），不整列表刷新；
  // 尚未被懒加载放行的条目无需处理，放行时自然取到最新 thumb。
  async function registerThumbnailListener() {
    if (thumbnailListenerRegistered) return;
    thumbnailListenerRegistered = true;
    await listen<ThumbnailUpdatedPayload>("thumbnail-updated", (ev) => {
      const { path, thumb } = ev.payload;
      if (!path || !thumb) return;
      const item = gridItems.value.find((it) => it.path === path);
      if (item && item.thumb !== thumb) {
        item.thumb = thumb;
      }
    });
  }
  void registerThumbnailListener();

  // ---- 选项卡切换 ----
  async function setSource(next: WallpaperSource) {
    if (source.value === next) return;
    source.value = next;
    gridItems.value = [];
    loadedCount = 0;
    allEntries = [];
    hasMore.value = false;
    allCount.value = 0;
    await loadWallpapers();
  }

  // 设置来源可见性：仅负责"隐藏当前选项卡时自动切回本地"的联动，
  // 可见性更新与持久化由 useSourceState.updateSourceVisibility 完成
  function setSourceVisibility(key: "system" | "favorites" | "bing", visible: boolean) {
    updateSourceVisibility(key, visible);
    if (!visible && source.value === key) {
      void setSource("local");
    }
  }

  // ---- 设置壁纸（核心） ----
  // 必应在线壁纸：先把原图下载到本地（目录由壁纸目录派生），再交给 set_wallpaper
  // —— Win32 SPI_SETDESKWALLPAPER 只接受本地文件路径
  async function ensureLocalPath(item: WallpaperItem): Promise<string> {
    if (item.kind !== "bing" || !isRemoteSrc(item.path || "")) return item.path || "";
    // 下载目录不可单独配置：后端按 `<壁纸目录>\BingWallpaper` 派生，
    // 这里只需把当前壁纸目录传过去（空 = 后端用默认图片目录）
    const local = (await invoke("download_bing_wallpaper", {
      url: item.path || "",
      date: item.date || "",
      directory: resolveDirArg(),
    })) as string;
    if (item.date && local) {
      bingLocalPaths.value = { ...bingLocalPaths.value, [item.date]: local };
      // 刚下载完的文件一定在壁纸目录里，徽标立刻点亮（不必等下一次扫描）
      const next = new Set(bingDownloadedDates.value);
      next.add(item.date);
      bingDownloadedDates.value = next;
    }
    return local;
  }

  // 只下载、**不设置桌面**。
  //
  // 用户明确要"必应我只想下载、不想换桌面"这条路径 —— 与 `setItemAsDesktop`
  // 共用 `ensureLocalPath`（同一套派生目录 + 同名文件复用），所以：
  //   · 重复点不会重复走网络；
  //   · 下载完 `bingDownloadedDates` 立刻更新，「已下载」徽标当场点亮，
  //     不必等下一次目录扫描；
  //   · `bingLocalPaths` 一并登记 —— 它只用于"打开所在目录"和"当前壁纸绿框"，
  //     而绿框要求 `=== currentWallpaper.path`，下载不改桌面 ⇒ 不会误亮。
  async function downloadBingWallpaper(item: WallpaperItem): Promise<boolean> {
    if (item.kind !== "bing") return false;
    if (isBingDownloaded(item)) {
      toast("这张已经在本地壁纸目录里了", "warning");
      return true;
    }
    bingDownloadingDate.value = item.date || item.path || "";
    try {
      const local = await ensureLocalPath(item);
      if (!local) throw new Error("未返回本地路径");
      // 提示里报可读日期而不是完整文件名 —— 文件名带 `BingWallpaper_` 前缀，读起来费劲
      const date = bingDateFromPath(local);
      toast(`已下载到本地：${date ? bingDateQuery(date) : baseName(local)}`, "success");
      return true;
    } catch (err: unknown) {
      toast("下载失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      bingDownloadingDate.value = "";
    }
  }

  // 从本地文件名反推必应日期（`BingWallpaper_YYYYMMDD.jpg`）。
  //
  // **`BingWallpaper_` 前缀是"这是必应下载的"的唯一标记**，正则必须锚定它 ——
  // 否则用户自己的 `IMG_20260917.jpg` 会被当成必应壁纸改写展示名。
  // 只认自家命名：`.jpg.part` 这类半截文件结尾不是 `.jpg`，自然落选。
  // 扩展名大小写不敏感（与 Rust 侧 `parse_cached_name` 同一口径）。
  const BING_FILE_RE = /^bingwallpaper_(\d{8})\.jpg$/i;

  function bingDateFromPath(path: string): string {
    const m = BING_FILE_RE.exec(baseName(path || ""));
    return m ? m[1] : "";
  }

  // 跳转定位用的搜索词：取可读形式 `2026-09-17`（与 bingDateTitle 里那一段一致），
  // 在搜索框里比 `20260917` 更像个日期，用户一眼知道该怎么清掉。
  function bingDateQuery(date: string): string {
    return /^\d{8}$/.test(date)
      ? `${date.slice(0, 4)}-${date.slice(4, 6)}-${date.slice(6, 8)}`
      : date;
  }

  // 必应壁纸在「本地」列表里的展示名。"必应每日壁纸"本来就以日期为身份，
  // 用日期比露出 `BingWallpaper_20260917.jpg` 这个原始文件名有信息量。
  function bingDateTitle(date: string): string {
    return /^\d{8}$/.test(date)
      ? `必应壁纸 ${date.slice(0, 4)}-${date.slice(4, 6)}-${date.slice(6, 8)}`
      : "必应壁纸";
  }

  // 给条目补上"必应下载文件"的展示名（只认 `BingWallpaper_*` 前缀，其它文件原样返回）。
  //
  // ⚠️ 必须作用在 **allEntries / rawEntries 这一层**（loadWallpapers 里赋值之后、
  // `rawEntries = allEntries` 之前），不能设在 appendBatch —— refineEntries 读的是
  // allEntries，设在 appendBatch 里的话**搜索和排序都看不到这个标题**。
  function withBingTitle(e: WallpaperEntryData): WallpaperEntryData {
    const date = bingDateFromPath(e.path);
    return date ? { ...e, title: bingDateTitle(date) } : e;
  }

  // `YYYYMMDD` → 归档用的年月（按月分片）。非法日期返回 null。
  function yearMonthOf(date: string): { year: number; month: number } | null {
    if (!/^\d{8}$/.test(date)) return null;
    return { year: Number(date.slice(0, 4)), month: Number(date.slice(4, 6)) };
  }

  // 归档游标往前推一个月；到 2016-03（仓库归档起点）返回 null 表示到底。
  function prevArchiveMonth(
    cursor: { year: number; month: number }
  ): { year: number; month: number } | null {
    if (cursor.month > 1) return { year: cursor.year, month: cursor.month - 1 };
    if (cursor.year > 2016) return { year: cursor.year - 1, month: 12 };
    return null;
  }

  // 刷新"哪些日期的必应壁纸已经在本地了"——「已下载」徽标的唯一数据来源。
  //
  // 判据刻意是**扫本地壁纸目录**（后端 `list_downloaded_bing_wallpapers`），
  // 而不是记"我下载过谁"：这样徽标与「本地」选项卡能否看到该文件共用同一个事实
  // 来源，徽标亮着就一定能跳过去定位到；反过来也不会出现"文件早被删了徽标还亮着"。
  // 失败静默 —— 徽标不显示而已，不该拖垮整个必应列表。
  async function refreshDownloadedBingDates(): Promise<void> {
    try {
      const list = (await invoke("list_downloaded_bing_wallpapers", {
        directory: resolveDirArg(),
      })) as DownloadedBingData[];
      bingDownloadedDates.value = new Set(list.map((d) => d.date).filter(Boolean));
    } catch {
      bingDownloadedDates.value = new Set();
    }
  }

  // 必应在线卡片是否"本地已下载"（左上角徽标判据）
  function isBingDownloaded(item: WallpaperItem): boolean {
    if (item.kind !== "bing" || !item.date) return false;
    return bingDownloadedDates.value.has(item.date);
  }

  // 刷新必应下载目录信息（设置页打开时调一次）。
  //
  // 顺带把老版本遗留键 `BING_DIR_KEY` 的值带给后端：那个键**只读不写** ——
  // 它记录的独立下载目录已经不再是配置项，但里面的历史文件仍值得提示一句。
  // 失败静默置 null：设置页降级成"不显示目录行"，不该报错打扰用户。
  //
  // ⚠️ 这里的两侧命名**故意不对称**，别"统一"成一种：
  //   传入的命令参数用 camelCase（`legacyDir`）—— Tauri 默认把 JS 的 camelCase
  //     参数映射到 Rust 的 snake_case 形参（同 `set_close_behavior` 的 `hideToTray`）；
  //   读回的返回字段用 snake_case（`legacy_dir`）—— 返回值是 serde 直接序列化，
  //     本项目所有结构体都没有 `rename_all`，线上格式就是 Rust 字段名。
  // 两边都写错不会报任何错，只会让 legacy_dir 恒为 undefined、提示永不出现。
  async function refreshBingDirInfo(): Promise<void> {
    let legacy: string | null = null;
    try {
      legacy = localStorage.getItem(BING_DIR_KEY) || null;
    } catch {
      legacy = null;
    }
    try {
      bingDirInfo.value = (await invoke("get_bing_download_dir_info", {
        directory: resolveDirArg(),
        legacyDir: legacy,
      })) as BingDownloadDirInfo;
    } catch {
      bingDirInfo.value = null;
    }
  }

  // 点徽标：跳到「本地」选项卡并定位到那张文件。
  //
  // 用**搜索词定位**而不是滚动定位：分页是 12 张/页、`loadMore` 还有 200ms 防抖，
  // 几千张的图库要滚到第 N 张得循环加载几十秒；而 refineEntries 在**分页之前**过滤，
  // 把搜索词设成该日期就一步到位。同时把类型筛选复位成"全部"，
  // 否则当前停在"视频"时照样什么都看不到。
  async function openInLocal(item: WallpaperItem): Promise<void> {
    if (item.kind !== "bing" || !item.date) return;
    if (source.value !== "local") await setSource("local");
    if (localFilter.value !== "all") setLocalFilter("all");
    setLocalQuery(bingDateQuery(item.date));
  }

  // 某项是否为"当前已设置到桌面的壁纸"
  // （必应项 path 为远程 URL，需经下载记录比对；本地项直接比对路径）
  // 视频项走动态壁纸状态判定（isCurrentVideo），不参与静态壁纸路径比对
  function isCurrentItem(item: WallpaperItem): boolean {
    if (item.kind === "video") return isCurrentVideo(item);
    const cur = currentWallpaper.value;
    if (!cur?.path) return false;
    if (item.kind === "bing") {
      const local = item.date ? bingLocalPaths.value[item.date] : "";
      return !!local && local === cur.path;
    }
    return !!item.path && item.path === cur.path;
  }

  async function doSetWallpaper(item: WallpaperItem): Promise<{ path: string }> {
    const payload = item.path || "";
    const result = (await invoke("set_wallpaper", {
      path: payload,
      dir: resolveDirArg(),
    })) as { path: string } | string;
    return typeof result === "string" ? { path: result } : result;
  }

  function resolveDirArg(): string | null {
    return currentDir.value && currentDir.value.trim()
      ? currentDir.value.trim()
      : null;
  }

  // 点击卡片：仅选中/预览，不改动桌面（选中态与当前桌面解耦）
  function selectItem(item: WallpaperItem) {
    previewItem.value = item;
  }

  // 读取系统当前壁纸样式（填充/适应等），供预览与"设为壁纸"使用
  async function loadDesktopStyle() {
    try {
      const s = (await invoke("get_wallpaper_style")) as { style: number; tile: boolean };
      desktopStyle.value = {
        style: Number(s.style) || 10,
        tile: Boolean(s.tile),
      };
    } catch (err: unknown) {
      console.error("读取壁纸样式失败:", err);
      desktopStyle.value = null;
    }
  }

  // 将某张壁纸设为桌面壁纸；可选同步应用桌面样式（仅当与系统当前样式不一致时写注册表）
  // 视频项（kind === "video"）走动态壁纸分支：不写静态壁纸，不套用壁纸效果
  async function setItemAsDesktop(
    item: WallpaperItem,
    style?: { style: number; tile: boolean }
  ): Promise<boolean> {
    if (isApplying.value) return false;
    const path = item.path || "";
    if (!path) {
      toast("壁纸路径无效", "warning");
      return false;
    }
    // 视频动态壁纸：后端创建置底 WebView 窗口播放，与静态壁纸是两条独立路径
    if (item.kind === "video") {
      return setVideoWallpaper(path);
    }
    isApplying.value = true;
    try {
      // 必应壁纸：先下载原图到本地缓存，再进入效果合成或直接设置
      // （apply_wallpaper_effect / set_wallpaper 均只接受本地文件路径）
      const localPath = await ensureLocalPath(item);
      if (!localPath) {
        toast("壁纸下载失败，无法设置", "error");
        return false;
      }
      // 启用模糊遮罩效果时：后端合成（模糊+遮罩）后设置，样式注册表一并写入
      if (wallpaperEffect.value.enabled) {
        const styleToApply = style ?? desktopStyle.value ?? { style: 10, tile: false };
        const result = (await invoke("apply_wallpaper_effect", {
          path: localPath,
          style: styleToApply.style,
          tile: styleToApply.tile,
          effect: {
            enabled: true,
            blur: wallpaperEffect.value.blur,
            opacity: wallpaperEffect.value.opacity,
            color: wallpaperEffect.value.color,
          },
        })) as { path: string };
        desktopStyle.value = { ...styleToApply };
        currentWallpaper.value = {
          key: "current_" + (result.path || ""),
          kind: "local",
          path: result.path,
          title: item.kind === "bing" ? item.title || undefined : undefined,
        };
        toast("壁纸设置成功", "success");
        return true;
      }

      if (
        style &&
        desktopStyle.value &&
        (style.style !== desktopStyle.value.style || style.tile !== desktopStyle.value.tile)
      ) {
        await invoke("set_desktop_style", { style: style.style, tile: style.tile });
        desktopStyle.value = { ...style };
      }
      const result = await doSetWallpaper({ ...item, path: localPath });
      currentWallpaper.value = {
        key: "current_" + (result.path || ""),
        kind: "local",
        path: result.path,
        // 保留必应中文标题，避免右侧"当前壁纸"只剩 BingWallpaper_20260911.jpg 这类文件名
        title: item.kind === "bing" ? item.title || undefined : undefined,
      };
      toast("壁纸设置成功", "success");
      return true;
    } catch (err: unknown) {
      toast("设置失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      isApplying.value = false;
    }
  }

  // 将右侧正在预览（无预览时为当前桌面）的壁纸设为桌面（Ctrl+S / 设为壁纸按钮）
  async function applyPreviewAsDesktop(
    style?: { style: number; tile: boolean }
  ): Promise<boolean> {
    const target = previewTarget.value;
    if (!target || !target.path) {
      toast("当前无可预览壁纸", "warning");
      return false;
    }
    return setItemAsDesktop(target, style);
  }

  // ---- 壁纸源加载（分页） ----
  // 本地来源的列表加工：**类型筛选 + 名称搜索 + 排序**。
  //
  // 三者都必须在**分页之前**完成，allCount / hasMore / "x/y" 计数才与可见列表一致。
  // 其余来源（system/favorites/bing）原样返回，不参与加工。
  function refineEntries(entries: WallpaperEntryData[]): WallpaperEntryData[] {
    if (source.value !== "local") return entries;

    let out = entries;

    // ① 类型筛选：按 kind 判定图片/视频。判定口径与 appendBatch 一致（按扩展名），
    //    保证"筛出来的"与"列表里显示的"完全相同。
    if (localFilter.value !== "all") {
      const wantVideo = localFilter.value === "video";
      out = out.filter((e) => {
        const kind = e.kind ?? (isVideoPath(e.path) ? "video" : "local");
        return wantVideo ? kind === "video" : kind !== "video";
      });
    }

    // ② 名称搜索：匹配**标题与文件名**（都只看名称、不含目录），大小写不敏感的子串。
    //    **两者都要匹配，不能用 `title || baseName` 二选一** —— 必应下载下来的文件
    //    带了"必应壁纸 2026-09-17"这种标题，一旦 title 存在就会把文件名遮掉，
    //    用户按 `BingWallpaper_20260917` 或标题里的字搜就什么都搜不到了。
    const q = localQuery.value.trim().toLowerCase();
    if (q) {
      out = out.filter((e) =>
        `${e.title || ""} ${baseName(e.path)}`.toLowerCase().includes(q)
      );
    }

    // ③ 排序：default 保持后端扫描顺序（不做任何排序，等于旧行为）
    if (localSort.value !== "default") {
      const nameOf = (e: WallpaperEntryData) => e.title || baseName(e.path);
      // 自然序：wallpaper2 排在 wallpaper10 前面（纯字典序会反过来）；
      // sensitivity:"base" 让大小写不影响比较
      const byName = (a: WallpaperEntryData, b: WallpaperEntryData) =>
        nameOf(a).localeCompare(nameOf(b), undefined, { numeric: true, sensitivity: "base" });
      // 数值比较。**缺失的排序键一律沉底，且不随方向翻转** ——
      // 否则"升序"会把 stat 失败、读不到时间的文件顶到最前面，看起来像排序坏了。
      const byNum = (a: number | null | undefined, b: number | null | undefined, dir: 1 | -1) => {
        const av = a ?? null;
        const bv = b ?? null;
        if (av === null && bv === null) return 0;
        if (av === null) return 1;
        if (bv === null) return -1;
        return (av - bv) * dir;
      };

      const sorted = [...out];
      switch (localSort.value) {
        case "name-asc":
          sorted.sort(byName);
          break;
        case "name-desc":
          sorted.sort((a, b) => byName(b, a));
          break;
        case "mtime-desc":
          sorted.sort((a, b) => byNum(a.mtime, b.mtime, -1));
          break;
        case "mtime-asc":
          sorted.sort((a, b) => byNum(a.mtime, b.mtime, 1));
          break;
        case "size-desc":
          sorted.sort((a, b) => byNum(a.size, b.size, -1));
          break;
        case "size-asc":
          sorted.sort((a, b) => byNum(a.size, b.size, 1));
          break;
      }
      out = sorted;
    }

    return out;
  }

  // 用当前的筛选/搜索/排序**重算列表**（不重新扫描目录）。
  //
  // 这是搜索能逐字符实时响应的前提：后端 list_local_wallpapers 是一次真实的目录扫描
  // 加逐文件 stat，每敲一个字都调一遍既慢又无谓 —— 原始条目已经在 rawEntries 里了。
  // 顺带把 setLocalFilter 从"重新加载"改成"重算"，切类型也不再触发目录扫描。
  function applyLocalRefine() {
    allEntries = refineEntries(rawEntries);
    allCount.value = allEntries.length;
    // 同一个 tick 内清空再 appendBatch，Vue 只会渲染一次，不会看到列表闪空
    gridItems.value = [];
    loadedCount = 0;
    appendBatch();
  }

  // 切换本地类型筛选
  function setLocalFilter(next: LocalFilter) {
    if (localFilter.value === next) return;
    localFilter.value = next;
    if (source.value === "local") applyLocalRefine();
  }

  // 输入名称搜索词（逐字符实时生效：加工全在内存里完成）
  function setLocalQuery(next: string) {
    if (localQuery.value === next) return;
    localQuery.value = next;
    if (source.value === "local") applyLocalRefine();
  }

  // 切换排序方式并持久化
  function setLocalSort(next: LocalSort) {
    if (localSort.value === next) return;
    localSort.value = next;
    try {
      localStorage.setItem(LOCAL_SORT_KEY, next);
    } catch {
      /* localStorage 不可用时静默降级：本次排序仍生效，只是不持久化 */
    }
    if (source.value === "local") applyLocalRefine();
  }

  async function loadWallpapers() {
    gridItems.value = [];
    loadedCount = 0;
    rawEntries = [];
    allEntries = [];
    // 归档游标一并复位：否则必应加载失败时会把上一次的游标留着，
    // 之后切回来滚到底会从错误的月份开始拉（bing 分支成功时会重设，这里是兜底）
    bingArchiveCursor.value = null;
    bingArchiveEnd.value = false;
    try {
      // 后端列表返回：原图路径 + 缩略图路径（缩略图可能为空 → 列表显示占位）
      if (source.value === "system") {
        allEntries = (await invoke("list_system_wallpapers")) as WallpaperEntryData[];
      } else if (source.value === "favorites") {
        // 收藏夹：按收藏路径在本地/系统两个来源中取交集，复用缩略图缓存
        allEntries = await loadFavoriteEntries();
      } else if (source.value === "bing") {
        // 必应每日壁纸 = **纯在线图库**（远程原图 URL + 远程 400x240 缩略图）。
        //
        // 下载下来的文件归「本地」选项卡管 —— 下载目录就在本地壁纸目录里面，
        // 递归扫描天然能看到，所以**不要把本地缓存并进这个列表**，
        // 否则同一个文件会在两个选项卡里各出现一次。
        // 已下载的那几张只在这里打个"已下载"徽标回指（refreshDownloadedBingDates）。
        //
        // 首批只取官方接口的最近 16 天（实时、权威）；更早的历史由归档源逐月补
        // （滚动触发，见 `loadMoreBingArchive`）。
        const list = (await invoke("list_bing_wallpapers")) as BingWallpaperData[];
        allEntries = list.map((b) => ({
          path: b.url,
          thumb: b.thumb,
          title: b.title,
          date: b.date,
          kind: "bing" as WallpaperKind,
        }));
        // 归档游标 = **最旧那条所在的月份**（列表是倒序，故取末条）。必须**含**当月：
        // 官方只给了那个月的最后几天，前面还有大半没加载过，跳到上个月会漏掉它们。
        bingArchiveCursor.value = yearMonthOf(list.length ? list[list.length - 1].date : "");
        bingArchiveEnd.value = false;
        // 徽标数据在列表就位之后再取：它失败只该影响徽标，不该让整个列表报错
        await refreshDownloadedBingDates();
      } else {
        allEntries = (await invoke("list_local_wallpapers", {
          directory: resolveDirArg(),
        })) as WallpaperEntryData[];
      }
      // 给必应下载下来的文件补上认得出的展示名（`BingWallpaper_20260917.jpg` →
      // `必应壁纸 2026-09-17`）。
      // **必须在这一层做**：refineEntries 的搜索与排序读的都是 allEntries/rawEntries，
      // 设到 gridItems 上它们看不见。
      allEntries = allEntries.map(withBingTitle);

      // 原始集合存档：之后改筛选/搜索/排序都在它上面纯内存重算，不再重新扫描目录
      rawEntries = allEntries;

      // 统计原始集合（加工前）：视频数量决定"图片/视频"分段控件是否显示，总数供比例展示
      if (source.value === "local") {
        localTotalCount.value = rawEntries.length;
        localVideoCount.value = rawEntries.filter((e) => {
          const kind = e.kind ?? (isVideoPath(e.path) ? "video" : "local");
          return kind === "video";
        }).length;
        // 分段控件将因"已无视频"而隐藏时，强制回退到"全部"：
        // 否则会停留在"视频"筛选却看不到任何切换入口（控件已消失）
        if (!localVideoCount.value && localFilter.value !== "all") {
          localFilter.value = "all";
        }
      } else {
        localTotalCount.value = 0;
        localVideoCount.value = 0;
      }

      // 加工（筛选 + 搜索 + 排序）必须在分页之前：allCount / hasMore / x-y 计数都由此派生
      applyLocalRefine();
    } catch (err: unknown) {
      toast("加载壁纸失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  function appendBatch() {
    const batch = allEntries.slice(loadedCount, loadedCount + PAGE_SIZE);
    batch.forEach((e) => {
      // 条目可能自带 kind（必应在线壁纸）/ title / date，缺省按本地壁纸处理；
      // 本地来源中的视频按扩展名分类为 "video"（动态壁纸，走另一套预览/设置流程）
      const kind = e.kind ?? (isVideoPath(e.path) ? "video" : "local");
      gridItems.value.push({
        key: `${kind}_${e.path}`,
        kind,
        path: e.path,
        title: e.title || baseName(e.path),
        thumb: e.thumb || undefined,
        date: e.date,
      });
      loadedCount++;
    });
    // 「还有更多」= 本地缓冲还没渲染完 **或** 必应还有更早的归档月份可拉。
    // 后半句不能漏：必应首屏只有 15 条，缓冲渲染完后 hasMore 若为 false，
    // 底部会错误显示"已加载全部"，`fillGridUntilOverflow` 也会提前收手不再拉归档。
    hasMore.value =
      loadedCount < allEntries.length ||
      (source.value === "bing" && !bingArchiveEnd.value && bingArchiveCursor.value !== null);
  }

  async function loadMore() {
    if (loadingMoreLock) return;
    // 必应来源：本地缓冲已渲染完 ⇒ 去归档源拉更早的一个月（其余来源只有缓冲这一层）
    if (source.value === "bing" && loadedCount >= allEntries.length && !bingArchiveEnd.value) {
      loadingMoreLock = true;
      loadingMore.value = true;
      try {
        await loadMoreBingArchive();
      } finally {
        loadingMoreLock = false;
        loadingMore.value = false;
      }
      return;
    }
    if (!hasMore.value) return;
    loadingMoreLock = true;
    loadingMore.value = true;
    try {
      await new Promise((r) => setTimeout(r, 200)); // 防抖
      appendBatch();
    } finally {
      loadingMoreLock = false;
      loadingMore.value = false;
    }
  }

  // 从归档源往前拉一个月，追加进列表。
  //
  // 与官方那 16 天必然重叠（当月月末几天）—— 按 date 去重，重叠的丢弃。
  // **连续 3 个月都没有新数据才判定到底**：归档早期有缺月，单月为空不能当终点。
  // 拉取失败（所有 CDN 镜像都不通）直接停下并提示一次，不反复重试 ——
  // 否则用户每滚一下弹一个错；已加载的部分不受影响。
  async function loadMoreBingArchive(): Promise<void> {
    let rounds = 0;
    while (rounds < 3) {
      rounds += 1;
      const cursor = bingArchiveCursor.value;
      if (!cursor) break;
      let list: BingWallpaperData[];
      try {
        list = (await invoke("list_bing_archive_month", {
          year: cursor.year,
          month: cursor.month,
        })) as BingWallpaperData[];
      } catch (err: unknown) {
        toast(
          "加载更早的必应壁纸失败：" + ((err as Error)?.message || String(err)),
          "error"
        );
        bingArchiveEnd.value = true;
        hasMore.value = false;
        return;
      }
      bingArchiveCursor.value = prevArchiveMonth(cursor);
      const known = new Set(allEntries.map((e) => e.date).filter(Boolean));
      const fresh = list.filter((b) => b.date && !known.has(b.date));
      if (!fresh.length) continue; // 整月都是重叠（或该月本就没有归档），继续往前
      allEntries = allEntries.concat(
        fresh.map((b) => ({
          path: b.url,
          thumb: b.thumb,
          title: b.title,
          date: b.date,
          kind: "bing" as WallpaperKind,
        }))
      );
      rawEntries = allEntries;
      allCount.value = allEntries.length;
      appendBatch(); // 只渲染 PAGE_SIZE 条，其余留给下一次滚动（hasMore 由它设置）
      return;
    }
    bingArchiveEnd.value = true;
    hasMore.value = false;
  }

  // 获取当前桌面壁纸文件（用于右侧展示）
  async function loadCurrentWallpaper() {
    try {
      const path = (await invoke("get_current_wallpaper")) as string;
      if (path) {
        currentWallpaper.value = { key: "current_" + path, kind: "local", path };
        // 桌面壁纸若来自必应下载目录（`BingWallpaper_YYYYMMDD.jpg`），
        // 登记 date → 本地路径，保证启动后切到必应来源时对应卡片仍能显示"当前"绿框
        const bingDate = bingDateFromPath(path);
        if (bingDate) {
          bingLocalPaths.value = { ...bingLocalPaths.value, [bingDate]: path };
        }
      } else {
        currentWallpaper.value = null;
      }
    } catch (err: unknown) {
      toast("获取当前壁纸失败：" + ((err as Error)?.message || String(err)), "error");
      currentWallpaper.value = null;
    }
  }

  // ---- 目录持久化与选择 ----
  function restoreDir() {
    try {
      const saved = localStorage.getItem(DIR_STORAGE_KEY);
      if (saved) currentDir.value = saved;
    } catch {
      /* ignore */
    }
  }

  async function pickAndApplyDirectory() {
    try {
      const picked = (await invoke("pick_wallpaper_directory")) as string | null;
      if (picked) {
        currentDir.value = picked;
        try {
          localStorage.setItem(DIR_STORAGE_KEY, picked);
        } catch {
          /* ignore */
        }
        toast("已选择目录：" + picked, "success");
        await loadWallpapers();
      }
    } catch (err: unknown) {
      toast("选择目录失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  // ---- 动态壁纸（MVP：视频） ----

  // 上次生效的动态壁纸记录：用于**应用重启后自动恢复**（见 restoreVideoWallpaper）。
  // 后端状态是进程内的，重启即清空，所以这份记录是恢复的唯一依据。
  //
  // 读写一律吞掉异常：隐私模式/存储被禁用时 localStorage 会抛错，
  // 而"恢复不了动态壁纸"只是少一项便利，不该让整个 store 初始化失败。
  function readVideoActive(): { path: string; monitors: number[] } | null {
    try {
      const raw = localStorage.getItem(VIDEO_ACTIVE_KEY);
      if (!raw) return null;
      const o = JSON.parse(raw) as { path?: unknown; monitors?: unknown };
      if (typeof o?.path !== "string" || !o.path) return null;
      return {
        path: o.path,
        monitors: Array.isArray(o.monitors)
          ? o.monitors.filter((i): i is number => typeof i === "number")
          : [],
      };
    } catch {
      return null;
    }
  }

  function writeVideoActive(path: string, monitors: number[]) {
    try {
      localStorage.setItem(VIDEO_ACTIVE_KEY, JSON.stringify({ path, monitors }));
    } catch {
      /* ignore */
    }
  }

  function clearVideoActive() {
    try {
      localStorage.removeItem(VIDEO_ACTIVE_KEY);
    } catch {
      /* ignore */
    }
  }

  // 选择本地视频并设为桌面动态壁纸（后端挂载置底 WebView 窗口播放）
  // 返回是否设置成功，供列表卡片 / 右侧按钮统一提示
  //
  // `opts.silent`：抑制成功 toast，供**启动自动恢复**使用 —— 那时用户没有主动操作，
  // 弹一条"已启用"是噪音（面板上的状态与文件名本身就是反馈）。失败仍会提示。
  async function setVideoWallpaper(
    path: string,
    monitors?: number[],
    opts?: { silent?: boolean }
  ): Promise<boolean> {
    if (!path) return false;
    // mkv / mov 在 WebView2 上大概率无法解码，置底窗口只会黑屏：
    // 提前拦截并给出明确原因，避免用户以为功能失效
    if (!isWebviewPlayable(path)) {
      toast(
        "该视频格式（mkv/mov）无法在 WebView 中播放，请改用 mp4 / webm",
        "warning"
      );
      return false;
    }
    if (isApplying.value) return false;
    isApplying.value = true;
    // 未显式传 monitors 时，沿用当前界面上的选择（保持用户偏好）
    const sel = monitors ?? selectedMonitors.value;
    try {
      await invoke("set_video_wallpaper", {
        path,
        monitors: sel,
        // 把持久化的静音偏好一并带给后端：后端状态是**进程内**的，应用重启后
        // 回到默认值，而播放页读的是后端状态 —— 不带过去就会"上次静音了、
        // 这次启用又有声音"。
        muted: videoMuted.value,
      });
      // 设完回读一次后端状态，而不是在本地拼一份：
      // `paused` 的取舍规则（换视频复位、只改播放范围保持）只写在后端一处，
      // 本地再抄一遍就是第二份会漂移的规则。
      await loadVideoWallpaperState();
      selectedMonitors.value = [...sel];
      // 记下"上次生效项"，下次启动据此自动恢复。
      // 只改播放范围也要更新（applyVideoMonitors 走的是同一条路径）——
      // 否则恢复时会用回旧范围，用户上次调过的选择就丢了。
      writeVideoActive(path, sel);
      if (opts?.silent) return true;
      const scope = sel.length
        ? `（${sel.length} 台显示器）`
        : "（全部显示器）";
      // 提示**不带文件名**：用户刚从列表里点的就是这个文件，名字已经在眼前，
      // 再念一遍是冗余；而长文件名会挤爆 toast 甚至换行。失败时才带具体原因。
      toast("动态壁纸已启用" + scope, "success");
      return true;
    } catch (err: unknown) {
      toast(
        "视频壁纸启用失败：" + ((err as Error)?.message || String(err)),
        "error"
      );
      return false;
    } finally {
      isApplying.value = false;
    }
  }

  // 仅切换生效显示器（不换视频）：复用当前视频路径 + 新选择
  async function applyVideoMonitors(monitors: number[]): Promise<boolean> {
    const cur = videoWallpaper.value;
    if (!cur.enabled || !cur.path) return false;
    return setVideoWallpaper(cur.path, monitors);
  }

  // 切换动态壁纸静音。一次写三处：
  //   ① 本地偏好 ref —— UI 立即反馈（开关状态不等待 IPC 往返）
  //   ② localStorage   —— 持久化，下次设动态壁纸仍然记得
  //   ③ 后端           —— 由后端广播给置底窗口（播放页只认后端状态）
  async function setVideoMuted(muted: boolean): Promise<boolean> {
    videoMuted.value = muted;
    try {
      localStorage.setItem(VIDEO_MUTED_KEY, muted ? "1" : "0");
    } catch {
      /* 存储不可用时只影响"下次是否记得"，不影响本次播放 */
    }
    try {
      await invoke("set_video_wallpaper_muted", { muted });
      return true;
    } catch (err: unknown) {
      toast(
        "静音设置失败：" + ((err as Error)?.message || String(err)),
        "error"
      );
      return false;
    }
  }

  // 暂停 / 恢复动态壁纸：冻结当前帧（**不销毁置底窗口**，桌面不会退回静态壁纸），
  // 恢复时从暂停处继续。与 stopVideoWallpaper 的区别见其注释。
  async function setVideoPaused(paused: boolean): Promise<boolean> {
    try {
      await invoke("set_video_wallpaper_paused", { paused });
      videoWallpaper.value = { ...videoWallpaper.value, paused };
      return true;
    } catch (err: unknown) {
      toast(
        "暂停设置失败：" + ((err as Error)?.message || String(err)),
        "error"
      );
      return false;
    }
  }

  // 拉取显示器列表（右侧显示器选择与动态壁纸共用）
  async function loadMonitors() {
    try {
      availableMonitors.value = (await invoke("list_monitors")) as typeof availableMonitors.value;
    } catch {
      availableMonitors.value = [];
    }
    // 当前操作对象失效（拔掉显示器 / 首次加载）时回退到第一台
    if (!availableMonitors.value.some((m) => m.index === activeMonitor.value)) {
      activeMonitor.value = availableMonitors.value[0]?.index ?? 0;
    }
  }

  // 切换"正在预览"的显示器（**纯预览操作，不改变任何桌面设置**）
  function setActiveMonitor(index: number) {
    activeMonitor.value = index;
  }

  // 停止动态壁纸：销毁置底窗口并复位状态（桌面回到静态壁纸）
  // **静音偏好不清** —— 它是长期偏好，不是本次播放的临时状态。
  async function stopVideoWallpaper() {
    await invoke("stop_video_wallpaper");
    videoWallpaper.value = {
      enabled: false,
      path: "",
      monitors: [],
      paused: false,
    };
    // 清除"上次生效项"：停止是明确的用户意图，
    // 不清掉的话下次启动会自作主张地又播起来。
    clearVideoActive();
  }

  // 某项是否为"当前正在播放的动态壁纸"（列表绿框 / 右侧状态展示共用）
  function isCurrentVideo(item: WallpaperItem): boolean {
    return (
      item.kind === "video" &&
      videoWallpaper.value.enabled &&
      !!item.path &&
      item.path === videoWallpaper.value.path
    );
  }

  // 启动/恢复主界面时同步动态壁纸状态（播放页挂载也用它拉取兜底）
  //
  // **刻意不读后端的 muted**：静音的事实来源是本地偏好（videoMuted），
  // 后端那份只是它的镜像。若在这里用后端的值覆盖本地偏好，
  // 应用重启后（后端 muted 回到默认 false）就会把用户上次的静音选择抹掉。
  async function loadVideoWallpaperState() {
    try {
      const st = (await invoke("get_video_wallpaper_state")) as {
        enabled: boolean;
        path: string;
        monitors?: number[];
        paused?: boolean;
      };
      const monitors = Array.isArray(st.monitors) ? st.monitors : [];
      videoWallpaper.value = {
        enabled: !!st.enabled,
        path: st.path || "",
        monitors,
        paused: !!st.paused,
      };
      // 恢复上次的显示器选择（空数组 = 全部）
      if (monitors.length) selectedMonitors.value = [...monitors];
    } catch {
      /* ignore */
    }
  }

  // 应用启动时**自动恢复**上次的动态壁纸：退出前设过、且没有被主动停止的话，
  // 重放一次即可（后端状态是进程内的，重启后必然是空的）。
  //
  // 为什么放在前端而不是后端 setup：持久化走 localStorage 是本项目约定；
  // 且这里复用既有的 setVideoWallpaper 路径（回读状态 + 带静音偏好），
  // 后端无需新增存储与序列化代码。
  //
  // **失败就清除记录**：最常见原因是视频文件已被删除/移动（后端会校验存在性并
  // 返回 Err），留着只会让每次启动都白试一次、白弹一条错误。
  async function restoreVideoWallpaper() {
    const saved = readVideoActive();
    if (!saved) return;
    // 后端已经在同一路径上播放 → 无需重放。常见于页面重载 / HMR：
    // 那时进程没退出、后端状态还在，再设一次纯属多余（虽不会重建窗口，
    // 但会把播放范围重算一遍）。真正的冷启动后端状态必为空，不会走到这里。
    if (videoWallpaper.value.enabled && videoWallpaper.value.path === saved.path) {
      return;
    }

    // 显示器可能已变化（拔屏 / 换接口 / 改为扩展模式）：先拉列表再丢失效索引。
    // 后端 targets_of 同样会过滤，但前端先滤一遍能让"播放范围"的勾选同步正确。
    await loadMonitors();
    let monitors = saved.monitors;
    if (availableMonitors.value.length) {
      const valid = monitors.filter((i) => i < availableMonitors.value.length);
      // 全部失效 → 回退"全部显示器"（空数组），而不是一台都不播
      monitors = valid.length ? valid : [];
    }

    const ok = await setVideoWallpaper(saved.path, monitors, { silent: true });
    if (!ok) clearVideoActive();
  }

  // ---- 收藏夹 ----
  // 收藏页数据：直接按收藏路径向后端查询，不经过当前壁纸目录，
  // 因此切换壁纸目录后收藏依然完整；已被外部删除的失效路径由后端过滤不展示
  async function loadFavoriteEntries(): Promise<WallpaperEntryData[]> {
    const favPaths = [...favorites.value];
    if (!favPaths.length) return [];
    return (await invoke<WallpaperEntryData[]>("list_wallpapers_by_paths", {
      paths: favPaths,
    })) as WallpaperEntryData[];
  }

  // 从当前已加载列表移除某路径（收藏页取消收藏时即时消失，避免整表重载闪烁）
  function dropFavoriteFromGrid(path: string) {
    allEntries = allEntries.filter((e) => e.path !== path);
    gridItems.value = gridItems.value.filter((it) => it.path !== path);
    loadedCount = gridItems.value.length;
    allCount.value = allEntries.length;
    hasMore.value = loadedCount < allEntries.length;
  }

  // 增删收藏书签；返回是否已收藏（true = 刚加入）
  function toggleFavorite(path: string | undefined | null): boolean {
    if (!path) return false;
    // 在线壁纸（必应）不可收藏：其 path 是远程 URL，收藏夹按本地路径向后端检索，
    // 收藏它只会留下永远命中不了的失效书签
    if (isRemoteSrc(path)) return false;
    const had = isFavorite(path);
    if (had) removeFavorite(path);
    else addFavorite(path);

    // 在收藏夹页取消收藏：立即移除卡片并清空对应预览，避免幽灵项
    if (had && source.value === "favorites") {
      if (previewItem.value?.path === path) previewItem.value = null;
      dropFavoriteFromGrid(path);
    }
    return !had;
  }

  // ---- 外部拖入保存 ----
  // 将 Tauri 原生拖放事件给出的本地文件路径复制到壁纸目录并刷新列表。
  // 图片与视频都可拖入（放行规则与目录扫描一致）；视频可能很大，后端已把复制
  // 放到 blocking 线程，这里再补一个"进行中"提示，免得用户以为拖入没生效。
  async function saveDroppedPaths(paths: string[]) {
    if (!paths.length) return;

    const closeLoading = toastLoading(`正在导入 ${paths.length} 个文件…`);
    let res: { saved: string[]; skipped: string[] } | null = null;
    try {
      res = (await invoke("save_dropped_paths", {
        paths,
        dir: resolveDirArg(),
      })) as { saved: string[]; skipped: string[] };
    } catch (err: unknown) {
      toast("保存失败：" + ((err as Error)?.message || String(err)), "error");
      return;
    } finally {
      closeLoading();
    }

    if (!res) return;

    if (res.saved.length) {
      // 有跳过项时把**第一条原因**带出来（如"文件夹里没有壁纸文件"、
      // "文件太多，本次只扫描前 500 个"、"a.mkv: mkv 无法在桌面播放"）——
      // 只报个数字用户无从判断发生了什么。
      // 后端给的是**纯原因**，不带"已跳过"这类后缀，拼出来才不重复。
      const msg =
        `已保存 ${res.saved.length} 个壁纸` +
        (res.skipped.length ? `，跳过 ${res.skipped.length} 个：${res.skipped[0]}` : "");
      toast(msg, res.skipped.length ? "warning" : "success");
      await loadWallpapers();
    } else if (res.skipped.length) {
      toast("没有可保存的文件：" + res.skipped[0], "warning");
    }
  }

  // ---- 右键菜单动作 ----
  async function handleContextAction(action: string, item: WallpaperItem | null) {
    closeContextMenu();
    if (!item) return;

    if (action === "set-wallpaper") {
      // 设置桌面不改变"正在预览"的选中态：蓝（预览）与绿（桌面）独立。
      // 静态图统一走原生全局设置；视频项由 setItemAsDesktop 内部转交动态壁纸分支，
      // 那里会按"当前操作显示器"决定播放范围。
      await setItemAsDesktop(item);
      return;
    }

    // 只下载到本地壁纸目录，不改变桌面（必应在线壁纸专用）
    if (action === "download-bing") {
      if (item.kind === "bing") await downloadBingWallpaper(item);
      return;
    }

    if (action === "toggle-favorite") {      // 收藏夹以本地文件路径为书签，必应选项卡不参与该体系
      //（下载后的文件在「本地」选项卡里可正常收藏）
      if (item.kind === "bing") {
        toast("必应壁纸暂不支持收藏", "warning");
        return;
      }
      // 收藏 / 取消收藏：仅书签标记，不删文件
      const fav = toggleFavorite(item.path);
      if (fav) toast("已收藏：" + baseName(item.path || ""), "success");
      else toast("已取消收藏", "warning");
      return;
    }

    if (action === "reveal-folder") {
      // 在系统资源管理器中打开文件所在目录并定位（本地/系统壁纸通用）。
      // 必应分两种：在线项要先查下载记录（未下载则给引导提示）；
      // 本地缓存项（超出在线窗口的老壁纸）item.path 本身就是本地文件，直接定位。
      const localPath =
        item.kind === "bing"
          ? (item.date && bingLocalPaths.value[item.date]) ||
            (!isRemoteSrc(item.path || "") ? item.path || "" : "")
          : item.path || "";
      if (!localPath) {
        toast(
          item.kind === "bing"
            ? "这张还没下载到本地，先点卡片上的「下载」存到壁纸目录"
            : "当前壁纸无本地路径",
          "warning"
        );
        return;
      }
      try {
        await invoke("reveal_in_explorer", { path: localPath });
      } catch (err: unknown) {
        toast("打开目录失败：" + ((err as Error)?.message || String(err)), "error");
      }
      return;
    }

    if (action === "delete-wallpaper" && item.kind !== "current") {
      // 系统壁纸来源只读：纵深防御，禁止进入删除流程；
      // 必应选项卡同样不提供删除（在线项本地无文件；本地缓存项的删除入口
      // 统一留在「本地」选项卡，避免同一文件两处可删）
      if (source.value === "system" || item.kind === "bing") return;
      await removeWallpaper(item);
    }
  }

  // 右键删除：本地磁盘永久删除壁纸文件
  async function removeWallpaper(item: WallpaperItem): Promise<void> {
    if (!item.path) return;
    // 只读保护：系统壁纸来源不允许删除（同时后端 delete_wallpaper 亦拦截系统路径）
    if (source.value === "system") return;
    const name = baseName(item.path);

    const confirmed = await confirmDanger({
      title: "删除壁纸",
      content: `确定要删除壁纸「${name}」吗？\n该文件会从本地磁盘永久删除，无法恢复。`,
      positiveText: "删除",
    });
    if (!confirmed) return;

    try {
      await invoke("delete_wallpaper", { path: item.path });
      // 文件已物理删除：同步清理收藏书签，避免收藏夹出现失效路径
      if (isFavorite(item.path)) removeFavorite(item.path);
      // 删除的正是正在播放的动态壁纸：同步停掉置底窗口，避免窗口指向已删文件
      if (isCurrentVideo(item)) {
        await stopVideoWallpaper();
      }
      toast("已删除壁纸：" + name, "success");
      await loadWallpapers();
      // 删除的正是当前桌面壁纸：同步刷新桌面真值
      if (currentWallpaper.value?.path === item.path) {
        await loadCurrentWallpaper();
      }
      // 删除的正是正在预览的壁纸：清空选中态，大预览回退到当前桌面
      if (previewItem.value?.path === item.path) {
        previewItem.value = null;
      }
    } catch (err: unknown) {
      toast("删除失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  return {
    // state
    source,
    currentDir,
    gridItems,
    currentWallpaper,
    previewItem,
    desktopStyle,
    isApplying,
    loadingMore,
    allCount,
    wallpaperEffect,
    ctxVisible,
    ctxX,
    ctxY,
    ctxItem,
    ctxReadOnly,
    sourceVisibility,
    sourceOrder,
    localFilter,
    localQuery,
    localSort,
    localVideoCount,
    localTotalCount,
    bingDirInfo,
    bingDownloadingDate,
    // getters
    hasMore,
    previewTarget,
    showLocalFilter,
    showLocalTools,
    isCurrentItem,
    isCurrentVideo,
    isBingDownloaded,
    videoWallpaper,
    videoMuted,
    availableMonitors,
    selectedMonitors,
    activeMonitor,
    activeMonitorInfo,
    hasMultipleMonitors,
    // actions
    setSource,
    setLocalFilter,
    setLocalQuery,
    setLocalSort,
    setActiveMonitor,
    openContextMenu,
    closeContextMenu,
    selectItem,
    openInLocal,
    downloadBingWallpaper,
    refreshBingDirInfo,
    loadFavorites,
    isFavorite,
    toggleFavorite,
    setItemAsDesktop,
    applyPreviewAsDesktop,
    loadDesktopStyle,
    loadWallpapers,
    loadMore,
    loadCurrentWallpaper,
    restoreDir,
    pickAndApplyDirectory,
    saveDroppedPaths,
    handleContextAction,
    removeWallpaper,
    setSourceVisibility,
    persistSourceVisibility,
    setVideoWallpaper,
    applyVideoMonitors,
    setVideoMuted,
    setVideoPaused,
    loadMonitors,
    stopVideoWallpaper,
    loadVideoWallpaperState,
    restoreVideoWallpaper,
    setEffect,
    moveSourceOrder,
    moveSourceOrderTo,
  };
});
