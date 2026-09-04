<script setup lang="ts">
// Sidebar - 左栏：本地壁纸网格（分页加载 / 右键菜单 / 应用当前壁纸）
import { computed, nextTick, ref, watch } from "vue";
import { NButton, NIcon, useMessage } from "naive-ui";
import { FolderOpen } from "lucide-vue-next";
import {
  baseName,
  displaySrc,
  useWallpaperStore,
  type WallpaperItem,
} from "../stores/wallpaper";

const store = useWallpaperStore();

const dirHint = computed(() => {
  if (store.source === "system") {
    return "系统壁纸：C:\\Windows\\Web\\Wallpaper（只读）";
  }
  return store.currentDir ? "壁纸目录：" + store.currentDir : "壁纸目录：默认（图片）";
});

const gridWrap = ref<HTMLElement | null>(null);

function isActive(item: WallpaperItem) {
  if (!store.currentWallpaper || !item.path) return false;
  return item.path === store.currentWallpaper.path;
}

function onItemClick(item: WallpaperItem) {
  void store.applyItem(item);
}

function onItemContext(e: MouseEvent, item: WallpaperItem) {
  e.preventDefault();
  store.openContextMenu(item, e.clientX, e.clientY);
}

// 滚动接近底部时触发加载更多
function onScroll() {
  const el = gridWrap.value;
  if (!el) return;
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 80) {
    void store.loadMore();
  }
}

// 渲染后若尚未撑满可视区且还有更多，自动补页直到出现滚动条
watch(
  () => store.gridItems.length,
  async () => {
    await nextTick();
    const el = gridWrap.value;
    if (!el) return;
    if (el.scrollHeight <= el.clientHeight && store.hasMore) {
      void store.loadMore();
    }
  }
);

const message = useMessage();

async function onPickDirectory() {
  try {
    await store.pickAndApplyDirectory();
  } catch (err) {
    message.error(String(err));
  }
}
</script>

<template>
  <aside class="sidebar flex min-w-0 flex-[1.15] flex-col">
    <!-- 顶部：源标签 + 操作 -->
    <div class="sidebar-top mb-2.5 flex items-center justify-between gap-2">
      <div class="source-tabs flex items-center gap-2">
        <button
          class="tab-btn cursor-pointer rounded-full px-3 py-1 text-[12px] font-medium transition-colors"
          :class="
            store.source === 'local'
              ? 'bg-accent-soft text-accent'
              : 'text-dim hover:text-tx'
          "
          @click="store.setSource('local')"
        >
          本地
        </button>
        <button
          class="tab-btn cursor-pointer rounded-full px-3 py-1 text-[12px] font-medium transition-colors"
          :class="
            store.source === 'system'
              ? 'bg-accent-soft text-accent'
              : 'text-dim hover:text-tx'
          "
          @click="store.setSource('system')"
        >
          系统
        </button>
        <span v-if="store.allCount > 0" class="tab-count text-[11px] text-dim">
          {{ store.gridItems.length }}/{{ store.allCount }}
        </span>
      </div>

      <!-- 选择目录仅对本地来源可用；系统壁纸为只读固定目录 -->
      <div v-if="store.source === 'local'" class="sidebar-actions shrink-0">
        <NButton size="small" secondary @click="onPickDirectory">
          <template #icon>
            <NIcon :component="FolderOpen" />
          </template>
          选择目录
        </NButton>
      </div>
    </div>

    <!-- 网格容器 -->
    <div class="grid-wrap relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-panel/55">
      <div
        ref="gridWrap"
        class="grid grid-cols-[repeat(auto-fill,minmax(148px,1fr))] flex-1 gap-2.5 overflow-y-auto p-2.5"
        @scroll="onScroll"
      >
        <template v-if="store.gridItems.length">
          <div
            v-for="item in store.gridItems"
            :key="item.key"
            class="wallpaper-item group relative flex min-h-[150px] cursor-pointer flex-col overflow-hidden rounded-[10px] border border-line bg-panel-2 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent hover:shadow-[0_8px_20px_rgba(0,0,0,0.32)]"
            :class="{
              'active !border-ok !shadow-[0_0_0_1px_var(--color-ok),0_8px_20px_rgba(0,0,0,0.32)]': isActive(item),
              'applying opacity-70': item.applying,
            }"
            @click="onItemClick(item)"
            @contextmenu="onItemContext($event, item)"
          >
            <div class="thumb-holder min-h-0 w-full flex-1 overflow-hidden bg-elev">
              <img
                class="thumb h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.05]"
                :src="displaySrc(item)"
                loading="lazy"
                :alt="item.title || baseName(item.path || '') || 'wallpaper'"
              />
            </div>
            <div class="meta flex min-w-0 items-center gap-1.5 px-2 py-1.5">
              <span class="name truncate text-[11.5px] text-dim">
                {{ item.title || baseName(item.path || "") || "壁纸" }}
              </span>
              <span v-if="isActive(item)" class="active-dot ml-auto h-1.5 w-1.5 shrink-0 rounded-full bg-ok"></span>
            </div>
            <div
              v-if="item.applying"
              class="applying-overlay absolute inset-0 z-10 flex items-center justify-center bg-black/45 text-[12px] text-white backdrop-blur-[2px]"
            >
              设置中…
            </div>
          </div>
        </template>

        <!-- 空态 -->
        <div v-else class="empty col-span-full flex flex-col items-center justify-center py-16 text-center">
          <p class="text-[13px] text-dim">暂无壁纸</p>
          <p class="empty-sub mt-1 text-[11.5px] text-faint">
            {{
              store.source === "local"
                ? "点击右上角选择目录"
                : "系统壁纸目录中暂无可用图片"
            }}
          </p>
        </div>

        <!-- 底部加载状态 -->
        <div v-if="store.loadingMore" class="load-more col-span-full py-3 text-center text-[11.5px] text-faint">
          加载中…
        </div>
        <div
          v-else-if="store.gridItems.length && !store.hasMore"
          class="load-more dim col-span-full py-3 text-center text-[11px] text-faint"
        >
          已加载全部 {{ store.allCount }} 张
        </div>
      </div>
    </div>

    <!-- 底部：目录提示 -->
    <div class="sidebar-footer mt-2.5 min-w-0">
      <span class="dir-hint block truncate text-[11px] text-faint" :title="dirHint">
        {{ dirHint }}
      </span>
    </div>
  </aside>
</template>
