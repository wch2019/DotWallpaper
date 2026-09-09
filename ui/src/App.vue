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
import UpdateDialog from "./components/UpdateDialog.vue";
import { useWallpaperStore } from "./stores/wallpaper";
import { useUpdaterStore } from '@/stores/updater'

const store = useWallpaperStore();
const updaterStore = useUpdaterStore();
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
  // 下拉选择器：触发器/下拉菜单与 main.css 面板色对齐，选中态用冰蓝
  Select: {
    peers: {
      InternalSelection: {
        color: "#171e2e",
        colorActive: "#171e2e",
        border: "1px solid rgba(255, 255, 255, 0.07)",
        borderHover: "1px solid rgba(127, 168, 255, 0.6)",
        borderActive: "1px solid rgba(127, 168, 255, 0.9)",
        borderFocus: "1px solid rgba(127, 168, 255, 0.9)",
        boxShadowActive: "0 0 0 2px rgba(127, 168, 255, 0.18)",
        boxShadowFocus: "0 0 0 2px rgba(127, 168, 255, 0.18)",
        textColor: "#9aa6ba",
        placeholderColor: "#5f6c82",
        caretColor: "#7fa8ff",
        arrowColor: "#5f6c82",
        borderRadius: "8px",
        heightSmall: "28px",
      },
      InternalSelectMenu: {
        color: "#1e2739",
        borderRadius: "8px",
        optionTextColor: "#9aa6ba",
        optionTextColorActive: "#7fa8ff",
        optionTextColorPressed: "#e7ebf3",
        optionColorPending: "rgba(127, 168, 255, 0.16)",
        optionColorActive: "rgba(127, 168, 255, 0.1)",
        optionColorActivePending: "rgba(127, 168, 255, 0.2)",
        optionCheckColor: "#7fa8ff",
        groupHeaderTextColor: "#5f6c82",
      },
    },
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
  // Ctrl+S = 将正在预览的壁纸设为桌面壁纸
  if (e.ctrlKey && e.key.toLowerCase() === "s") {
    e.preventDefault();
    void store.applyPreviewAsDesktop();
  }
  // Ctrl+R = 重新加载壁纸列表
  if (e.ctrlKey && e.key.toLowerCase() === "r") {
    e.preventDefault();
    void store.loadWallpapers();
  }
  // Ctrl+F = 收藏当前预览壁纸 / 取消收藏（toggle）
  if (e.ctrlKey && e.key.toLowerCase() === "f") {
    e.preventDefault();
    const p = store.previewTarget?.path;
    if (p) {
      const fav = store.toggleFavorite(p);
      if (fav) store.message?.success("已收藏");
      else store.message?.info("已取消收藏");
    }
  }
}

// 点击页面任意非菜单区域关闭右键菜单
function onGlobalMouseDown(e: MouseEvent) {
  const t = e.target as HTMLElement;
  if (!t.closest(".context-menu")) store.closeContextMenu();
}

onMounted(() => {
  // 统一在父组件初始化：先恢复目录记忆与收藏书签，再加载数据
  store.restoreDir();
  store.loadFavorites();
  void store.loadCurrentWallpaper();
  void store.loadDesktopStyle();
  void store.loadWallpapers();
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("mousedown", onGlobalMouseDown);
  window.addEventListener("blur", store.closeContextMenu);
  setTimeout(() => {
    updaterStore.checkUpdate()
  }, 3000)
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

          <!-- 全局更新弹窗（TitleBar / 关于页共用） -->
          <UpdateDialog />
        </NaiveBridge>
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>
