<script setup lang="ts">
// TitleBar - 自定义标题栏：可拖拽移动窗口 + 最小化/最大化/关闭
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Images, Minus, Square, X, RefreshCw } from "lucide-vue-next";
import SettingsCenter from "./SettingsCenter/SettingsCenter.vue";
import { useUpdaterStore } from "@/stores/updater";
import dotCode from "@/assets/dotCode.png";
import { ref } from "vue";

const appWindow = getCurrentWindow();
const updaterStore = useUpdaterStore();

async function toggleMinimize() {
  const minimized = await appWindow.isMinimized();
  if (minimized) await appWindow.unminimize();
  else await appWindow.minimize();
}

async function toggleMaximize() {
  await appWindow.toggleMaximize();
}

async function closeWindow() {
  await appWindow.close();
}

// 双击标题栏（非窗口控制区）→ 最小化/还原
function onTitlebarDblclick(e: MouseEvent) {
  const t = e.target as HTMLElement;
  if (t.closest(".window-controls")) return;
  void toggleMinimize();
}
</script>

<template>
  <header
    id="titlebar"
    class="title-bar app-drag flex h-10 shrink-0 select-none items-center justify-between px-4"
    @dblclick="onTitlebarDblclick"
  >
    <div class="titlebar-left flex min-w-0 items-center gap-2">
      <span class="app-icon flex h-[18px] w-[18px] items-center justify-center rounded-[6px] bg-accent-soft text-accent">
        <Images :size="11" :stroke-width="2.4" />
          <img :src="dotCode" alt="App Icon"/>
      </span>
      <h1 class="truncate text-[13px] font-semibold tracking-wide text-tx">壁纸工具</h1>
    </div>

    <div class="titlebar-right flex items-center">
      <div class="window-controls app-no-drag flex items-center gap-1">
        <!-- 更新按钮：仅在发现新版本时显示（预留 .update-badge 样式） -->
        <button
          v-if="updaterStore.hasUpdate"
          class="update-badge"
          :title="'发现新版本 v' + updaterStore.latestVersion + '，点击查看更新'"
          @click="updaterStore.openUpdateDialog()"
        >
          <RefreshCw :size="13" :stroke-width="2" />
          <span class="dot"></span>
        </button>
        <!-- 设置中心 -->
        <SettingsCenter />
        <span class="divider mx-1 h-3.5 w-px shrink-0 bg-line"></span>
        <button
          class="ctrl-btn flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-dim transition-colors hover:bg-white/10 hover:text-tx"
          title="最小化"
          @click="toggleMinimize"
        >
          <Minus :size="13" :stroke-width="2" />
        </button>
        <button
          class="ctrl-btn flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-dim transition-colors hover:bg-white/10 hover:text-tx"
          title="最大化"
          @click="toggleMaximize"
        >
          <Square :size="11" :stroke-width="1.8" />
        </button>
        <button
          class="ctrl-btn close-btn ml-0.5 flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-dim transition-colors hover:bg-danger hover:text-white"
          title="关闭"
          @click="closeWindow"
        >
          <X :size="14" :stroke-width="2" />
        </button>
      </div>
    </div>
  </header>
</template>

<style scoped>
.update-badge {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: 6px;
  color: var(--color-accent);
  cursor: pointer;
  transition: all 0.2s;
}
.update-badge:hover {
  background: var(--color-accent-soft);
}
.update-badge .dot {
  position: absolute;
  top: 3px;
  right: 3px;
  width: 6px;
  height: 6px;
  background: var(--color-danger);
  border-radius: 50%;
  pointer-events: none;
  animation: pulse 1.5s infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
</style>
