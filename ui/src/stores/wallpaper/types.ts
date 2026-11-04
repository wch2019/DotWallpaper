// DotWallpaper 壁纸工具 - 共享类型与常量（stores/wallpaper.ts 拆分子模块）
export type WallpaperKind =
  | "local"
  | "current"
  | "bing"
  | "pixabay"
  /// Pixabay 在线**视频**。刻意与 `pixabay` 分开：
  /// 前者是远程 URL + 需要下载才能设成动态壁纸，后者走 `SPI_SETDESKWALLPAPER`。
  /// 合成一个 kind 会让每个分支都要再判一次"这条到底是图还是视频"。
  | "pixabay-video"
  /// Wallhaven 在线壁纸（纯图片，没有视频）。与 `pixabay` 分开是因为
  /// 身份键（6 位字母数字 id vs 数字 id）和下载落盘前缀都不同。
  | "wallhaven"
  | "video";

/// 壁纸来源选项卡：local = 本地壁纸（可增删），system = Windows 自带系统壁纸（只读），
/// favorites = 收藏夹（书签视图：跨本地/系统来源，仅标记不删文件），
/// bing = 必应在线壁纸（日期流，列表远程 URL，设为壁纸时才下载本地），
/// pixabay = Pixabay 在线图库（搜索式，同样列表远程、设为壁纸才下载），
/// wallhaven = Wallhaven 在线图库（搜索式，免 key，只提供 SFW）
export type WallpaperSource = "local" | "system" | "favorites" | "bing" | "pixabay" | "wallhaven";

/// 本地来源的类型筛选：all = 全部（图片 + 视频），image = 仅静态图片，video = 仅视频动态壁纸。
/// 只作用于 local 来源；其余来源类型单一（system 仅图片、bing 仅在线图片）
/// 或语义不同（favorites 为跨来源书签集合），不参与该筛选。
export type LocalFilter = "all" | "image" | "video";

/// 本地来源的排序方式。
///
/// `default` 保留后端扫描顺序（不做任何排序，等于旧行为）；其余是"排序键 + 方向"的扁平组合
/// —— 做成平铺选项而不是"选键 + 选方向"两个控件，是因为下拉里一眼能看全，
/// 且方向对每个键的默认值不同（时间/大小通常想"从大到小"，名称想"A→Z"），
/// 拆两个控件反而要先想清楚方向该配哪个。
///
/// 名称排序用**自然序**（`numeric: true`），否则 wallpaper10 会排在 wallpaper2 前面。
export const LOCAL_SORTS = [
  "default",
  "name-asc",
  "name-desc",
  "mtime-desc",
  "mtime-asc",
  "size-desc",
  "size-asc",
] as const;
export type LocalSort = (typeof LOCAL_SORTS)[number];

/// 本地排序偏好的 localStorage 键。
/// **搜索词刻意不持久化** —— 那是一次性输入，下次启动还留着会让人以为"壁纸变少了"。
export const LOCAL_SORT_KEY = "dot-wallpaper-local-sort";

/// 各来源选项卡的可见性：local 默认强制开启、不可关闭
export const SOURCE_VIS_KEY = "dot-wallpaper-source-visibility";
export type SourceVisibility = {
  local: true;
  system: boolean;
  favorites: boolean;
  bing: boolean;
  pixabay: boolean;
  wallhaven: boolean;
};
// 选项卡展示顺序（独立于可见性：顺序控制渲染次序，可见性控制是否显示）
export const SOURCE_ORDER_KEY = "dot-wallpaper-source-order";
export const SOURCE_ORDER_DEFAULT: WallpaperSource[] = [
  "local",
  "favorites",
  "system",
  "bing",
  "pixabay",
  "wallhaven",
];

export interface WallpaperItem {
  key: string;
  kind: WallpaperKind; // 类型：本地 / 当前壁纸（右键目标）/ 视频动态壁纸 / 在线源
  path?: string; // 本地绝对路径（原图/视频）或在线远程 URL
  title?: string;
  thumb?: string; // 缩略图绝对路径（列表加载用；无缩略图时为空）
  date?: string; // 必应壁纸日期（YYYYMMDD），作为下载缓存与卡片对应键
  /// 在线源的稳定标识（**不含必应**）：Pixabay 用它记录"这张下到哪了"。
  ///
  /// 必应继续用 `date`，两条链路刻意不合并 —— 必应那套已验证过（日期即身份，
  /// 下载文件名里也带日期），把 id 塞进 `date` 会让 `bingDateFromPath` 之类
  /// 的日期正则误判。等 Pixabay 稳定运行一个版本再考虑统一成单一字段。
  onlineId?: string;
  /// 作者署名与主页（仅 Pixabay 等需要署名的在线源）。
  /// Pixabay API 条款要求标明作者、且作者名可点回其主页，故卡片上必须能拿到。
  author?: string;
  authorUrl?: string;
  /// 视频时长（秒），仅 Pixabay 在线视频有 —— 卡片角标显示 `0:12`
  duration?: number;
  /// 实际拿到的清晰度档位（large / medium / small / tiny），仅在线视频有。
  /// 与用户偏好的档位不一致时说明"4K 素材不存在已降级"，预览区据此如实提示。
  quality?: string;
  /// 视频文件大小（字节），0 = 接口没给。用于提示"约 xx MB"
  sizeBytes?: number;
  /// 在线原图的 MIME（如 `image/jpeg`），仅 Wallhaven 有。
  /// 下载时用它决定落盘扩展名 —— 按 URL 猜在 CDN 改写地址时会猜错。
  fileType?: string;
  applying?: boolean; // 是否正在设置中
}

/// 后端列表命令返回条目：原图路径 + 缩略图路径（可能为空）
/// 必应在线壁纸列表额外携带 kind/title/date（其余来源可缺省）
export interface WallpaperEntryData {
  path: string;
  thumb: string;
  kind?: WallpaperKind; // 来源类型（在线壁纸为 "bing"/"pixabay"；缺省按本地处理）
  title?: string; // 壁纸标题（在线壁纸自带；缺省用文件名）
  date?: string; // 必应壁纸日期（YYYYMMDD）
  onlineId?: string; // Pixabay 图片 id（字符串）
  author?: string; // 作者名（Pixabay 署名）
  authorUrl?: string; // 作者主页 URL（署名可点回）
  duration?: number; // 视频时长（秒），仅 Pixabay 在线视频
  quality?: string; // 实际拿到的清晰度档位（large/medium/small/tiny），仅在线视频
  sizeBytes?: number; // 视频大小（字节），0 = 接口没给
  fileType?: string; // 在线原图 MIME（如 image/jpeg），仅 Wallhaven
  /// 文件修改时间（Unix 秒）—— 本地/系统来源由后端 stat 得到，**仅供排序**。
  /// 必应在线壁纸没有本地文件，恒为 undefined。
  mtime?: number | null;
  /// 文件大小（字节）—— 同上，仅供排序。
  size?: number | null;
}

/// 后端后台缩略图完成事件 payload：原图路径 + 缩略图路径
export interface ThumbnailUpdatedPayload {
  path: string;
  thumb: string;
  /// 视频时长（秒）。仅视频有，且**可能是 undefined**：
  /// 探测失败 / 非视频 / 老缓存还没补探。前端据此决定要不要画时长角标。
  duration?: number | null;
}

// ---------- 常量 ----------
export const DIR_STORAGE_KEY = "dot-wallpaper-dir"; // localStorage 持久化键
/// 首次启动引导是否已完成（值 "1" = 已完成）。
/// 必须是独立键：因为"恢复默认目录"会清空 `dot-wallpaper-dir`，不能用它判断是否首次使用。
export const ONBOARDED_KEY = "dot-wallpaper-onboarded";
/// 【遗留键，只读不写】旧版"必应壁纸下载目录"的 localStorage 键。
///
/// 下载目录现在是**派生**的（`<壁纸目录>\BingWallpaper`，见 `bing::download_dir`），
/// 不再提供独立配置。这个键只剩一个用途：设置页检测到旧值、且那个目录里确实还有
/// 必应壁纸时，提示一次"旧目录里的文件不会自动搬移"，并给出打开目录的入口。
///
/// **刻意不删除它**：用户把文件搬走之前，这条提示应该一直在（删了就再也没人提醒
/// 那批文件在哪）。前端全程只读。
export const BING_DIR_KEY = "dot-wallpaper-bing-dir";
export const FAVORITES_KEY = "dot-wallpaper-favorites"; // localStorage 收藏书签集合键
export const EFFECT_STORAGE_KEY = "dot-wallpaper-effect"; // localStorage 壁纸效果键
export const CLOSE_BEHAVIOR_KEY = "dot-wallpaper-close-behavior"; // localStorage 关闭窗口行为键
export type CloseBehavior = "exit" | "tray"; // 关闭窗口：exit=直接退出，tray=最小化到托盘（后台运行）

/// 开机自启动时"要不要打开主界面"的 localStorage 键（值 "window" = 打开；缺失/其它 = 静默）。
///
/// 默认静默 —— 这是"开机常驻后台"的默认预期，也让老用户升级后行为保持不变。
/// 后端在 `--autostart` 下**一律不显示窗口**（避免开机瞬间闪一下），由前端挂载后
/// 按本偏好决定要不要补一次 `apply_autostart_window_pref`。
export const AUTOSTART_SHOW_KEY = "dot-wallpaper-autostart-show-window";
export type AutostartWindowMode = "silent" | "window"; // 开机启动后：silent=只留托盘，window=打开主界面

/// 动态壁纸静音偏好的 localStorage 键（值 "1" = 静音）。
///
/// 持久化的是**偏好**而非运行时状态：暂停不持久化（重启后本就没有动态壁纸，
/// 存它没有意义），静音则希望"下次设动态壁纸还是静的"。
export const VIDEO_MUTED_KEY = "dot-wallpaper-video-muted";

/// 动态壁纸播放速率偏好的 localStorage 键（值形如 "1.5"）。
///
/// 与静音同一性质：**偏好**，不是本次播放的临时状态 ——
/// 换视频、改播放范围、停止动态壁纸都该保留"我习惯 1.5 倍速"。
export const VIDEO_SPEED_KEY = "dot-wallpaper-video-speed";

/// 可选的播放速率。**顺序即 UI 顺序**，1.0 在中间便于一眼找到默认值。
///
/// 刻意不做滑块：速率是离散的观感选择（0.75 与 0.8 肉眼无差），
/// 连续值只会让人反复微调，还得额外处理"取整到两位小数"的显示问题。
///
/// 上界 2.0 是**保守取值**：后端允许到 3.0，但交叉循环在高速下留给淡出的
/// 余量会被压得很薄（见 `VideoWallpaper.vue` 的 `crossStep`），
/// 与其让用户踩到 3 倍速的接缝，不如只提供稳定区间。
export const VIDEO_SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2] as const;

/// 循环模式偏好的 localStorage 键（值 "loop" / "once"）。
///
/// 与静音、速率同属**偏好**：换视频、停止、重启都该保留
/// "我只想让它播一遍"这种口味。
export const VIDEO_LOOP_KEY = "dot-wallpaper-video-loop";

/// 循环模式。刻意**没有 "pingpong"（往复）**：
/// HTML5 的 `playbackRate` 不接受负值，Chromium 没有反向解码能力，
/// 倒放只能靠逐帧 seek —— 4K 素材上必然掉帧，与"无感循环"的目标正好相反。
export type VideoLoopMode = "loop" | "once";

/// 动态壁纸"上次生效项"的 localStorage 键（值 JSON：`{ path, monitors }`）。
///
/// 用于**应用重启后自动恢复桌面动态壁纸**：后端状态是进程内的，重启即清空，
/// 所以要靠前端把"上次设了什么、播在哪几台"记下来，启动时重放一次。
///
/// 生命期：设为动态壁纸时写入（含只改播放范围），主动"停止"时清除 ——
/// 停止是明确的用户意图，清掉才能保证下次启动不会自作主张地又播起来。
/// 记录里的文件若已被删除/移动，恢复会失败，届时清除记录（后端会校验文件存在）。
export const VIDEO_ACTIVE_KEY = "dot-wallpaper-video-active";

export const PAGE_SIZE = 12; // 每页加载张数

/// 动态壁纸视频扩展名（与后端 wallpaper::SUPPORTED_VIDEO_EXTS 保持一致）
export const VIDEO_EXTS = ["mp4", "webm", "mkv", "mov"];

/// WebView2 可稳定解码的容器：列表首帧抓取与右侧预览/桌面播放均依赖
/// WebView2 内置解码器，mkv / mov 在 Windows 上大概率无法播放，
/// 需在 UI 上明确标记"不支持预览"，避免用户以为功能坏了。
///
/// 与后端 `wallpaper::PLAYABLE_VIDEO_EXTS` **一一对应** ——
/// 后端用它决定拖入是否接收（mkv/mov 拖入即拒）。**两处必须同步改**，
/// 否则会出现"前端说能播、后端不给导"这类自相矛盾。
export const WEBVIEW_PLAYABLE_EXTS = ["mp4", "webm"];

/// 取路径扩展名（小写，不含点）；无扩展名返回空串。
///
/// **先剥掉 `?query` / `#hash`**：在线视频地址可能带 `?download=1` 这类参数
/// （Pixabay 文档就教用户这么拼下载链接），不剥的话扩展名会算成
/// `mp4?download=1`，`isWebviewPlayable` 直接判 false ⇒ 能播的视频被标成"不兼容"。
/// Windows 文件名里不允许 `?`，所以先剥不会误伤本地路径。
export function extOf(path: string | undefined | null): string {
  if (!path) return "";
  const name = (path.split(/[\\/]/).pop() || "").split(/[?#]/)[0];
  const i = name.lastIndexOf(".");
  return i >= 0 ? name.slice(i + 1).toLowerCase() : "";
}

/// 是否动态壁纸视频（按扩展名判定）
export function isVideoPath(path: string | undefined | null): boolean {
  return VIDEO_EXTS.includes(extOf(path));
}

/// 是否 WebView2 可播放（mp4 / webm）；mkv/mov 返回 false
export function isWebviewPlayable(path: string | undefined | null): boolean {
  return WEBVIEW_PLAYABLE_EXTS.includes(extOf(path));
}

/// 必应在线壁纸列表条目（对应后端 bing.rs BingWallpaper）
export interface BingWallpaperData {
  url: string; // 远程原图完整 URL（https://www.bing.com/...）
  thumb: string; // 远程缩略图 URL（全尺寸 URL 追加 &w=... 参数）
  title: string; // 壁纸标题
  date: string; // 日期（YYYYMMDD），作为下载缓存文件名与卡片对应键
}

/// 本地已下载的必应壁纸条目（对应后端 `list_downloaded_bing_wallpapers`）。
///
/// 后端扫的是**本地壁纸目录**（递归找 `BingWallpaper_*`），
/// 因此这份结果与「本地」选项卡"能否看到该文件"是同一个事实来源：
/// 它亮徽标 ⇒ 跳到本地选项卡一定定位得到。
export interface DownloadedBingData {
  date: string; // 日期（YYYYMMDD），与在线条目同格式，作为徽标判定键
  path: string; // 本地绝对路径
}

/// 必应下载目录信息（对应后端 `get_bing_download_dir_info`，设置页只读展示用）。
///
/// 下载目录是**派生**的（`<壁纸目录>\BingWallpaper`），前端不自己拼路径 ——
/// 派生规则只在 Rust 侧存在一份。
///
/// ⚠️ 字段名保持 **snake_case**：本项目 serde 结构体一律没有 `rename_all`，
/// 线上格式就是 Rust 字段名（`position_hint` / `scale_factor` 同理）。
/// 改成 camelCase 不会报任何错，只会让 `legacy_dir` 静默变成 undefined、
/// 遗留目录提示永远不出现。
export interface BingDownloadDirInfo {
  dir: string; // 当前生效的下载目录（已格式化为反斜杠，可直接展示）
  legacy_dir: string | null; // 老版本遗留的独立下载目录，未设置则 null
  legacy_count: number; // 遗留目录里还剩多少张必应壁纸（0 = 不提示）
}

// ---------- Pixabay 在线图库 ----------

/// Pixabay 下载目录信息（设置页只读展示用）。
/// 与必应的 `BingDownloadDirInfo` 不同：Pixabay 从上线起就是派生目录，
/// 没有老版本遗留的独立目录，所以只有 `dir` 一个字段。
export interface PixabayDownloadDirInfo {
  dir: string; // `<壁纸目录>\Pixabay`（已格式化为反斜杠，可直接展示）
}

/// Pixabay API key 的 localStorage 键。
///
/// **用户自填**（后端不持有、不落盘、不写日志）：每个用户用自己的配额，
/// 也避免公开仓库里出现明文 key 被刷爆。
export const PIXABAY_KEY_KEY = "dot-wallpaper-pixabay-key";

/// 分类 / 排序偏好。
///
/// **搜索词现在也持久化**（2026-10-28 改）：原先不持久化，结果每次启动点进
/// Pixabay 都是空搜索 ⇒ 发无 q 请求 ⇒ 全站最热门杂图（蝴蝶、花、池塘、海），
/// 用户会以为"这个图库就是这些破图"。留着他上次搜的词，至少首屏是他想看的东西。
export const PIXABAY_CATEGORY_KEY = "dot-wallpaper-pixabay-category";
export const PIXABAY_ORDER_KEY = "dot-wallpaper-pixabay-order";
export const PIXABAY_QUERY_KEY = "dot-wallpaper-pixabay-query";

/// Pixabay 内"看哪种媒体"的偏好。
///
/// **只有两档：图片 / 视频** —— 两者走的是完全不同的接口
/// （`/api/` vs `/api/videos/`），参数集也不同，这是唯一必须分叉的维度。
///
/// ⚠️ 历史值迁移：旧版本这里是 `"photo"` / `"illustration"` / `"video"` 三段，
/// 把"插画"当成媒体种类。现在"插画"归到**内容类型**（`image_type`）里，
/// 旧值必须迁移过去，否则老用户升级后选择被静默重置成"照片"。
/// 迁移在 `readPixabayMediaState()` 里一次做完（见那里的注释）。
export const PIXABAY_MEDIA_KEY = "dot-wallpaper-pixabay-media";
export type PixabayMedia = "image" | "video";

/// 图片**内容类型**（接口的 `image_type`）—— 对应 Pixabay 网页筛选栏的「内容类型」。
///
/// 插画是壁纸感最强的一类（实测 `city night + illustration + 精选` 出的是
/// 赛博朋克霓虹街道、天际线剪影，而 photo 多是素材库实拍），
/// 所以它必须是一个**独立可见的筛选项**，而不是藏在分类里。
///
/// `all` 会把照片/插画/矢量混在一起 —— 观感会变杂，但网页也提供这个选项，
/// 用户明确要"全部"时就该给他。
export const PIXABAY_IMAGE_TYPE_KEY = "dot-wallpaper-pixabay-image-type";
export type PixabayImageType = "photo" | "illustration" | "vector" | "all";
export const PIXABAY_IMAGE_TYPES = [
  { label: "照片", value: "photo" },
  { label: "插画", value: "illustration" },
  { label: "矢量", value: "vector" },
  { label: "全部", value: "all" },
] as const;

/// 图像方向（接口的 `orientation`）—— **仅图片接口支持**。
///
/// ⚠️ 视频接口**没有**这个参数：实测传 `orientation=vertical` 返回的 50 条里
/// 仍有 48 条是横屏，与 `horizontal` 完全同分布 ⇒ 等于不存在。
/// 视频的"方向"是后端在解析响应时按宽高自己筛的（`VideoOrientation`），
/// 用的是同一个键与同一套取值，只是生效路径完全不同。
export const PIXABAY_ORIENTATION_KEY = "dot-wallpaper-pixabay-orientation";
/// 视频方向偏好。**与图片分开存**：两者是两套接口，用户对"看竖屏图"和
/// "看竖屏视频"的偏好未必一致（竖屏视频当横屏壁纸会被裁）。
export const PIXABAY_VIDEO_ORIENTATION_KEY = "dot-wallpaper-pixabay-video-orientation";
export type PixabayOrientation = "horizontal" | "vertical" | "all";
export const PIXABAY_ORIENTATIONS = [
  { label: "横向", value: "horizontal" },
  { label: "竖向", value: "vertical" },
  { label: "全部", value: "all" },
] as const;

/// 尺寸档位 → 接口的 `min_width` / `min_height`。
///
/// 网页的「尺寸」是**精确分辨率**，但接口只提供最小宽/高两个下限
/// ⇒ 只能做成"至少 N×M"档位，这一点无法对齐网页，属于接口能力边界。
///
/// 默认 `"1920"` 与旧版硬编码的 `min_width=1920` 完全一致，
/// 升级后不会出现"图突然变小/变多"的观感突变。
export const PIXABAY_SIZE_KEY = "dot-wallpaper-pixabay-size";
export type PixabaySize = "" | "1920" | "2560" | "3840";
export const PIXABAY_SIZES = [
  { label: "不限", value: "", w: 0, h: 0 },
  { label: "≥1920×1080", value: "1920", w: 1920, h: 1080 },
  { label: "≥2560×1440", value: "2560", w: 2560, h: 1440 },
  { label: "≥3840×2160", value: "3840", w: 3840, h: 2160 },
] as const;

/// 颜色筛选（接口的 `colors`，逗号分隔多值）—— **仅图片接口支持**。
///
/// 取值必须与接口文档逐字一致（`grayscale` 与 `gray` 是两个不同的值，不能合并）。
/// `hex` 只用于渲染色块，不参与请求。
export const PIXABAY_COLORS_KEY = "dot-wallpaper-pixabay-colors";
export const PIXABAY_COLORS = [
  { value: "grayscale", label: "黑白", hex: "#9ca3af" },
  { value: "red", label: "红", hex: "#ef4444" },
  { value: "orange", label: "橙", hex: "#f97316" },
  { value: "yellow", label: "黄", hex: "#eab308" },
  { value: "green", label: "绿", hex: "#22c55e" },
  { value: "turquoise", label: "青", hex: "#14b8a6" },
  { value: "blue", label: "蓝", hex: "#3b82f6" },
  { value: "lilac", label: "紫", hex: "#a855f7" },
  { value: "pink", label: "粉", hex: "#ec4899" },
  { value: "white", label: "白", hex: "#f8fafc" },
  { value: "gray", label: "灰", hex: "#6b7280" },
  { value: "black", label: "黑", hex: "#111827" },
  { value: "brown", label: "棕", hex: "#92400e" },
  { value: "transparent", label: "透明", hex: "transparent" },
] as const;

/// 视频类型（接口的 `video_type`）—— 仅视频接口支持。
export const PIXABAY_VIDEO_TYPE_KEY = "dot-wallpaper-pixabay-video-type";
export type PixabayVideoType = "all" | "film" | "animation";
export const PIXABAY_VIDEO_TYPES = [
  { label: "全部", value: "all" },
  { label: "实拍", value: "film" },
  { label: "动画", value: "animation" },
] as const;

/// 视频清晰度偏好（"设置里让用户选"）。
///
/// 只给两档：`medium` = 1080p（默认，3~10MB/条），`large` = 多为 4K（可能几十 MB）。
/// 不给 small/tiny —— 那是接口的降级兜底，不是用户会主动选的目标。
export const PIXABAY_VIDEO_QUALITY_KEY = "dot-wallpaper-pixabay-video-quality";
export type PixabayVideoQuality = "medium" | "large";

/// 清晰度选项。`hint` 说明代价（体积），因为这是唯一需要用户权衡的地方。
export const PIXABAY_VIDEO_QUALITIES = [
  { label: "1080p（推荐）", value: "medium", hint: "单条约 3~10MB，下载快" },
  { label: "4K（更清晰）", value: "large", hint: "单条可能几十 MB；老素材没有 4K 时自动降到 1080p" },
] as const;

/// 分类选项（value 与 API 的 `category` 参数一一对应；空串 = 不限）
///
/// 首项标签是「全部」不是「全部分类」：分类下拉只有 88px，
/// 4 个字（56px）加内边距 38px 会超出，NSelect 会直接裁掉半个字。
export const PIXABAY_CATEGORIES = [
  { label: "全部", value: "" },
  { label: "背景", value: "backgrounds" },
  { label: "自然", value: "nature" },
  { label: "旅行", value: "travel" },
  { label: "建筑", value: "buildings" },
  { label: "动物", value: "animals" },
  { label: "电脑", value: "computer" },
  { label: "交通", value: "transportation" },
  { label: "运动", value: "sports" },
  { label: "美食", value: "food" },
] as const;

/// 排序选项
export const PIXABAY_ORDERS = [
  { label: "热门", value: "popular" },
  { label: "最新", value: "latest" },
] as const;

/// 每页条数：24 是 PAGE_SIZE(12) 的整数倍，滚动加载不会出现"半页"观感
export const PIXABAY_PAGE_SIZE = 24;

/// 单条 Pixabay 结果（对应后端 `pixabay::PixabayWallpaper`）
///
/// ⚠️ 字段名保持 **snake_case**：本项目 serde 结构体没有 `rename_all`，
/// 线上格式就是 Rust 字段名。写成 camelCase 不会报错，只会让 `user_url`
/// 静默变成 undefined、作者链接永远渲染不出来。
export interface PixabayWallpaperData {
  id: number;
  title: string;
  tags: string;
  user: string;
  user_url: string;
  page_url: string;
  url: string;
  thumb: string;
  width: number;
  height: number;
}

/// 一页结果（对应后端 `pixabay::PixabayPage`）
export interface PixabayPageData {
  items: PixabayWallpaperData[];
  count: number;
  total_hits: number;
  page: number;
  has_more: boolean;
  /// true = 免费 key，只拿到 1280px（没有 full API access）
  degraded: boolean;
}

/// 本地已下载的 Pixabay 图（对应后端 `list_downloaded_pixabay_wallpapers`）
export interface DownloadedPixabayData {
  id: string; // 图片 id（字符串形式，与 onlineId 同格式）
  path: string; // 本地绝对路径
}

/// 单条 Pixabay 视频（对应后端 `pixabay::PixabayVideo`）
///
/// 同样 **snake_case**。`url` 是**已按清晰度偏好降级选好**的流地址，
/// `quality` 回报实际拿到的档位（可能与偏好不同：老素材没有 4K）。
export interface PixabayVideoData {
  id: number;
  title: string;
  tags: string;
  user: string;
  user_url: string;
  page_url: string;
  url: string;
  thumb: string;
  width: number;
  height: number;
  duration: number;
  quality: string;
  size: number;
}

/// 一页视频结果（对应后端 `pixabay::PixabayVideoPage`）
///
/// 与图片页不同：**没有 `degraded`**。视频的降级是逐条的"素材没有 4K"，
/// 不是整页的"key 权限不够"，用页级标记会表达错意思。
export interface PixabayVideoPageData {
  items: PixabayVideoData[];
  count: number;
  total_hits: number;
  page: number;
  has_more: boolean;
}

// ---------- Wallhaven 在线图库 ----------

/// Wallhaven 下载目录信息（设置页只读展示用）。
/// 与必应 / Pixabay 一样是派生目录（`<壁纸目录>\Wallhaven`），没有遗留目录。
export interface WallhavenDownloadDirInfo {
  dir: string;
}

/// 搜索词 / 筛选偏好。
///
/// 与 Pixabay 同一套理由：**全部持久化**。不持久化时首屏永远是接口默认的
/// `date_added` 最新一批（大量无关杂图），用户会以为"这个图库就这水平"。
export const WALLHAVEN_QUERY_KEY = "dot-wallpaper-wallhaven-query";
export const WALLHAVEN_CATEGORY_KEY = "dot-wallpaper-wallhaven-category";
export const WALLHAVEN_SORT_KEY = "dot-wallpaper-wallhaven-sort";
export const WALLHAVEN_ATLEAST_KEY = "dot-wallpaper-wallhaven-atleast";
export const WALLHAVEN_RATIO_KEY = "dot-wallpaper-wallhaven-ratio";
export const WALLHAVEN_COLOR_KEY = "dot-wallpaper-wallhaven-color";

/// 自定义接口地址（留空 = 官方）。
///
/// 官方没有中国节点，国内访问时通时不通；社区流传的第三方镜像不可信
/// （无授权、可能篡改内容、随时会死），所以**不内置任何镜像**，
/// 只给一个入口：想用自建反代 / 加速地址的人自己填，不填就还是官方。
export const WALLHAVEN_API_BASE_KEY = "dot-wallpaper-wallhaven-api-base";

/// 自定义图片地址（替换 `th.` / `w.` 两个 CDN 主机，留空 = 官方 CDN）。
///
/// 和接口地址分开是必须的：反代常常只接管了 `/api/`，图片仍指向
/// `th.wallhaven.cc` —— 表现就是"列表拉到了、缩略图全灰"。
export const WALLHAVEN_IMAGE_BASE_KEY = "dot-wallpaper-wallhaven-image-base";

/// 官方地址，仅用于输入框占位提示（后端也有一份，真正生效的是后端那份）。
export const WALLHAVEN_DEFAULT_API_BASE = "https://wallhaven.cc/api/v1/search";

/// 分类：接口是 3 位位掩码（常规 / 动漫 / 人物，各 0 或 1）。
///
/// 做成单选而不是多选：位掩码的 7 种组合对普通用户没有意义，
/// 「全部 / 常规 / 动漫 / 人物」四个选项已经覆盖真实使用场景。
export type WallhavenCategory = "111" | "100" | "010" | "001";
export const WALLHAVEN_CATEGORIES = [
  { label: "全部", value: "111" },
  { label: "常规", value: "100" },
  { label: "动漫", value: "010" },
  { label: "人物", value: "001" },
] as const;

/// 排序。`random` 会由接口生成 seed，前端必须把它带回来翻页（见 store）。
/// `relevance` 只对有关键词的搜索有意义 —— 后端在空搜索时会自动退回最新。
export type WallhavenSort =
  | "favorites"
  | "date_added"
  | "relevance"
  | "random"
  | "views"
  | "toplist";
export const WALLHAVEN_SORTS = [
  { label: "热门", value: "favorites" },
  { label: "最新", value: "date_added" },
  { label: "相关", value: "relevance" },
  { label: "随机", value: "random" },
  { label: "浏览", value: "views" },
  { label: "榜单", value: "toplist" },
] as const;

/// 最低分辨率（接口的 `atleast`，格式 `宽x高`）。空串 = 不限。
export type WallhavenAtleast = "" | "1920x1080" | "2560x1440" | "3840x2160";
export const WALLHAVEN_ATLEASTS = [
  { label: "不限", value: "" },
  { label: "≥1920×1080", value: "1920x1080" },
  { label: "≥2560×1440", value: "2560x1440" },
  { label: "≥3840×2160", value: "3840x2160" },
] as const;

/// 画面比例（接口的 `ratios`）。空串 = 不限。
/// 9:16 是竖屏 —— 竖屏显示器 / 手机壁纸场景会用到，所以保留。
export type WallhavenRatio = "" | "16x9" | "16x10" | "21x9" | "9x16";
export const WALLHAVEN_RATIOS = [
  { label: "不限", value: "" },
  { label: "16:9", value: "16x9" },
  { label: "16:10", value: "16x10" },
  { label: "21:9", value: "21x9" },
  { label: "9:16 竖屏", value: "9x16" },
] as const;

/// 颜色筛选。
///
/// ⚠️ **不能复用 `PIXABAY_COLORS`**：Pixabay 的 `colors` 是英文色名
/// （`turquoise` / `grayscale`），Wallhaven 要的是 **6 位十六进制且不带 `#`**。
/// 两者混用不会报错，只会让颜色筛选静默失效（接口收到 `turquoise` 当成无效值）。
export const WALLHAVEN_COLORS = [
  { value: "000000", label: "黑", hex: "#111827" },
  { value: "808080", label: "灰", hex: "#6b7280" },
  { value: "ffffff", label: "白", hex: "#f8fafc" },
  { value: "ff0000", label: "红", hex: "#ef4444" },
  { value: "ff8000", label: "橙", hex: "#f97316" },
  { value: "ffff00", label: "黄", hex: "#eab308" },
  { value: "00ff00", label: "绿", hex: "#22c55e" },
  { value: "00ffff", label: "青", hex: "#14b8a6" },
  { value: "0000ff", label: "蓝", hex: "#3b82f6" },
  { value: "8000ff", label: "紫", hex: "#a855f7" },
  { value: "ff00ff", label: "粉", hex: "#ec4899" },
  { value: "8b4513", label: "棕", hex: "#92400e" },
] as const;

/// 每页条数：接口固定 24 条/页，**没有 `per_page` 参数**，这里只用于前端提示。
export const WALLHAVEN_PAGE_SIZE = 24;

/// 单条 Wallhaven 结果（对应后端 `wallhaven::WallhavenWallpaper`）
///
/// ⚠️ 字段名保持 **snake_case**：本项目 serde 结构体没有 `rename_all`。
/// 写成 camelCase 不会报错，只会让 `page_url` / `file_type` 静默 undefined。
export interface WallhavenWallpaperData {
  id: string;
  title: string;
  resolution: string;
  category: string;
  page_url: string;
  url: string;
  thumb: string;
  width: number;
  height: number;
  file_size: number;
  file_type: string;
}

/// 一页结果（对应后端 `wallhaven::WallhavenPage`）
export interface WallhavenPageData {
  items: WallhavenWallpaperData[];
  count: number;
  total: number;
  page: number;
  last_page: number;
  has_more: boolean;
  /// `sorting = random` 时第一页返回的种子，后续页必须原样回传
  seed: string | null;
}

/// 本地已下载的 Wallhaven 图（对应后端 `list_downloaded_wallhaven_wallpapers`）
export interface DownloadedWallhavenData {
  id: string; // 6 位字母数字 id，与 onlineId 同格式
  path: string; // 本地绝对路径
}

/// 壁纸模糊遮罩效果参数（与后端 wallpaper::WallpaperEffect 对齐）
export interface WallpaperEffect {
  enabled: boolean; // 是否启用效果
  blur: number; // 高斯模糊强度（0~30）
  opacity: number; // 遮罩不透明度百分比（0~80）
  color: string; // 遮罩颜色 #RRGGBB（默认黑色 #000000）
}
