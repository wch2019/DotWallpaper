<script setup lang="ts">
// MediaGrid - 左栏：媒体筛选/搜索 + 图片/视频缩略图网格
// 加载、空目录、读取失败三种状态分开呈现；大目录分批渲染避免一次性挂载上千节点。
import { computed, onUnmounted, ref, watch } from "vue";
import { localSrc, useApp } from "../composables/useApp";
import type { MediaItem, MediaKind } from "../types";
import SvgIcon from "./SvgIcon.vue";

const app = useApp();

type Filter = "all" | MediaKind;
type Sort = "name" | "name-desc" | "newest" | "oldest";
const PAGE = 60;

const SORTS: { v: Sort; t: string }[] = [
  { v: "name", t: "名称 A→Z" },
  { v: "name-desc", t: "名称 Z→A" },
  { v: "newest", t: "最近修改" },
  { v: "oldest", t: "最早修改" },
];

const query = ref("");
const filter = ref<Filter>("all");
const sort = ref<Sort>("name");
const shown = ref(PAGE);

const all = computed<MediaItem[]>(() => app.media.value);
const loading = computed(() => app.loadingMedia.value);
const error = computed(() => app.mediaError.value);

const filtered = computed<MediaItem[]>(() => {
  const q = query.value.trim().toLowerCase();
  return all.value.filter(
    (m) =>
      (filter.value === "all" || m.kind === filter.value) &&
      (!q || m.name.toLowerCase().includes(q))
  );
});

// 名称序沿用后端的默认序规则（忽略大小写，同键再比路径）
function byName(a: MediaItem, b: MediaItem) {
  return (
    a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) ||
    a.path.localeCompare(b.path)
  );
}

// mtime = 0 表示后端没读到修改时间，两种时间序里都按名称排在末尾，不冒充"最早"
const sorted = computed<MediaItem[]>(() => {
  const list = filtered.value;
  if (sort.value === "name") return [...list].sort(byName);
  if (sort.value === "name-desc") return [...list].sort(byName).reverse();
  const known = list.filter((m) => m.mtime > 0);
  const unknown = list.filter((m) => !m.mtime).sort(byName);
  known.sort(sort.value === "newest" ? (a, b) => b.mtime - a.mtime : (a, b) => a.mtime - b.mtime);
  return [...known, ...unknown];
});

// 分批渲染：DOM 里只保留前 shown 项，滚动到底再追加
const visible = computed(() => sorted.value.slice(0, shown.value));
const hasMore = computed(() => sorted.value.length > shown.value);

watch([query, filter, sort], () => {
  shown.value = PAGE;
});
watch(all, () => {
  shown.value = PAGE;
});

// 加载超过 1.2s 才提示"正在读取壁纸目录"，避免闪屏
const slow = ref(false);
let slowTimer = 0;
watch(loading, (v) => {
  window.clearTimeout(slowTimer);
  slow.value = false;
  if (v) slowTimer = window.setTimeout(() => (slow.value = true), 1200);
});
onUnmounted(() => window.clearTimeout(slowTimer));

// 已应用到某台显示器的壁纸路径集合（用于角标圆点）
const appliedPaths = computed(() => {
  const set = new Set<string>();
  for (const s of app.states.value) {
    if (s.assignment?.path) set.add(s.assignment.path);
  }
  return set;
});

// 缩略图加载失败 → 显示纯 CSS 渐变占位
const failed = ref<Set<string>>(new Set());
function onThumbError(item: MediaItem) {
  if (failed.value.has(item.path)) return;
  const next = new Set(failed.value);
  next.add(item.path);
  failed.value = next;
}

function thumbFor(item: MediaItem): string {
  return failed.value.has(item.path) ? "" : localSrc(item.thumb) || localSrc(item.path);
}

function onClick(item: MediaItem) {
  app.selectItem(item.path);
}

function onKeydown(e: KeyboardEvent, item: MediaItem) {
  if (e.key === "Enter" || e.key === " ") {
    e.preventDefault();
    app.selectItem(item.path);
  }
}

// 滚动接近底部时追加下一批
let scrollLock = false;
function onScroll(e: Event) {
  if (!hasMore.value || scrollLock) return;
  const el = e.target as HTMLElement;
  if (el.scrollTop + el.clientHeight > el.scrollHeight - 320) {
    scrollLock = true;
    shown.value += PAGE;
    window.setTimeout(() => (scrollLock = false), 60);
  }
}
</script>

<template>
  <section class="dw-library-pane flex min-h-0 min-w-0 flex-1 flex-col">
    <div class="flex h-10 shrink-0 items-center justify-between px-4">
      <div class="flex items-baseline gap-2">
        <span class="text-[12px] font-semibold tracking-tight text-tx">媒体库</span>
        <span class="dw-eyebrow">WALLPAPERS</span>
      </div>
      <div v-if="all.length" class="flex min-w-0 items-center gap-2">
        <span class="text-[11px] text-faint">
          {{ filter === "all" && !query ? all.length : `${filtered.length} / ${all.length}` }} 项
        </span>
        <select
          v-model="sort"
          class="mac-select shrink-0 text-[11px]"
          aria-label="媒体库排序方式"
          :title="`排序：${SORTS.find((s) => s.v === sort)?.t}`"
        >
          <option v-for="opt in SORTS" :key="opt.v" :value="opt.v">{{ opt.t }}</option>
        </select>
      </div>
    </div>

    <!-- 读取失败 -->
    <div
      v-if="error"
      class="flex min-h-0 flex-1 flex-col items-center justify-center gap-2.5 px-6 pb-8 text-center"
    >
      <div class="flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-danger/50">
        <SvgIcon name="close" :size="26" class="text-danger" />
      </div>
      <div>
        <p class="text-[13px] font-medium text-tx">读取壁纸目录失败</p>
        <p class="mt-1 max-w-[320px] break-all text-[11.5px] leading-relaxed text-faint">
          {{ error }}
        </p>
      </div>
      <div class="flex items-center gap-2">
        <button class="mac-btn mac-btn-primary" @click="app.loadMedia()">
          <SvgIcon name="refresh" :size="12" />
          重试
        </button>
        <button class="mac-btn" @click="app.pickFolder()">
          <SvgIcon name="folder" :size="12" />
          选择其他文件夹
        </button>
      </div>
    </div>

    <!-- 首次加载：骨架屏 -->
    <div
      v-else-if="loading && !all.length"
      class="min-h-0 flex-1 overflow-y-auto px-3.5 pb-3.5"
    >
      <p v-if="slow" class="py-2 text-[11.5px] text-faint">正在读取壁纸目录…</p>
      <ul class="grid grid-cols-[repeat(auto-fill,minmax(132px,1fr))] gap-2.5">
        <li v-for="i in 12" :key="i" class="overflow-hidden rounded-[10px] border border-line bg-panel">
          <div class="aspect-[16/10] w-full animate-pulse bg-panel-2"></div>
          <div class="px-2 py-2">
            <div class="h-2 w-2/3 animate-pulse rounded-full bg-panel-2"></div>
          </div>
        </li>
      </ul>
    </div>

    <!-- 空目录引导 -->
    <div
      v-else-if="!all.length"
      class="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 px-6 pb-8 text-center"
    >
      <div class="media-fallback flex h-20 w-20 items-center justify-center rounded-2xl border border-dashed border-line-2">
        <SvgIcon name="image" :size="30" class="text-faint" />
      </div>
      <div>
        <p class="text-[13px] font-medium text-tx">媒体库还是空的</p>
        <p class="mt-1 text-[11.5px] leading-relaxed text-faint">
          选择一个包含图片 / 视频的文件夹，或直接把文件拖入窗口导入
        </p>
        <p class="mt-0.5 text-[11px] text-faint">当前目录：{{ app.libraryDir.value || "未选择" }}</p>
      </div>
      <div class="flex items-center gap-2">
        <button class="mac-btn mac-btn-primary" @click="app.pickFolder()">
          <SvgIcon name="folder" :size="12" />
          选择文件夹
        </button>
        <span class="text-[11px] text-faint">或拖入图片 / 视频</span>
      </div>
    </div>

    <!-- 有内容：筛选栏 + 网格 -->
    <template v-else>
      <div class="flex shrink-0 items-center gap-2 px-4 pb-2.5">
        <div class="relative flex min-w-0 flex-1 items-center">
          <SvgIcon name="search" :size="12" class="pointer-events-none absolute left-2 text-faint" />
          <input
            v-model="query"
            type="search"
            placeholder="按文件名搜索"
            class="mac-input w-full pl-7"
            aria-label="搜索媒体文件名"
          />
        </div>
        <div class="mac-segmented shrink-0" role="radiogroup" aria-label="媒体类型筛选">
          <button
            v-for="opt in [
              { v: 'all', t: '全部' },
              { v: 'image', t: '图片' },
              { v: 'video', t: '视频' },
            ]"
            :key="opt.v"
            class="mac-segment cursor-pointer !px-2 text-[11px]"
            :class="filter === opt.v ? 'mac-segment-active' : ''"
            role="radio"
            :aria-checked="filter === opt.v"
            @click="filter = opt.v as Filter"
          >
            {{ opt.t }}
          </button>
        </div>
      </div>

      <div v-if="loading" class="px-3.5 pb-1 text-[11px] text-faint">正在刷新媒体库…</div>

      <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4" @scroll.passive="onScroll">
        <ul class="grid grid-cols-[repeat(auto-fill,minmax(142px,1fr))] gap-3">
          <li
            v-for="item in visible"
            :key="item.path"
            class="dw-media-tile group relative cursor-pointer overflow-hidden rounded-[12px] border bg-panel transition-all duration-150"
            :class="
              item.path === app.selectedPath.value
                ? 'dw-media-tile-selected'
                : 'border-line'
            "
            tabindex="0"
            role="button"
            :aria-label="item.name"
            :aria-selected="item.path === app.selectedPath.value"
            @click="onClick(item)"
            @keydown="onKeydown($event, item)"
          >
            <div class="media-fallback relative aspect-[16/10] w-full overflow-hidden">
              <img
                v-if="thumbFor(item)"
                :src="thumbFor(item)"
                :alt="item.name"
                class="h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.03]"
                loading="lazy"
                decoding="async"
                draggable="false"
                @error="onThumbError(item)"
              />
              <!-- 视频角标 -->
              <span
                v-if="item.kind === 'video'"
                class="absolute bottom-1.5 right-1.5 flex h-[18px] w-[18px] items-center justify-center rounded-full bg-black/60 text-white backdrop-blur-sm"
                title="视频"
              >
                <SvgIcon name="play" :size="9" fill />
              </span>
              <!-- 已在桌面上显示 -->
              <span
                v-if="appliedPaths.has(item.path)"
                class="absolute left-1.5 top-1.5 h-2 w-2 rounded-full bg-ok shadow-[0_0_0_2px_rgba(0,0,0,0.35)]"
                title="已作为壁纸"
              ></span>
              <span
                v-if="item.path === app.selectedPath.value"
                class="dw-media-tile-check"
                aria-label="已选择"
              >
                <SvgIcon name="check" :size="11" />
              </span>
              <span v-if="item.path === app.selectedPath.value" class="dw-media-tile-wash" aria-hidden="true"></span>
            </div>
            <div class="flex min-w-0 items-center gap-1 px-2 py-1.5">
              <SvgIcon
                :name="item.kind === 'video' ? 'film' : 'image'"
                :size="11"
                class="shrink-0 text-faint"
              />
              <span
                class="min-w-0 truncate text-[11px]"
                :class="item.path === app.selectedPath.value ? 'text-tx' : 'text-dim'"
                :title="item.name"
              >
                {{ item.name }}
              </span>
            </div>
          </li>
        </ul>

        <!-- 筛选无结果 -->
        <div
          v-if="!filtered.length"
          class="flex flex-col items-center justify-center gap-2 py-10 text-center"
        >
          <p class="text-[12px] text-dim">没有匹配的图片 / 视频</p>
          <button
            class="mac-btn"
            @click="
              query = '';
              filter = 'all';
            "
          >
            清除筛选
          </button>
        </div>

        <p v-else-if="hasMore" class="py-3 text-center text-[11px] text-faint">
          继续滚动加载剩余 {{ filtered.length - shown }} 项
        </p>
      </div>
    </template>
  </section>
</template>
