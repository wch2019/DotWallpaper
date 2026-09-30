// C FFI bridge layer — exposes core functionality to Swift/Xcode via C ABI.
// All functions are `extern "C"` and use only C-compatible types.

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json;

use crate::types::{ControlAction, FitMode, WallpaperAssignment};

// ── Callback types ──────────────────────────────────────────────────

/// State change callback: receives JSON string of DisplayWallpaperState
pub type FfiStateCallback = extern "C" fn(*const std::ffi::c_char);

/// Thumbnail ready callback: receives (path_json, thumb_json)
pub type FfiThumbCallback = extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char);

/// Directory pick callback: (persisted path, error); both null on cancel.
pub type FfiDirectoryCallback = extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char);

/// File pick callback: receives JSON array of paths or null on cancel
pub type FfiStringArrayCallback = extern "C" fn(*const std::ffi::c_char);

// ── Initialization ─────────────────────────────────────────────────

static INIT: OnceLock<()> = OnceLock::new();

/// Initialize the core library. Must be called once before any other function.
/// `config_dir` is the path to the application support directory.
/// `cache_dir` is the path to the cache directory.
///
/// # Safety
/// `config_dir` must be null or point to a valid, NUL-terminated C string.
/// `_cache_dir` is currently ignored and may be null.
#[no_mangle]
pub unsafe extern "C" fn dw_init(
    config_dir: *const std::ffi::c_char,
    _cache_dir: *const std::ffi::c_char,
) -> i32 {
    if INIT.get().is_some() {
        return 0; // already initialized
    }

    let config_path = if config_dir.is_null() {
        match crate::runtime::app_support_dir() {
            Ok(p) => p,
            Err(_) => return -1,
        }
    } else {
        let c_str = CStr::from_ptr(config_dir);
        PathBuf::from(c_str.to_string_lossy().to_string())
    };

    // Ensure config directory exists
    if std::fs::create_dir_all(&config_path).is_err() {
        return -2;
    }

    // Initialize runtime
    crate::runtime::init();

    // Initialize settings
    crate::settings::init_with_path(config_path);

    // Initialize engine callbacks
    crate::engine::set_state_callback(ffi_state_handler);
    crate::thumbs::set_thumb_callback(ffi_thumb_handler);

    let _ = INIT.set(());
    0
}

/// Shutdown the core library. Call before app exit.
#[no_mangle]
pub extern "C" fn dw_shutdown() {
    crate::engine::teardown_all_sync();
}

// ── App Snapshot ───────────────────────────────────────────────────

/// Get the full app snapshot as JSON string.
/// Caller must free the returned string with `dw_free_string`.
#[no_mangle]
pub extern "C" fn dw_get_app_snapshot() -> *mut std::ffi::c_char {
    let s = crate::settings::get();
    let displays = crate::displays::enumerate_on_main_safe();
    let states = crate::engine::states_snapshot();

    let snapshot = serde_json::json!({
        "libraryDir": s.library_dir,
        "defaultFitMode": s.default_fit_mode,
        "defaultMuted": s.default_muted,
        "onboardingCompleted": s.onboarding_completed,
        "displays": displays,
        "states": states,
    });

    let json = serde_json::to_string(&snapshot).unwrap_or_default();
    CString::new(json).unwrap_or_default().into_raw()
}

// ── Media ──────────────────────────────────────────────────────────

/// Scan library directory and return media list as JSON.
/// Caller must free the returned string with `dw_free_string`.
#[no_mangle]
pub extern "C" fn dw_list_media() -> *mut std::ffi::c_char {
    let dir = crate::settings::get().library_dir;
    match crate::media::scan(&dir) {
        Ok(items) => {
            let entries = crate::thumbs::make_entries(items);
            let json = serde_json::to_string(&entries).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let err = serde_json::json!({"error": e});
            let json = serde_json::to_string(&err).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
    }
}

/// Import media files into the library directory.
/// `paths_json` is a JSON array of file paths.
/// Returns JSON with `saved` and `skipped` arrays.
/// Caller must free the returned string with `dw_free_string`.
///
/// # Safety
/// `paths_json` must point to a valid, NUL-terminated JSON C string.
#[no_mangle]
pub unsafe extern "C" fn dw_import_media(
    paths_json: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let c_str = unsafe { CStr::from_ptr(paths_json) };
    let paths: Vec<String> =
        serde_json::from_str(c_str.to_string_lossy().as_ref()).unwrap_or_default();

    let (saved, skipped) = crate::media::import(&paths);
    let result = serde_json::json!({"saved": saved, "skipped": skipped});
    let json = serde_json::to_string(&result).unwrap_or_default();
    CString::new(json).unwrap_or_default().into_raw()
}

/// Delete a media file from the library.
/// Returns 0 on success, error code on failure (error message via dw_get_last_error).
///
/// # Safety
/// `path` must point to a valid, NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn dw_delete_media(path: *const std::ffi::c_char) -> i32 {
    let c_str = unsafe { CStr::from_ptr(path) };
    let path_str = c_str.to_string_lossy().to_string();

    match crate::media::delete(&path_str) {
        Ok(()) => 0,
        Err(e) => {
            set_last_error(&e);
            -1
        }
    }
}

// ── Displays ───────────────────────────────────────────────────────

/// Get display list as JSON array.
/// Caller must free the returned string with `dw_free_string`.
#[no_mangle]
pub extern "C" fn dw_list_displays() -> *mut std::ffi::c_char {
    let displays = crate::displays::enumerate_on_main_safe();
    let json = serde_json::to_string(&displays).unwrap_or_default();
    CString::new(json).unwrap_or_default().into_raw()
}

// ── Wallpaper ──────────────────────────────────────────────────────

/// Apply wallpaper. `assignment_json` is a JSON object matching WallpaperAssignment.
/// Returns JSON of DisplayWallpaperState.
/// Caller must free the returned string with `dw_free_string`.
///
/// # Safety
/// `assignment_json` must point to a valid, NUL-terminated JSON C string.
#[no_mangle]
pub unsafe extern "C" fn dw_apply_wallpaper(
    assignment_json: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let c_str = unsafe { CStr::from_ptr(assignment_json) };
    let assignment: WallpaperAssignment =
        match serde_json::from_str(c_str.to_string_lossy().as_ref()) {
            Ok(a) => a,
            Err(e) => {
                let err = serde_json::json!({"error": format!("解析失败: {e}")});
                let json = serde_json::to_string(&err).unwrap_or_default();
                return CString::new(json).unwrap_or_default().into_raw();
            }
        };

    match crate::engine::apply(assignment) {
        Ok(state) => {
            let json = serde_json::to_string(&state).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let err = serde_json::json!({"error": e});
            let json = serde_json::to_string(&err).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
    }
}

/// Control playback. `action` is "pause", "resume", or "stop".
/// Returns JSON of DisplayWallpaperState.
/// Caller must free the returned string with `dw_free_string`.
///
/// # Safety
/// `display_id` and `action` must each point to valid, NUL-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn dw_control_playback(
    display_id: *const std::ffi::c_char,
    action: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let id_cstr = unsafe { CStr::from_ptr(display_id) };
    let action_cstr = unsafe { CStr::from_ptr(action) };

    let id = id_cstr.to_string_lossy().to_string();
    let action_str = action_cstr.to_string_lossy().to_string();

    let ctrl_action = match action_str.as_str() {
        "pause" => ControlAction::Pause,
        "resume" => ControlAction::Resume,
        "stop" => ControlAction::Stop,
        _ => {
            let err = serde_json::json!({"error": format!("未知操作: {action_str}")});
            let json = serde_json::to_string(&err).unwrap_or_default();
            return CString::new(json).unwrap_or_default().into_raw();
        }
    };

    match crate::engine::control(id, ctrl_action) {
        Ok(state) => {
            let json = serde_json::to_string(&state).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
        Err(e) => {
            let err = serde_json::json!({"error": e});
            let json = serde_json::to_string(&err).unwrap_or_default();
            CString::new(json).unwrap_or_default().into_raw()
        }
    }
}

// ── Settings ───────────────────────────────────────────────────────

/// Update settings. `settings_json` contains fields to update.
/// Returns 0 on success, -1 on failure.
///
/// # Safety
/// `settings_json` must point to a valid, NUL-terminated JSON C string.
#[no_mangle]
pub unsafe extern "C" fn dw_update_settings(settings_json: *const std::ffi::c_char) -> i32 {
    let c_str = unsafe { CStr::from_ptr(settings_json) };
    let json_str = c_str.to_string_lossy().to_string();

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct UpdateReq {
        #[serde(default)]
        default_fit_mode: Option<FitMode>,
        #[serde(default)]
        default_muted: Option<bool>,
        onboarding_completed: Option<bool>,
    }

    let req: UpdateReq = match serde_json::from_str(&json_str) {
        Ok(r) => r,
        Err(e) => {
            set_last_error(&format!("解析失败: {e}"));
            return -1;
        }
    };

    let was_onboarded = crate::settings::get().onboarding_completed;
    let updated = crate::settings::update_checked(|s| {
        if let Some(fit) = req.default_fit_mode {
            s.default_fit_mode = fit;
        }
        if let Some(muted) = req.default_muted {
            s.default_muted = muted;
        }
        if let Some(completed) = req.onboarding_completed {
            s.onboarding_completed = completed;
        }
    });
    let updated = match updated {
        Ok(settings) => settings,
        Err(error) => {
            set_last_error(&error);
            return -1;
        }
    };
    if !was_onboarded && updated.onboarding_completed {
        // Startup intentionally skipped existing assignments during setup.
        crate::engine::restore_on_startup();
    }

    0
}

/// Pick and persist a library directory via native dialog. An unsuccessful write is
/// an error, not a successful selection that disappears after restarting.
#[no_mangle]
pub extern "C" fn dw_pick_library_directory(cb: FfiDirectoryCallback) {
    let result = crate::runtime::on_main(move |mtm| match crate::desktop::pick_directory(mtm) {
        None => cb(std::ptr::null(), std::ptr::null()),
        Some(path) => match crate::settings::set_library_directory(&path) {
            Ok(saved) => {
                let path = CString::new(saved).unwrap_or_default();
                cb(path.as_ptr(), std::ptr::null());
            }
            Err(error) => {
                let message = CString::new(error).unwrap_or_default();
                cb(std::ptr::null(), message.as_ptr());
            }
        },
    });
    if let Err(error) = result {
        let message = CString::new(error).unwrap_or_default();
        cb(std::ptr::null(), message.as_ptr());
    }
}

/// Pick media files via native dialog.
/// `cb` is called with JSON array of selected paths (or null on cancel).
#[no_mangle]
pub extern "C" fn dw_pick_media_files(cb: FfiStringArrayCallback) {
    let _ = crate::runtime::on_main(move |_mtm| {
        let files = crate::desktop::pick_files(_mtm);
        if files.is_empty() {
            cb(std::ptr::null());
        } else {
            let json = serde_json::to_string(&files).unwrap_or_default();
            let c_str = CString::new(json).unwrap_or_default();
            cb(c_str.as_ptr());
        }
    });
}

// ── Startup / Monitor ─────────────────────────────────────────────

/// Restore wallpapers on startup and begin monitoring display changes.
#[no_mangle]
pub extern "C" fn dw_startup() {
    crate::engine::restore_on_startup();
    crate::engine::spawn_monitor();
}

// ── Callbacks ──────────────────────────────────────────────────────

static mut STATE_CALLBACK: Option<FfiStateCallback> = None;
static mut THUMB_CALLBACK: Option<FfiThumbCallback> = None;

/// Register state change callback.
#[no_mangle]
pub extern "C" fn dw_set_state_callback(cb: FfiStateCallback) {
    unsafe {
        STATE_CALLBACK = Some(cb);
    }
}

/// Register thumbnail ready callback.
#[no_mangle]
pub extern "C" fn dw_set_thumb_callback(cb: FfiThumbCallback) {
    unsafe {
        THUMB_CALLBACK = Some(cb);
    }
}

extern "C" fn ffi_state_handler(json: *const std::ffi::c_char) {
    unsafe {
        if let Some(cb) = STATE_CALLBACK {
            cb(json);
        }
    }
}

extern "C" fn ffi_thumb_handler(path: *const std::ffi::c_char, thumb: *const std::ffi::c_char) {
    unsafe {
        if let Some(cb) = THUMB_CALLBACK {
            cb(path, thumb);
        }
    }
}

// ── Memory management ─────────────────────────────────────────────

/// Free a string returned by any dw_* function.
///
/// # Safety
/// `s` must be null or a pointer previously returned by one of the `dw_*`
/// functions that return an owned string, and it must not have been freed yet.
#[no_mangle]
pub unsafe extern "C" fn dw_free_string(s: *mut std::ffi::c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

// ── Error handling ────────────────────────────────────────────────

static LAST_ERROR: OnceLock<std::sync::Mutex<String>> = OnceLock::new();

fn set_last_error(msg: &str) {
    let guard = LAST_ERROR.get_or_init(|| std::sync::Mutex::new(String::new()));
    if let Ok(mut e) = guard.lock() {
        *e = msg.to_string();
    }
}

/// Get the last error message.
/// Caller must free with `dw_free_string`.
#[no_mangle]
pub extern "C" fn dw_get_last_error() -> *mut std::ffi::c_char {
    let guard = LAST_ERROR.get_or_init(|| std::sync::Mutex::new(String::new()));
    if let Ok(e) = guard.lock() {
        CString::new(e.clone()).unwrap_or_default().into_raw()
    } else {
        std::ptr::null_mut()
    }
}
