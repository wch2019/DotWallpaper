// 前后端共享的数据结构

use serde::{Deserialize, Serialize};

/// 媒体类型：首版仅静态图片与视频
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
}

/// 填充模式：fill 按比例铺满允许裁剪；fit 完整显示允许留边
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FitMode {
    Fill,
    Fit,
}

/// 显示器壁纸阶段
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Static,
    Preparing,
    Playing,
    Paused,
    Error,
}

/// 播放控制动作
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ControlAction {
    Pause,
    Resume,
    Stop,
}

/// 壁纸分配（path 为本地绝对路径，display_id 为稳定显示器标识）
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperAssignment {
    pub display_id: String,
    pub path: String,
    pub kind: MediaKind,
    pub fit_mode: FitMode,
    pub muted: bool,
}

/// 显示器壁纸状态
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayWallpaperState {
    pub display_id: String,
    pub phase: Phase,
    pub assignment: Option<WallpaperAssignment>,
    pub error: Option<String>,
}

/// 显示器信息
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub logical_bounds: (f64, f64, f64, f64),
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor: f64,
    pub primary: bool,
    pub mirrored: bool,
    pub temporary: bool,
}

/// 媒体库条目
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub path: String,
    pub name: String,
    pub kind: MediaKind,
    pub thumb: String,
    pub mtime: i64,
}

/// 设置更新载荷。库目录不属于此结构，目录只能由原生选择器修改。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendSettings {
    pub default_fit_mode: FitMode,
    pub default_muted: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
}

/// 前端启动时使用的完整应用快照。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub library_dir: String,
    pub default_fit_mode: FitMode,
    pub default_muted: bool,
    pub onboarding_completed: bool,
    pub displays: Vec<DisplayInfo>,
    pub states: Vec<DisplayWallpaperState>,
}
