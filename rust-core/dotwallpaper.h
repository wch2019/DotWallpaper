#ifndef DOTWALLPAPER_H
#define DOTWALLPAPER_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * State change callback: receives JSON string of DisplayWallpaperState
 */
typedef void (*FfiStateCallback)(const char*);

/**
 * Thumbnail ready callback: receives (path, thumb)
 */
typedef void (*FfiThumbCallback)(const char*, const char*);

/**
 * Directory pick callback: (persisted path, error); both null on cancel
 */
typedef void (*FfiDirectoryCallback)(const char*, const char*);

/**
 * File pick callback: receives JSON array of paths or null on cancel
 */
typedef void (*FfiStringArrayCallback)(const char*);

/**
 * Initialize the core library. Must be called once before any other function.
 * `config_dir` is the path to the application support directory.
 * `cache_dir` is the path to the cache directory.
 * Returns 0 on success, negative on failure.
 */
int32_t dw_init(const char *config_dir, const char *_cache_dir);

/**
 * Shutdown the core library. Call before app exit.
 */
void dw_shutdown(void);

/**
 * Get the full app snapshot as JSON string.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_get_app_snapshot(void);

/**
 * Scan library directory and return media list as JSON.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_list_media(void);

/**
 * Import media files into the library directory.
 * `paths_json` is a JSON array of file paths.
 * Returns JSON with `saved` and `skipped` arrays.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_import_media(const char *paths_json);

/**
 * Delete a media file from the library.
 * Returns 0 on success, error code on failure (error message via dw_get_last_error).
 */
int32_t dw_delete_media(const char *path);

/**
 * Get display list as JSON array.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_list_displays(void);

/**
 * Apply wallpaper. `assignment_json` is a JSON object matching WallpaperAssignment.
 * Returns JSON of DisplayWallpaperState.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_apply_wallpaper(const char *assignment_json);

/**
 * Control playback. `action` is "pause", "resume", or "stop".
 * Returns JSON of DisplayWallpaperState.
 * Caller must free the returned string with `dw_free_string`.
 */
char *dw_control_playback(const char *display_id, const char *action);

/**
 * Update settings. `settings_json` contains fields to update.
 * Returns 0 on success, -1 on failure.
 */
int32_t dw_update_settings(const char *settings_json);

/**
 * Pick library directory via native dialog.
 * `cb` receives the persisted path, or an error; both null on cancel.
 */
void dw_pick_library_directory(FfiDirectoryCallback cb);

/**
 * Pick media files via native dialog.
 * `cb` is called with JSON array of selected paths (or null on cancel).
 */
void dw_pick_media_files(FfiStringArrayCallback cb);

/**
 * Restore wallpapers on startup and begin monitoring display changes.
 */
void dw_startup(void);

/**
 * Register state change callback.
 */
void dw_set_state_callback(FfiStateCallback cb);

/**
 * Register thumbnail ready callback.
 */
void dw_set_thumb_callback(FfiThumbCallback cb);

/**
 * Free a string returned by any dw_* function.
 */
void dw_free_string(char *s);

/**
 * Get the last error message.
 * Caller must free with `dw_free_string`.
 */
char *dw_get_last_error(void);

#endif /* DOTWALLPAPER_H */
