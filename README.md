# DotWallpaper

原生 macOS 桌面壁纸应用：管理本地静态图片与视频动态壁纸，为多显示器独立设置壁纸，并通过菜单栏控制播放。应用宿主使用 SwiftUI / AppKit / WKWebView，Rust static library 负责媒体、壁纸播放和系统集成，Vue 3 + TypeScript 提供管理界面。当前构建面向 Apple Silicon，最低 macOS 26.0。

## 功能概览

### 壁纸与播放

- 静态图片：JPG、JPEG、PNG、BMP、WebP、HEIC；支持填充或适应显示模式，设置后从系统读回验证。
- 视频动态壁纸：MP4、MOV；通过 `AVQueuePlayer` + `AVPlayerLooper` 循环播放，可设为静音。视频首帧就绪后才替换当前壁纸；准备失败或超时会保留原壁纸。
- 每台显示器独立应用、暂停、恢复或停止壁纸。停止视频后桌面保留首帧静态海报。
- 显示器热插拔和休眠唤醒后尝试恢复播放；支持稳定显示器 ID，并提示无序列号屏幕使用临时 ID。
- 菜单栏提供管理窗口、全部暂停、全部恢复、全部停止和退出操作；关闭管理窗口不会退出应用或停止播放。
- 切换壁纸采用事务式处理；快速连续切换由 generation 令牌防止旧请求覆盖新选择。播放状态通过原生 bridge 实时同步至界面。

### 媒体库与设置

- 首次启动引导用户选择媒体库目录，也可稍后设置；通过 macOS 原生目录选择器取得所选目录的访问权限。
- 递归扫描壁纸目录，支持拖放或原生文件选择器导入；导入文件复制到媒体库，重名时自动编号。
- 支持媒体搜索、类型筛选、排序、缩略图缓存及库内文件删除；被壁纸分配或当前显示使用的文件会受到删除保护。
- 设置面板可更换媒体库目录、配置默认图片显示方式和视频静音偏好，并说明设置文件与缓存位置。
- 应用仅访问用户选择的媒体目录，不要求屏幕录制或辅助功能权限。

### 尚未支持

GIF 动态壁纸、动态 HEIC、网络壁纸源、开机自启动、定时轮换、播放列表、按 Space 独立配置、锁屏壁纸接管、Intel/Universal 构建和应用内自动更新均未实现。静态图片叠加水滴、雪花、樱花等动态效果也尚未实现。镜像显示目前只展示系统报告的镜像状态，不提供镜像屏去重或独立控制保证。

## 技术栈

| 层级 | 技术 |
| --- | --- |
| macOS 宿主 | SwiftUI、AppKit、WKWebView（Xcode 27） |
| 系统与壁纸核心 | Rust static library、objc2、AppKit、AVFoundation、CoreGraphics、ImageIO |
| 管理界面 | Vue 3、TypeScript、Tailwind CSS v4 |
| 构建目标 | Apple Silicon arm64，最低 macOS 26.0 |

## 仓库结构

```text
scripts/                  macOS 原生构建与发布脚本
ui/                       Vue 管理界面及 WKWebView bridge
xcode/                    SwiftUI/AppKit/WKWebView Xcode 工程
rust-core/                Rust 壁纸与媒体核心（static library）
  dotwallpaper.h           Swift 调用的 C ABI
  src/                     FFI、播放引擎、媒体、设置与测试
.github/workflows/         原生 macOS 发布工作流
```

## 开发与验证

环境：macOS 26+、Xcode 27+、Node.js/npm、Rust stable。准备 Rust 交叉编译目标：

```bash
rustup target add aarch64-apple-darwin
npm install --prefix ui
```

启动前端开发服务器（仅用于 UI 开发）：

```bash
npm run dev
```

构建并运行原生应用时，WKWebView 使用打包后的前端资源；单独构建的 `.app` 会自动进行本机 ad-hoc 签名并执行完整性校验：

```bash
npm run build:mac:app
open "$(pwd)/build/xcode-derived/Build/Products/Release/WallpaperEngine.app"
```

运行离线检查：

```bash
npm run build --prefix ui
cargo test --manifest-path rust-core/Cargo.toml
cargo clippy --manifest-path rust-core/Cargo.toml --all-targets -- -D warnings
```

## 构建与发布

构建 `.app` 并生成 DMG：

```bash
npm run release:mac
```

发布脚本依次构建原生应用、对最终主程序执行 `strip -x`、签名与验证，再通过 macOS `diskutil image create from` 生成压缩 DMG。输出路径：

```text
build/xcode-derived/Build/Products/Release/WallpaperEngine.app
build/WallpaperEngine_<版本>_arm64.dmg
```

默认使用 ad-hoc 签名，仅适合本机或内部验证；这不等于可安全对外分发的 Developer ID 签名和公证。正式分发需在钥匙串中配置 Developer ID，并保存公证凭据：

```bash
export DW_SIGN_IDENTITY="Developer ID Application: 证书名称 (TEAMID)"
export DW_NOTARY_PROFILE="notarytool 凭据名称"
npm run release:mac
```

提供签名身份但不提供 `DW_NOTARY_PROFILE` 时，脚本会提示产物尚未公证。身份无效时会失败退出，不会静默退回 ad-hoc 签名。

## 当前验证边界

构建成功与单元测试通过不能替代真实 GUI 和壁纸播放验收。多显示器热插拔、休眠唤醒、长时间播放、首次启动引导和目录授权、菜单栏交互、Mission Control 表现，以及 Developer ID + 公证的正向发布链路，仍需在目标机器和对应签名环境中验证。

## 启动故障与排查（2026-09-24）

本机已修复管理窗口空白、后台线程调用 AppKit 导致的崩溃，并将媒体扫描移出 WebKit 主线程。请从仓库根目录执行上面的完整路径 `open` 命令；**不要打开旧的 `build/WallpaperEngine.app`**（旧生成物已移到 `build/legacy-artifacts/`，不参与构建）。首次打开时，引导应先出现；请用系统目录选择器重新选择媒体库，让 macOS 授予当前原生应用该目录的访问权限。若目录读取超过 15 秒，会显示错误而不是永久转圈，可重新选目录或稍后重试。

历史故障记录：首次引导及管理窗口曾有崩溃与空白问题；当时的 DMG 不是后来修复的版本。最新验证见下方。默认 ad-hoc 签名 DMG 仅供本机/内部验证。

## 2026-09-24 视频与设置修复复核

- 视频循环器使用模板 `AVPlayerItem` 复制播放项；就绪检测现在检查 `AVQueuePlayer.currentItem`、队列播放器和循环器状态，并将各自的失败原因显示出来。以前检查模板项会把可播放视频误报为「播放器准备超时」。对本机 `2k_pro_60059.mp4` 的只读 AVFoundation 探针确认：实际播放项、播放器和循环器均 Ready，而模板项仍为 Unknown。探针**没有更改桌面壁纸**；应用内视频播放和循环仍需按 `docs/p0-verification-checklist.md` 实机验收。
- 首次引导与设置的样式现已进入 WKWebView 发布 CSS；弹层限制窗口高度，内容可滚动。媒体目录的更改/重新选择集中在设置中，顶栏目录名会打开设置。
- 如果刚改完代码却仍看到旧行为：先退出正在运行的旧进程，再执行 `npm run build:mac:app`，从 `build/xcode-derived/Build/Products/Release/WallpaperEngine.app` 打开。不要从旧 DMG 启动。**构建 .app 不会更新已有 DMG**；分发测试需重新运行 `npm run release:mac`。
- 若媒体库提示「读取壁纸目录超时」，在设置中重新选择原文件夹。本机 2026-09-24 曾采样到工作线程阻塞在系统 `opendir`；通过原生选择器重新选择**同一目录**后，媒体库实际加载出 90 项。视频预览可解码和媒体库扫描成功仍不能替代桌面播放验收。

## 2026-09-24 无法打开的修复与本机复测

- 此前 `npm run build:mac:app` 设置 `CODE_SIGNING_ALLOWED=NO`，产出的 `.app` 虽能在当前机器上通过 `open` 启动，但 `codesign --verify --deep --strict` 报资源封装无效。现在构建脚本在生成 bundle 后进行 ad-hoc 签名和严格验证；`npm run release:mac` 仍会对最终 strip 后的 bundle 重新签名。
- 旧 DMG 早于视频准备修复；本轮重新运行 `npm run release:mac`，生成 `build/WallpaperEngine_0.1.2_arm64.dmg`。`hdiutil verify`、挂载后 `.app` 的严格签名校验、从挂载 DMG 打开管理窗口和读取原媒体库 90 项均通过。早上的四份旧 `.ips` 对应此前 main-thread 崩溃，复测没有新增报告。
- 备份并恢复了测试期间被临时修改的首次引导设置；当前 `onboardingCompleted=true`，媒体库目录和三台显示器分配与原备份语义一致。
- 若仍无法打开，请确认打开的是上述新产物，退出旧进程后再试。ad-hoc 签名**不等于** Developer ID + 公证：本机 Gatekeeper 目前关闭，因此此测试不代表在默认安全策略的其他 Mac 上能直接双击打开；对外分发仍需 Developer ID 签名与公证。
- 上轮界面操作曾对 `2k_pro_60059.mp4` 验证「播放中 → 暂停 → 恢复播放中」，随后恢复原静态图片；这是状态/控制验证，**没有**目视确认桌面视频画面和无缝循环。首次引导在约 900×512 窗口完整可见，800×480 的最小窗口尚未实测。
