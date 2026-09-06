<script setup lang="ts">
// Sidebar - 左栏：本地壁纸网格（分页加载 / 右键菜单 / 应用当前壁纸）
import { computed, nextTick, onUnmounted, reactive, ref, watch } from "vue";
import defaultJpg from '../assets/images/default.jpg'
import {
  baseName,
  thumbSrc,
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

// ---- 列表缩略图懒加载（IntersectionObserver 驱动，接近视口才真正设置 src）----
// revealed 记录已放行的 item.path（响应式 Set：add 会触发模板 :src 重新求值）
const revealed = reactive(new Set<string>());
let lazyObserver: IntersectionObserver | null = null;
const lazyWatched = new Set<HTMLImageElement>(); // 已挂载观察的 img，避免重复 observe

function getLazyObserver(): IntersectionObserver {
  if (lazyObserver) return lazyObserver;
  lazyObserver = new IntersectionObserver(
    (entries) => {
      for (const en of entries) {
        if (!en.isIntersecting) continue;
        const img = en.target as HTMLImageElement;
        const path = img.dataset.path;
        if (path) revealed.add(path); // 放行真实 src，触发该卡片 <img> 解码加载
        lazyObserver!.unobserve(img);
        lazyWatched.delete(img);
      }
    },
    // rootMargin 600px：进入视口前约 600px 预加载；root 为滚动网格容器
    { root: gridWrap.value, rootMargin: "600px 0px", threshold: 0 }
  );
  return lazyObserver;
}

// 扫描滚动容器内所有"未放行且未观察"的缩略图并挂载观察（在 DOM 更新后调用）
function observePendingThumbs() {
  const wrap = gridWrap.value;
  if (!wrap) return;
  const imgs = wrap.querySelectorAll<HTMLImageElement>("img[data-path]");
  for (const img of imgs) {
    const path = img.dataset.path;
    if (!path || revealed.has(path) || lazyWatched.has(img)) continue;
    getLazyObserver().observe(img);
    lazyWatched.add(img);
  }
}

onUnmounted(() => {
  lazyObserver?.disconnect();
  lazyObserver = null;
  lazyWatched.clear();
});

// 是否桌面上真正设置的当前壁纸（绿色，已设置到桌面）
function isCurrent(item: WallpaperItem) {
  return !!item.path && !!store.currentWallpaper && item.path === store.currentWallpaper.path;
}

// 是否正在预览/选中（蓝色；当前壁纸绿框优先，两态不叠加显示）
function isSelected(item: WallpaperItem) {
  if (isCurrent(item)) return false;
  return !!item.path && !!store.previewItem && item.path === store.previewItem.path;
}

// 点击卡片：仅选中并预览，桌面不发生变化
function onItemClick(item: WallpaperItem) {
  store.selectItem(item);
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

// 渲染后若尚未撑满可视区且还有更多，自动补页直到出现滚动条；
// 同时把新追加/新渲染的缩略图交给 IO 懒加载
watch(
  () => store.gridItems.length,
  async () => {
    await nextTick();
    observePendingThumbs();
    const el = gridWrap.value;
    if (!el) return;
    if (el.scrollHeight <= el.clientHeight && store.hasMore) {
      void store.loadMore();
    }
  }
);

// 卡片图源：缩略图就绪且已放行 → 真实缩略图；否则一律 default.jpg 兜底（加载中/失败同图）
function thumbSrcFor(item: WallpaperItem): string {
  return item.thumb && revealed.has(item.path || "") ? thumbSrc(item) : defaultJpg;
}

// 加载失败统一回退默认图（default.jpg 为打包资源，可稳定加载）；
// dataset.fallback 标记防止默认图自身失败造成无限循环
const handleImageError = (event: Event) => {
  const img = event.target as HTMLImageElement;
  if (img.dataset.fallback === "1") return;
  img.dataset.fallback = "1";
  img.src = defaultJpg;
};

// 加载成功（含切回真实缩略图）后清除回退标记，允许后续失败再次回退
function onThumbLoad(e: Event) {
  const img = e.currentTarget as HTMLImageElement;
  delete img.dataset.fallback;
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
      </div>
      <span v-if="store.allCount > 0" class="tab-count text-[11px] text-dim">
          {{ store.gridItems.length }}/{{ store.allCount }}
        </span>
    </div>

    <!-- 网格容器 -->
    <div class="grid-wrap relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-panel/55">
      <!-- 首次加载 loading 层：allCount 已拉取但 gridItems 尚未渲染时展示 -->
      <div
        class="loading-overlay absolute inset-0 z-20 flex flex-col items-center justify-center rounded-xl"
        :class="store.allCount > 0 && !store.gridItems.length && !store.loadingMore ? 'opacity-100' : 'opacity-0 invisible transition-opacity duration-200'"
      >
        <div class="relative h-14 w-14">
          <div class="absolute inset-0 rounded-full border-4 border-t-accent bg-line/20 outline outline-accent/10 outline-2 outline-offset-4" style="border-radius:inherit"></div>
          <div class="absolute inset-0 rounded-full border-4 border-b-accent/70 bg-line/10 outline outline-accent/20 outline-2 outline-offset-4" style="border-radius:inherit;animation:spin 1s linear infinite"></div>
        </div>
        <span class="mt-4 font-medium text-accent/90 text-xs">正在加载…</span>
      </div>
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
              'state-current !border-ok !shadow-[0_0_0_1px_var(--color-ok),0_8px_20px_rgba(0,0,0,0.32)]': isCurrent(item),
              'state-selected !border-accent !shadow-[0_0_0_1px_var(--color-accent),0_8px_20px_rgba(0,0,0,0.32)]': isSelected(item),
              'applying opacity-70': item.applying,
            }"
            @click="onItemClick(item)"
            @contextmenu="onItemContext($event, item)"
          >
            <div class="thumb-holder min-h-0 w-full flex-1 overflow-hidden relative">
              <!-- 缩略图：thumb 就绪且进入视口后赋缩略图 src；未就绪/加载失败均回退 default.jpg -->
              <img
                class="thumb h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.05]"
                :src="thumbSrcFor(item)"
                :data-path="item.path"
                loading="lazy"
                decoding="async"
                :alt="item.title || baseName(item.path || '') || 'wallpaper'"
                @error="handleImageError"
                @load="onThumbLoad"
              />
            </div>
            <div class="meta flex min-w-0 items-center gap-1.5 px-2 py-1.5">
              <span class="name truncate text-[11.5px] text-dim">
                {{ item.title || baseName(item.path || "") || "壁纸" }}
              </span>
              <!-- 状态点：绿=当前壁纸(已设置到桌面)，蓝=正在预览(选中) -->
              <span
                v-if="isCurrent(item) || isSelected(item)"
                class="status-dot ml-auto h-1.5 w-1.5 shrink-0 rounded-full"
                :class="isCurrent(item) ? 'bg-ok' : 'bg-accent'"
                :title="isCurrent(item) ? '当前壁纸（已设置到桌面）' : '正在预览'"
              ></span>
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
                  ? "此目录下暂无可用图片，可到设置中更换壁纸目录"
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

<style scoped>
@keyframes spin {
  to { transform: rotate(360deg); }
}

.loading-overlay {
  background: linear-gradient(135deg, rgba(13,18,28,0.85), rgba(10,14,22,0.92));
  backdrop-filter: blur(2px);
}
</style>
