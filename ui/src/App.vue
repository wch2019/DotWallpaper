<script setup lang="ts">
// App - macOS 紧凑三段式布局：顶部工具栏 / 左侧媒体网格 / 右侧预览与控制
import { onMounted, onUnmounted } from "vue";
import { useApp } from "./composables/useApp";
import TopBar from "./components/TopBar.vue";
import MediaGrid from "./components/MediaGrid.vue";
import PreviewPanel from "./components/PreviewPanel.vue";
import Toaster from "./components/Toaster.vue";
import SvgIcon from "./components/SvgIcon.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import Onboarding from "./components/Onboarding.vue";
import { ref } from "vue";

const app = useApp();
const settingsOpen = ref(false);

// ---------- 拖放导入 ----------
// 原生 Xcode/WKWebView 使用 HTML5 拖放；可取得路径时交给 Rust，
// 浏览器环境则给出明确提示。
onMounted(() => {
  void app.refreshAll();
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
});

function onHtmlDrop(e: DragEvent) {
  const files = Array.from(e.dataTransfer?.files ?? []);
  const paths = files
    .map((f) => (f as File & { path?: string }).path || f.name)
    .filter((p) => p.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(p));
  app.setDragging(false);
  if (paths.length) void app.importPaths(paths);
  else if (files.length) app.toast("浏览器环境无法取得文件路径，请在应用内拖放导入", "warning");
}

function onHtmlDragOver(e: DragEvent) {
  e.preventDefault();
  app.setDragging(true);
}

// ---------- 快捷键 ----------
function onKeydown(e: KeyboardEvent) {
  if (!e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "r") {
    e.preventDefault();
    void app.refreshAll();
  } else if (key === "s") {
    e.preventDefault();
    void app.applySelected();
  }
}
</script>

<template>
  <div
    class="dw-app-shell flex h-screen w-screen flex-col overflow-hidden bg-bg text-tx"
    @dragover="onHtmlDragOver"
    @dragleave.self="app.setDragging(false)"
    @drop.prevent="onHtmlDrop"
  >
    <TopBar @open-settings="settingsOpen = true" />

    <main class="dw-workspace flex min-h-0 flex-1">
      <MediaGrid />
      <PreviewPanel />
    </main>

    <!-- 拖放遮罩 -->
    <Teleport to="body">
      <Transition name="drag">
        <div
          v-if="app.dragging.value"
          class="pointer-events-none fixed inset-0 z-[100] flex items-center justify-center bg-black/40 backdrop-blur-[2px]"
        >
          <div class="flex items-center gap-2 rounded-2xl border border-accent/45 bg-elev px-5 py-3.5 text-[13px] font-medium text-accent shadow-[0_18px_44px_var(--color-drop)]">
            <SvgIcon name="import" :size="18" />
            松开即导入壁纸目录
          </div>
        </div>
      </Transition>
    </Teleport>

    <SettingsPanel v-if="settingsOpen" @close="settingsOpen = false" />
    <Onboarding v-if="app.snapshotReady.value && !app.onboardingCompleted.value" />
    <Toaster />
  </div>
</template>

<style scoped>
.drag-enter-active,
.drag-leave-active {
  transition: opacity 0.15s ease;
}
.drag-enter-from,
.drag-leave-to {
  opacity: 0;
}
</style>
