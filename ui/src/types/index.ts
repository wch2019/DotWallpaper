// DotWallpaper 前端类型定义 —— 与 Rust/Swift native bridge 契约一一对应，字段全部 camelCase

/// 媒体类型
export type MediaKind = "image" | "video";

/// 壁纸填充模式
export type FitMode = "fill" | "fit";

/// 显示器壁纸阶段
export type PlaybackPhase = "static" | "preparing" | "playing" | "paused" | "error";

/// 播放控制动作
export type PlaybackAction = "pause" | "resume" | "stop";

/// 一次壁纸分配（应用到某台显示器）
export interface WallpaperAssignment {
  displayId: string;
  path: string;
  kind: MediaKind;
  fitMode: FitMode;
  muted: boolean;
}

/// 某台显示器的当前壁纸状态
export interface DisplayWallpaperState {
  displayId: string;
  phase: PlaybackPhase;
  assignment: WallpaperAssignment | null;
  error: string | null;
}

/// 显示器信息
export interface DisplayInfo {
  id: string;
  name: string;
  logicalBounds: [number, number, number, number];
  /// 显示器实际像素尺寸；Retina 屏通常是逻辑尺寸的 2 倍。
  pixelWidth: number;
  pixelHeight: number;
  scaleFactor: number;
  primary: boolean;
  mirrored: boolean;
  /// true = 该屏未上报 EDID 序列号，ID 仅在本次连接内有效
  temporary: boolean;
}

/// 媒体库条目：thumb 也是本地文件路径
export interface MediaItem {
  path: string;
  name: string;
  kind: MediaKind;
  thumb: string;
  /// 修改时间（Unix 秒），0 = 后端取不到
  mtime: number;
}

/// 首次加载快照
export interface AppSnapshot {
  libraryDir: string;
  defaultFitMode: FitMode;
  defaultMuted: boolean;
  onboardingCompleted: boolean;
  displays: DisplayInfo[];
  states: DisplayWallpaperState[];
}

/// `update_settings` 的入参：只有显示方式与静音两个偏好。
/// 壁纸库目录**不在这里**——它只能由 `pick_library_directory` 改并在后端立即落盘；
/// 放进本载荷的话，快照还没读回来时点一下静音就会把空目录写进设置，下次扫描直接失败。
export interface Settings {
  defaultFitMode: FitMode;
  defaultMuted: boolean;
  onboardingCompleted: boolean;
}

/// 导入结果
export interface ImportResult {
  saved: string[];
  skipped: string[];
}
