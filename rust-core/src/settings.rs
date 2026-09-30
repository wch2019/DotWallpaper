// 版本化 JSON 设置：壁纸目录、逐显示器分配、默认填充/静音、暂停状态。
// 单文件存储于应用配置目录，不引入 SQLite。损坏时回退默认并备份。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::types::{FitMode, WallpaperAssignment};

/// v2：显示器 ID 去掉了内嵌易变 CGDisplayID 的 model 段（旧格式拔插后必然失配），
/// 载入时就地迁移，见 `migrate_display_ids`。
pub const SETTINGS_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub version: u32,
    pub library_dir: String,
    pub default_fit_mode: FitMode,
    pub default_muted: bool,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub assignments: HashMap<String, WallpaperAssignment>,
    /// 处于用户暂停状态的显示器 ID
    #[serde(default)]
    pub paused_displays: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            library_dir: String::new(),
            default_fit_mode: FitMode::Fill,
            default_muted: true,
            onboarding_completed: false,
            assignments: HashMap::new(),
            paused_displays: Vec::new(),
        }
    }
}

struct Store {
    path: PathBuf,
    settings: AppSettings,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

/// 启动时加载设置；文件缺失用默认值，解析失败或版本不受支持则备份后回退默认。
pub fn init_with_path(config_dir: PathBuf) {
    let path = config_dir.join("settings.json");
    let mut settings = load_from(&path);
    if settings.library_dir.trim().is_empty() {
        settings.library_dir = crate::runtime::picture_dir().to_string_lossy().to_string();
    }
    let mut store = STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *store = Some(Store { path, settings });
    // Do not use `expect` here: settings initialization runs during app startup,
    // and a recoverable persistence error must never abort the native host.
    if let Some(store_ref) = store.as_mut() {
        if let Err(error) = save_locked(store_ref) {
            eprintln!("[settings] 初始化保存失败: {error}");
        }
    }
}

pub(crate) fn load_from(path: &std::path::Path) -> AppSettings {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(s) if s.version <= SETTINGS_VERSION => migrate(s),
            Ok(s) => {
                eprintln!("[settings] 未知配置版本 {}，回退默认并备份", s.version);
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
                AppSettings::default()
            }
            Err(e) => {
                eprintln!("[settings] 配置损坏，回退默认并备份: {e}");
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    }
}

/// 旧版显示器 ID 形如 `disp-<vendor>-<model>-<serial>`，其中 model 混进了易变的
/// CGDisplayID——重新插拔后就再也匹配不上，配置会变成永远恢复不了的幽灵项。
/// 迁移为现行 `disp-<vendor>-<serial>`；同型号多屏被折叠到同一键时以载入顺序后者为准。
fn migrate(mut s: AppSettings) -> AppSettings {
    fn normalize(id: &str) -> String {
        let Some(rest) = id.strip_prefix("disp-") else {
            return id.to_string();
        };
        let parts: Vec<&str> = rest.split('-').collect();
        if parts.len() == 3 {
            return format!("disp-{}-{}", parts[0], parts[2]);
        }
        id.to_string()
    }

    let old = std::mem::take(&mut s.assignments);
    for (key, a) in old {
        let key = normalize(&key);
        s.assignments.insert(
            key.clone(),
            WallpaperAssignment {
                display_id: key,
                ..a
            },
        );
    }
    s.paused_displays = s.paused_displays.iter().map(|d| normalize(d)).collect();
    s.version = SETTINGS_VERSION;
    s
}

pub fn get() -> AppSettings {
    // A poisoned settings mutex should degrade to the last recoverable value,
    // not panic on the monitor thread and terminate the whole macOS app.
    let guard = match STORE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            eprintln!("[settings] 配置锁已中毒，继续使用当前内存配置");
            poisoned.into_inner()
        }
    };
    guard
        .as_ref()
        .map(|s| s.settings.clone())
        .unwrap_or_default()
}

fn save_locked(store: &mut Store) -> Result<(), String> {
    if let Some(parent) = store.path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&store.settings)
        .map_err(|e| format!("序列化设置失败: {e}"))?;
    let tmp = store.path.with_extension("json.tmp");
    let result = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &store.path));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("保存设置文件失败: {e}"));
    }
    Ok(())
}

/// UI-originated updates must not claim success if the new settings are not durable.
/// Revert the in-memory copy when the atomic file replacement fails.
pub fn update_checked<F: FnOnce(&mut AppSettings)>(f: F) -> Result<AppSettings, String> {
    let mut guard = STORE.lock().map_err(|e| format!("配置锁不可用: {e}"))?;
    let store = guard.as_mut().ok_or("配置尚未初始化")?;
    let previous = store.settings.clone();
    f(&mut store.settings);
    store.settings.version = SETTINGS_VERSION;
    if let Err(error) = save_locked(store) {
        store.settings = previous;
        return Err(error);
    }
    Ok(store.settings.clone())
}

pub fn update<F: FnOnce(&mut AppSettings)>(f: F) -> AppSettings {
    match update_checked(f) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("[settings] {error}");
            get()
        }
    }
}

/// Persist the directory selected by the native open panel before the UI claims success.
/// The path is checked here so a stale or inaccessible selection cannot replace a valid library.
pub fn set_library_directory(path: &str) -> Result<String, String> {
    let canonical =
        std::fs::canonicalize(path).map_err(|e| format!("无法访问所选目录 {path}: {e}"))?;
    if !canonical.is_dir() {
        return Err(format!("所选路径不是文件夹: {}", canonical.display()));
    }
    let dir = canonical.to_string_lossy().into_owned();
    update_checked(|settings| settings.library_dir = dir.clone())?;
    Ok(dir)
}

pub fn record_assignment(assignment: &WallpaperAssignment) {
    update(|s| {
        s.assignments
            .insert(assignment.display_id.clone(), assignment.clone());
    });
}

/// 用户显式停止某显示器的壁纸：清除持久化分配，避免下次启动又被自动恢复。
pub fn forget_assignment(display_id: &str) {
    update(|s| {
        s.assignments.remove(display_id);
    });
}

#[cfg(test)]
pub fn init_for_test(path: PathBuf, settings: AppSettings) {
    let mut store = STORE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *store = Some(Store { path, settings });
}

pub fn set_display_paused(display_id: &str, paused: bool) {
    update(|s| {
        let mut set: HashSet<String> = s.paused_displays.iter().cloned().collect();
        if paused {
            set.insert(display_id.to_string());
        } else {
            set.remove(display_id);
        }
        s.paused_displays = set.into_iter().collect();
    });
}
