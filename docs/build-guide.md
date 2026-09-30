# WallpaperEngine macOS 26+ 构建指南

当前构建链统一为 **原生 Xcode + SwiftUI/WKWebView + Rust static library**，并由 macOS 原生工具完成 `.app` / `.dmg` 构建与签名。目标平台为 **Apple Silicon arm64**，最低系统版本为
**macOS 26.0**。

## 环境要求

- macOS 26 或更高版本
- Xcode 27 或更高版本，并已选择完整 Xcode：
  `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`
- Node.js / npm
- Rust stable，以及 `aarch64-apple-darwin` target：

```bash
rustup target add aarch64-apple-darwin
```

## 安装依赖

在仓库根目录执行：

```bash
npm install --prefix ui
```

根目录的 `package.json` 只提供构建快捷命令；Xcode 构建阶段会调用
`npm --prefix ui run build` 生成 Vue 前端。

## 只构建并验证 `.app`

```bash
npm run build:mac:app
```

该命令执行：

1. `xcodebuild -project xcode/WallpaperEngine.xcodeproj -scheme WallpaperEngine`
2. 构建 Vue `ui/dist`
3. 用 Cargo 生成 `aarch64-apple-darwin` Rust static library
4. 链接 Swift/AppKit/WKWebView 应用
5. 校验产物确实是 `arm64`，且 `LSMinimumSystemVersion=26.0`

产物：

```text
build/xcode-derived/Build/Products/Release/WallpaperEngine.app
```

本地运行：

```bash
open build/xcode-derived/Build/Products/Release/WallpaperEngine.app
```

## 生成安装 DMG

```bash
npm run release:mac
# 等价命令：npm run build:mac
```

发布脚本顺序固定为：

1. Xcode Release 构建
2. 对最终主二进制执行 `strip -x`
3. ad-hoc 签名（默认，仅适合本机/内部测试）或 Developer ID 签名
4. `codesign --verify --deep --strict` 校验
5. 使用 macOS 26 标准 `diskutil image create from` 创建 UDZO DMG
6. 如提供公证配置，提交公证并 staple

产物：

```text
build/xcode-derived/Build/Products/Release/WallpaperEngine.app
build/WallpaperEngine_0.1.2_arm64.dmg
```

`diskutil image create from` 是 macOS 26 的标准镜像创建命令；脚本不再调用已弃用的 `hdiutil create`。

## Developer ID 签名与公证

默认没有 Developer ID 时，脚本使用 ad-hoc 签名，适合本机验证，不代表可对外分发。
正式发布前设置：

```bash
export DW_SIGN_IDENTITY="Developer ID Application: 你的证书名 (TEAMID)"
export DW_NOTARY_PROFILE="已通过 xcrun notarytool store-credentials 保存的 profile"
npm run release:mac
```

脚本会在找不到签名身份时直接失败，不会静默退回 ad-hoc。没有
`DW_NOTARY_PROFILE` 时可以签名，但会明确提示尚未公证。

验证签名：

```bash
codesign --verify --strict --verbose=1 \
  build/xcode-derived/Build/Products/Release/WallpaperEngine.app
hdiutil verify build/WallpaperEngine_0.1.2_arm64.dmg
```

## 开发与检查

Vue 前端开发服务器：

```bash
npm run dev
```

它只启动 Vite 开发服务器；原生 WKWebView 运行时仍应使用 Xcode 构建的 `.app`。

离线检查：

```bash
npm run build --prefix ui
cargo test --manifest-path rust-core/Cargo.toml
cargo clippy --manifest-path rust-core/Cargo.toml --all-targets -- -D warnings
```

## 目录说明

- `xcode/WallpaperEngine.xcodeproj`：主工程、SwiftUI/WKWebView 宿主
- `xcode/WallpaperEngineApp/Sources/App/ContentView.swift`：WKWebView 与原生消息桥
- `xcode/WallpaperEngineApp/Sources/Core/DotWallpaperCore.swift`：Rust FFI 调用层
- `rust-core/`：Rust 核心库（static library）
- `rust-core/dotwallpaper.h`：C ABI 头文件
- `ui/`：Vue 前端；构建后资源会内联进 `ui/dist/index.html`，适配 `file://` WKWebView
- `scripts/build_xcode_mac.sh`：仅构建 `.app`
- `scripts/release_mac.sh`：构建、签名、校验、DMG 发布

## 已知边界

- 只生成 arm64，不生成 Intel 或 Universal 包。
- ad-hoc 签名不等于 Developer ID + notarization；下载后可能被 Gatekeeper 拦截。
- 多显示器热插拔、长时间播放、休眠唤醒等仍需在目标机器上做真实运行验收，不能仅用编译成功替代。
