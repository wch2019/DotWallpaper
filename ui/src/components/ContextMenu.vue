<script setup lang="ts">
// ContextMenu - 全局右键菜单（壁纸项）
import { NIcon } from "naive-ui";
import { CheckCircle2, Download, FolderSearch, MonitorDown, Star, Trash2, X } from "lucide-vue-next";
import { computed } from "vue";
import { useWallpaperStore } from "../stores/wallpaper";

const store = useWallpaperStore();

// 必应在线壁纸且尚未下载时，才提供"只下载、不改桌面"这一项
// （已下载的再点只是白跑一趟；要操作就去「本地」选项卡）
const canDownloadBing = computed(
  () => store.ctxItem?.kind === "bing" && !store.isBingDownloaded(store.ctxItem)
);

function onAction(action: string) {
  store.handleContextAction(action, store.ctxItem);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="menu-pop">
      <div
        v-if="store.ctxVisible"
        class="context-menu app-no-drag fixed z-[90] w-[196px] overflow-hidden rounded-xl border border-line-2 bg-[rgba(24,31,45,0.96)] p-1 shadow-[0_14px_36px_rgba(0,0,0,0.45)] backdrop-blur-xl"
        :style="{ left: store.ctxX + 'px', top: store.ctxY + 'px' }"
        @mousedown.stop
      >
        <div class="ctx-head flex items-center gap-1.5 px-2.5 pb-1.5 pt-1.5">
          <NIcon class="text-accent" :component="CheckCircle2" :size="13" />
          <span class="text-[10.5px] font-medium tracking-wide text-faint">壁纸操作</span>
        </div>

        <button
          class="cm-item flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-tx transition-colors hover:bg-white/10"
          @click="onAction('set-wallpaper')"
        >
          <NIcon :component="MonitorDown" :size="14" class="text-dim" />
          设为桌面壁纸
        </button>

        <!-- 必应在线壁纸：只下载到本地壁纸目录，不改桌面。
             放在"设为桌面壁纸"正下方 —— 这两个是该来源下的两条并列路径。 -->
        <button
          v-if="canDownloadBing"
          class="cm-item flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-tx transition-colors hover:bg-white/10"
          @click="onAction('download-bing')"
        >
          <NIcon :component="Download" :size="14" class="text-dim" />
          下载到本地
        </button>

        <!-- 收藏 / 取消收藏：书签标记，不删文件（本地/系统壁纸通用）；必应在线壁纸为远程 URL，收藏夹书签体系不适用，不显示该入口 -->
        <button
          v-if="store.ctxItem?.kind !== 'bing'"
          class="cm-item flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-tx transition-colors hover:bg-white/10"
          @click="onAction('toggle-favorite')"
        >
          <NIcon
            :component="Star"
                :fill="store.isFavorite(store.ctxItem?.path) ? 'currentColor' : 'none'"
            :size="14"
            :class="store.isFavorite(store.ctxItem?.path) ? 'text-amber-300' : 'text-dim'"
          />
          {{ store.isFavorite(store.ctxItem?.path) ? "取消收藏" : "收藏" }}
        </button>

        <!-- 跳转到当前文件目录：本地/系统壁纸通用 -->
        <button
          class="cm-item flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-tx transition-colors hover:bg-white/10"
          @click="onAction('reveal-folder')"
        >
          <NIcon :component="FolderSearch" :size="14" class="text-dim" />
          跳转到当前文件目录
        </button>

        <!-- 系统壁纸只读：禁止出现删除入口（ctxReadOnly） -->
        <template v-if="store.ctxItem?.kind !== 'current' && !store.ctxReadOnly">
          <div class="cm-sep my-1 h-px bg-line"></div>
          <button
            class="cm-item cm-danger flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] transition-colors hover:bg-red-500/10"
            @click="onAction('delete-wallpaper')"
          >
            <NIcon :component="Trash2" :size="14" />
            删除壁纸
          </button>
        </template>

        <div class="cm-sep my-1 h-px bg-line"></div>

        <button
          class="cm-item flex w-full cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-[12px] text-dim transition-colors hover:bg-white/10 hover:text-tx"
          @click="store.closeContextMenu"
        >
          <NIcon :component="X" :size="14" />
          取消
        </button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.menu-pop-enter-active,
.menu-pop-leave-active {
  transition:
    opacity 0.12s ease,
    transform 0.12s ease;
  transform-origin: top left;
}
.menu-pop-enter-from,
.menu-pop-leave-to {
  opacity: 0;
  transform: scale(0.96);
}

.cm-danger,
.cm-danger .n-icon {
  color: #f87171;
}
</style>
