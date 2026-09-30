import SwiftUI
import WebKit

struct ContentView: View {
    @StateObject private var bridge = WebViewBridge()

    var body: some View {
        WebViewRepresentable(bridge: bridge)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

final class WebViewBridge: NSObject, ObservableObject {
    weak var webView: WKWebView?
    let core = DotWallpaperCore.shared
    private var installed = false
    private let mediaQueue = DispatchQueue(label: "com.dot.wallpaper.media", qos: .userInitiated, attributes: .concurrent)
    private var observers: [NSObjectProtocol] = []

    override init() {
        super.init()
        observers.append(NotificationCenter.default.addObserver(
            forName: .dotWallpaperStateUpdate,
            object: nil,
            queue: .main
        ) { [weak self] note in
            guard let json = note.userInfo?["json"] as? String else { return }
            self?.sendEvent(name: "wallpaper-state", json: json)
        })
        observers.append(NotificationCenter.default.addObserver(
            forName: .dotWallpaperSettingsChanged,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.sendEvent(name: "settings-changed", json: "{}")
        })
        observers.append(NotificationCenter.default.addObserver(
            forName: .dotWallpaperThumbReady,
            object: nil,
            queue: .main
        ) { [weak self] note in
            guard let path = note.userInfo?["path"] as? String,
                  let thumb = note.userInfo?["thumb"] as? String else { return }
            let json = "{\"path\":\(Self.jsonString(path)),\"thumb\":\(Self.jsonString(thumb))}"
            self?.sendEvent(name: "thumbnail-ready", json: json)
        })
    }

    deinit {
        observers.forEach(NotificationCenter.default.removeObserver)
    }

    func install(on webView: WKWebView) {
        guard !installed else { return }
        installed = true
        self.webView = webView
        webView.configuration.userContentController.add(
            WeakScriptMessageHandler(bridge: self),
            name: "nativeBridge"
        )
    }

    func handleMessage(_ message: [String: Any]) {
        guard let action = message["action"] as? String else { return }
        let requestId = message["requestId"] as? String

        switch action {
        case "getAppSnapshot":
            reply(requestId, core.getAppSnapshot())
        case "listMedia":
            // Recursive scans and thumbnail prefetch may wait for filesystem I/O
            // and AVFoundation on the main thread. Never block WebKit's callback.
            mediaQueue.async { [weak self] in
                guard let self else { return }
                self.reply(requestId, self.core.listMedia())
            }
        case "listDisplays":
            reply(requestId, core.listDisplays())
        case "applyWallpaper":
            guard let assignment = message["assignment"] as? String else {
                replyError(requestId, "缺少壁纸分配参数")
                return
            }
            reply(requestId, core.applyWallpaper(assignment))
        case "controlPlayback":
            guard let displayId = message["displayId"] as? String,
                  let controlAction = message["controlAction"] as? String else {
                replyError(requestId, "缺少播放控制参数")
                return
            }
            reply(requestId, core.controlPlayback(displayId, controlAction))
        case "updateSettings":
            guard let settings = message["settings"] as? String else {
                replyError(requestId, "缺少设置参数")
                return
            }
            if core.updateSettings(settings) {
                reply(requestId, "{\"ok\":true}")
            } else {
                replyError(requestId, core.lastError() ?? "保存设置失败")
            }
        case "importMedia":
            guard let paths = message["paths"] as? String else {
                replyError(requestId, "缺少导入文件参数")
                return
            }
            reply(requestId, core.importMedia(paths))
        case "deleteMedia":
            guard let path = message["path"] as? String else {
                replyError(requestId, "缺少媒体路径")
                return
            }
            if core.deleteMedia(path) {
                reply(requestId, "{\"ok\":true}")
            } else {
                replyError(requestId, core.lastError() ?? "删除媒体失败")
            }
        case "pickLibraryDirectory":
            core.pickLibraryDirectory { [weak self] path, error in
                if let error {
                    self?.replyError(requestId, error)
                } else {
                    let value = path.map(Self.jsonString) ?? "null"
                    self?.reply(requestId, "{\"pickedDirectory\":\(value)}")
                }
            }
        case "pickMediaFiles":
            core.pickMediaFiles { [weak self] json in
                self?.reply(requestId, "{\"pickedFiles\":\(json ?? "null")}")
            }
        default:
            replyError(requestId, "未知操作: \(action)")
        }
    }

    private func reply(_ requestId: String?, _ json: String?) {
        guard let requestId else { return }
        guard let json else {
            replyError(requestId, "原生核心未返回结果")
            return
        }
        sendJavaScript("window.__nativeCallback && window.__nativeCallback(\(Self.jsonString(requestId)), \(json));")
    }

    private func replyError(_ requestId: String?, _ message: String) {
        guard let requestId else { return }
        let payload = "{\"error\":\(Self.jsonString(message))}"
        sendJavaScript("window.__nativeCallback && window.__nativeCallback(\(Self.jsonString(requestId)), \(payload));")
    }

    private func sendEvent(name: String, json: String) {
        sendJavaScript("window.__nativeEvent && window.__nativeEvent(\(Self.jsonString(name)), \(json));")
    }

    private func sendJavaScript(_ javascript: String) {
        guard let webView else { return }
        DispatchQueue.main.async {
            webView.evaluateJavaScript(javascript, completionHandler: nil)
        }
    }

    private static func jsonString(_ value: String) -> String {
        let data = try! JSONSerialization.data(withJSONObject: [value])
        let array = String(data: data, encoding: .utf8)!
        return String(array.dropFirst().dropLast())
    }
}

final class WeakScriptMessageHandler: NSObject, WKScriptMessageHandler {
    weak var bridge: WebViewBridge?

    init(bridge: WebViewBridge) {
        self.bridge = bridge
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        if let body = message.body as? [String: Any] {
            bridge?.handleMessage(body)
        }
    }
}

struct WebViewRepresentable: NSViewRepresentable {
    let bridge: WebViewBridge

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.preferences.setValue(true, forKey: "developerExtrasEnabled")
        let bridgeScript = """
        window.__nativeCallback = function(id, payload) {
            window.dispatchEvent(new CustomEvent('dotwallpaper-response', { detail: { id: id, payload: payload } }));
        };
        window.__nativeEvent = function(name, payload) {
            window.dispatchEvent(new CustomEvent('dotwallpaper-event', { detail: { name: name, payload: payload } }));
        };
        window.DotWallpaperNative = {
            invoke: function(action, params) {
                window.webkit.messageHandlers.nativeBridge.postMessage(
                    Object.assign({action: action}, params || {})
                );
            }
        };
        """
        config.userContentController.addUserScript(
            WKUserScript(source: bridgeScript, injectionTime: .atDocumentStart, forMainFrameOnly: true)
        )

        let webView = WKWebView(frame: .zero, configuration: config)
        webView.navigationDelegate = context.coordinator
        bridge.install(on: webView)

        if let distURL = Bundle.main.url(forResource: "index", withExtension: "html", subdirectory: "dist") {
            // The frontend is bundled under Contents/Resources/dist, while the native
            // core returns absolute paths for the user's selected media library. The
            // app intentionally runs unsandboxed, so the file URL read scope must cover
            // both locations; using ~/ here makes the bundled UI load as a blank page
            // when the app is installed outside the user's home directory.
            let fileReadScope = URL(fileURLWithPath: "/", isDirectory: true)
            NSLog("[WallpaperEngine] loading frontend: %@ (read scope: %@)", distURL.path, fileReadScope.path)
            webView.loadFileURL(distURL, allowingReadAccessTo: fileReadScope)
        } else {
            NSLog("[WallpaperEngine] frontend resource missing: dist/index.html")
        }
        return webView
    }

    func updateNSView(_ nsView: WKWebView, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            NSLog("[WallpaperEngine] WebView loaded: %@", webView.url?.absoluteString ?? "unknown")
        }

        func webView(
            _ webView: WKWebView,
            didFail navigation: WKNavigation!,
            withError error: Error
        ) {
            NSLog("[WallpaperEngine] WebView load failed: %@", error.localizedDescription)
        }

        func webView(
            _ webView: WKWebView,
            didFailProvisionalNavigation navigation: WKNavigation!,
            withError error: Error
        ) {
            NSLog("[WallpaperEngine] WebView provisional load failed: %@", error.localizedDescription)
        }
    }
}
