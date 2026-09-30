import Foundation
import AppKit

/// Swift wrapper around the Rust FFI library (libdotwallpaper.a)
class DotWallpaperCore {

    static let shared = DotWallpaperCore()
    private var initialized = false

    // Pending completions for pick operations
    private static var pendingDirectoryCompletion: ((String?, String?) -> Void)?
    private static var pendingFilesCompletion: ((String?) -> Void)?

    init() {
        initialize()
    }

    deinit {
        if initialized {
            dw_shutdown()
        }
    }

    private func initialize() {
        // Get application support directory
        let fm = FileManager.default
        let appSupport = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!
        let configDir = appSupport.appendingPathComponent("com.dot.wallpaper")
        let cacheDir = fm.urls(for: .cachesDirectory, in: .userDomainMask).first!
            .appendingPathComponent("com.dot.wallpaper")

        // Create directories if needed
        try? fm.createDirectory(at: configDir, withIntermediateDirectories: true)
        try? fm.createDirectory(at: cacheDir, withIntermediateDirectories: true)

        // Register callbacks
        dw_set_state_callback { cString in
            guard let cString = cString else { return }
            let json = String(cString: cString)
            NotificationCenter.default.post(
                name: .dotWallpaperStateUpdate,
                object: nil,
                userInfo: ["json": json]
            )
        }

        dw_set_thumb_callback { pathPtr, thumbPtr in
            guard let pathPtr = pathPtr, let thumbPtr = thumbPtr else { return }
            let path = String(cString: pathPtr)
            let thumb = String(cString: thumbPtr)
            NotificationCenter.default.post(
                name: .dotWallpaperThumbReady,
                object: nil,
                userInfo: ["path": path, "thumb": thumb]
            )
        }

        // Initialize library
        let configPath = configDir.path
        let cachePath = cacheDir.path

        let result = configPath.withCString { configPtr in
            cachePath.withCString { cachePtr in
                dw_init(configPtr, cachePtr)
            }
        }

        if result == 0 {
            initialized = true
            print("[DotWallpaper] Core initialized successfully")
        } else {
            print("[DotWallpaper] Failed to initialize core: \(result)")
        }
    }

    // MARK: - Public API

    func getAppSnapshot() -> String? {
        guard initialized else { return nil }
        let ptr = dw_get_app_snapshot()
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func listMedia() -> String? {
        guard initialized else { return nil }
        let ptr = dw_list_media()
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func listDisplays() -> String? {
        guard initialized else { return nil }
        let ptr = dw_list_displays()
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func applyWallpaper(_ assignmentJson: String) -> String? {
        guard initialized else { return nil }
        let ptr = assignmentJson.withCString { buf in
            dw_apply_wallpaper(buf)
        }
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func controlPlayback(_ displayId: String, _ action: String) -> String? {
        guard initialized else { return nil }
        let ptr = displayId.withCString { idPtr in
            action.withCString { actionPtr in
                dw_control_playback(idPtr, actionPtr)
            }
        }
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func updateSettings(_ settingsJson: String) -> Bool {
        guard initialized else { return false }
        let result = settingsJson.withCString { buf in
            dw_update_settings(buf)
        }
        return result == 0
    }

    func importMedia(_ pathsJson: String) -> String? {
        guard initialized else { return nil }
        let ptr = pathsJson.withCString { buf in
            dw_import_media(buf)
        }
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func deleteMedia(_ path: String) -> Bool {
        guard initialized else { return false }
        let result = path.withCString { buf in
            dw_delete_media(buf)
        }
        return result == 0
    }

    func lastError() -> String? {
        guard initialized else { return nil }
        let ptr = dw_get_last_error()
        defer { dw_free_string(ptr) }
        return ptr.map { String(cString: $0) }
    }

    func pickLibraryDirectory(completion: @escaping (String?, String?) -> Void) {
        guard initialized else {
            completion(nil, "原生核心尚未初始化")
            return
        }

        DotWallpaperCore.pendingDirectoryCompletion = completion

        dw_pick_library_directory { pathPtr, errorPtr in
            guard let completion = DotWallpaperCore.pendingDirectoryCompletion else { return }
            DotWallpaperCore.pendingDirectoryCompletion = nil
            let path = pathPtr.map { String(cString: $0) }
            let error = errorPtr.map { String(cString: $0) }
            DispatchQueue.main.async { completion(path, error) }
        }
    }

    func pickMediaFiles(completion: @escaping (String?) -> Void) {
        guard initialized else {
            completion(nil)
            return
        }

        DotWallpaperCore.pendingFilesCompletion = completion

        dw_pick_media_files { ptr in
            guard let completion = DotWallpaperCore.pendingFilesCompletion else { return }
            DotWallpaperCore.pendingFilesCompletion = nil
            if let ptr = ptr {
                let json = String(cString: ptr)
                DispatchQueue.main.async { completion(json) }
            } else {
                DispatchQueue.main.async { completion(nil) }
            }
        }
    }

    func startup() {
        guard initialized else { return }
        dw_startup()
    }

    func shutdown() {
        guard initialized else { return }
        dw_shutdown()
        initialized = false
    }

    func pauseAll() {
        if let displaysJson = listDisplays(),
           let data = displaysJson.data(using: .utf8),
           let displays = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            for display in displays {
                if let id = display["id"] as? String {
                    _ = controlPlayback(id, "pause")
                }
            }
        }
    }

    func resumeAll() {
        if let displaysJson = listDisplays(),
           let data = displaysJson.data(using: .utf8),
           let displays = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            for display in displays {
                if let id = display["id"] as? String {
                    _ = controlPlayback(id, "resume")
                }
            }
        }
    }

    func stopAll() {
        if let displaysJson = listDisplays(),
           let data = displaysJson.data(using: .utf8),
           let displays = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] {
            for display in displays {
                if let id = display["id"] as? String {
                    _ = controlPlayback(id, "stop")
                }
            }
        }
    }
}

// MARK: - Notifications

extension Notification.Name {
    static let dotWallpaperStateUpdate = Notification.Name("dotWallpaperStateUpdate")
    static let dotWallpaperThumbReady = Notification.Name("dotWallpaperThumbReady")
    static let dotWallpaperSettingsChanged = Notification.Name("dotWallpaperSettingsChanged")
}
