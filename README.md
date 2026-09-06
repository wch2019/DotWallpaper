# DotWallpaper

Windows 桌面壁纸管理工具。基于 Tauri 2 + Rust + Vue 3 构建，提供本地壁纸管理、一键设壁、拖入导入与自定义目录能力，支持深色主题。

## 核心功能

- **壁纸应用**：点击壁纸即可设为桌面壁纸
- **当前壁纸**：实时读取并展示当前桌面壁纸，提供"重新设为壁纸"按钮
- **右键管理**：右键菜单支持设为桌面壁纸、永久删除文件
- **拖入导入**：将本地图片拖入应用窗口，自动复制保存到壁纸目录（支持 JPG/PNG/BMP/WebP）
- **目录切换**：自由选择壁纸存放目录，选择记忆持久化到 localStorage
- **分页加载**：壁纸列表按页加载，适配大量壁纸目录
- **深色主题**：全局深色毛玻璃设计，视觉贴合桌面环境
- **快捷键**：Ctrl+S 重新设置当前壁纸、Ctrl+R 刷新壁纸列表

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2 |
| 系统层 | Rust（Win32 API 壁纸设置、文件系统） |
| 前端框架 | Vue 3 + TypeScript |
| UI 组件 | Naive UI |
| 样式方案 | Tailwind CSS v4 |
| 图标库 | Lucide |
| 状态管理 | Pinia |

## 项目结构

```
DotWallpaper/
├── ui/                              # 前端项目
│   ├── src/
│   │   ├── App.vue                  # 根组件（布局、快捷键、拖放入口）
│   │   ├── main.ts                  # 入口（Pinia + 主题配置）
│   │   ├── styles/
│   │   │   └── main.css             # Tailwind 主题配置
│   │   ├── lib/
│   │   │   └── naive-host.ts        # Naive UI 宿主绑定
│   │   ├── stores/
│   │   │   └── wallpaper.ts         # Pinia 壁纸状态
│   │   └── components/
│   │       ├── TitleBar.vue         # 自定义标题栏
│   │       ├── Sidebar.vue          # 左侧壁纸网格
│   │       ├── CurrentPanel.vue     # 右侧当前壁纸展示
│   │       ├── ContextMenu.vue      # 右键菜单
│   │       ├── DropZone.vue         # 拖入区域
│   │       └── NaiveBridge.vue      # Naive UI 全局上下文桥接
│   ├── index.html
│   └── package.json
├── src-tauri/                       # Rust 后端
│   ├── src/
│   │   ├── main.rs                  # Tauri 入口、命令注册
│   │   └── wallpaper.rs             # 壁纸管理核心逻辑
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   └── icons/
├── README.md
└── package.json
```

## 环境要求

- **操作系统**：Windows 10 / 11
- **Node.js**：18+
- **Rust**：稳定工具链（需 MSVC 构建工具链）

## 快速开始

### 安装依赖

```bash
# 1. 前端依赖
cd ui
npm install

# 2. Rust 依赖
cd ../src-tauri
cargo fetch
```

### 开发模式

```bash
npm run tauri dev
```

### 构建发布版

```bash
$env:TAURI_SIGNING_PRIVATE_KEY="Path or content of your private key"

npm run tauri build
```


构建产物位于 `src-tauri/target/release/bundle/`。

## 核心命令

Rust 后端暴露的 Tauri 命令：

| 命令 | 描述 |
|------|------|
| `set_wallpaper` | 将本地壁纸路径设为桌面壁纸 |
| `get_current_wallpaper` | 获取当前桌面壁纸路径 |
| `list_local_wallpapers` | 列出本地壁纸文件 |
| `list_system_wallpapers` | 列出 Windows 自带系统壁纸（只读） |
| `pick_wallpaper_directory` | 弹出目录选择框 |
| `save_dropped_paths` | 保存拖入的本地图片到壁纸目录 |
| `delete_wallpaper` | 永久删除壁纸文件 |
| `get_wallpaper_style` | 获取桌面壁纸样式 |
| `get_desktop_screen` | 获取主屏幕分辨率与缩放比 |

## 使用方式

1. **查看当前壁纸**：启动后右侧展示当前桌面壁纸
2. **浏览壁纸**：左侧网格展示本地壁纸列表，滚动加载更多
3. **设置壁纸**：点击壁纸卡片，右侧预览同步更新并设为桌面
4. **拖入壁纸**：将本地图片拖入窗口，自动保存并刷新列表
5. **删除壁纸**：右键壁纸 → "删除壁纸"，确认后从磁盘永久删除（系统壁纸来源只读，不提供删除）
6. **切换目录**：点击顶部"切换目录"，自由指定壁纸存放位置

## 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Ctrl+S` | 将当前壁纸重新设为桌面壁纸 |
| `Ctrl+R` | 刷新壁纸列表 |

## 后续规划

- 多壁纸分组与分类管理
- 每日定时自动切换
- macOS 版本适配

---
