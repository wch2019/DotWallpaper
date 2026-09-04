<script setup lang="ts">
import { onMounted } from "vue";
import {
  NConfigProvider,
  NDialogProvider,
  NMessageProvider,
  darkTheme,
  type GlobalThemeOverrides,
} from "naive-ui";
import { getCurrentWindow } from "@tauri-apps/api/window";
import TitleBar from "./components/TitleBar.vue";
import Sidebar from "./components/Sidebar.vue";
import CurrentPanel from "./components/CurrentPanel.vue";
import ContextMenu from "./components/ContextMenu.vue";
import DropZone from "./components/DropZone.vue";
import NaiveBridge from "./components/NaiveBridge.vue";
import { useWallpaperStore } from "./stores/wallpaper";

const store = useWallpaperStore();
const appWindow = getCurrentWindow();

// Naive UI 主题令牌：与 main.css 设计令牌对齐（冰蓝主色、圆角）
const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: "#7fa8ff",
    primaryColorHover: "#97b8ff",
    primaryColorPressed: "#5f8df0",
    primaryColorSuppl: "#7fa8ff",
    borderRadius: "8px",
    fontFamily:
      '"Segoe UI Variable Text", "Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", system-ui, sans-serif',
  },
  Dialog: {
    borderRadius: "12px",
  },
  Message: {
    borderRadius: "10px",
  },
};

// 双击主区域空白（非按钮/卡片/当前壁纸区域）→ 最小化/还原
function onMainAreaDblclick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  if (
    t.closest(".n-button") ||
    t.closest(".wallpaper-item") ||
    t.closest(".current-box")
  ) {
    return;
  }
  toggleMinimize();
}

async function toggleMinimize() {
  const minimized = await appWindow.isMinimized();
  if (minimized) await appWindow.unminimize();
  else await appWindow.minimize();
}

// Tauri 原生拖放事件回调：收到的本地文件路径列表
async function onDropFiles(paths: string[]) {
  await store.saveDroppedPaths(paths);
}

function onKeydown(e: KeyboardEvent) {
  // Ctrl+S = 设置为当前壁纸
  if (e.ctrlKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    void store.setCurrentAsDesktop();
  }
  // Ctrl+R = 重新加载壁纸列表
  if (e.ctrlKey && e.key.toLowerCase() === "r") {
    e.preventDefault();
    void store.loadWallpapers();
  }
}

// 点击页面任意非菜单区域关闭右键菜单
function onGlobalMouseDown(e: MouseEvent) {
  const t = e.target as HTMLElement;
  if (!t.closest(".context-menu")) store.closeContextMenu();
}

onMounted(() => {
  // 统一在父组件初始化：先恢复目录记忆，再加载数据
  store.restoreDir();
  void store.loadCurrentWallpaper();
  void store.loadWallpapers();
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("mousedown", onGlobalMouseDown);
  window.addEventListener("blur", store.closeContextMenu);
});
</script>

<template>
  <n-config-provider :theme="darkTheme" :theme-overrides="themeOverrides">
    <n-message-provider placement="top">
      <n-dialog-provider>
        <NaiveBridge>
          <div
            class="app-shell relative flex h-screen w-screen flex-col overflow-hidden rounded-[14px] border border-line-2 bg-[rgba(13,18,28,0.92)] shadow-[0_18px_48px_rgba(0,0,0,0.42)]"
          >
            <TitleBar />

            <!-- 主区域：左右分栏 -->
            <div class="main-area flex min-h-0 flex-1 gap-3.5 p-3.5" @dblclick="onMainAreaDblclick">
              <!-- 左栏：壁纸网格 -->
              <Sidebar />

              <!-- 分割线 -->
              <div class="divider w-px shrink-0 bg-line"></div>

              <!-- 右栏：当前壁纸 -->
              <CurrentPanel />
            </div>

            <!-- 全局右键菜单 -->
            <ContextMenu />

            <!-- 全局文件拖放接收 -->
            <DropZone @drop-files="onDropFiles" />
          </div>
        </NaiveBridge>
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
