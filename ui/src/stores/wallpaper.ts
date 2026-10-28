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
  VIDEO_SPEEDS,
  VIDEO_SPEED_KEY,
  VIDEO_LOOP_KEY,
  type VideoLoopMode,
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
  type PixabayDownloadDirInfo,
  type LocalFilter,
  type LocalSort,
  PIXABAY_KEY_KEY,
  PIXABAY_CATEGORY_KEY,
  PIXABAY_ORDER_KEY,
  PIXABAY_QUERY_KEY,
  PIXABAY_PAGE_SIZE,
  PIXABAY_MEDIA_KEY,
  PIXABAY_IMAGE_TYPE_KEY,
  PIXABAY_ORIENTATION_KEY,
  PIXABAY_VIDEO_ORIENTATION_KEY,
  PIXABAY_SIZE_KEY,
  PIXABAY_COLORS_KEY,
  PIXABAY_VIDEO_TYPE_KEY,
  PIXABAY_SIZES,
  PIXABAY_COLORS,
  PIXABAY_VIDEO_QUALITY_KEY,
  type PixabayMedia,
  type PixabayImageType,
  type PixabayOrientation,
  type PixabaySize,
  type PixabayVideoType,
  type PixabayVideoQuality,
  type PixabayWallpaperData,
  type PixabayPageData,
  type PixabayVideoData,
  type PixabayVideoPageData,
  type DownloadedPixabayData,
  // Wallhaven
  WALLHAVEN_QUERY_KEY,
  WALLHAVEN_CATEGORY_KEY,
  WALLHAVEN_SORT_KEY,
  WALLHAVEN_ATLEAST_KEY,
  WALLHAVEN_RATIO_KEY,
  WALLHAVEN_COLOR_KEY,
  WALLHAVEN_API_BASE_KEY,
  WALLHAVEN_IMAGE_BASE_KEY,
  WALLHAVEN_CATEGORIES,
  WALLHAVEN_SORTS,
  WALLHAVEN_ATLEASTS,
  WALLHAVEN_RATIOS,
  WALLHAVEN_COLORS,
  type WallhavenCategory,
  type WallhavenSort,
  type WallhavenAtleast,
  type WallhavenRatio,
  type WallhavenWallpaperData,
  type WallhavenPageData,
  type DownloadedWallhavenData,
  type WallhavenDownloadDirInfo,
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

/// 读取用户自填的 Pixabay API key（空串 = 还没填，列表会显示引导）
function readPixabayKey(): string {
  try {
    return (localStorage.getItem(PIXABAY_KEY_KEY) ?? "").trim();
  } catch {
    return "";
  }
}

/// 读取 Pixabay 分类偏好（空串 = 全部分类）
function readPixabayCategory(): string {
  try {
    return localStorage.getItem(PIXABAY_CATEGORY_KEY) ?? "";
  } catch {
    return "";
  }
}

/// 读取 Pixabay 排序偏好，非法值回退 popular
function readPixabayOrder(): string {
  try {
    const raw = localStorage.getItem(PIXABAY_ORDER_KEY) ?? "";
    return raw === "latest" || raw === "popular" ? raw : "popular";
  } catch {
    return "popular";
  }
}

/// 一次性读出「媒体 + 内容类型」，并顺手完成旧值迁移。
///
/// ⚠️ **必须合成一个函数**：旧版本把"插画"存在**媒体键**里（值 `illustration`），
/// 新版本媒体键只认 `image` / `video`。如果先读媒体再读内容类型、且读媒体时
/// 顺手把键归一化成 `image`，那么 `illustration` 这个信号就永久丢失了 ——
/// 老用户升级后"插画"会静默变回"照片"，只会觉得自己的选择莫名其妙没了。
/// 所以两个值必须在同一次读取里都取到，再一起落盘归一化。
function readPixabayMediaState(): { media: PixabayMedia; imageType: PixabayImageType } {
  let media: PixabayMedia = "image";
  let imageType: PixabayImageType = "photo";
  try {
    const legacy = localStorage.getItem(PIXABAY_MEDIA_KEY) ?? "";
    media = legacy === "video" ? "video" : "image";
    const raw = localStorage.getItem(PIXABAY_IMAGE_TYPE_KEY) ?? "";
    if (raw === "photo" || raw === "illustration" || raw === "vector" || raw === "all") {
      imageType = raw;
    } else if (legacy === "illustration") {
      // 旧版三段媒体里的"插画" → 新版的内容类型
      imageType = "illustration";
    }
    // 归一化落盘：下次启动不必再猜。写失败（隐私模式）不影响本次生效。
    localStorage.setItem(PIXABAY_MEDIA_KEY, media);
    localStorage.setItem(PIXABAY_IMAGE_TYPE_KEY, imageType);
  } catch {
    /* localStorage 不可用：用默认值，本次仍可用 */
  }
  return { media, imageType };
}

/// 读取图像方向，默认横向（壁纸场景只吃横图，与旧版硬编码一致）
function readPixabayOrientation(): PixabayOrientation {
  try {
    const raw = localStorage.getItem(PIXABAY_ORIENTATION_KEY) ?? "";
    return raw === "vertical" || raw === "all" ? raw : "horizontal";
  } catch {
    return "horizontal";
  }
}

/// 读取**视频**方向，默认横向。
///
/// 与图片方向分开存：竖屏视频当横屏壁纸会被裁成中间一条，但竖屏显示器上正好相反 ——
/// 用户对两者的偏好未必一致，共用一个键会让切媒体时"方向"莫名其妙地跳。
function readPixabayVideoOrientation(): PixabayOrientation {
  try {
    const raw = localStorage.getItem(PIXABAY_VIDEO_ORIENTATION_KEY) ?? "";
    return raw === "vertical" || raw === "all" ? raw : "horizontal";
  } catch {
    return "horizontal";
  }
}

/// 读取尺寸档位，默认 `"1920"`（= 旧版硬编码的 min_width=1920）。
///
/// **必须区分"键不存在"与"值是空串"**：空串是用户主动选的"不限"，
/// 用 `?? ""` 会把"没存过"也当成"不限"，导致升级后图片突然多出一堆小图。
function readPixabaySize(): PixabaySize {
  try {
    const raw = localStorage.getItem(PIXABAY_SIZE_KEY);
    if (raw === null) return "1920";
    return raw === "" || raw === "1920" || raw === "2560" || raw === "3840" ? raw : "1920";
  } catch {
    return "1920";
  }
}

/// 读取已选颜色（数组）。存 JSON；任何非法值一律过滤掉而不是整体丢弃 ——
/// 接口颜色值将来增删时，旧数据里失效的那一两个不该让其余选择一起消失。
function readPixabayColors(): string[] {
  try {
    const raw = localStorage.getItem(PIXABAY_COLORS_KEY) ?? "";
    if (!raw) return [];
    const arr: unknown = JSON.parse(raw);
    if (!Array.isArray(arr)) return [];
    const allowed = PIXABAY_COLORS.map((c) => c.value as string);
    return arr.filter((v): v is string => typeof v === "string" && allowed.includes(v));
  } catch {
    return [];
  }
}

/// 读取视频类型，默认 all
function readPixabayVideoType(): PixabayVideoType {
  try {
    const raw = localStorage.getItem(PIXABAY_VIDEO_TYPE_KEY) ?? "";
    return raw === "film" || raw === "animation" ? raw : "all";
  } catch {
    return "all";
  }
}

/// 读取上次的搜索词。
///
/// 持久化是**有意为之**：不持久化时每次启动首屏都是全站热门杂图
/// （见 `PIXABAY_QUERY_KEY` 的注释）。空串仍然合法 —— 那就是"随便逛逛"，
/// 后端会走"背景类目 + 精选"的预设，不是无 q 直查。
function readPixabayQuery(): string {
  try {
    return localStorage.getItem(PIXABAY_QUERY_KEY) ?? "";
  } catch {
    return "";
  }
}

/// 读取视频清晰度偏好，非法值回退 medium（1080p）
function readPixabayVideoQuality(): PixabayVideoQuality {
  try {
    return localStorage.getItem(PIXABAY_VIDEO_QUALITY_KEY) === "large" ? "large" : "medium";
  } catch {
    return "medium";
  }
}

// ---- Wallhaven 偏好读取 ----
//
// 与 Pixabay 同一套原则：**非法值一律回退默认**。手改 localStorage、旧版本残留
// 都会让 NSelect 拿到一个不在选项里的值、显示为空白（不报错，最难查）。

function readWallhavenQuery(): string {
  try {
    return localStorage.getItem(WALLHAVEN_QUERY_KEY) ?? "";
  } catch {
    return "";
  }
}

/// 分类默认「全部」（111）
function readWallhavenCategory(): WallhavenCategory {
  try {
    const raw = localStorage.getItem(WALLHAVEN_CATEGORY_KEY) ?? "";
    return (WALLHAVEN_CATEGORIES as readonly { value: string }[]).some((c) => c.value === raw)
      ? (raw as WallhavenCategory)
      : "111";
  } catch {
    return "111";
  }
}

/// 排序默认「热门」（favorites）。`date_added` 首屏太杂，
/// `favorites` 更接近"壁纸站首页"的观感，与 Pixabay 默认 popular 同理。
function readWallhavenSort(): WallhavenSort {
  try {
    const raw = localStorage.getItem(WALLHAVEN_SORT_KEY) ?? "";
    return (WALLHAVEN_SORTS as readonly { value: string }[]).some((s) => s.value === raw)
      ? (raw as WallhavenSort)
      : "favorites";
  } catch {
    return "favorites";
  }
}

function readWallhavenAtleast(): WallhavenAtleast {
  try {
    const raw = localStorage.getItem(WALLHAVEN_ATLEAST_KEY);
    if (raw === null) return "";
    return (WALLHAVEN_ATLEASTS as readonly { value: string }[]).some((s) => s.value === raw)
      ? (raw as WallhavenAtleast)
      : "";
  } catch {
    return "";
  }
}

function readWallhavenRatio(): WallhavenRatio {
  try {
    const raw = localStorage.getItem(WALLHAVEN_RATIO_KEY);
    if (raw === null) return "";
    return (WALLHAVEN_RATIOS as readonly { value: string }[]).some((s) => s.value === raw)
      ? (raw as WallhavenRatio)
      : "";
  } catch {
    return "";
  }
}

/// 颜色默认不筛（空串）。存的是 6 位十六进制（不带 #）。
function readWallhavenColor(): string {
  try {
    const raw = localStorage.getItem(WALLHAVEN_COLOR_KEY) ?? "";
    return (WALLHAVEN_COLORS as readonly { value: string }[]).some((c) => c.value === raw)
      ? raw
      : "";
  } catch {
    return "";
  }
}

/// 自定义接口地址，默认空串（= 走官方）。
///
/// 这里**只做粗筛**：非 `http(s)://` 开头的一律当没填，避免用户粘了一串
/// 中文说明或少了协议头，导致整个源静默不可用。真正的规范化（补 `/api/v1/search`、
/// 吃掉尾斜杠）在后端 `resolve_api_base` 里，两边各管一段。
function readWallhavenApiBase(): string {
  try {
    const raw = (localStorage.getItem(WALLHAVEN_API_BASE_KEY) ?? "").trim();
    if (!raw) return "";
    return /^https?:\/\//i.test(raw) ? raw : "";
  } catch {
    return "";
  }
}

/// 自定义图片地址，默认空串（= 官方 CDN，不替换）。
///
/// 粗筛口径与接口地址一致：不是 `http(s)://` 开头就当没填 —— 宁可回退官方，
/// 也不要让一串错字把所有缩略图变成 404。
function readWallhavenImageBase(): string {
  try {
    const raw = (localStorage.getItem(WALLHAVEN_IMAGE_BASE_KEY) ?? "").trim();
    if (!raw) return "";
    return /^https?:\/\//i.test(raw) ? raw : "";
  } catch {
    return "";
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

/// 读取持久化的播放速率偏好。
///
/// **必须校验取值是否在 `VIDEO_SPEEDS` 里**，而不是"能解析成数字就用"：
/// 旧版本残留、手改 localStorage、甚至把后端上限 3.0 写进去，
/// 都会让 UI 上的速率高亮一个都点不亮（没有任何一项匹配），
/// 看起来像"速度设置坏了"。非法值一律回退 1.0。
function readVideoSpeed(): number {
  try {
    const n = Number(localStorage.getItem(VIDEO_SPEED_KEY));
    return (VIDEO_SPEEDS as readonly number[]).includes(n) ? n : 1;
  } catch {
    return 1;
  }
}

/// 读取持久化的循环模式偏好。非法值一律回退 "loop" ——
/// 与 `readLocalSort` 同一条规则：下拉/高亮里出现一个不在选项里的值，
/// 表现就是"一个都不亮"，比回退成默认值更难懂。
function readVideoLoop(): VideoLoopMode {
  try {
    return localStorage.getItem(VIDEO_LOOP_KEY) === "once" ? "once" : "loop";
  } catch {
    return "loop";
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

  // ---- Pixabay 在线图库 ----
  //
  // 与必应的结构差异：必应一次把最近 16 天全给回来、更早的按月归档补；
  // Pixabay 是**服务端分页**（每页 24 条），滚到底再拉下一页。
  // 所以这里多一组 `pixabayPage` / `pixabayHasMore`，且 hasMore 以后端返回的
  // `has_more` 为准 —— 不能像本地那样靠"缓冲里还剩几条"推算。

  /// 用户自填的 API key（空 = 未配置，列表显示引导而不是报错）
  const pixabayKey = ref(readPixabayKey());
  /// 搜索词。**持久化** —— 不持久化时首屏永远是全站热门杂图，见 `readPixabayQuery`
  const pixabayQuery = ref(readPixabayQuery());
  const pixabayCategory = ref(readPixabayCategory());
  const pixabayOrder = ref(readPixabayOrder());
  /// 已加载到第几页（0 = 还没加载过）
  const pixabayPage = ref(0);
  /// 服务端说还有下一页
  const pixabayHasMore = ref(false);
  const pixabayLoading = ref(false);
  /// true = 免费 key，只拿到 1280px（没有 full API access）
  const pixabayDegraded = ref(false);
  /// Pixabay 图片 id → 本地路径。对应必应的 `bingLocalPaths`，但**刻意不复用同一份**：
  /// 两边身份键不同（id vs 日期），混在一起会让 `bingDateFromPath` 的日期正则误判。
  const pixabayLocalPaths = ref<Record<string, string>>({});
  /// 本地壁纸目录里确实存在的 Pixabay 图片 id（「已下载」徽标的唯一判据）
  const pixabayDownloadedIds = ref<Set<string>>(new Set());
  /// 正在下载的图片 id（空串 = 没有下载在进行）
  const pixabayDownloadingId = ref("");

  // ---- Pixabay 视频（同一选项卡内的另一种媒体）----
  //
  // 分页状态**与图片共用** `pixabayPage` / `pixabayHasMore`：同一时刻只显示一种媒体，
  // 两套独立游标只会让"切换媒体后忘了复位"变成新的 bug 来源。
  // 切换媒体 ⇒ 整个列表重来，这是符合直觉的（搜索结果本来就是重新搜的）。

  /// 当前看的是图片还是视频（只有这两档，两者是两套接口）
  const pixabayMediaState = readPixabayMediaState();
  const pixabayMedia = ref<PixabayMedia>(pixabayMediaState.media);

  // ---- Pixabay 筛选条件（对应网页筛选栏）----
  //
  // 这些**全部是服务端参数**（除了视频接口不支持的那几个，见下），
  // 改任何一个都要复位分页重拉 —— 由各自的 setter 统一处理。
  //
  // ⚠️ 能力边界（实测确认，别再试）：
  //   - `orientation` / `colors` 只有**图片**接口支持，视频接口传了会被静默忽略
  //     ⇒ 视频模式下这两项**不展示、也不下发**（见 fetchPixabayVideoPage）
  //   - 「发布日期」接口**根本没有对应参数**，网页上那一项做不了
  //   - 「尺寸」只能做"至少 N×M"，接口给的是 min_width / min_height 下限，
  //     没有精确分辨率筛选
  const pixabayImageType = ref<PixabayImageType>(pixabayMediaState.imageType);
  const pixabayOrientation = ref<PixabayOrientation>(readPixabayOrientation());
  /// 视频方向。后端按宽高本地筛（视频接口没有 `orientation` 参数）。
  const pixabayVideoOrientation = ref<PixabayOrientation>(readPixabayVideoOrientation());
  const pixabaySize = ref<PixabaySize>(readPixabaySize());
  const pixabayColors = ref<string[]>(readPixabayColors());
  const pixabayVideoType = ref<PixabayVideoType>(readPixabayVideoType());
  /// 视频清晰度偏好（设置页可改）
  const pixabayVideoQuality = ref<PixabayVideoQuality>(readPixabayVideoQuality());
  /// 已下载的 Pixabay **视频** id。
  /// 与图片那份 `pixabayDownloadedIds` 刻意分开：落盘前缀不同
  /// （`PixabayVideo_` vs `Pixabay_`），两份集合混用会让徽标互相点亮。
  const pixabayDownloadedVideoIds = ref<Set<string>>(new Set());
  /// Pixabay 视频 id → 本地路径（"打开所在目录"用，对应图片的 `pixabayLocalPaths`）
  const pixabayVideoLocalPaths = ref<Record<string, string>>({});

  // 必应下载目录信息（设置页只读展示 + 老版本遗留目录提示）。
  // 由后端 `get_bing_download_dir_info` 填充 —— 派生规则与默认路径的真实值都在 Rust 侧。
  const bingDirInfo = ref<BingDownloadDirInfo | null>(null);
  // Pixabay 下载目录（`<壁纸目录>\Pixabay`），同样只在设置页只读展示。
  const pixabayDirInfo = ref<PixabayDownloadDirInfo | null>(null);

  // ---- Wallhaven 在线图库 ----
  //
  // 结构与 Pixabay 图片那条同构（服务端分页 + 远程 URL + 设为壁纸才下载），
  // 但**没有视频分支**、**不需要 key**，身份键是 6 位字母数字 id。
  //
  // 唯一的额外状态是 `wallhavenSeed`：`sorting = random` 时第一页响应会带一个
  // seed，后续页必须原样回传，否则每翻一页都是新的一批随机图（会看到重复图、
  // 也永远翻不到底）。任何筛选变更都必须把 seed 清掉，让接口重新生成一批。
  const wallhavenQuery = ref(readWallhavenQuery());
  const wallhavenCategory = ref<WallhavenCategory>(readWallhavenCategory());
  const wallhavenSort = ref<WallhavenSort>(readWallhavenSort());
  const wallhavenAtleast = ref<WallhavenAtleast>(readWallhavenAtleast());
  const wallhavenRatio = ref<WallhavenRatio>(readWallhavenRatio());
  /// 颜色（6 位十六进制，空串 = 不筛）
  const wallhavenColor = ref(readWallhavenColor());
  /// 自定义接口地址（空串 = 官方）。
  ///
  /// 只解决"当前网络连不上 wallhaven.cc"这一种情况：用户自建反代 / 用可用的
  /// 加速地址时填在这里。**不内置任何第三方镜像** —— 镜像无授权、可能篡改内容、
  /// 随时会死，写进产品等于给自己埋雷。
  const wallhavenApiBase = ref(readWallhavenApiBase());
  /// 自定义图片地址（空串 = 官方 CDN）。
  ///
  /// 单独一项而不是复用接口地址：反代通常只配了 `/api/`，
  /// 缩略图仍指向 `th.wallhaven.cc` —— 这就是"列表有图、缩略图全灰"的成因。
  const wallhavenImageBase = ref(readWallhavenImageBase());
  /// 已加载到第几页（0 = 还没加载过）
  const wallhavenPage = ref(0);
  const wallhavenHasMore = ref(false);
  const wallhavenLoading = ref(false);
  /// 随机排序的种子（空串 = 没有 / 需要重新生成）
  const wallhavenSeed = ref("");
  /// Wallhaven 图片 id → 本地路径（"打开所在目录"与绿框判定用）
  const wallhavenLocalPaths = ref<Record<string, string>>({});
  /// 本地壁纸目录里确实存在的 Wallhaven id（「已下载」徽标的唯一判据）
  const wallhavenDownloadedIds = ref<Set<string>>(new Set());
  /// 正在下载的 id（空串 = 没有下载在进行）
  const wallhavenDownloadingId = ref("");
  /// 最近一次加载失败的原始错误（空串 = 没有错误）。
  ///
  /// 单独存一份的原因：toast 三秒就消失，而"连不上 Wallhaven"是一个**持续状态**——
  /// 列表空着、用户不知道是没结果还是没连上，得在空态里常驻显示 + 给重试入口。
  const wallhavenError = ref("");
  // Wallhaven 下载目录（`<壁纸目录>\Wallhaven`），只在设置页只读展示。
  const wallhavenDirInfo = ref<WallhavenDownloadDirInfo | null>(null);

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
  // paused：**用户手动暂停**（冻结当前帧）；不持久化，停止 / 换视频都会复位
  // autoPause：环境自动暂停原因（"fullscreen" | "battery" | "locked"），null = 无。
  //   **与 paused 分开存**：手动暂停是用户意图、一直保持；自动暂停由后端巡检
  //   每 4s 重算，条件消失自动恢复。混成一个字段就分不清"暂停是谁给的"。
  const videoWallpaper = ref<{
    enabled: boolean;
    path: string;
    monitors: number[];
    paused: boolean;
    autoPause: string | null;
    // 后端**上次挂载是否失败**：窗口建出来了，但没能挂到桌面层。
    // "设置成功"只代表窗口建出来了 —— 这个字段才回答"桌面到底有没有画面"。
    attachFailed: boolean;
  }>({
    enabled: false,
    path: "",
    monitors: [],
    paused: false,
    autoPause: null,
    attachFailed: false,
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

  // 动态壁纸播放速率偏好（持久化）。
  //
  // 与 videoMuted 完全同一性质，因此同样**独立于 videoWallpaper**：
  // 那个对象每次停止/重载都会被整体重置，速率偏好不该跟着一起丢。
  const videoSpeed = ref(readVideoSpeed());

  // 循环模式偏好（持久化）。与静音、速率同一性质。
  const videoLoop = ref<VideoLoopMode>(readVideoLoop());

  // 片段循环区间（秒）。**不持久化、也不跨视频保留** ——
  // 秒数绑死在某一段素材上，换到另一个长度的视频上就是无意义的数字。
  // 它是**后端状态**而非偏好，因此由 `loadVideoWallpaperState` 回读，
  // 前端只做展示与提交；后端在换视频时会把它复位成 0。
  const videoClipStart = ref(0);
  const videoClipEnd = ref(0);

  // 最近自动暂停历史（**最新在前**，后端最多留 20 条）。
  //
  // 存在的意义：锁屏 / 电池这两类暂停发生时用户看不见桌面，
  // 画面"暂停过"与"一直在播"完全一样；release 版又没有控制台。
  // 用户报"锁屏回来视频不对"时，这份列表是唯一能自查的证据。
  //
  // **不持久化**：它的用途是"刚才到底发生了什么"，
  // 跨重启保留只会让一份过期流水账越积越多。
  const autoPauseHistory = ref<
    { reason: string; started_at: number; duration_ms: number | null }[]
  >([]);

  // 拉取自动暂停历史。失败时清空而不是保留旧值 ——
  // 一份过期的暂停记录会让人误判"刚刚又锁屏了"。
  async function loadAutoPauseHistory() {
    try {
      autoPauseHistory.value = (await invoke("get_auto_pause_history")) as {
        reason: string;
        started_at: number;
        duration_ms: number | null;
      }[];
    } catch {
      autoPauseHistory.value = [];
    }
  }

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

  // Pixabay 工具条（搜索框 + 分类）是否显示：只要停在该选项卡就显示。
  // 与本地不同，这里没有"内容为空就收起"的理由 —— 搜索框本身就是
  // "从空到有"的入口，收起等于把用户唯一的出路藏起来。
  const showPixabayTools = computed(() => source.value === "pixabay");

  // Wallhaven 工具条（搜索 + 筛选）是否显示：同样只要停在该选项卡就显示 ——
  // 搜索框本身就是"从空到有"的入口，没有"内容为空就收起"的理由。
  const showWallhavenTools = computed(() => source.value === "wallhaven");

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
      item.kind === "bing" ||
      item.kind === "pixabay" ||
      item.kind === "wallhaven" ||
      // 在线视频同样是远程资源，本地无文件；删除入口统一留在「本地」选项卡
      item.kind === "pixabay-video";
  }

  function closeContextMenu() {
    ctxVisible.value = false;
    ctxItem.value = null;
    ctxReadOnly.value = false;
  }

  // ---- 后台缩略图渐进更新 ----
  // 后端列表命令不再等待全量缩略图生成：缺失项由后台线程池渐进生成，
  // 每完成一张推送 "thumbnail-updated" { path, thumb, duration }。命中当前列表
  // 条目时仅更新该条 thumb（响应式触发对应 <img> :src 重算加载新图），不整列表刷新；
  // 尚未被懒加载放行的条目无需处理，放行时自然取到最新 thumb。
  //
  // `duration` 只有视频才有，且列表返回时通常还没探完（探测要打开文件，
  // 绝不能进列表命令的同步路径）—— 所以时长角标是"过一会儿才出现"的，
  // 这是设计如此，不是 bug。
  async function registerThumbnailListener() {
    if (thumbnailListenerRegistered) return;
    thumbnailListenerRegistered = true;
    await listen<ThumbnailUpdatedPayload>("thumbnail-updated", (ev) => {
      const { path, thumb, duration } = ev.payload;
      if (!path) return;
      const item = gridItems.value.find((it) => it.path === path);
      if (!item) return;
      if (thumb && item.thumb !== thumb) {
        item.thumb = thumb;
      }
      // 时长一旦探到就写死：0 / null 一律视为"未知"，不要覆盖掉已有的值。
      if (duration && duration > 0 && item.duration !== duration) {
        item.duration = duration;
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
  function setSourceVisibility(
    key: "system" | "favorites" | "bing" | "pixabay" | "wallhaven",
    visible: boolean
  ) {
    updateSourceVisibility(key, visible);
    if (!visible && source.value === key) {
      void setSource("local");
    }
  }

  // ---- 设置壁纸（核心） ----
  // 必应在线壁纸：先把原图下载到本地（目录由壁纸目录派生），再交给 set_wallpaper
  // —— Win32 SPI_SETDESKWALLPAPER 只接受本地文件路径
  async function ensureLocalPath(item: WallpaperItem): Promise<string> {
    // 非远程地址（本地 / 系统 / 视频）原样返回，不涉及下载。
    // ⚠️ 这里**必须先判远程再按 kind 分派**：原来写成
    // `if (item.kind !== "bing" || !isRemoteSrc(...)) return item.path`，
    // 加了新在线源之后，Pixabay 项会带着远程 URL 直接返回，
    // 最后拿 `https://...` 去调 SPI_SETDESKWALLPAPER —— 必然失败且报错莫名其妙。
    if (!isRemoteSrc(item.path || "")) return item.path || "";

    // Pixabay 视频：同样按 id 下载，但落盘是 `PixabayVideo_<id>.mp4`。
    // 注意下载下来的**一定是本地 mp4/webm** —— 动态壁纸只吃本地文件，
    // 不能让置底窗口去播远程地址（拖慢启动、且断网就没画面）。
    if (item.kind === "pixabay-video") {
      const id = Number(item.onlineId || 0);
      if (!id) throw new Error("Pixabay 视频缺少 id，无法下载");
      const local = (await invoke("download_pixabay_video", {
        url: item.path || "",
        id,
        directory: resolveDirArg(),
      })) as string;
      const key = String(id);
      if (local) {
        pixabayVideoLocalPaths.value = { ...pixabayVideoLocalPaths.value, [key]: local };
        const next = new Set(pixabayDownloadedVideoIds.value);
        next.add(key);
        pixabayDownloadedVideoIds.value = next;
      }
      return local;
    }

    // Pixabay：按 id 下载，登记在**独立**的两份记录里
    // （不复用必应的日期键，见 pixabayLocalPaths 的注释）
    if (item.kind === "pixabay") {
      const id = Number(item.onlineId || 0);
      if (!id) throw new Error("Pixabay 图片缺少 id，无法下载");
      const local = (await invoke("download_pixabay_wallpaper", {
        url: item.path || "",
        id,
        directory: resolveDirArg(),
      })) as string;
      const key = String(id);
      if (local) {
        pixabayLocalPaths.value = { ...pixabayLocalPaths.value, [key]: local };
        // 刚下载完的文件一定在壁纸目录里，徽标立刻点亮（不必等下一次扫描）
        const next = new Set(pixabayDownloadedIds.value);
        next.add(key);
        pixabayDownloadedIds.value = next;
      }
      return local;
    }

    // Wallhaven：按 6 位字母数字 id 下载，落盘 `Wallhaven_<id>.<ext>`。
    // 与 Pixabay 一样登记在**独立**的两份记录里（身份键不同，混用会互相点亮徽标）。
    if (item.kind === "wallhaven") {
      const id = item.onlineId || "";
      if (!id) throw new Error("Wallhaven 图片缺少 id，无法下载");
      const local = (await invoke("download_wallhaven_wallpaper", {
        url: item.path || "",
        id,
        // 接口给的 MIME 比 URL 后缀可靠（CDN 地址可能被改写）
        fileType: item.fileType || "",
        directory: resolveDirArg(),
      })) as string;
      if (local) {
        wallhavenLocalPaths.value = { ...wallhavenLocalPaths.value, [id]: local };
        // 刚下载完的文件一定在壁纸目录里，徽标立刻点亮（不必等下一次扫描）
        const next = new Set(wallhavenDownloadedIds.value);
        next.add(id);
        wallhavenDownloadedIds.value = next;
      }
      return local;
    }

    if (item.kind !== "bing") return item.path || "";
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

  // ---- Pixabay 在线图库：列表 / 下载 / 徽标 ----

  /// 后端条目 → 列表项。`onlineId` 是「已下载」徽标与"当前壁纸"回指的唯一键。
  function pixabayToEntry(b: PixabayWallpaperData): WallpaperEntryData {
    return {
      path: b.url,
      thumb: b.thumb,
      title: b.title || "Pixabay",
      kind: "pixabay" as WallpaperKind,
      onlineId: String(b.id),
      // 署名是 Pixabay API 条款的硬要求（标明作者 + 可点回主页），不是可选装饰。
      // `user_url` 后端已保证空 user 时为空串，前端据此隐藏整块署名。
      author: b.user || "",
      authorUrl: b.user_url || "",
    };
  }

  /// 后端视频条目 → 列表项。
  ///
  /// `path` 放**远程 URL**：列表与预览都用它（Pixabay 允许直接嵌入播放），
  /// 只有"设为动态壁纸"时才由 `ensureLocalPath` 下载成本地文件。
  function pixabayVideoToEntry(b: PixabayVideoData): WallpaperEntryData {
    return {
      path: b.url,
      thumb: b.thumb,
      title: b.title || "Pixabay 视频",
      kind: "pixabay-video" as WallpaperKind,
      onlineId: String(b.id),
      author: b.user || "",
      authorUrl: b.user_url || "",
      duration: b.duration || 0,
      quality: b.quality || "",
      sizeBytes: b.size || 0,
    };
  }

  /// 调后端拉一页视频（参数 camelCase / 返回 snake_case，同图片）
  ///
  /// ⚠️ **不要给视频下发 `colors`**：视频接口没有这个参数，传过去不报错、
  /// 也不生效（实测确认），只会白白让缓存指纹分叉。
  /// `orientation` 则**要下发** —— 但注意它和后端 `build_video_params` 无关：
  /// 视频接口没有这个参数，后端拿到它之后是在**解析响应时按宽高筛**的。
  async function fetchPixabayVideoPage(page: number): Promise<PixabayVideoPageData> {
    const size = currentSize();
    return (await invoke("list_pixabay_videos", {
      key: pixabayKey.value,
      query: pixabayQuery.value,
      category: pixabayCategory.value || null,
      videoType: pixabayVideoType.value,
      orientation: pixabayVideoOrientation.value,
      minWidth: size.w,
      minHeight: size.h,
      order: pixabayOrder.value,
      // 清晰度是用户偏好；后端会在"素材没有 4K"时自动降级，
      // 并把实际档位写回每条的 `quality`
      quality: pixabayVideoQuality.value,
      page,
      perPage: PIXABAY_PAGE_SIZE,
    })) as PixabayVideoPageData;
  }

  /// 拉一页（图片或视频，按当前媒体类型分派）。
  ///
  /// 抽出来是为了让 `loadWallpapers` 与 `loadMorePixabay` **只写一次分派**，
  /// 免得两处各写一个 if、将来加第三种媒体时漏改一处。
  async function fetchPixabayAnyPage(
    page: number
  ): Promise<{ entries: WallpaperEntryData[]; has_more: boolean; degraded: boolean }> {
    if (pixabayMedia.value === "video") {
      const vp = await fetchPixabayVideoPage(page);
      return {
        entries: vp.items.map(pixabayVideoToEntry),
        has_more: vp.has_more,
        // 视频没有页级降级标记（降级是逐条的"素材没 4K"），恒为 false
        degraded: false,
      };
    }
    const p = await fetchPixabayPage(page);
    return { entries: p.items.map(pixabayToEntry), has_more: p.has_more, degraded: p.degraded };
  }

  /// 调后端拉一页**图片**。
  ///
  /// 参数用 **camelCase**（Tauri 默认把 JS 的 camelCase 映射到 Rust 的 snake_case 形参），
  /// 返回值用 **snake_case**（serde 直接序列化，本项目结构体没有 `rename_all`）。
  /// 两边写错都不会报错，只会静默 `undefined`。
  /// 当前尺寸档位对应的最小宽高（`""` = 不限，返回 0/0）。
  ///
  /// 找不到（持久化里存了未知值）按"不限"处理而不是回退到 1920：
  /// 未知值说明数据被外部改坏了，此时"少过滤"比"多过滤导致搜不到"更安全。
  function currentSize(): { w: number; h: number } {
    const hit = PIXABAY_SIZES.find((s) => s.value === pixabaySize.value);
    return { w: hit?.w ?? 0, h: hit?.h ?? 0 };
  }

  async function fetchPixabayPage(page: number): Promise<PixabayPageData> {
    const size = currentSize();
    return (await invoke("list_pixabay_wallpapers", {
      key: pixabayKey.value,
      query: pixabayQuery.value,
      category: pixabayCategory.value || null,
      orientation: pixabayOrientation.value,
      // 0 由后端当成"不筛"（`filter(|m| *m > 0)`），传 0 比传 null 少一次类型分支
      minWidth: size.w,
      minHeight: size.h,
      // 多值用逗号分隔（接口原生格式）；空数组传 null 表示不筛颜色
      colors: pixabayColors.value.length ? pixabayColors.value.join(",") : null,
      order: pixabayOrder.value,
      // 内容类型现在是**独立筛选**，不再由媒体种类推导。
      // `all` 会混入照片/插画/矢量 —— 用户明确选"全部"时才这样。
      imageType: pixabayImageType.value,
      page,
      perPage: PIXABAY_PAGE_SIZE,
    })) as PixabayPageData;
  }

  /// 只下载、**不设置桌面**（对应必应的 `downloadBingWallpaper`）
  ///
  /// 图片与视频共用这一条：两者的差别只在 `ensureLocalPath` 里按 kind 分派，
  /// 而"下载到本地壁纸目录、徽标立刻点亮、不重复走网络"这套语义完全一致。
  async function downloadPixabayWallpaper(item: WallpaperItem): Promise<boolean> {
    if (item.kind !== "pixabay" && item.kind !== "pixabay-video") return false;
    if (isPixabayDownloaded(item)) {
      toast("这张已经在本地壁纸目录里了", "warning");
      return true;
    }
    pixabayDownloadingId.value = item.onlineId || "";
    try {
      const local = await ensureLocalPath(item);
      if (!local) throw new Error("未返回本地路径");
      toast(`已下载到本地：${baseName(local)}`, "success");
      return true;
    } catch (err: unknown) {
      toast("下载失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      pixabayDownloadingId.value = "";
    }
  }

  /// 刷新"哪些 Pixabay 图已经在本地了"—— 与必应同一套口径（扫本地壁纸目录），
  /// 保证徽标亮着就一定能在「本地」选项卡里找到。失败静默（徽标不显示而已）。
  async function refreshDownloadedPixabayIds(): Promise<void> {
    try {
      const list = (await invoke("list_downloaded_pixabay_wallpapers", {
        directory: resolveDirArg(),
      })) as DownloadedPixabayData[];
      pixabayDownloadedIds.value = new Set(list.map((d) => d.id).filter(Boolean));
    } catch {
      pixabayDownloadedIds.value = new Set();
    }
  }

  /// 刷新已下载的 Pixabay **视频**（`PixabayVideo_<id>.{mp4,webm}`）。
  /// 与图片分开一个命令与一份集合：前缀不同，混用会让徽标互相点亮。
  async function refreshDownloadedPixabayVideoIds(): Promise<void> {
    try {
      const list = (await invoke("list_downloaded_pixabay_videos", {
        directory: resolveDirArg(),
      })) as DownloadedPixabayData[];
      pixabayDownloadedVideoIds.value = new Set(list.map((d) => d.id).filter(Boolean));
    } catch {
      pixabayDownloadedVideoIds.value = new Set();
    }
  }

  /// Pixabay 在线卡片是否"本地已下载"（左上角徽标判据）。
  /// 图片与视频各查各的集合 —— 两者 id 可能撞号。
  function isPixabayDownloaded(item: WallpaperItem): boolean {
    if (!item.onlineId) return false;
    if (item.kind === "pixabay") return pixabayDownloadedIds.value.has(item.onlineId);
    if (item.kind === "pixabay-video")
      return pixabayDownloadedVideoIds.value.has(item.onlineId);
    return false;
  }

  /// 点徽标：跳到「本地」并定位到该文件。
  ///
  /// 搜索词带上**完整前缀**（`Pixabay_` / `PixabayVideo_`）而不是只搜 id：
  /// 图片与视频 id 可能撞号，只搜 id 会同时命中两个文件。
  async function openInLocalPixabay(item: WallpaperItem): Promise<void> {
    if (!item.onlineId) return;
    const prefix = item.kind === "pixabay-video" ? "PixabayVideo_" : "Pixabay_";
    if (item.kind !== "pixabay" && item.kind !== "pixabay-video") return;
    if (source.value !== "local") await setSource("local");
    if (localFilter.value !== "all") setLocalFilter("all");
    setLocalQuery(`${prefix}${item.onlineId}`);
  }

  /// 重置分页并重新拉第一页（改搜索词 / 分类 / 排序 / key 之后调用）
  async function reloadPixabay(): Promise<void> {
    pixabayPage.value = 0;
    pixabayHasMore.value = false;
    await loadWallpapers();
  }

  function setPixabayKey(next: string) {
    const v = next.trim();
    if (pixabayKey.value === v) return;
    pixabayKey.value = v;
    try {
      localStorage.setItem(PIXABAY_KEY_KEY, v);
    } catch {
      /* localStorage 不可用时静默降级：本次仍生效，只是不持久化 */
    }
  }

  async function setPixabayQuery(next: string) {
    if (pixabayQuery.value === next) return;
    pixabayQuery.value = next;
    try {
      localStorage.setItem(PIXABAY_QUERY_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  async function setPixabayCategory(next: string) {
    if (pixabayCategory.value === next) return;
    pixabayCategory.value = next;
    try {
      localStorage.setItem(PIXABAY_CATEGORY_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  async function setPixabayOrder(next: string) {
    if (pixabayOrder.value === next) return;
    pixabayOrder.value = next;
    try {
      localStorage.setItem(PIXABAY_ORDER_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  /// 切换"图片 / 视频"。**整个列表重来** —— 两者是两套接口，
  /// 结果不能拼接，分页游标也必须复位（否则视频第一页会接在图片第 3 页后面）。
  async function setPixabayMedia(next: PixabayMedia) {
    if (pixabayMedia.value === next) return;
    pixabayMedia.value = next;
    try {
      localStorage.setItem(PIXABAY_MEDIA_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  // ---- Pixabay 筛选条件的 setter ----
  //
  // 全部遵循同一个套路：值没变直接返回（避免重复拉取）→ 写 localStorage →
  // 必要时复位分页重拉。**每个新筛选都必须在这里落一份持久化**，
  // 漏了不报错，只表现为"重启后筛选被重置"。

  /// 内容类型（照片/插画/矢量/全部）。仅图片模式有意义 ——
  /// 视频模式下改它不必重拉（请求里根本没有这个参数）。
  async function setPixabayImageType(next: PixabayImageType) {
    if (pixabayImageType.value === next) return;
    pixabayImageType.value = next;
    try {
      localStorage.setItem(PIXABAY_IMAGE_TYPE_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "image") {
      await reloadPixabay();
    }
  }

  /// 图像方向。**仅图片接口支持**，视频模式不重拉。
  async function setPixabayOrientation(next: PixabayOrientation) {
    if (pixabayOrientation.value === next) return;
    pixabayOrientation.value = next;
    try {
      localStorage.setItem(PIXABAY_ORIENTATION_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "image") {
      await reloadPixabay();
    }
  }

  /// 尺寸档位。图片与视频接口都支持，两种模式下都要重拉。
  async function setPixabaySize(next: PixabaySize) {
    if (pixabaySize.value === next) return;
    pixabaySize.value = next;
    try {
      localStorage.setItem(PIXABAY_SIZE_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  /// 颜色（多选）。**仅图片接口支持**，视频模式不重拉。
  async function setPixabayColors(next: string[]) {
    // 数组要按内容比，不能按引用比 —— 传进来的通常是新建的数组
    const a = pixabayColors.value.join(",");
    const b = next.join(",");
    if (a === b) return;
    pixabayColors.value = next;
    try {
      localStorage.setItem(PIXABAY_COLORS_KEY, JSON.stringify(next));
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "image") {
      await reloadPixabay();
    }
  }

  /// 视频类型（实拍/动画/全部）。仅视频模式有意义。
  async function setPixabayVideoType(next: PixabayVideoType) {
    if (pixabayVideoType.value === next) return;
    pixabayVideoType.value = next;
    try {
      localStorage.setItem(PIXABAY_VIDEO_TYPE_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "video") {
      await reloadPixabay();
    }
  }

  /// 视频方向。仅视频模式有意义（图片方向是另一个 setter）。
  async function setPixabayVideoOrientation(next: PixabayOrientation) {
    if (pixabayVideoOrientation.value === next) return;
    pixabayVideoOrientation.value = next;
    try {
      localStorage.setItem(PIXABAY_VIDEO_ORIENTATION_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "video") {
      await reloadPixabay();
    }
  }

  /// 把筛选面板里的条件恢复默认（**不动搜索词与媒体种类**）。
  ///
  /// 一次性写全部字段再**只重拉一次** —— 逐个调 setter 会触发 N 次重拉，
  /// 用户会看到列表连续闪好几下。
  async function resetPixabayFilters() {
    pixabayCategory.value = "";
    pixabayImageType.value = "photo";
    pixabayOrientation.value = "horizontal";
    pixabayVideoOrientation.value = "horizontal";
    pixabaySize.value = "1920";
    pixabayColors.value = [];
    pixabayVideoType.value = "all";
    try {
      localStorage.setItem(PIXABAY_CATEGORY_KEY, "");
      localStorage.setItem(PIXABAY_IMAGE_TYPE_KEY, "photo");
      localStorage.setItem(PIXABAY_ORIENTATION_KEY, "horizontal");
      localStorage.setItem(PIXABAY_VIDEO_ORIENTATION_KEY, "horizontal");
      localStorage.setItem(PIXABAY_SIZE_KEY, "1920");
      localStorage.setItem(PIXABAY_COLORS_KEY, "[]");
      localStorage.setItem(PIXABAY_VIDEO_TYPE_KEY, "all");
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay") await reloadPixabay();
  }

  /// 筛选按钮上的角标数字：**偏离默认值**的项数。
  ///
  /// 默认值（照片/横向/1920）是应用的基线口径，不是"用户选的条件" ——
  /// 把它们算进去会让角标一开始就显示 3，等于告诉用户"你筛了东西"，是误导。
  /// 按当前媒体种类统计：视频模式下面板里根本没有内容类型/方向/颜色三项。
  const pixabayFilterCount = computed(() => {
    let n = 0;
    if (pixabayCategory.value) n += 1;
    if (pixabaySize.value !== "1920") n += 1;
    if (pixabayMedia.value === "video") {
      if (pixabayVideoType.value !== "all") n += 1;
      if (pixabayVideoOrientation.value !== "horizontal") n += 1;
    } else {
      if (pixabayImageType.value !== "photo") n += 1;
      if (pixabayOrientation.value !== "horizontal") n += 1;
      if (pixabayColors.value.length) n += 1;
    }
    return n;
  });

  /// 改视频清晰度偏好。只在视频模式下需要重拉 ——
  /// 图片列表里没有档位这个概念，无谓地清空重来只会闪一下。
  async function setPixabayVideoQuality(next: PixabayVideoQuality) {
    if (pixabayVideoQuality.value === next) return;
    pixabayVideoQuality.value = next;
    try {
      localStorage.setItem(PIXABAY_VIDEO_QUALITY_KEY, next);
    } catch {
      /* ignore */
    }
    if (source.value === "pixabay" && pixabayMedia.value === "video") {
      await reloadPixabay();
    }
  }

  /// 拉下一页并追加（服务端分页，与必应的"归档往前翻一个月"是两件事）。
  /// 图片与视频共用这一条，靠 `fetchPixabayAnyPage` 分派。
  async function loadMorePixabay(): Promise<void> {
    if (pixabayLoading.value || !pixabayHasMore.value) return;
    pixabayLoading.value = true;
    try {
      const next = pixabayPage.value + 1;
      const page = await fetchPixabayAnyPage(next);
      pixabayPage.value = next;
      pixabayHasMore.value = page.has_more;
      pixabayDegraded.value = page.degraded;
      if (!page.entries.length) {
        // 服务端说还有但这一页是空的：直接收手，否则前端会一直转圈
        pixabayHasMore.value = false;
        return;
      }
      allEntries = allEntries.concat(page.entries);
      rawEntries = allEntries;
      allCount.value = allEntries.length;
      appendBatch(); // 只渲染 PAGE_SIZE 条，其余留给下一次滚动
    } catch (err: unknown) {
      toast(
        `加载更多 Pixabay ${pixabayMedia.value === "video" ? "视频" : "图片"}失败：` +
          ((err as Error)?.message || String(err)),
        "error"
      );
      pixabayHasMore.value = false;
    } finally {
      pixabayLoading.value = false;
    }
  }

  // ---- Wallhaven 在线图库：列表 / 下载 / 徽标 ----
  //
  // 与 Pixabay 图片那条同构，差别只有三点：
  // ① 不需要 key；② 身份键是 6 位字母数字 id（不能 Number() 化）；
  // ③ `sorting = random` 要回传 seed。

  /// 后端条目 → 列表项。`onlineId` 是「已下载」徽标与"当前壁纸"回指的唯一键。
  ///
  /// 接口没有标题也没有作者，`title` 用分辨率（后端已把 `x` 换成 `×`）——
  /// 对壁纸场景来说"这张是 2560×1600"比"wallhaven-4x16yv"有用。
  function wallhavenToEntry(b: WallhavenWallpaperData): WallpaperEntryData {
    return {
      path: b.url,
      thumb: b.thumb,
      title: b.title || b.resolution || "Wallhaven",
      kind: "wallhaven" as WallpaperKind,
      // 统一小写：后端扫描文件名时会 lowercase（`parse_cached_name`），
      // 这里不归一化的话徽标判定会在大小写不一致时永远不亮。
      onlineId: (b.id || "").toLowerCase(),
      fileType: b.file_type || "",
    };
  }

  /// 调后端拉一页。
  ///
  /// 参数 camelCase / 返回 snake_case（同 Pixabay）。
  /// `seed` 只在随机排序下回传：其它排序带上它没有意义，还会让缓存指纹多分叉。
  async function fetchWallhavenPage(page: number): Promise<WallhavenPageData> {
    const isRandom = wallhavenSort.value === "random";
    return (await invoke("list_wallhaven_wallpapers", {
      query: wallhavenQuery.value,
      categories: wallhavenCategory.value,
      sorting: wallhavenSort.value,
      order: "desc",
      atleast: wallhavenAtleast.value || null,
      ratios: wallhavenRatio.value || null,
      colors: wallhavenColor.value || null,
      page,
      seed: isRandom && wallhavenSeed.value ? wallhavenSeed.value : null,
      apiBase: wallhavenApiBase.value || null,
      imageBase: wallhavenImageBase.value || null,
    })) as WallhavenPageData;
  }

  /// 只下载、**不设置桌面**（对应必应 / Pixabay 的同名函数）
  async function downloadWallhavenWallpaper(item: WallpaperItem): Promise<boolean> {
    if (item.kind !== "wallhaven") return false;
    if (isWallhavenDownloaded(item)) {
      toast("这张已经在本地壁纸目录里了", "warning");
      return true;
    }
    wallhavenDownloadingId.value = item.onlineId || "";
    try {
      const local = await ensureLocalPath(item);
      if (!local) throw new Error("未返回本地路径");
      toast(`已下载到本地：${baseName(local)}`, "success");
      return true;
    } catch (err: unknown) {
      toast("下载失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      wallhavenDownloadingId.value = "";
    }
  }

  /// 刷新"哪些 Wallhaven 图已经在本地了"—— 与必应 / Pixabay 同一套口径
  /// （扫本地壁纸目录），保证徽标亮着就一定能在「本地」选项卡里找到。
  async function refreshDownloadedWallhavenIds(): Promise<void> {
    try {
      const list = (await invoke("list_downloaded_wallhaven_wallpapers", {
        directory: resolveDirArg(),
      })) as DownloadedWallhavenData[];
      wallhavenDownloadedIds.value = new Set(list.map((d) => d.id).filter(Boolean));
    } catch {
      wallhavenDownloadedIds.value = new Set();
    }
  }

  function isWallhavenDownloaded(item: WallpaperItem): boolean {
    if (item.kind !== "wallhaven" || !item.onlineId) return false;
    return wallhavenDownloadedIds.value.has(item.onlineId);
  }

  /// 点徽标：跳到「本地」并定位到该文件。搜索词带完整前缀，避免和别的前缀撞。
  async function openInLocalWallhaven(item: WallpaperItem): Promise<void> {
    if (item.kind !== "wallhaven" || !item.onlineId) return;
    if (source.value !== "local") await setSource("local");
    if (localFilter.value !== "all") setLocalFilter("all");
    setLocalQuery(`Wallhaven_${item.onlineId}`);
  }

  /// 重置分页并重新拉第一页（改搜索词 / 筛选 / 排序之后调用）。
  /// **seed 一并清掉**：任何筛选变更都该换一批随机图。
  async function reloadWallhaven(): Promise<void> {
    wallhavenPage.value = 0;
    wallhavenHasMore.value = false;
    wallhavenSeed.value = "";
    wallhavenError.value = "";
    if (source.value === "wallhaven") await loadWallpapers();
  }

  // Wallhaven 筛选 setter。套路与 Pixabay 一致：
  // 值没变直接返回 → 写 localStorage → 停在当前页签时复位重拉。
  function persistWallhaven(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      /* localStorage 不可用时静默降级：本次仍生效，只是不持久化 */
    }
  }

  async function setWallhavenQuery(next: string) {
    if (wallhavenQuery.value === next) return;
    wallhavenQuery.value = next;
    persistWallhaven(WALLHAVEN_QUERY_KEY, next);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  async function setWallhavenCategory(next: WallhavenCategory) {
    if (wallhavenCategory.value === next) return;
    wallhavenCategory.value = next;
    persistWallhaven(WALLHAVEN_CATEGORY_KEY, next);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  async function setWallhavenSort(next: WallhavenSort) {
    if (wallhavenSort.value === next) return;
    wallhavenSort.value = next;
    persistWallhaven(WALLHAVEN_SORT_KEY, next);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  async function setWallhavenAtleast(next: WallhavenAtleast) {
    if (wallhavenAtleast.value === next) return;
    wallhavenAtleast.value = next;
    persistWallhaven(WALLHAVEN_ATLEAST_KEY, next);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  async function setWallhavenRatio(next: WallhavenRatio) {
    if (wallhavenRatio.value === next) return;
    wallhavenRatio.value = next;
    persistWallhaven(WALLHAVEN_RATIO_KEY, next);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  /// 颜色是**单选**（接口 `colors` 只吃一个十六进制值），再点一次同一个色块 = 取消。
  async function setWallhavenColor(next: string) {
    const v = wallhavenColor.value === next ? "" : next;
    if (wallhavenColor.value === v) return;
    wallhavenColor.value = v;
    persistWallhaven(WALLHAVEN_COLOR_KEY, v);
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  /// 自定义接口地址（空串 = 回到官方）。
  ///
  /// 前端只挡"少了协议头"这种明显错误（`readWallhavenApiBase` 会丢弃并回退官方），
  /// 真正的规范化 —— 补 `/api/v1/search`、吃尾斜杠 —— 在 Rust 侧 `resolve_api_base`。
  /// 填错的代价只是拉不到数据、错误提示里会让他检查地址，不会让整个来源消失。
  /// 自定义接口地址（空串 = 回到官方）。
  ///
  /// 前端只挡"少了协议头"这种明显错误（`readWallhavenApiBase` 会丢弃并回退官方），
  /// 真正的规范化 —— 补 `/api/v1/search`、吃尾斜杠 —— 在 Rust 侧 `resolve_api_base`。
  /// 填错的代价只是拉不到数据、错误提示里会让他检查地址，不会让整个来源消失。
  ///
  /// **不在这里重载列表**（与 `setPixabayKey` 同一约定）：它不是 UI 动作，
  /// 两个地址一起保存时各刷一次会并发拉两遍，由调用方保存完统一刷一次。
  function setWallhavenApiBase(next: string) {
    const v = next.trim();
    if (wallhavenApiBase.value === v) return;
    wallhavenApiBase.value = v;
    persistWallhaven(WALLHAVEN_API_BASE_KEY, v);
  }

  /// 设置自定义图片地址（空串 = 回到官方 CDN）。
  ///
  /// 只影响展示：后端在返回前替换 `th.` / `w.` 两个主机，路径原样保留。
  /// 所以反代要覆盖的路径是 `/lg/`、`/orig/`、`/small/`（缩略图）与 `/full/`（原图）。
  function setWallhavenImageBase(next: string) {
    const v = next.trim();
    if (wallhavenImageBase.value === v) return;
    wallhavenImageBase.value = v;
    persistWallhaven(WALLHAVEN_IMAGE_BASE_KEY, v);
  }

  /// 已启用的筛选数（只统计**偏离默认值**的项，否则一进来就显示 4，等于谎报）。
  /// 排序不在此列 —— 它在页脚，与 Pixabay 的口径一致。
  const wallhavenFilterCount = computed(() => {
    let n = 0;
    if (wallhavenCategory.value !== "111") n++;
    if (wallhavenAtleast.value !== "") n++;
    if (wallhavenRatio.value !== "") n++;
    if (wallhavenColor.value !== "") n++;
    return n;
  });

  /// 一键重置全部筛选（**一次重载**，不要逐个 setter 调 —— 那会连拉四次）。
  async function resetWallhavenFilters() {
    wallhavenCategory.value = "111";
    wallhavenAtleast.value = "";
    wallhavenRatio.value = "";
    wallhavenColor.value = "";
    persistWallhaven(WALLHAVEN_CATEGORY_KEY, "111");
    persistWallhaven(WALLHAVEN_ATLEAST_KEY, "");
    persistWallhaven(WALLHAVEN_RATIO_KEY, "");
    persistWallhaven(WALLHAVEN_COLOR_KEY, "");
    if (source.value === "wallhaven") await reloadWallhaven();
  }

  /// 拉下一页并追加（服务端分页）。
  async function loadMoreWallhaven(): Promise<void> {
    if (wallhavenLoading.value || !wallhavenHasMore.value) return;
    wallhavenLoading.value = true;
    try {
      const next = wallhavenPage.value + 1;
      const page = await fetchWallhavenPage(next);
      wallhavenPage.value = page.page || next;
      wallhavenHasMore.value = page.has_more;
      // 随机排序：把第一页的 seed 一直带下去（后端也会回显同一个 seed）
      if (page.seed) wallhavenSeed.value = page.seed;
      if (!page.items.length) {
        // 服务端说还有但这一页是空的：直接收手，否则前端会一直转圈
        wallhavenHasMore.value = false;
        return;
      }
      allEntries = allEntries.concat(page.items.map(wallhavenToEntry));
      rawEntries = allEntries;
      allCount.value = allEntries.length;
      appendBatch(); // 只渲染 PAGE_SIZE 条，其余留给下一次滚动
    } catch (err: unknown) {
      toast("加载更多 Wallhaven 壁纸失败：" + ((err as Error)?.message || String(err)), "error");
      wallhavenHasMore.value = false;
    } finally {
      wallhavenLoading.value = false;
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

  /// 刷新 Pixabay 下载目录（`<壁纸目录>\Pixabay`），设置页只读展示用。
  /// 与 `refreshBingDirInfo` 分开两个命令：Pixabay 没有老版本遗留目录，
  /// 不需要传 `legacyDir` 也不需要扫第二个目录，硬塞进必应那个结构体只会让字段半空。
  async function refreshPixabayDirInfo(): Promise<void> {
    try {
      pixabayDirInfo.value = (await invoke("get_pixabay_download_dir_info", {
        directory: resolveDirArg(),
      })) as PixabayDownloadDirInfo;
    } catch {
      pixabayDirInfo.value = null;
    }
  }

  /// 刷新 Wallhaven 下载目录（`<壁纸目录>\Wallhaven`），设置页只读展示用。
  async function refreshWallhavenDirInfo(): Promise<void> {
    try {
      wallhavenDirInfo.value = (await invoke("get_wallhaven_download_dir_info", {
        directory: resolveDirArg(),
      })) as WallhavenDownloadDirInfo;
    } catch {
      wallhavenDirInfo.value = null;
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
    if (item.kind === "video" || item.kind === "pixabay-video") return isCurrentVideo(item);
    const cur = currentWallpaper.value;
    if (!cur?.path) return false;
    if (item.kind === "bing") {
      const local = item.date ? bingLocalPaths.value[item.date] : "";
      return !!local && local === cur.path;
    }
    if (item.kind === "pixabay") {
      const local = item.onlineId ? pixabayLocalPaths.value[item.onlineId] : "";
      return !!local && local === cur.path;
    }
    if (item.kind === "wallhaven") {
      const local = item.onlineId ? wallhavenLocalPaths.value[item.onlineId] : "";
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
    // Pixabay 在线视频：**先下载到本地再设**。
    //
    // 不能让置底窗口直接播远程地址：启动会等网络、断网就没画面，
    // 而且 Pixabay 的 API 条款也不鼓励把它的 CDN 当长期播放源。
    //
    // 这里必须先把 isApplying 清掉再调 setVideoWallpaper —— 它开头有
    // `if (isApplying.value) return false`，不清就会静默什么都不做。
    if (item.kind === "pixabay-video") {
      isApplying.value = true;
      let local = "";
      try {
        local = await ensureLocalPath(item);
      } catch (err: unknown) {
        toast("视频下载失败：" + ((err as Error)?.message || String(err)), "error");
        return false;
      } finally {
        isApplying.value = false;
      }
      if (!local) {
        toast("视频下载失败，无法设为动态壁纸", "error");
        return false;
      }
      return setVideoWallpaper(local);
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
      } else if (source.value === "pixabay") {
        // Pixabay 在线图库 = 服务端分页（每页 24 条），图片与视频共用这套分页。
        //
        // 没有 key 时**不发请求、也不报错** —— 空态里会显示"去填 key"的引导，
        // 再弹一个红色 toast 只会让人以为功能坏了。
        pixabayPage.value = 0;
        pixabayHasMore.value = false;
        if (!pixabayKey.value.trim()) {
          allEntries = [];
          pixabayDegraded.value = false;
        } else {
          const page = await fetchPixabayAnyPage(1);
          allEntries = page.entries;
          pixabayPage.value = 1;
          pixabayHasMore.value = page.has_more;
          pixabayDegraded.value = page.degraded;
          // 徽标数据在列表就位之后再取：它失败只该影响徽标，不该让列表报错。
          // 只刷当前媒体那一份 —— 另一份在切过去时自然会刷。
          if (pixabayMedia.value === "video") {
            await refreshDownloadedPixabayVideoIds();
          } else {
            await refreshDownloadedPixabayIds();
          }
        }
      } else if (source.value === "wallhaven") {
        // Wallhaven 在线图库 = 服务端分页（固定 24 条/页），**不需要 key**。
        // 每次重载都清 seed：换搜索词/筛选后应该是一批新的图，
        // 沿用旧 seed 会让"随机"看起来没生效。
        wallhavenPage.value = 0;
        wallhavenHasMore.value = false;
        wallhavenSeed.value = "";
        const page = await fetchWallhavenPage(1);
        allEntries = page.items.map(wallhavenToEntry);
        wallhavenPage.value = page.page || 1;
        wallhavenHasMore.value = page.has_more;
        wallhavenSeed.value = page.seed || "";
        // 拉到了就说明这次是通的，把上一次的失败提示清掉
        wallhavenError.value = "";
        // 徽标数据在列表就位之后再取：它失败只该影响徽标，不该让列表报错
        await refreshDownloadedWallhavenIds();
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
      const msg = (err as Error)?.message || String(err);
      // Wallhaven 单独留一份：空态里要常驻显示"是没连上，不是没结果"
      if (source.value === "wallhaven") wallhavenError.value = msg;
      toast("加载壁纸失败：" + msg, "error");
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
        // ⚠️ 在线源的附加字段**必须在这里逐项搬运**：这个对象是新建的，
        // 漏一个就静默变 undefined，而且不会报错 ——
        // 漏 onlineId ⇒ 下载时抛"缺少 id"、徽标永远不亮；
        // 漏 author/authorUrl ⇒ 卡片上没有署名（Pixabay 条款要求）。
        onlineId: e.onlineId,
        author: e.author,
        authorUrl: e.authorUrl,
        // 在线视频的附加字段，同理必须逐项搬运：
        // 漏 duration ⇒ 卡片上没有时长角标；
        // 漏 quality ⇒ 预览区无法提示"4K 不可用已降级"；
        // 漏 sizeBytes ⇒ 无法提示这条要下多少 MB。
        duration: e.duration,
        quality: e.quality,
        sizeBytes: e.sizeBytes,
        // 漏 fileType ⇒ Wallhaven 下载时按 URL 猜扩展名，CDN 改写地址时会存成错的格式
        fileType: e.fileType,
      });
      loadedCount++;
    });
    // 「还有更多」= 本地缓冲还没渲染完 **或** 必应还有更早的归档月份可拉。
    // 后半句不能漏：必应首屏只有 15 条，缓冲渲染完后 hasMore 若为 false，
    // 底部会错误显示"已加载全部"，`fillGridUntilOverflow` 也会提前收手不再拉归档。
    hasMore.value =
      loadedCount < allEntries.length ||
      (source.value === "bing" && !bingArchiveEnd.value && bingArchiveCursor.value !== null) ||
      // Pixabay 是服务端分页：缓冲渲染完后还有没有下一页，只有后端知道
      (source.value === "pixabay" && pixabayHasMore.value) ||
      (source.value === "wallhaven" && wallhavenHasMore.value);
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
    // Pixabay：本地缓冲渲染完 ⇒ 向服务端要下一页（与必应的"归档往前翻"不同）
    if (source.value === "pixabay" && loadedCount >= allEntries.length && pixabayHasMore.value) {
      loadingMoreLock = true;
      loadingMore.value = true;
      try {
        await loadMorePixabay();
      } finally {
        loadingMoreLock = false;
        loadingMore.value = false;
      }
      return;
    }
    // Wallhaven：同样是服务端分页，缓冲渲染完 ⇒ 要下一页
    if (
      source.value === "wallhaven" &&
      loadedCount >= allEntries.length &&
      wallhavenHasMore.value
    ) {
      loadingMoreLock = true;
      loadingMore.value = true;
      try {
        await loadMoreWallhaven();
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
        // 速率同理：不带的话重启后第一次设壁纸会掉回原速
        speed: videoSpeed.value,
        // 循环模式也是偏好，一起带过去
        loopMode: videoLoop.value,
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

  // 切换播放速率。与静音同样是"一次写三处"：
  //   ① 本地偏好 ref ② localStorage ③ 后端（后端广播给置底窗口）
  //
  // **不做乐观更新的回滚**：失败时保持用户点的值反而更糟 ——
  // 界面显示 2×、实际按 1× 播，比"点不动"更难发现。
  // 所以失败时把 ref 改回去，让 UI 与真实播放速率一致。
  async function setVideoSpeed(speed: number): Promise<boolean> {
    const prev = videoSpeed.value;
    videoSpeed.value = speed;
    try {
      localStorage.setItem(VIDEO_SPEED_KEY, String(speed));
    } catch {
      /* 存储不可用时只影响"下次是否记得"，不影响本次播放 */
    }
    try {
      await invoke("set_video_wallpaper_speed", { speed });
      return true;
    } catch (err: unknown) {
      videoSpeed.value = prev;
      try {
        localStorage.setItem(VIDEO_SPEED_KEY, String(prev));
      } catch {
        /* ignore */
      }
      toast(
        "播放速度设置失败：" + ((err as Error)?.message || String(err)),
        "error"
      );
      return false;
    }
  }

  // 切换循环模式。同样是"一次写三处"（ref / localStorage / 后端）。
  async function setVideoLoop(mode: VideoLoopMode): Promise<boolean> {
    const prev = videoLoop.value;
    videoLoop.value = mode;
    try {
      localStorage.setItem(VIDEO_LOOP_KEY, mode);
    } catch {
      /* 存储不可用时只影响"下次是否记得" */
    }
    try {
      await invoke("set_video_wallpaper_loop_mode", { loopMode: mode });
      return true;
    } catch (err: unknown) {
      videoLoop.value = prev;
      toast(
        "循环模式设置失败：" + ((err as Error)?.message || String(err)),
        "error"
      );
      return false;
    }
  }

  // 设置片段循环区间。`end = 0` 表示"到结尾"。
  //
  // **不做乐观更新**：区间合法性由后端校验（end 必须大于 start），
  // 前端先写会让界面短暂显示一个非法区间，随后又被错误提示打回。
  // 成功后以后端广播回来的值为准。
  async function setVideoClip(start: number, end: number): Promise<boolean> {
    try {
      await invoke("set_video_wallpaper_clip", {
        clipStart: start,
        clipEnd: end,
      });
      videoClipStart.value = start;
      videoClipEnd.value = end;
      return true;
    } catch (err: unknown) {
      toast(
        "片段设置失败：" + ((err as Error)?.message || String(err)),
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
      autoPause: null,
      attachFailed: false,
    };
    // 清除"上次生效项"：停止是明确的用户意图，
    // 不清掉的话下次启动会自作主张地又播起来。
    clearVideoActive();
    // 片段区间跟着当前视频走，停止后一并清空（后端也复位了）
    videoClipStart.value = 0;
    videoClipEnd.value = 0;
    // 后端在停止时会给最后一条未结束的记录补上时长，这里回拉一次让面板立刻反映。
    void loadAutoPauseHistory();
  }

  // 某项是否为"当前正在播放的动态壁纸"（列表绿框 / 右侧状态展示共用）
  function isCurrentVideo(item: WallpaperItem): boolean {
    if (!videoWallpaper.value.enabled) return false;
    const playing = videoWallpaper.value.path;
    if (!playing) return false;
    if (item.kind === "video") return !!item.path && item.path === playing;
    // 在线视频：卡片上存的是**远程 URL**，而桌面播的是**下载后的本地文件**，
    // 直接比 path 永远不相等 —— 绿点就永远不亮，用户以为没设上。
    // 必须经下载记录换算一次（与必应 / Pixabay 图片"绿框"的判据同一套思路）。
    if (item.kind === "pixabay-video") {
      const local = item.onlineId ? pixabayVideoLocalPaths.value[item.onlineId] : "";
      return !!local && local === playing;
    }
    return false;
  }

  // 启动/恢复主界面时同步动态壁纸状态（播放页挂载也用它拉取兜底）
  //
  // **刻意不读后端的 muted / speed / loop_mode**：这三项的事实来源都是本地偏好，
  // 后端那份只是镜像。若在这里用后端的值覆盖本地偏好，
  // 应用重启后（后端回到默认值）就会把用户上次的选择抹掉。
  //
  // 片段区间是**例外**：它绑在当前这段视频上、不是偏好，而且后端在换视频时
  // 会把它复位 —— 所以必须回读，否则换完视频界面上还显示着上一段的秒数。
  async function loadVideoWallpaperState() {
    try {
      const st = (await invoke("get_video_wallpaper_state")) as {
        enabled: boolean;
        path: string;
        monitors?: number[];
        paused?: boolean;
        auto_pause?: string | null;
        clip_start?: number;
        clip_end?: number;
        attach_failed?: boolean;
      };
      const monitors = Array.isArray(st.monitors) ? st.monitors : [];
      videoWallpaper.value = {
        enabled: !!st.enabled,
        path: st.path || "",
        monitors,
        paused: !!st.paused,
        autoPause: st.auto_pause ?? null,
        attachFailed: !!st.attach_failed,
      };
      videoClipStart.value = Number(st.clip_start) || 0;
      videoClipEnd.value = Number(st.clip_end) || 0;
      // 恢复上次的显示器选择（空数组 = 全部）
      if (monitors.length) selectedMonitors.value = [...monitors];
    } catch {
      /* ignore */
    }
    // 历史与 autoPause 是同一份后端状态的两个侧面，一起同步：
    // 否则"在全屏里设壁纸"这种一上来就被暂停的情况，
    // 历史里已经有记录、面板却不显示（事件回来时 autoPause 已是同一个值，
    // `syncVideoAutoPause` 会提前 return）。
    void loadAutoPauseHistory();
  }

  // 后端状态事件 → 只同步"环境自动暂停原因"。
  //
  // **刻意只写 autoPause**：
  // - `paused` 是用户手动意图，`setVideoPaused` 已乐观更新；这里再写会与点击竞争；
  // - `muted` 的事实来源是本地偏好（videoMuted），后端那份只是镜像；
  // - `path` / `monitors` 由设置动作驱动，事件回声没有新信息。
  // 自动暂停是**唯一**由后端巡检单方面发起的状态变化，所以也只有它需要走事件。
  function syncVideoAutoPause(payload: { auto_pause?: string | null } | null) {
    if (!payload) return;
    const next = payload.auto_pause ?? null;
    if (videoWallpaper.value.autoPause === next) return;
    videoWallpaper.value = { ...videoWallpaper.value, autoPause: next };
    // 后端是"先落历史、再广播"，所以这里回拉必定拿到含本次变化的新列表：
    // 进入暂停时多一条"进行中"，解除时那条刚补上时长。
    void loadAutoPauseHistory();
  }

  /// 后端广播"挂载失败"状态 → 同步到面板。
  ///
  /// 单独一个函数而不是塞进 `syncVideoAutoPause`：两者的触发源完全不同
  /// （一个是环境检测，一个是窗口挂载），混在一起以后没法单独改。
  /// 这个字段是**唯一**能让用户知道"提示设置成功、桌面却什么都没变"的线索。
  function syncVideoAttachFailed(payload: { attach_failed?: boolean } | null) {
    if (!payload) return;
    const next = !!payload.attach_failed;
    if (videoWallpaper.value.attachFailed === next) return;
    videoWallpaper.value = { ...videoWallpaper.value, attachFailed: next };
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

    // 只下载到本地壁纸目录，不改变桌面（必应 / Pixabay 在线图片 / 在线视频专用）
    if (action === "download-bing") {
      if (item.kind === "bing") await downloadBingWallpaper(item);
      else if (item.kind === "pixabay" || item.kind === "pixabay-video") {
        await downloadPixabayWallpaper(item);
      } else if (item.kind === "wallhaven") {
        await downloadWallhavenWallpaper(item);
      }
      return;
    }

    if (action === "toggle-favorite") {      // 收藏夹以本地文件路径为书签，在线源不参与该体系
      //（下载后的文件在「本地」选项卡里可正常收藏）
      if (
        item.kind === "bing" ||
        item.kind === "pixabay" ||
        item.kind === "pixabay-video" ||
        item.kind === "wallhaven"
      ) {
        toast("在线壁纸暂不支持收藏", "warning");
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
      // 在线源分两种：在线项要先查下载记录（未下载则给引导提示）；
      // 本地缓存项（超出在线窗口的老壁纸）item.path 本身就是本地文件，直接定位。
      const localPath =
        item.kind === "bing"
          ? (item.date && bingLocalPaths.value[item.date]) ||
            (!isRemoteSrc(item.path || "") ? item.path || "" : "")
          : item.kind === "pixabay"
            ? (item.onlineId && pixabayLocalPaths.value[item.onlineId]) ||
              (!isRemoteSrc(item.path || "") ? item.path || "" : "")
            : item.kind === "pixabay-video"
              ? (item.onlineId && pixabayVideoLocalPaths.value[item.onlineId]) ||
                (!isRemoteSrc(item.path || "") ? item.path || "" : "")
              : item.kind === "wallhaven"
                ? (item.onlineId && wallhavenLocalPaths.value[item.onlineId]) ||
                  (!isRemoteSrc(item.path || "") ? item.path || "" : "")
                : item.path || "";
      if (!localPath) {
        // ⚠️ 在线项不能直接拿 `item.path` 去 reveal —— 那是 `https://...`，
        // 传给 reveal_in_explorer 只会得到一个莫名其妙的"打开目录失败"
        toast(
          item.kind === "bing"
            ? "这张还没下载到本地，先点卡片上的「下载」存到壁纸目录"
            : item.kind === "pixabay"
              ? "这张还没下载到本地，先点卡片上的「下载」存到壁纸目录"
              : item.kind === "wallhaven"
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
      // 在线源（必应 / Pixabay）选项卡同样不提供删除（在线项本地无文件；
      // 本地缓存项的删除入口统一留在「本地」选项卡，避免同一文件两处可删）
      if (
        source.value === "system" ||
        item.kind === "bing" ||
        item.kind === "pixabay" ||
        item.kind === "wallhaven"
      )
        return;
      await removeWallpaper(item);
    }
  }

  // 右键删除：把壁纸文件移入回收站（可还原，不做永久抹除）
  async function removeWallpaper(item: WallpaperItem): Promise<void> {
    if (!item.path) return;
    // 只读保护：系统壁纸来源不允许删除（同时后端 delete_wallpaper 亦拦截系统路径）
    if (source.value === "system") return;
    const name = baseName(item.path);

    const confirmed = await confirmDanger({
      title: "删除壁纸",
      content: `确定要删除壁纸「${name}」吗？\n该文件会被移入回收站，之后可从回收站还原。`,
      positiveText: "移到回收站",
    });
    if (!confirmed) return;

    try {
      await invoke("delete_wallpaper", { path: item.path });
      // 文件已移出目录：同步清理收藏书签，避免收藏夹出现失效路径
      if (isFavorite(item.path)) removeFavorite(item.path);
      // 删除的正是正在播放的动态壁纸：同步停掉置底窗口，避免窗口指向已删文件
      if (isCurrentVideo(item)) {
        await stopVideoWallpaper();
      }
      toast("已移入回收站：" + name, "success");
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
    pixabayDirInfo,
    bingDownloadingDate,
    pixabayKey,
    pixabayQuery,
    pixabayCategory,
    pixabayOrder,
    pixabayDegraded,
    pixabayLoading,
    pixabayDownloadingId,
    pixabayMedia,
    pixabayImageType,
    pixabayOrientation,
    pixabayVideoOrientation,
    pixabaySize,
    pixabayColors,
    pixabayVideoType,
    pixabayVideoQuality,
    wallhavenDirInfo,
    wallhavenQuery,
    wallhavenCategory,
    wallhavenSort,
    wallhavenAtleast,
    wallhavenRatio,
    wallhavenColor,
    wallhavenApiBase,
    wallhavenImageBase,
    wallhavenLoading,
    wallhavenError,
    wallhavenDownloadingId,
    // getters
    hasMore,
    previewTarget,
    showLocalFilter,
    showLocalTools,
    showPixabayTools,
    showWallhavenTools,
    pixabayFilterCount,
    wallhavenFilterCount,
    isCurrentItem,
    isCurrentVideo,
    isBingDownloaded,
    isPixabayDownloaded,
    isWallhavenDownloaded,
    videoWallpaper,
    videoMuted,
    // 播放参数四个 ref **必须导出**：面板要读它们做高亮、摘要与片段输入框回填。
    // 漏导出的表现极具迷惑性 —— action 仍然可用（invoke 照发、后端照改），
    // 但界面读到的全是 undefined，于是"点了没反应"，看起来像整块功能都失效。
    videoSpeed,
    videoLoop,
    videoClipStart,
    videoClipEnd,
    autoPauseHistory,
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
    downloadPixabayWallpaper,
    openInLocalPixabay,
    downloadWallhavenWallpaper,
    openInLocalWallhaven,
    reloadWallhaven,
    setWallhavenQuery,
    setWallhavenCategory,
    setWallhavenSort,
    setWallhavenApiBase,
    setWallhavenImageBase,
    setWallhavenAtleast,
    setWallhavenRatio,
    setWallhavenColor,
    resetWallhavenFilters,
    setPixabayKey,
    setPixabayQuery,
    setPixabayCategory,
    setPixabayOrder,
    setPixabayMedia,
    setPixabayImageType,
    setPixabayOrientation,
    setPixabayVideoOrientation,
    setPixabaySize,
    setPixabayColors,
    setPixabayVideoType,
    resetPixabayFilters,
    setPixabayVideoQuality,
    reloadPixabay,
    refreshBingDirInfo,
    refreshPixabayDirInfo,
    refreshWallhavenDirInfo,
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
    setVideoSpeed,
    setVideoLoop,
    setVideoClip,
    setVideoPaused,
    loadMonitors,
    stopVideoWallpaper,
    loadVideoWallpaperState,
    syncVideoAutoPause,
    syncVideoAttachFailed,
    loadAutoPauseHistory,
    restoreVideoWallpaper,
    setEffect,
    moveSourceOrder,
    moveSourceOrderTo,
  };
});
