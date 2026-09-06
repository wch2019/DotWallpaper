<script setup lang="ts">
// DropZone - 全局文件拖放接收组件
// 基于 Tauri 原生 drag-drop 事件（dragDropEnabled 需为默认 true），
// 不依赖 WebView 的 DOM DnD，Windows 下从资源管理器拖入稳定可靠。
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FolderDown } from "lucide-vue-next";

const emit = defineEmits<{
  (e: "drop-files", paths: string[]): void;
}>();

const isDragging = ref(false);

let unlisten: (() => void) | null = null;
const handler = (e: Event) => {
  if ((e as KeyboardEvent).key === "Escape") {
    isDragging.value = false;
  }
};

// 快捷键：Ctrl + I → 拖放遮罩
const handlerKey = (e: KeyboardEvent) => {
  if (e.ctrlKey && e.key.toLowerCase() === "i") {
    e.preventDefault();
    isDragging.value = true;
  }
};

onMounted(async () => {
  try {
    unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      if (payload.type === "enter" || payload.type === "over") {
        isDragging.value = true;
      } else if (payload.type === "leave") {
        isDragging.value = false;
      } else if (payload.type === "drop") {
        isDragging.value = false;
        const paths = payload.paths || [];
        if (paths.length) {
          emit("drop-files", paths);
        }
      }
    });
  } catch (err) {
    console.error("onDragDropEvent 注册失败", err);
  }
  window.addEventListener("keydown", handlerKey);
  window.addEventListener("keydown", handler);
});

onUnmounted(() => {
  unlisten?.();
  window.removeEventListener("keydown", handlerKey);
  window.removeEventListener("keydown", handler);
});
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="isDragging"
        class="drag-overlay pointer-events-none fixed inset-0 z-[80] flex items-center justify-center bg-black/35 backdrop-blur-[3px]"
      >
        <div
          class="drag-overlay-inner flex items-center gap-2.5 rounded-2xl border border-accent/40 bg-accent-soft px-6 py-4 text-[14px] font-medium text-accent shadow-[0_16px_44px_rgba(0,0,0,0.42)]"
        >
          <FolderDown :size="20" :stroke-width="2" />
          松开鼠标，保存到壁纸目录
          <span class="ml-2 inline-flex items-center gap-1 rounded-md bg-black/15 px-2 py-0.5 text-[11px] text-accent/70">
            Ctrl + I
          </span>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.15s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
