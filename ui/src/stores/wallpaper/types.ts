// DotWallpaper 壁纸工具 - 共享类型与常量（stores/wallpaper.ts 拆分子模块）
export type WallpaperKind = "local" | "current" | "bing" | "video";

/// 壁纸来源选项卡：local = 本地壁纸（可增删），system = Windows 自带系统壁纸（只读），
/// favorites = 收藏夹（书签视图：跨本地/系统来源，仅标记不删文件），
/// bing = 必应在线壁纸（列表远程 URL，设为壁纸时才下载本地）
export type WallpaperSource = "local" | "system" | "favorites" | "bing";

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
};
// 选项卡展示顺序（独立于可见性：顺序控制渲染次序，可见性控制是否显示）
export const SOURCE_ORDER_KEY = "dot-wallpaper-source-order";
export const SOURCE_ORDER_DEFAULT: WallpaperSource[] = [
  "local",
  "favorites",
  "system",
  "bing",
];

export interface WallpaperItem {
  key: string;
  kind: WallpaperKind; // 类型：本地 / 当前壁纸（右键目标）/ 视频动态壁纸
  path?: string; // 本地绝对路径（原图/视频）或必应远程 URL
  title?: string;
  thumb?: string; // 缩略图绝对路径（列表加载用；无缩略图时为空）
  date?: string; // 必应壁纸日期（YYYYMMDD），作为下载缓存与卡片对应键
  applying?: boolean; // 是否正在设置中
}

/// 后端列表命令返回条目：原图路径 + 缩略图路径（可能为空）
/// 必应在线壁纸列表额外携带 kind/title/date（其余来源可缺省）
export interface WallpaperEntryData {
  path: string;
  thumb: string;
  kind?: WallpaperKind; // 来源类型（必应在线壁纸为 "bing"；缺省按本地处理）
  title?: string; // 壁纸标题（必应在线壁纸自带；缺省用文件名）
  date?: string; // 必应壁纸日期（YYYYMMDD）
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
}

// ---------- 常量 ----------
export const DIR_STORAGE_KEY = "dot-wallpaper-dir"; // localStorage 持久化键
export const BING_DIR_KEY = "dot-wallpaper-bing-dir"; // localStorage 必应壁纸目录键
export const FAVORITES_KEY = "dot-wallpaper-favorites"; // localStorage 收藏书签集合键
export const EFFECT_STORAGE_KEY = "dot-wallpaper-effect"; // localStorage 壁纸效果键
export const CLOSE_BEHAVIOR_KEY = "dot-wallpaper-close-behavior"; // localStorage 关闭窗口行为键
export type CloseBehavior = "exit" | "tray"; // 关闭窗口：exit=直接退出，tray=最小化到托盘（后台运行）

/// 动态壁纸静音偏好的 localStorage 键（值 "1" = 静音）。
///
/// 持久化的是**偏好**而非运行时状态：暂停不持久化（重启后本就没有动态壁纸，
/// 存它没有意义），静音则希望"下次设动态壁纸还是静的"。
export const VIDEO_MUTED_KEY = "dot-wallpaper-video-muted";

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

/// 取路径扩展名（小写，不含点）；无扩展名返回空串
export function extOf(path: string | undefined | null): string {
  if (!path) return "";
  const name = path.split(/[\\/]/).pop() || "";
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

/// 壁纸模糊遮罩效果参数（与后端 wallpaper::WallpaperEffect 对齐）
export interface WallpaperEffect {
  enabled: boolean; // 是否启用效果
  blur: number; // 高斯模糊强度（0~30）
  opacity: number; // 遮罩不透明度百分比（0~80）
  color: string; // 遮罩颜色 #RRGGBB（默认黑色 #000000）
}
