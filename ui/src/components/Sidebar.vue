<script setup lang="ts">
// Sidebar - 左栏：壁纸网格（分页加载 / 右键菜单 / 应用当前壁纸 / 收藏夹）
// 列表中混有静态图片与动态壁纸视频：视频卡片用 <video> 自抓首帧作为缩略图，
// 并在角标上标明视频类型与"WebView 不可播放"的告警（mkv/mov）。
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import type { Component } from "vue";
import { NIcon, NInput, NSelect } from "naive-ui";
import { AlertTriangle, Globe, Layers, Monitor, Play, Search, Star } from "lucide-vue-next";
import defaultJpg from '../assets/images/default.jpg'
import { toast } from "../lib/naive-host";
import {
  baseName,
  isWebviewPlayable,
  thumbSrc,
  useWallpaperStore,
  type WallpaperItem,
  type WallpaperSource,
  type LocalFilter,
  type LocalSort,
} from "../stores/wallpaper";

const store = useWallpaperStore();

// 各来源选项卡元信息：图标与文案（渲染顺序由 store.sourceOrder 决定）
const sourceTabMeta: Record<
  WallpaperSource,
  { label: string; icon: Component }
> = {
  local: { label: "本地", icon: Layers },
  system: { label: "系统", icon: Monitor },
  favorites: { label: "收藏", icon: Star },
  bing: { label: "必应", icon: Globe },
};

// 按用户配置顺序 + 可见性过滤后实际渲染的选项卡列表
//（local 恒可见，其余来源读取各自可见性开关）
const visibleSourceTabs = computed(() =>
  store.sourceOrder
    .filter((key) => key === "local" || store.sourceVisibility[key])
    .map((key) => ({ key, ...sourceTabMeta[key] }))
);

// 本地来源的类型筛选选项：全部 / 图片 / 视频
// 纯筛选项，不展示计数（计数交给列表底部的"已加载全部 N 项"）
const LOCAL_FILTERS = computed(() => [
  { key: "all" as const, label: "全部", title: "显示全部壁纸" },
  { key: "image" as const, label: "图片", title: "仅显示静态图片" },
  { key: "video" as const, label: "视频", title: "仅显示视频动态壁纸" },
]);

// 本地排序选项。
//
// 做成 7 个平铺选项而非"选键 + 选方向"两个控件：下拉里一眼看全，
// 且方向对每个键的默认值不同（时间/大小想"从大到小"，名称想 A→Z），
// 拆两个控件反而要先想清楚方向该配哪个。
// 名称排序是**自然序**（wallpaper2 在 wallpaper10 前），与 store 里的比较器一致。
// 用 NSelect 而非分段控件：分段控件在本项目里只用于"全部/图片/视频"三项固定短词。
//
// ⚠️ 标签**必须 ≤4 个汉字宽**：单行布局下下拉定宽 110px，而 NSelect 的
// `size="small"` 字号是 14px（不是 12px！`_styles/common` 的 fontSizeSmall），
// 5 个汉字就是 70px，加上左右内边距与箭头必然裁字。
// 所以这里刻意不写"最新修改优先"这种完整短语 —— 下拉本身已经表明了这是排序。
const LOCAL_SORT_OPTIONS: { label: string; value: LocalSort }[] = [
  { label: "默认顺序", value: "default" },
  { label: "名称 A→Z", value: "name-asc" },
  { label: "名称 Z→A", value: "name-desc" },
  { label: "最近修改", value: "mtime-desc" },
  { label: "最早修改", value: "mtime-asc" },
  { label: "文件大→小", value: "size-desc" },
  { label: "文件小→大", value: "size-asc" },
];

const dirHint = computed(() => {
  if (store.source === "system") {
    return "系统壁纸：C:\\Windows\\Web\\Wallpaper（只读）";
  }
  if (store.source === "favorites") {
    return "收藏夹：右键壁纸或点击卡片星标即可收藏/取消收藏";
  }
  if (store.source === "bing") {
    return "必应每日壁纸：在线图源，设为桌面时自动下载到本地";
  }
  return store.currentDir ? "壁纸目录：" + store.currentDir : "壁纸目录：默认（图片）";
});
const gridWrap = ref<HTMLElement | null>(null);
const filterBar = ref<HTMLElement | null>(null);
// 筛选栏实测高度（px）：供加载遮罩计算偏移，避免硬编码 top 值。
// 在筛选栏显隐/字体变化时由 ResizeObserver 同步。
const filterBarHeight = ref(0);

// ---- 分段控件滑块几何 ----
// 滑块位置/宽度必须实测（文字长短不一，写死宽度会错位）。
// segEls 按 LOCAL_FILTERS 顺序保存各分段按钮元素。
const segEls = ref<(HTMLElement | null)[]>([]);
function setSegRef(el: HTMLElement | null, i: number) {
  // 筛选栏 v-if 销毁重建时 Vue 会先以 null 回调旧元素，直接按位置覆盖即可；
  // 用普通赋值而非 push，避免重复挂载导致数组无限增长。
  segEls.value[i] = el;
}
// 当前选中分段的下标
const activeFilterIndex = computed(() =>
  Math.max(
    0,
    LOCAL_FILTERS.value.findIndex((f) => f.key === store.localFilter)
  )
);
// 滑块几何：offset 相对分段容器左边缘，width 取当前分段实测宽度
const thumbGeo = ref({ offset: 0, width: 0 });

function syncThumb() {
  const el = segEls.value[activeFilterIndex.value];
  if (!el) {
    thumbGeo.value = { offset: 0, width: 0 };
    return;
  }
  thumbGeo.value = { offset: el.offsetLeft, width: el.offsetWidth };
}

// 切换类型筛选
function onFilterChange(key: LocalFilter) {
  void store.setLocalFilter(key);
}

// 切换排序方式。
// 排序控件挂在列表**底部**页脚，改完之后列表顶部的内容会整体换掉，而控件本身
// 不在视线焦点上 —— 不滚回顶部的话，用户看到的是"列表突然变了样、不知道从哪看起"。
// 必须等 nextTick：setLocalSort → applyLocalRefine() 会清空并重建 gridItems，
// DOM 要到下一帧才更新；在更新前设 scrollTop 虽然也是 0（合法值），
// 但放到 DOM 就位之后更稳妥，也不会被内容变短后的高度回收干扰。
async function onSortChange(next: LocalSort) {
  store.setLocalSort(next);
  await nextTick();
  if (gridWrap.value) gridWrap.value.scrollTop = 0;
}

// ---- 列表缩略图懒加载（IntersectionObserver 驱动，接近视口才真正设置 src）----
// revealed 记录已放行的 item.path（响应式 Set：add 会触发模板 :src 重新求值）
const revealed = reactive(new Set<string>());
let lazyObserver: IntersectionObserver | null = null;
// 已挂载观察的缩略图/视频元素，避免重复 observe（img 与 video 共用）
const lazyWatched = new Set<Element>();

function getLazyObserver(): IntersectionObserver {
  if (lazyObserver) return lazyObserver;
  lazyObserver = new IntersectionObserver(
    (entries) => {
      for (const en of entries) {
        if (!en.isIntersecting) continue;
        const el = en.target as HTMLElement;
        // 图片用 data-path，视频缩略图（同为 <img>）用 data-video-path —— 两者都要读。
        // 只读 dataset.path 会让视频永远进不了 revealed，
        // 于是 thumbSrcFor 一直返回 default.jpg，视频卡片永远是占位图。
        const path = el.dataset.path || el.dataset.videoPath;
        if (path) revealed.add(path);
        lazyObserver!.unobserve(el);
        lazyWatched.delete(el);
      }
    },
    // rootMargin 600px：进入视口前约 600px 预加载；root 为滚动网格容器
    { root: gridWrap.value, rootMargin: "600px 0px", threshold: 0 }
  );
  return lazyObserver;
}

// 扫描滚动容器内所有"未放行且未观察"的缩略图并挂载观察（在 DOM 更新后调用）
// 图片与视频缩略图都是 <img>，只是标记属性不同（data-path / data-video-path），
// 两者都要观察 —— 漏掉任何一个都会让那类卡片永远停在占位图。
function observePendingThumbs() {
  const wrap = gridWrap.value;
  if (!wrap) return;
  const nodes = wrap.querySelectorAll<HTMLImageElement>("img[data-path], img[data-video-path]");
  for (const img of nodes) {
    const path = img.dataset.path || img.dataset.videoPath;
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
// 判断交给 store：必应来源列表项持有的是远程 URL，需经本地下载记录比对
function isCurrent(item: WallpaperItem) {
  return store.isCurrentItem(item);
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

// 双击视频卡片：直接设为动态壁纸（对标图片卡片"点击预览 → 右侧设为壁纸"，
// 视频多给一条直达路径，避免每次都要移动到右侧按钮）。
// 多显示器下"播在哪几台"由右侧显示器选择决定（空选 = 全部），此处只负责触发。
function onItemDblClick(item: WallpaperItem) {
  if (item.kind !== "video") return;
  void store.setItemAsDesktop(item);
}

// 收藏星标状态
function isFav(item: WallpaperItem) {
  return store.isFavorite(item.path);
}

// 点击星标：收藏/取消收藏（stopPropagation 防止触发卡片预览）
function onToggleFav(item: WallpaperItem) {
  const fav = store.toggleFavorite(item.path);
  if (fav) toast("已收藏", "success");
  else toast("已取消收藏", "warning");
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
let filling = false;

// 核心补页：只要内容未溢出滚动容器且 hasMore，就继续拉下一页，
// 直到出现滚动条（scrollHeight > clientHeight）或全部加载完。
// 与滚动 onScroll 互为兜底：滚动条未出现时 onScroll 永不触发，靠本函数补足。
async function fillGridUntilOverflow() {
  if (filling) return;
  const el = gridWrap.value;
  if (!el) return;
  filling = true;
  try {
    for (let i = 0; i < 12; i++) {
      observePendingThumbs();
      if (!store.hasMore) break;
      // +2 容差：避免"刚好差 1px"反复补页/抖动
      if (el.scrollHeight > el.clientHeight + 2) break;
      await store.loadMore(); // loadMore 自带锁与防抖：并发调用立即返回
      await nextTick();
      // 双 rAF：等滚动容器完成布局后再测量
      await new Promise<void>((r) =>
        requestAnimationFrame(() => requestAnimationFrame(() => r()))
      );
    }
  } finally {
    filling = false;
  }
}

// 兜底触发器：watch 依赖响应式数组 length 的时机在数据异步 append 时不可靠
//（数据到达时组件可能没有排队的渲染任务，flush post 永不执行）。
// 直接观察滚动容器 DOM 子节点变化：任何一批卡片渲染完成都会触发补页检查，
// 与 watch 完全解耦，天然覆盖 appendBatch / 收藏即时移除 / 切源等所有场景。
let domObserver: MutationObserver | null = null;
function startDomObserver() {
  const el = gridWrap.value;
  if (!el || domObserver) return;
  domObserver = new MutationObserver(() => {
    void fillGridUntilOverflow();
  });
  domObserver.observe(el, { childList: true });
}

// 筛选栏高度观测：加载遮罩的 top 偏移依赖它。
// 筛选栏是 v-if 控制的条件渲染，出现/消失都要重新测量，
// 故监听其尺寸变化并在其显隐时手动补测一次。
let filterRo: ResizeObserver | null = null;
function syncFilterBarHeight() {
  filterBarHeight.value = filterBar.value?.offsetHeight ?? 0;
  syncThumb(); // 筛选栏尺寸变化时滑块可能也要重算
}

onMounted(() => {
  startDomObserver();
  syncFilterBarHeight();
  filterRo = new ResizeObserver(() => syncFilterBarHeight());
  // 筛选栏可能此刻还未渲染（v-if），下一帧再尝试挂观测
  requestAnimationFrame(() => {
    if (filterBar.value) filterRo?.observe(filterBar.value);
    else syncFilterBarHeight();
    // 首帧滑块初测：setSegRef 在渲染阶段才填充 segEls，
    // 而 watch(activeFilterIndex) 只在"变化"时触发 —— 初始选中态不会触发它，
    // 所以这里必须补一次，否则滑块初始宽度为 0（看不见）。
    requestAnimationFrame(() => syncThumb());
  });
  // 挂载时补一次初查：若首屏数据在 observer 生效前已 append（length 不再变化），
  // 且内容不满一屏，此时只能靠这里触发补页，否则将永远无滚动条、无法加载后续
  void fillGridUntilOverflow();
});

onUnmounted(() => {
  filterRo?.disconnect();
  filterRo = null;
});

// 工具条显隐（切来源 / 目录内容变化）后重新测量高度并重挂观测：
// v-if 使元素被销毁重建，旧的 observe 目标已失效。
// 注意监听的是 showLocalTools 而非 showLocalFilter —— 现在承载 ref="filterBar" 的是
// 整条工具条（搜索+排序+分段），分段控件只是它内部的一行。
watch(
  () => store.showLocalTools,
  async () => {
    await nextTick();
    filterRo?.disconnect();
    // syncFilterBarHeight 内部会连带 syncThumb：
    // 分段控件从"不存在"变为"存在"时，segEls 刚被填充，滑块必须在这一刻补测，
    // 否则会停留在 {offset:0,width:0}（看不见）
    syncFilterBarHeight();
    if (filterBar.value) filterRo?.observe(filterBar.value);
    requestAnimationFrame(() => syncThumb());
  }
);

// 分段控件的显隐独立于整条工具条（纯图片目录下工具条在、分段不在），
// 滑块几何也要跟着重测一次
watch(
  () => store.showLocalFilter,
  async () => {
    await nextTick();
    requestAnimationFrame(() => syncThumb());
  }
);

// 选中项变化 → 滑块平移。等 DOM 更新后再测，确保 ref 已指向新渲染的分段。
// 双 rAF 的必要性：切换筛选会触发列表重载，布局可能在同一帧内变动，
// 单次 nextTick 测量到的位置可能仍是旧布局。
watch(activeFilterIndex, async () => {
  await nextTick();
  requestAnimationFrame(() => requestAnimationFrame(() => syncThumb()));
});

onUnmounted(() => {
  domObserver?.disconnect();
  domObserver = null;
});

// 卡片图源：缩略图就绪且已放行 → 真实缩略图；否则一律 default.jpg 兜底（加载中/失败同图）
//
// 视频缩略图同样由 Rust 后端生成（video_thumbs.rs，Media Foundation 取首帧），
// 与图片走同一条 thumb 管线与缓存目录，因此这里无需区分 kind。
// 为什么不在前端抓帧：视频经 convertFileSrc 得到 asset:// 跨源地址，
// 画进 canvas 会污染画布，toDataURL() 必抛 SecurityError（已实测）。
function thumbSrcFor(item: WallpaperItem): string {
  // revealed 的 key 是 item.path，图片与视频共用同一套放行集合
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
          v-for="tab in visibleSourceTabs"
          :key="tab.key"
          class="tab-btn flex cursor-pointer items-center gap-1 rounded-md px-3 py-1 text-[12px] font-medium transition-colors"
          :class="
            store.source === tab.key
              ? 'bg-accent-soft text-accent'
              : 'text-dim hover:text-tx'
          "
          @click="store.setSource(tab.key)"
        >
          <NIcon :component="tab.icon" :size="12" />
          {{ tab.label }}
        </button>
      </div>
      <span v-if="store.allCount > 0" class="tab-count text-[11px] text-dim">
          {{ store.gridItems.length }}/{{ store.allCount }}
        </span>
    </div>

    <!-- 网格容器：与右栏预览容器（CurrentPanel.vue 的 preview-container）
         保持**完全一致**的面板样式（rounded-xl + border-line + bg-panel/55）。
         左右两栏是并排的视觉对偶，任何一边单独改动都会造成不对称。 -->
    <div class="grid-wrap relative flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-line bg-panel/55">
      <!-- 本地工具条（纳入列表区内），**单行**：
             名称搜索（占满剩余宽度）+ 类型分段控件。
             分段仅在目录中确实存在视频时出现 —— 纯图片目录给"图片/视频"切换毫无意义；
             它不显示时搜索框自动吃掉那段宽度，纯靠 flex，无需 JS 介入。
             整条仅在本地来源且有内容时显示；搜索无结果时**不隐藏**，
             否则用户连清空搜索词的入口都没有了。

             ⚠️ 宽度预算（按最小窗口 800px 算）：
             main-area = 800 − 28(左右 p-3.5) = 772；减去 1px 分割线与 2×14px 间隙，
             两栏共分 743px；左栏 flex-[1.15] ≈ 397px；再去掉本条的 px-3 = 373px。
             现在只剩 分段控件约 150 + 6px 间距，**搜索框能拿到约 217px**。
             （排序下拉曾经也在这行，占掉 110px 把搜索框压到 125px，
              已挪到下方页脚 —— 那里整行是空的。详见 .dir-footer 的注释。） -->
      <div
        v-if="store.showLocalTools"
        ref="filterBar"
        class="filter-bar flex flex-none items-center gap-1.5 px-3 py-2"
      >
        <NInput
          :value="store.localQuery"
          size="small"
          class="search-input"
          placeholder="搜索名称…"
          clearable
          @update:value="(v: string) => store.setLocalQuery(v ?? '')"
        >
          <template #prefix>
            <NIcon :component="Search" :size="13" />
          </template>
        </NInput>

        <!-- 类型分段控件：shrink-0 必须有 —— 段内文字 nowrap，被压缩就是直接裁字 -->
        <div
          v-if="store.showLocalFilter"
          class="segmented relative flex shrink-0 items-center"
        >
          <!-- 滑块：用 transform 平移到当前选中段，transition 提供滑动动画。
               位置与宽度由实测值驱动，文字长短变化时自动适配 -->
          <span
            class="segmented-thumb absolute top-0 bottom-0"
            :style="{
              width: thumbGeo.width + 'px',
              transform: `translateX(${thumbGeo.offset}px)`,
            }"
          ></span>
          <button
            v-for="(f, i) in LOCAL_FILTERS"
            :key="f.key"
            :ref="(el) => setSegRef(el as HTMLElement | null, i)"
            class="segmented-item relative z-[1] cursor-pointer text-[12px] transition-colors"
            :class="
              store.localFilter === f.key
                ? 'text-tx'
                : 'text-dim hover:text-tx'
            "
            :title="f.title"
            @click="onFilterChange(f.key)"
          >
            {{ f.label }}
          </button>
        </div>
      </div>

      <!-- 首次加载 loading 层：allCount 已拉取但 gridItems 尚未渲染时展示。
           覆盖范围排除上方本地工具条（工具条是常驻控件，加载中也应保持可用）：
           用 flex 兄弟关系而非硬编码 top 值，工具条高度变化时无需同步修改 -->
      <div
        class="loading-overlay absolute inset-x-0 bottom-0 z-20 flex flex-col items-center justify-center"
        :style="{ top: store.showLocalTools ? filterBarHeight + 'px' : '0px' }"
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
        style="grid-auto-rows: 182px"
        @scroll="onScroll"
      >
        <template v-if="store.gridItems.length">
          <div
            v-for="item in store.gridItems"
            :key="item.key"
            class="wallpaper-item group relative flex cursor-pointer flex-col overflow-hidden rounded-[10px] border border-line bg-panel-2 transition-all duration-200 hover:-translate-y-0.5 hover:border-accent hover:shadow-[0_8px_20px_rgba(0,0,0,0.32)]"
            :class="{
              'state-current !border-ok !shadow-[0_0_0_1px_var(--color-ok),0_8px_20px_rgba(0,0,0,0.32)]': isCurrent(item),
              'state-selected !border-accent !shadow-[0_0_0_1px_var(--color-accent),0_8px_20px_rgba(0,0,0,0.32)]': isSelected(item),
              'applying opacity-70': item.applying,
            }"
            @click="onItemClick(item)"
            @dblclick="onItemDblClick(item)"
            @contextmenu="onItemContext($event, item)"
          >
            <div
              class="thumb-holder relative w-full flex-none overflow-hidden"
              style="height: 150px"
            >
              <!-- 视频卡片：首帧缩略图由 Rust 后端（Media Foundation）生成后落入同一条 thumb
                   管线，前端只负责按 revealed 懒加载显示，未就绪/失败统一回退 default.jpg -->
              <template v-if="item.kind === 'video'">
                <img
                  class="thumb absolute inset-0 h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.05]"
                  :src="thumbSrcFor(item)"
                  :data-video-path="item.path"
                  alt="video"
                  decoding="async"
                  @error="handleImageError"
                  @load="onThumbLoad"
                />
                <!-- 视频角标：统一标识动态壁纸；mkv/mov 额外标不支持预览 -->
                <span
                  class="video-badge absolute left-1.5 top-1.5 z-[6] flex items-center gap-1 rounded-md border border-white/10 bg-black/55 px-1.5 py-[2px] backdrop-blur-[2px]"
                  :title="
                    isWebviewPlayable(item.path)
                      ? '动态壁纸（视频）'
                      : '该格式（mkv/mov）无法在 WebView 中预览与播放'
                  "
                >
                  <NIcon
                    :component="isWebviewPlayable(item.path) ? Play : AlertTriangle"
                    :size="10"
                    :class="isWebviewPlayable(item.path) ? 'text-accent' : 'text-amber-300'"
                  />
                  <span class="text-[9.5px] leading-none text-white/85">
                    {{ isWebviewPlayable(item.path) ? "视频" : "不兼容" }}
                  </span>
                </span>
              </template>

              <!-- 图片缩略图：thumb 就绪且进入视口后赋缩略图 src；未就绪/加载失败均回退 default.jpg -->
              <img
                v-else
                class="thumb absolute inset-0 h-full w-full object-cover transition-transform duration-300 group-hover:scale-[1.05]"
                :src="thumbSrcFor(item)"
                :data-path="item.path"
                loading="lazy"
                decoding="async"
                :alt="item.title || baseName(item.path || '') || 'wallpaper'"
                @error="handleImageError"
                @load="onThumbLoad"
              />
            </div>
            <!-- 收藏星标：已收藏常显琥珀色，未收藏 hover 浮现；点击切换收藏
                （必应在线壁纸为远程 URL，不参与收藏夹书签体系，不显示星标） -->
            <button
              v-if="item.kind !== 'bing'"
              class="fav-badge absolute right-1.5 top-1.5 z-[6] flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md border border-white/10 bg-black/45 opacity-0 backdrop-blur-[2px] transition-opacity duration-200 group-hover:opacity-100 hover:border-white/25 hover:bg-black/65"
              :style="isFav(item) ? { opacity: 1 } : undefined"
              :title="isFav(item) ? '取消收藏' : '收藏 (Ctrl+F)'"
              @click.stop="onToggleFav(item)"
            >
              <NIcon
                :component="Star"
                :fill="isFav(item) ? 'currentColor' : 'none'"
                :size="12"
                :class="isFav(item) ? 'text-amber-300' : 'text-white/75'"
              />
            </button>
            <div class="meta flex min-w-0 items-center gap-1.5 px-2 py-1.5">
              <span class="name truncate text-[11.5px] text-dim">
                {{ item.title || baseName(item.path || "") || "壁纸" }}
              </span>
              <!-- 状态点：绿=当前壁纸(已设置到桌面/正在播放)，蓝=正在预览(选中) -->
              <span
                v-if="isCurrent(item) || isSelected(item)"
                class="status-dot ml-auto h-1.5 w-1.5 shrink-0 rounded-full"
                :class="isCurrent(item) ? 'bg-ok' : 'bg-accent'"
                :title="
                  isCurrent(item)
                    ? item.kind === 'video'
                      ? '正在播放（动态壁纸）'
                      : '当前壁纸（已设置到桌面）'
                    : '正在预览'
                "
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
          <p class="text-[13px] text-dim">
            {{
              store.source === "local" && store.localQuery.trim()
                ? "没有匹配结果"
                : "暂无壁纸"
            }}
          </p>
          <p class="empty-sub mt-1 text-[11.5px] text-faint">
            {{
              // 搜索分支排最前：它是"为什么看不到东西"里最直接的原因
              //（目录里确实有视频时筛选器不会停在"视频"，故不会互相遮蔽）
              store.source === "local" && store.localQuery.trim()
                ? `没有名称包含「${store.localQuery.trim()}」的壁纸，换个关键词或清空搜索框`
                : store.source === "local" && store.localFilter === "video"
                  ? "此目录下暂无视频，可切回「全部」或「图片」"
                  : store.source === "local" && store.localFilter === "image"
                    ? "此目录下暂无可用的静态图片，可切回「全部」或「视频」"
                    : store.source === "favorites"
                      ? "还没有收藏的壁纸：右键壁纸或点击卡片星标即可收藏"
                      : store.source === "system"
                        ? "系统壁纸目录中暂无可用图片"
                        : store.source === "bing"
                          ? "必应壁纸加载失败：请检查网络连接后重试"
                          : "此目录下暂无图片或视频，可到设置中更换壁纸目录"
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
          已加载全部 {{ store.allCount }} 项
        </div>
      </div>

      <!-- 底部：目录提示 + 排序（纳入列表容器内，作为列表框的页脚，
           与上方筛选栏共用 border 分隔，让整个列表区成为一个完整面板）。
           排序挂在这里而不是顶部工具条：顶部那行在最小窗口下只有约 373px，
           多一个 124px 的下拉会把搜索框压到 125px（已实测），搜索是高频操作、
           排序是低频操作，让低频的挪走。页脚本来整行空闲，右侧正好放它。
           用 size="tiny"（12px/22px）而不是全站统一的 small（14px/28px）：
           页脚是一行 10.5px 小字的低强调条，塞 28px 的控件会让整条变重。 -->
      <div class="dir-footer flex flex-none items-center gap-2 px-3 py-1">
        <span
          class="block min-w-0 flex-1 truncate text-[10.5px] text-faint"
          :title="dirHint"
        >
          {{ dirHint }}
        </span>
        <NSelect
          v-if="store.showLocalTools"
          :value="store.localSort"
          size="tiny"
          class="sort-select"
          :options="LOCAL_SORT_OPTIONS"
          @update:value="onSortChange"
        />
      </div>
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

/* 抓帧 <video>：仅用于解码首帧并绘制到 canvas，不参与视觉呈现。
   必须保留在渲染树中（display:none 时部分 WebView 不解码），故用 1px 透明定位。 */

/* 本地工具条（列表区头部）：名称搜索 + 类型分段（排序已挪到页脚）。
   下面的网格区不再用分割线，改由分段控件自身的外框划定区域（与参考图一致）。 */
.filter-bar {
  border-bottom: 1px solid var(--color-line);
}

/* 搜索框：占满分段控件以外的剩余宽度。
   min-width:0 必须有 —— flex item 默认 min-width:auto，输入内容/placeholder 会把
   搜索框顶宽，进而把右侧分段控件挤出容器。 */
.search-input {
  flex: 1 1 auto;
  min-width: 0;
}

/* 排序下拉定宽（挂在底部页脚里）。定宽依据 —— 已按 Naive UI 源码实测：
   - `size="tiny"` 字号 **12px**（`_styles/common` 的 fontSizeTiny），
     最宽标签是"文件大→小"：文件(24) + 大(12) + →(12) + 小(12) ≈ **60px**；
   - 触发器左右内边距 = `paddingSingle: "0 26px 0 12px"` → 12 + 26 = **38px**；
   - 展开后**选中项**右侧还要多留 20px 给"选中勾"
     （`padding-right: calc(padding-right + 20px)`），即 12 + 32 = 44px。
   所以触发器至少要 60 + 38 = 98px，弹层选中项至少要 60 + 44 = 104px。
   之前定 104px 正好卡在这个临界值上，实测会裁字 —— 现在取 **124px** 留足余量。
   ⚠️ 弹层宽度默认跟触发器一致（`consistentMenuWidth` 默认 true），
   所以**加宽触发器是唯一能让选项也不裁字的办法**（除非改用自定义 menu 宽度）。
   **标签再变长就必须同步加大这个值**，NSelect 裁字不报错、不换行，只是把字截掉。 */
.sort-select {
  flex: 0 0 auto;
  width: 124px;
}
/* NSelect 内部输入框有基础宽度、不会跟随外层收缩 —— 必须显式撑满 */
.sort-select :deep(.n-base-selection) {
  width: 100%;
}

/* 列表底部目录页脚：与筛选栏上下呼应（一条分隔线 + 一行小字）。
   右侧挂排序下拉，故用 gap-2 与左侧目录文字分开；
   目录文字 min-w-0 + truncate，路径再长也只压自己、不动右侧控件。 */
.dir-footer {
  border-top: 1px solid var(--color-line);
}

/* 分段控件外框：容器内的一个控件，用方角 + 细线，与卡片/徽标的 10px·6px 语言一致。
   刻意不用胶囊：
   1. 圆角适用半径约等于"元素高度减 2px"，三段式控件整体偏宽，胶囊两端
      会留下两段无内容的弧形空白，视觉上反而更散；
   2. 这已经处在一个 12px 圆角容器的内部（.grid-wrap），
      再嵌胶囊 + 胶囊内再套胶囊滑块 = 同一区域三层圆弧互相打架。
   内边距仍留 2px 作为滑块"轨道"，滑块不贴边才有浮起感。 */
.segmented {
  padding: 2px;
  border-radius: 7px;
  background: transparent;
}

/* 滑块（选中态指示器）：
   - 用 absolute + transform 平移，transition 产生滑动动画
   - 抬升感靠 1px 描边 + 柔和小阴影，不用重投影（深色主题下阴影过重会发脏）
   - z-index 低于文字（文字 z-[1]），保证文字始终清晰可读
   - 圆角 5px = 外框 7px 减 2px 内边距（同心圆角，避免内层圆弧错位） */
.segmented-thumb {
  left: 0;
  border-radius: 5px;
  background: var(--color-elev);
  border: 1px solid var(--color-line-2);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.28);
  transition:
    transform 0.22s cubic-bezier(0.4, 0, 0.2, 1),
    width 0.22s cubic-bezier(0.4, 0, 0.2, 1);
}

/* 分段按钮：左右内边距决定每段宽度（滑块宽度实测自它），
   三段宽度由文字长度自然决定，不做等宽处理。
   12px 是舒适值；排序下拉从这一行挪走后宽度已够（搜索框仍有约 217px），
   不必再为省地方而收紧。 */
.segmented-item {
  padding: 4px 12px;
  line-height: 1.2;
  white-space: nowrap;
  background: transparent;
}
/* 键盘可达性：保留焦点环；鼠标点击不显示（:focus-visible 语义） */
.segmented-item:focus-visible {
  outline: 2px solid var(--color-accent-soft);
  outline-offset: 1px;
}

</style>
