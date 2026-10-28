<script setup lang="ts">
// Sidebar - 左栏：壁纸网格（分页加载 / 右键菜单 / 应用当前壁纸 / 收藏夹）
// 列表中混有静态图片与动态壁纸视频：视频卡片用 <video> 自抓首帧作为缩略图，
// 并在角标上标明视频类型与"WebView 不可播放"的告警（mkv/mov）。
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import type { Component } from "vue";
import { NIcon, NInput, NSelect } from "naive-ui";
import { AlertTriangle, Check, Download, Globe, Image as ImageIcon, Layers, Monitor, Play, Plus, Search, SlidersHorizontal, Sparkles, Star } from "lucide-vue-next";
import { openUrl } from "@tauri-apps/plugin-opener";
import defaultJpg from '../assets/images/default.jpg'
import { toast } from "../lib/naive-host";
import {
  baseName,
  isWebviewPlayable,
  thumbSrc,
  useWallpaperStore,
  PIXABAY_CATEGORIES,
  PIXABAY_ORDERS,
  PIXABAY_IMAGE_TYPES,
  PIXABAY_ORIENTATIONS,
  PIXABAY_SIZES,
  PIXABAY_COLORS,
  PIXABAY_VIDEO_TYPES,
  WALLHAVEN_CATEGORIES,
  WALLHAVEN_SORTS,
  WALLHAVEN_ATLEASTS,
  WALLHAVEN_RATIOS,
  WALLHAVEN_COLORS,
  type WallpaperItem,
  type WallpaperSource,
  type LocalFilter,
  type LocalSort,
  type PixabayImageType,
  type PixabayOrientation,
  type PixabaySize,
  type PixabayVideoType,
  type WallhavenCategory,
  type WallhavenSort,
  type WallhavenAtleast,
  type WallhavenRatio,
} from "../stores/wallpaper";

const store = useWallpaperStore();

// Pixabay 下拉选项：常量是 `as const` 元组，NSelect 需要可变的 SelectOption[]，
// 这里转一次（不直接改常量，是为了让常量保持字面量类型供别处使用）
const PIXABAY_CATEGORY_OPTIONS = PIXABAY_CATEGORIES.map((c) => ({ label: c.label, value: c.value }));
const PIXABAY_ORDER_OPTIONS = PIXABAY_ORDERS.map((o) => ({ label: o.label, value: o.value }));

/// Pixabay 媒体种类分段选项。
///
/// **只有两档：图片 / 视频** —— 这是唯一必须分叉的维度（两套接口）。
/// "照片 / 插画 / 矢量" 是**内容类型**，已经下沉到筛选浮层里
/// （对应 Pixabay 网页的「内容类型」下拉）。
///
/// ⚠️ 历史：早先这里是"照片 / 插画 / 视频"三段，把插画当媒体种类。
/// 改成网页那套筛选体系后，插画必须能在浮层的「内容类型」里一眼找到 ——
/// 它是壁纸感最强的一类（赛博朋克霓虹街道、天际线剪影 vs 素材库实拍），
/// 别再把它塞回分类下拉里。
const PIXABAY_MEDIA_OPTIONS = [
  { label: "图片", value: "image" as const, title: "搜索 Pixabay 图片" },
  {
    label: "视频",
    value: "video" as const,
    title: "搜索 Pixabay 视频（先预览，设为动态壁纸时才下载）",
  },
];

// 浮层面板里各个下拉的选项（常量是 `as const` 元组，NSelect 需要可变数组）
const IMAGE_TYPE_OPTIONS = PIXABAY_IMAGE_TYPES.map((o) => ({ label: o.label, value: o.value }));
const ORIENTATION_OPTIONS = PIXABAY_ORIENTATIONS.map((o) => ({ label: o.label, value: o.value }));
const SIZE_OPTIONS = PIXABAY_SIZES.map((o) => ({ label: o.label, value: o.value }));
const VIDEO_TYPE_OPTIONS = PIXABAY_VIDEO_TYPES.map((o) => ({ label: o.label, value: o.value }));

// Wallhaven 下拉选项（同样转成 NSelect 需要的可变数组）
const WALLHAVEN_CATEGORY_OPTIONS = WALLHAVEN_CATEGORIES.map((o) => ({ label: o.label, value: o.value }));
const WALLHAVEN_SORT_OPTIONS = WALLHAVEN_SORTS.map((o) => ({ label: o.label, value: o.value }));
const WALLHAVEN_ATLEAST_OPTIONS = WALLHAVEN_ATLEASTS.map((o) => ({ label: o.label, value: o.value }));
const WALLHAVEN_RATIO_OPTIONS = WALLHAVEN_RATIOS.map((o) => ({ label: o.label, value: o.value }));

// 各来源选项卡元信息：图标与文案（渲染顺序由 store.sourceOrder 决定）
//
// `menuLabel` 只给「添加来源」浮层用：页签里「系统」「必应」够用（有上下文），
// 但浮层里单独一个「系统」容易被读成"系统设置"，所以那里换成完整名称。
const sourceTabMeta: Record<
  WallpaperSource,
  { label: string; icon: Component; menuLabel?: string }
> = {
  local: { label: "本地", icon: Layers },
  system: { label: "系统", icon: Monitor, menuLabel: "系统壁纸" },
  favorites: { label: "收藏", icon: Star },
  bing: { label: "必应", icon: Globe, menuLabel: "必应壁纸" },
  pixabay: { label: "Pixabay", icon: Sparkles, menuLabel: "Pixabay 图库" },
  wallhaven: { label: "Wallhaven", icon: ImageIcon, menuLabel: "Wallhaven 图库" },
};

// 按用户配置顺序 + 可见性过滤后实际渲染的选项卡列表
//（local 恒可见，其余来源读取各自可见性开关）
const visibleSourceTabs = computed(() =>
  store.sourceOrder
    .filter((key) => key === "local" || store.sourceVisibility[key])
    .map((key) => ({ key, ...sourceTabMeta[key] }))
);

/// 仍处于隐藏状态的来源（不含 local）。
///
/// 「添加来源」入口据此决定**是否出现**，所以全部开启后按钮会自己消失。
/// 顺序跟随 `sourceOrder`，用户在设置里排过序后这里也是同一顺序。
const hiddenSources = computed(() =>
  store.sourceOrder
    .filter(
      (key): key is Exclude<WallpaperSource, "local"> =>
        key !== "local" && !store.sourceVisibility[key]
    )
    .map((key) => ({
      key,
      icon: sourceTabMeta[key].icon,
      label: sourceTabMeta[key].menuLabel ?? sourceTabMeta[key].label,
    }))
);

// ---- 「添加来源」小浮层 ----
//
// 为什么要有它：默认只开「本地 + 收藏」，其余来源的开关藏在设置里，
// 新用户根本不知道还有必应 / Pixabay。页签条末尾放一个低调的 ＋，
// 点开只列**当前隐藏**的来源，点一行即开启。
//
// 之所以"只列隐藏的 + 全开时按钮消失"，是为了顺带解决宽度预算：
// 5 个页签全开时标签条已经吃掉约 322px（见下方注释），再加一个常驻按钮
// 就会顶破最小窗口 800px 的 396px 容器。
const srcMenuOpen = ref(false);
const srcMenuBtn = ref<HTMLElement | null>(null);
const srcMenuEl = ref<HTMLElement | null>(null);
const srcMenuStyle = ref<Record<string, string>>({ left: "0px", top: "0px" });
/// 浮层宽度。**改这里必须同步模板里的 width 绑定**（样式是内联算出来的）。
const SRC_MENU_WIDTH = 172;

function toggleSrcMenu() {
  if (srcMenuOpen.value) {
    srcMenuOpen.value = false;
    return;
  }
  const btn = srcMenuBtn.value;
  if (!btn) return;
  const r = btn.getBoundingClientRect();
  // 左对齐按钮；万一超出右边界就往回收（页签条在最左栏，正常不会触发）
  let left = r.left;
  const maxLeft = window.innerWidth - VIEWPORT_PAD - SRC_MENU_WIDTH;
  if (left > maxLeft) left = Math.max(VIEWPORT_PAD, maxLeft);
  if (left < VIEWPORT_PAD) left = VIEWPORT_PAD;
  srcMenuStyle.value = {
    left: `${Math.round(left)}px`,
    top: `${Math.round(r.bottom + PANEL_GAP)}px`,
    width: `${SRC_MENU_WIDTH}px`,
  };
  srcMenuOpen.value = true;
}

/// 点一行就开启该来源。
///
/// 最后一个隐藏来源被开启后，`hiddenSources` 变空 ⇒ ＋ 按钮消失 ⇒
/// 浮层失去锚点。必须顺手关掉，否则会留下一个"飘在原处、再也点不到按钮关"的浮层。
function enableSource(key: Exclude<WallpaperSource, "local">) {
  store.setSourceVisibility(key, true);
  if (!hiddenSources.value.length) srcMenuOpen.value = false;
}

/// 点浮层与 ＋ 以外的地方收起。用 `pointerdown` 捕获阶段，理由同筛选浮层。
/// 浮层里只有普通 button（没有 NSelect），所以不需要 `menu-props` 那套放行。
function onSrcMenuOutside(e: PointerEvent) {
  const t = e.target as Node | null;
  if (!t) return;
  if (srcMenuEl.value?.contains(t)) return;
  if (srcMenuBtn.value?.contains(t)) return;
  srcMenuOpen.value = false;
}

function onSrcMenuEsc(e: KeyboardEvent) {
  if (e.key === "Escape") srcMenuOpen.value = false;
}

function onSrcMenuResize() {
  srcMenuOpen.value = false;
}

watch(srcMenuOpen, (open) => {
  if (open) {
    document.addEventListener("pointerdown", onSrcMenuOutside, true);
    document.addEventListener("keydown", onSrcMenuEsc);
    window.addEventListener("resize", onSrcMenuResize);
  } else {
    document.removeEventListener("pointerdown", onSrcMenuOutside, true);
    document.removeEventListener("keydown", onSrcMenuEsc);
    window.removeEventListener("resize", onSrcMenuResize);
  }
});

onUnmounted(() => {
  document.removeEventListener("pointerdown", onSrcMenuOutside, true);
  document.removeEventListener("keydown", onSrcMenuEsc);
  window.removeEventListener("resize", onSrcMenuResize);
});

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

/// 当前生效的筛选条件摘要（如「插画 · 竖向 · 蓝色 · ≥3840×2160」）。
///
/// 为什么要有它：筛选全部收进浮层后，面板一关就看不见当前筛了什么，
/// 用户会对着"结果怎么这么少"发愣。挂在底部页脚的小字上，等于给筛选面板
/// 一个常驻的"当前状态"回显，也顺带让「插画」这类选项不因为收进浮层而消失。
const pixabayFilterSummary = computed(() => {
  const parts: string[] = [];
  const cat = PIXABAY_CATEGORIES.find((c) => c.value === store.pixabayCategory);
  if (cat && cat.value) parts.push(cat.label);
  if (store.pixabayMedia === "video") {
    if (store.pixabayVideoType !== "all") {
      const t = PIXABAY_VIDEO_TYPES.find((x) => x.value === store.pixabayVideoType);
      if (t) parts.push(t.label);
    }
    if (store.pixabayVideoOrientation !== "horizontal") {
      const o = PIXABAY_ORIENTATIONS.find((x) => x.value === store.pixabayVideoOrientation);
      if (o) parts.push(o.label);
    }
  } else {
    // 内容类型只有偏离默认（照片）时才提，否则每行都挂个"照片"是噪音
    if (store.pixabayImageType !== "photo") {
      const t = PIXABAY_IMAGE_TYPES.find((x) => x.value === store.pixabayImageType);
      if (t) parts.push(t.label);
    }
    if (store.pixabayOrientation !== "horizontal") {
      const o = PIXABAY_ORIENTATIONS.find((x) => x.value === store.pixabayOrientation);
      if (o) parts.push(o.label);
    }
    if (store.pixabayColors.length) {
      const names = store.pixabayColors
        .map((v) => PIXABAY_COLORS.find((c) => c.value === v)?.label ?? "")
        .filter(Boolean);
      if (names.length) parts.push(names.join("/"));
    }
  }
  if (store.pixabaySize !== "1920") {
    const s = PIXABAY_SIZES.find((x) => x.value === store.pixabaySize);
    if (s) parts.push(s.label);
  }
  return parts.join(" · ");
});

/// Wallhaven 的筛选摘要（与 Pixabay 同一套理由：面板一关就看不见筛了什么）。
const wallhavenFilterSummary = computed(() => {
  const parts: string[] = [];
  if (store.wallhavenCategory !== "111") {
    const c = WALLHAVEN_CATEGORIES.find((x) => x.value === store.wallhavenCategory);
    if (c) parts.push(c.label);
  }
  if (store.wallhavenAtleast) {
    const s = WALLHAVEN_ATLEASTS.find((x) => x.value === store.wallhavenAtleast);
    if (s) parts.push(s.label);
  }
  if (store.wallhavenRatio) {
    const r = WALLHAVEN_RATIOS.find((x) => x.value === store.wallhavenRatio);
    if (r) parts.push(r.label);
  }
  if (store.wallhavenColor) {
    const c = WALLHAVEN_COLORS.find((x) => x.value === store.wallhavenColor);
    if (c) parts.push(c.label);
  }
  return parts.join(" · ");
});

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
  if (store.source === "pixabay") {
    // 筛选摘要优先展示 —— 它回答的是"为什么结果是这样的"
    if (pixabayFilterSummary.value) {
      return `Pixabay 筛选：${pixabayFilterSummary.value}`;
    }
    if (store.pixabayMedia === "video") {
      // 视频没有"免费 key 只有 1280"的问题，改为说明清晰度偏好与下载时机
      const q = store.pixabayVideoQuality === "large" ? "4K 优先" : "1080p";
      return `Pixabay 视频：先预览，设为动态壁纸时下载到本地（${q}）`;
    }
    return store.pixabayDegraded
      ? "Pixabay：免费 key 仅 1280px，申请 full access 后自动升级为原图"
      : "Pixabay 在线图库：设为桌面时自动下载到本地";
  }
  if (store.source === "wallhaven") {
    if (wallhavenFilterSummary.value) {
      return `Wallhaven 筛选：${wallhavenFilterSummary.value}`;
    }
    return "Wallhaven 在线图库：设为桌面时自动下载到本地（仅 SFW）";
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

// ---- 图片 / 视频 分段控件（Pixabay 工具条内）----
//
// 与上面本地那条是**两个独立控件**，各有各的滑块几何：
// 两段宽度虽然现在相等，但以后改文案（"图片"/"视频 4K"）就会不等，
// 写死 50% 会立刻错位 —— 沿用"实测宽度"这套做法最省心。
const mediaSegEls = ref<(HTMLElement | null)[]>([]);
const mediaThumbGeo = ref({ offset: 0, width: 0 });
/// 当前选中项在分段里的下标。
/// **按 value 查，不写死 `=== "video" ? 1 : 0`** —— 段数已经从 2 变 3，
/// 写死索引的话再加一段就得再改一次，且改漏了滑块会停在错的段上（不报错）。
const mediaActiveIndex = computed(() => {
  const i = PIXABAY_MEDIA_OPTIONS.findIndex((m) => m.value === store.pixabayMedia);
  return i < 0 ? 0 : i;
});

function setMediaSegRef(el: HTMLElement | null, i: number) {
  mediaSegEls.value[i] = el;
}

function syncMediaThumb() {
  const el = mediaSegEls.value[mediaActiveIndex.value];
  if (!el) {
    mediaThumbGeo.value = { offset: 0, width: 0 };
    return;
  }
  mediaThumbGeo.value = { offset: el.offsetLeft, width: el.offsetWidth };
}

// ---- Pixabay 筛选浮层 ----
//
// 为什么是浮层、而不是像网页那样横排 6 个下拉：
// 左栏在最小窗口（800px）下工具条只有约 394px，6 个下拉 + 排序排不开
// —— 实测三段分段时就已把搜索框压到内部 49px（约 3.5 个汉字），再加必然崩。
// 浮层里竖排，每个控件都有 ~180px，标签不会被 NSelect 静默裁掉。
//
// 面板本身用 `Teleport to="body"` + `fixed` 定位，与项目里 ContextMenu 同一套做法：
// 放在 aside 内部会被 `.app-shell` 的 overflow-hidden 裁掉。
const filterOpen = ref(false);
const filterBtn = ref<HTMLElement | null>(null);
const filterPanelEl = ref<HTMLElement | null>(null);
/// 打开时按按钮位置实测出来的定位与最大高度
const filterPanelStyle = ref<Record<string, string>>({ left: "0px", top: "0px" });

/// 面板宽度。**改这里必须同步 `.filter-panel` 的 width**。
const PANEL_WIDTH = 268;
const PANEL_GAP = 6;
const VIEWPORT_PAD = 8;

/// 面板内所有 NSelect 的菜单标记类。
///
/// 菜单被 Naive UI Teleport 到 body，不在 `.filter-panel` 内 ⇒ 必须靠这个类
/// 让"点外面收起"的逻辑放行选项。**面板里每加一个下拉都要挂上 `FILTER_MENU_PROPS`**，
/// 漏挂的表现是"这个下拉选了没反应"（面板先被关掉，click 落不到选项上）。
const FILTER_MENU_CLASS = "filter-menu";
const FILTER_MENU_PROPS = { class: FILTER_MENU_CLASS };

function toggleFilterPanel() {
  if (filterOpen.value) {
    filterOpen.value = false;
    return;
  }
  const btn = filterBtn.value;
  if (!btn) return;
  const r = btn.getBoundingClientRect();
  // 按钮在工具条最右侧 ⇒ 面板右对齐按钮，再往左收，避免超出窗口右边
  let left = r.right - PANEL_WIDTH;
  if (left < VIEWPORT_PAD) left = VIEWPORT_PAD;
  const maxLeft = window.innerWidth - VIEWPORT_PAD - PANEL_WIDTH;
  if (left > maxLeft) left = Math.max(VIEWPORT_PAD, maxLeft);
  const below = window.innerHeight - r.bottom - PANEL_GAP - VIEWPORT_PAD;
  filterPanelStyle.value = {
    left: `${Math.round(left)}px`,
    top: `${Math.round(r.bottom + PANEL_GAP)}px`,
    // 最小窗口 800×480 时下方只剩约 370px，面板本身约 300px，够用；
    // 兜底给个 max-height + 内部滚动，窗口再矮也不会顶出屏幕。
    maxHeight: `${Math.max(200, Math.round(below))}px`,
  };
  filterOpen.value = true;
}

/// 点面板与按钮以外的地方收起。
///
/// 用 `pointerdown` 的**捕获阶段**：比 `click` 早一帧，避免"点卡片时面板先关、
/// 卡片自己的 click 又被吃掉"的错位。面板与按钮自身的点击都要放行。
///
/// ⚠️ **【坑】NSelect 的下拉菜单不在面板 DOM 里。**
/// Naive UI 把菜单 Teleport 到 `body > .v-binder-follower-container > ...`，
/// 所以 `filterPanelEl.contains(target)` 对菜单里的选项**恒为 false**。
/// 真实鼠标点选项时：pointerdown 先到 document 捕获阶段 ⇒ 判定"点在外面"
/// ⇒ 面板被 `v-if` 卸掉 ⇒ 选项连 click 都收不到 ⇒ **选了没反应**。
/// 程序化 `el.click()` 不产生 pointerdown，所以这个 bug 用 `el.click()` 测不出来
/// （`cdp-filter-click.mjs` 用真实鼠标事件才复现）。
/// 修法：给面板内所有 NSelect 的菜单打上 `filter-menu` 标记，一并放行。
/// **新增面板内的下拉必须带 `:menu-props="FILTER_MENU_PROPS"`，否则选项点不动。**
function onFilterOutside(e: PointerEvent) {
  const t = e.target as Node | null;
  if (!t) return;
  if (filterPanelEl.value?.contains(t)) return;
  if (filterBtn.value?.contains(t)) return;
  if (t instanceof Element && t.closest(`.${FILTER_MENU_CLASS}`)) return;
  filterOpen.value = false;
}

function onFilterEsc(e: KeyboardEvent) {
  if (e.key === "Escape") filterOpen.value = false;
}

/// 窗口尺寸一变就收起：面板的 left/top 是**打开那一刻**按按钮位置算出来的，
/// 窗口缩放后按钮会移动，面板却还停在原地 —— 看着像"飘在错的地方"。
/// 收起比重新定位简单，也不会出现定位抖动的中间帧。
function onFilterResize() {
  filterOpen.value = false;
}

watch(filterOpen, (open) => {
  if (open) {
    document.addEventListener("pointerdown", onFilterOutside, true);
    document.addEventListener("keydown", onFilterEsc);
    window.addEventListener("resize", onFilterResize);
  } else {
    document.removeEventListener("pointerdown", onFilterOutside, true);
    document.removeEventListener("keydown", onFilterEsc);
    window.removeEventListener("resize", onFilterResize);
  }
});

// 切来源时收起：面板里全是 Pixabay 的筛选，切到别处还浮着很怪
watch(
  () => store.source,
  () => {
    filterOpen.value = false;
  }
);

onUnmounted(() => {
  document.removeEventListener("pointerdown", onFilterOutside, true);
  document.removeEventListener("keydown", onFilterEsc);
  window.removeEventListener("resize", onFilterResize);
});

/// 颜色色块多选切换（接口的 `colors` 是 OR 语义，多值用逗号分隔）
function toggleColor(value: string) {
  const cur = store.pixabayColors;
  const next = cur.includes(value) ? cur.filter((v) => v !== value) : [...cur, value];
  void store.setPixabayColors(next);
}

/// 色块背景。`transparent` 必须用棋盘格 —— 直接给 transparent 会看不见这个色块，
/// 用户只会以为"这里少了一个按钮"。
function colorBg(value: string, hex: string): string {
  return value === "transparent"
    ? "repeating-conic-gradient(#8b95a5 0% 25%, #2b3240 0% 50%) 50% / 8px 8px"
    : hex;
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
//
// Pixabay 在线视频同样支持双击：会先下载到本地再设（见 store.setItemAsDesktop）。
function onItemDblClick(item: WallpaperItem) {
  if (item.kind !== "video" && item.kind !== "pixabay-video") return;
  void store.setItemAsDesktop(item);
}

// 卡片下方的显示名。必应下载下来的文件带标题（`迁飞路线上的生命`），
// 本地/系统项则退回文件名。
//
// 卡片宽度是自适应的（`minmax(148px, 1fr)`），标题长一点就会截断 ——
// 所以模板里把这个名字同时挂到 `title` 上，鼠标悬停能看到完整文字。
// 两处**必须用同一个函数**：分开写两遍表达式，早晚会改歪一处
//（曾把 `:alt` 留在旧写法上，结果 alt 与可见文字不一致）。
function itemLabel(item: WallpaperItem): string {
  return item.title || baseName(item.path || "") || "壁纸";
}

/// 视频时长角标：`12` → `0:12`，`95` → `1:35`。
/// 超过一小时的素材在这个场景基本不存在，所以不做 `h:mm:ss` 分支 ——
/// 真出现也只是显示成 `75:30`，仍然可读。
function formatDuration(sec: number | undefined): string {
  const s = Math.max(0, Math.round(sec || 0));
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
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

// 必应 / Pixabay 的下载按钮逻辑已并入下方的 `isOnlineDownloading` / `onDownloadOnline`
// —— 两者交互完全一致（只下载、不改桌面），只是落点不同，没必要各写一套。

// ---- Pixabay ----

// 搜索防抖：Pixabay 限流 100 次/60 秒，逐字发请求几秒就能打满。
// 400ms 是"打字停顿"的经验值，短于它会在输入中途反复触发。
let pixabayQueryTimer: number | null = null;
function onPixabayQueryInput(v: string) {
  const next = v ?? "";
  if (pixabayQueryTimer !== null) window.clearTimeout(pixabayQueryTimer);
  pixabayQueryTimer = window.setTimeout(() => {
    pixabayQueryTimer = null;
    void store.setPixabayQuery(next);
  }, 400);
}

// Wallhaven 同样要防抖：未认证限流 45 次/分钟，逐字发请求几秒就能打满。
// 400ms 与 Pixabay 一致 —— 同一个应用里两处搜索的节奏不该不一样。
let wallhavenQueryTimer: number | null = null;
function onWallhavenQueryInput(v: string) {
  const next = v ?? "";
  if (wallhavenQueryTimer !== null) window.clearTimeout(wallhavenQueryTimer);
  wallhavenQueryTimer = window.setTimeout(() => {
    wallhavenQueryTimer = null;
    void store.setWallhavenQuery(next);
  }, 400);
}

// 在线源（必应 / Pixabay）的下载按钮与「已下载」徽标结构相同、只有落点不同，
// 故模板共用一套，由这里按 kind 分派 —— 避免在模板里堆三元表达式。
/// 在线卡片是否正在下载。图片与视频共用 `pixabayDownloadingId` 这一个槽位
/// （同一时刻只可能有一个下载在跑，两个槽位反而会出现"两个卡片同时转"）。
function isOnlineDownloading(item: WallpaperItem): boolean {
  if (item.kind === "pixabay" || item.kind === "pixabay-video") {
    return !!item.onlineId && store.pixabayDownloadingId === item.onlineId;
  }
  if (item.kind === "wallhaven") {
    return !!item.onlineId && store.wallhavenDownloadingId === item.onlineId;
  }
  return !!item.date && store.bingDownloadingDate === item.date;
}

function isOnlineDownloaded(item: WallpaperItem): boolean {
  if (item.kind === "pixabay" || item.kind === "pixabay-video") {
    return store.isPixabayDownloaded(item);
  }
  if (item.kind === "wallhaven") {
    return store.isWallhavenDownloaded(item);
  }
  return store.isBingDownloaded(item);
}

function onDownloadOnline(item: WallpaperItem) {
  if (item.kind === "pixabay" || item.kind === "pixabay-video") {
    void store.downloadPixabayWallpaper(item);
  } else if (item.kind === "wallhaven") {
    void store.downloadWallhavenWallpaper(item);
  } else {
    void store.downloadBingWallpaper(item);
  }
}

function onOpenDownloaded(item: WallpaperItem) {
  if (item.kind === "pixabay" || item.kind === "pixabay-video") {
    void store.openInLocalPixabay(item);
  } else if (item.kind === "wallhaven") {
    void store.openInLocalWallhaven(item);
  } else {
    void store.openInLocal(item);
  }
}

/// 打开作者主页（Pixabay API 条款要求署名可点回）。
/// 走系统默认浏览器而不是 WebView 内跳转 —— 内跳会把整个应用界面替换掉且回不来。
async function onOpenAuthor(item: WallpaperItem) {
  if (!item.authorUrl) return;
  try {
    await openUrl(item.authorUrl);
  } catch (err: unknown) {
    toast("打开作者主页失败：" + ((err as Error)?.message || String(err)), "error");
  }
}

// 未配置 key 时，空态里给一个就地填写入口（比"去设置里翻"少两步）
const pixabayKeyInput = ref("");function savePixabayKey() {
  const v = pixabayKeyInput.value.trim();
  if (!v) {
    toast("请先粘贴 API key", "warning");
    return;
  }
  store.setPixabayKey(v);
  pixabayKeyInput.value = "";
  void store.reloadPixabay();
  toast("已保存，正在加载…", "success");
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
  syncMediaThumb(); // 图片/视频那条同理
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
    requestAnimationFrame(() => {
      syncThumb();
      syncMediaThumb();
    });
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

// Pixabay 工具条显隐：图片/视频分段只在它里面，出现时必须补测一次，
// 否则滑块停在 {offset:0,width:0}（看不见）
watch(
  () => store.showPixabayTools,
  async () => {
    await nextTick();
    requestAnimationFrame(() => syncMediaThumb());
  }
);

// Wallhaven 工具条显隐：元素被销毁重建，筛选栏高度与 ResizeObserver 的目标
// 都要重算 —— 否则加载遮罩的 top 会沿用上一条工具条的高度（两条高度接近，
// 但别赌，一像素的偏移就会让遮罩盖住/漏出工具条边缘）。
watch(
  () => store.showWallhavenTools,
  async () => {
    await nextTick();
    filterRo?.disconnect();
    syncFilterBarHeight();
    if (filterBar.value) filterRo?.observe(filterBar.value);
  }
);

// 图片 ⇄ 视频：滑块平移。等 DOM 更新后再测（切换会连带重载列表）
watch(mediaActiveIndex, async () => {
  await nextTick();
  requestAnimationFrame(() => requestAnimationFrame(() => syncMediaThumb()));
});

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
      <!-- 选项卡条。**每个 tab 都必须 shrink-0 + whitespace-nowrap**：
           不加的话空间一紧，flex 会去压缩 tab 内部，文字被挤成竖排（"本/地"两行），
           且不报错 —— 这是 5 个来源全开 + 最小窗口 800px 时真实踩到的坑。

           **实测宽度**（headless Chrome 800×480、12px 字、Windows 默认字体）：
             本地/收藏/系统/必应 = 56 each → 224
             Pixabay            = 74
             Wallhaven          = 92（拉丁字母最长的一个）
             5 个 gap-1.5(6px)  = 30   ⇒ 标签条合计 420
             右侧计数 "12/12"    = 24.5
             两处 flex gap-2     = 16
             合计 ≈ 460 / 容器 396px —— **6 个页签全开时必然放不下**。

           ⇒ 所以 `.source-tabs` 必须 `flex-wrap`：放不下就换行，而不是把标签条
           顶出去、把右侧计数挤没（后者不会报错，只是"计数莫名其妙消失"）。
           换行只发生在用户把 6 个来源全打开时；默认 2 个页签时没有任何变化。
           末尾那个「＋ 添加来源」约 21px + gap 6 = 27px，它只在还有来源被隐藏时
           渲染，全开时自己消失，所以不会额外占一行。 -->
      <div class="source-tabs flex min-w-0 flex-wrap items-center gap-1.5">
        <button
          v-for="tab in visibleSourceTabs"
          :key="tab.key"
          class="tab-btn flex shrink-0 cursor-pointer items-center gap-1 whitespace-nowrap rounded-md px-2 py-1 text-[12px] font-medium transition-colors"
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
        <!-- 「添加来源」入口：**只在还有隐藏来源时渲染**。
             全开时它自己消失，所以不占那 33px 的余量（见上方宽度预算）。
             低调处理：无文字、无边框，只有 hover 才提亮，不抢页签的视觉权重。 -->
        <button
          v-if="hiddenSources.length"
          ref="srcMenuBtn"
          class="src-add flex shrink-0 cursor-pointer items-center justify-center rounded-md p-1 transition-colors"
          :class="srcMenuOpen ? 'bg-accent-soft text-accent' : 'text-faint hover:bg-white/5 hover:text-tx'"
          :title="`添加壁纸来源（还有 ${hiddenSources.length} 个未显示）`"
          @click="toggleSrcMenu"
        >
          <NIcon :component="Plus" :size="13" />
        </button>
      </div>
      <span
        v-if="store.allCount > 0"
        class="tab-count shrink-0 whitespace-nowrap text-[11px] text-dim"
      >
          {{ store.gridItems.length }}/{{ store.allCount }}
        </span>
    </div>

    <!-- 「添加来源」浮层。与筛选浮层同一套做法（Teleport 到 body + fixed + 实测定位）：
         放在 aside 里会被 `.app-shell` 的 overflow-hidden 裁掉。 -->
    <Teleport to="body">
      <Transition name="filter-pop">
        <div
          v-if="srcMenuOpen && hiddenSources.length"
          ref="srcMenuEl"
          class="src-menu app-no-drag fixed z-[88] rounded-xl border border-line-2 bg-[rgba(24,31,45,0.97)] p-1.5 shadow-[0_14px_36px_rgba(0,0,0,0.45)] backdrop-blur-xl"
          :style="srcMenuStyle"
          @mousedown.stop
        >
          <div class="px-1.5 pb-1 pt-0.5 text-[10.5px] text-faint">添加来源</div>
          <button
            v-for="s in hiddenSources"
            :key="s.key"
            class="src-menu-row flex w-full cursor-pointer items-center gap-2 rounded-md px-1.5 py-1.5 text-left transition-colors hover:bg-white/5"
            :title="`开启后在左侧页签条显示${s.label}`"
            @click="enableSource(s.key)"
          >
            <NIcon :component="s.icon" :size="13" class="shrink-0 text-dim" />
            <span class="flex-1 text-[12px] text-tx">{{ s.label }}</span>
            <span class="shrink-0 text-[10.5px] text-accent">开启</span>
          </button>
        </div>
      </Transition>
    </Teleport>

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

      <!-- Pixabay 工具条：搜索框 + 媒体类型 + 分类。与本地那条互斥显示
           （v-else-if 必须紧邻 v-if，中间不能插别的元素）。
           宽度预算（最小窗口 800px，左栏可用 ≈394px）：
           搜索框 flex-1 + 分段 130px（三段×42 + 4 padding）+ 分类 88px + gap 4×2。
           三段比原来的两段多占 42px，是从 `.segmented-item` 的 padding 和
           分类下拉的定宽里各挤出来的 —— 改这里任何一项都要重新量一次。 -->
      <div
        v-else-if="store.showPixabayTools"
        ref="filterBar"
        class="filter-bar flex flex-none items-center gap-1 px-3 py-2"
      >
        <NInput
          :value="store.pixabayQuery"
          size="small"
          class="search-input"
          placeholder="搜索…"
          clearable
          @update:value="onPixabayQueryInput"
        >
          <template #prefix>
            <NIcon :component="Search" :size="13" />
          </template>
        </NInput>
        <!-- 图片 / 视频：Pixabay 一个来源里的两种媒体，共用搜索词、分类与排序。
             与本地那条类型筛选同一套视觉（复用 .segmented 三个类），
             但滑块几何是各自独立的（mediaThumbGeo）—— 两段宽度将来可能不等。 -->
        <div class="segmented relative flex shrink-0 items-center">
          <span
            class="segmented-thumb absolute top-0 bottom-0"
            :style="{
              width: mediaThumbGeo.width + 'px',
              transform: `translateX(${mediaThumbGeo.offset}px)`,
            }"
          ></span>
          <button
            v-for="(m, i) in PIXABAY_MEDIA_OPTIONS"
            :key="m.value"
            :ref="(el) => setMediaSegRef(el as HTMLElement | null, i)"
            class="segmented-item relative z-[1] cursor-pointer text-[12px] transition-colors"
            :class="store.pixabayMedia === m.value ? 'text-tx' : 'text-dim hover:text-tx'"
            :title="m.title"
            @click="store.setPixabayMedia(m.value)"
          >
            {{ m.label }}
          </button>
        </div>
        <!-- 筛选按钮：把网页那排下拉收进一个浮层。
             宽度预算（最小窗口 800px，工具条 ≈394px，内边距 32px ⇒ 362px）：
             分段 88（两段×42 + 4）+ 按钮 54 + gap 4×2 = 150
             ⇒ 搜索框 flex-1 拿到约 209px（内部 ≈150px，够打 10 个汉字）。
             原来"照片/插画/视频"三段时搜索框只剩 144px，现在两段反而更宽。
             角标只统计**偏离默认值**的筛选，否则一进来就显示 3，等于谎报。 -->
        <button
          ref="filterBtn"
          class="filter-btn flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-[12px] transition-colors"
          :class="
            filterOpen || store.pixabayFilterCount
              ? 'bg-accent-soft text-accent'
              : 'text-dim hover:text-tx'
          "
          :title="store.pixabayFilterCount ? `已启用 ${store.pixabayFilterCount} 项筛选` : '筛选条件'"
          @click="toggleFilterPanel"
        >
          <NIcon :component="SlidersHorizontal" :size="13" />
          筛选
          <span
            v-if="store.pixabayFilterCount"
            class="filter-badge flex items-center justify-center rounded-full"
          >{{ store.pixabayFilterCount }}</span>
        </button>
      </div>

      <!-- Wallhaven 工具条：搜索框 + 筛选。
           **没有媒体分段** —— Wallhaven 只有图片，摆一个"图片/视频"切换等于说谎。
           宽度预算：左栏可用 ≈394px，去掉内边距 24 + 筛选按钮 54 + gap 4
           ⇒ 搜索框 flex-1 拿到约 312px，比 Pixabay 那条宽松（它还要塞分段与分类）。 -->
      <div
        v-else-if="store.showWallhavenTools"
        ref="filterBar"
        class="filter-bar flex flex-none items-center gap-1 px-3 py-2"
      >
        <NInput
          :value="store.wallhavenQuery"
          size="small"
          class="search-input"
          placeholder="搜索…（支持 tag:xxx）"
          clearable
          @update:value="onWallhavenQueryInput"
        >
          <template #prefix>
            <NIcon :component="Search" :size="13" />
          </template>
        </NInput>
        <button
          ref="filterBtn"
          class="filter-btn flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-[12px] transition-colors"
          :class="
            filterOpen || store.wallhavenFilterCount
              ? 'bg-accent-soft text-accent'
              : 'text-dim hover:text-tx'
          "
          :title="store.wallhavenFilterCount ? `已启用 ${store.wallhavenFilterCount} 项筛选` : '筛选条件'"
          @click="toggleFilterPanel"
        >
          <NIcon :component="SlidersHorizontal" :size="13" />
          筛选
          <span
            v-if="store.wallhavenFilterCount"
            class="filter-badge flex items-center justify-center rounded-full"
          >{{ store.wallhavenFilterCount }}</span>
        </button>
      </div>

      <!-- 首次加载 loading 层：allCount 已拉取但 gridItems 尚未渲染时展示。
           覆盖范围排除上方本地工具条（工具条是常驻控件，加载中也应保持可用）：
           用 flex 兄弟关系而非硬编码 top 值，工具条高度变化时无需同步修改 -->
      <div
        class="loading-overlay absolute inset-x-0 bottom-0 z-20 flex flex-col items-center justify-center"
        :style="{ top: store.showLocalTools || store.showPixabayTools || store.showWallhavenTools ? filterBarHeight + 'px' : '0px' }"
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
              <!-- 必应卡片「已下载」徽标：只出现在必应在线壁纸上（本地项不显示）。
                   判据是"本地壁纸目录里能不能扫到这张"（store.isBingDownloaded），
                   所以徽标亮着就一定能跳过去定位到，不会出现"标了却没有"。
                   点击跳「本地」选项卡并按其日期过滤定位 —— 在线列表只有最近 16 天，
                   超出窗口的老壁纸只能去本地找。 -->
              <button
                v-if="isOnlineDownloaded(item)"
                class="downloaded-badge absolute left-1.5 top-1.5 z-[6] flex cursor-pointer items-center gap-1 rounded-md border border-white/10 bg-black/55 px-1.5 py-[2px] backdrop-blur-[2px] transition-colors duration-200 hover:border-white/25 hover:bg-black/70"
                title="已下载到本地 · 点击在「本地」选项卡中查看"
                @click.stop="onOpenDownloaded(item)"
              >
                <NIcon :component="Check" :size="10" class="text-ok" />
                <span class="text-[9.5px] leading-none text-white/85">已下载</span>
              </button>
              <!-- 必应卡片「下载」：只把原图存到本地壁纸目录，**不改桌面**
                   （用户明确要的路径：必应我只想下载）。
                   放在右上角 —— 那正是收藏星标的位置，而必应项不显示星标，槽位空着。
                   hover 浮现（动作类控件的既有约定，同收藏星标）；
                   **选中态也常显** —— 点卡片会选中并预览，那正是用户在决定"要不要这张"
                   的时刻，不该让按钮还藏着。
                   已下载时这个槽位让给左上角的「已下载」徽标，两者互斥不重叠。 -->
              <button
                v-if="(item.kind === 'bing' || item.kind === 'pixabay' || item.kind === 'wallhaven') && !isOnlineDownloaded(item)"
                class="online-dl absolute right-1.5 top-1.5 z-[6] flex cursor-pointer items-center gap-1 rounded-md border border-white/10 bg-black/45 px-1.5 py-[2px] opacity-0 backdrop-blur-[2px] transition-opacity duration-200 group-hover:opacity-100 hover:border-white/25 hover:bg-black/65"
                :style="isOnlineDownloading(item) || isSelected(item) ? { opacity: 1 } : undefined"
                :disabled="!!store.bingDownloadingDate || !!store.pixabayDownloadingId || !!store.wallhavenDownloadingId"
                :title="'下载到本地壁纸目录（不会改变桌面）'"
                @click.stop="onDownloadOnline(item)"
              >
                <NIcon
                  :component="Download"
                  :size="10"
                  :class="isOnlineDownloading(item) ? 'text-accent' : 'text-white/75'"
                />
                <span class="text-[9.5px] leading-none text-white/85">
                  {{ isOnlineDownloading(item) ? "下载中" : "下载" }}
                </span>
              </button>
              <!-- 视频时长角标：**常显**（本地视频与 Pixabay 在线视频共用）。
                   它和署名不一样 —— 署名是归属信息（hover 才出即可），
                   时长是"这条内容是什么"的实质信息，必须一眼看得到，
                   否则用户点开才发现是段 3 秒的短片，白等一次预览。
                   放左下角：右下角留给署名，右上角是下载按钮，左上角是「已下载」。
                   本地视频的时长由后端探测（Media Foundation 元数据，不解码）
                   落 `.dur` 边车文件，再经 `thumbnail-updated` 事件补进来；
                   探测完成前 `duration` 是 undefined ⇒ **不渲染**，
                   而不是显示 `0:00`（假数据比没有更糟）。 -->
              <span
                v-if="
                  (item.kind === 'video' || item.kind === 'pixabay-video') &&
                  item.duration
                "
                class="video-dur absolute bottom-2 left-2 z-[5] flex items-center gap-1 rounded bg-black/60 px-1.5 py-1 backdrop-blur-[2px]"
                :title="`视频时长 ${formatDuration(item.duration)}`"
              >
                <NIcon :component="Play" :size="9" class="shrink-0 text-white/85" />
                <span class="font-mono text-[9.5px] leading-none text-white/85">
                  {{ formatDuration(item.duration) }}
                </span>
              </span>
              <!-- Pixabay 署名：**hover 才出现，不常显**，位置在**缩略图右下角**。
                   依据（已核对官方原文）：Content License 明确写着 "Use Content without
                   having to attribute the author"，所以署名**不是合规项**；API Terms 要求的
                   只是"标明图片来自哪里"，那由页脚常显的「Pixabay：…」和选项卡名承担。
                   留它纯粹是体验：看到喜欢的图能点回作者主页找更多。
                   ① 放缩略图上而不是标题行 —— 标题行是所有来源共用的，为它留位置会把
                      本地/必应的长文件名一起截短（踩过）。
                   ② 做成右下角小角标，而不是底部通栏渐变 —— 通栏会压住缩略图下缘，
                      紧挨着下面的标题，视觉上像"两行标题"（用户否过）。
                   ③ ⚠️ **必须留在这个 `.thumb-holder` 里面**。绝对定位的基准是最近的
                      定位祖先：放在卡片里就是整张卡片（182px，含 150px 图 + 32px 标题行），
                      `bottom-2` 会直接落到标题上（踩过，实测压住标题 22px）。
                      放在 `.thumb-holder`（150px、`relative` + `overflow-hidden`）里，
                      才是"图片右下角"，且超出部分自动裁掉。 -->
              <button
                v-if="
                  (item.kind === 'pixabay' || item.kind === 'pixabay-video') &&
                  item.author
                "
                class="pixabay-credit absolute bottom-2 right-2 z-[5] flex max-w-[calc(100%_-_16px)] items-center rounded bg-black/60 px-1.5 py-1 text-left opacity-0 backdrop-blur-[2px] transition-opacity duration-200 group-hover:opacity-100"
                :class="item.authorUrl ? 'cursor-pointer' : 'cursor-default'"
                :title="
                  item.authorUrl
                    ? `作者：${item.author} · 点击在浏览器中打开其 Pixabay 主页`
                    : `作者：${item.author}`
                "
                @click.stop="onOpenAuthor(item)"
              >
                <span class="truncate text-[9.5px] leading-none text-white/85">
                  {{ item.author }}
                </span>
              </button>
            </div>
            <!-- 收藏星标：已收藏常显琥珀色，未收藏 hover 浮现；点击切换收藏
                （必应在线壁纸为远程 URL，不参与收藏夹书签体系，不显示星标） -->
            <button
              v-if="
                item.kind !== 'bing' &&
                item.kind !== 'pixabay' &&
                item.kind !== 'pixabay-video' &&
                item.kind !== 'wallhaven'
              "
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
              <!-- 卡片宽度自适应，标题长了会截断 —— 挂原生 title，鼠标悬停看全名。
                   可见文字与 title 同源（itemLabel），不会出现"看到的和提示的不一样"。
                   ⚠️ 只加 `min-w-0`（修 flex item 默认 min-width:auto 导致长标题溢出容器
                   而不是出省略号），**不要加任何 max-width** —— 这是所有来源共用的标题行，
                   给 Pixabay 署名留位置而限制宽度，会连带把本地/必应的长文件名截短。 -->
              <span
                class="name min-w-0 truncate text-[11.5px] text-dim"
                :title="itemLabel(item)"
              >
                {{ itemLabel(item) }}
              </span>
              <!-- 状态点：绿=当前壁纸(已设置到桌面/正在播放)，蓝=正在预览(选中)。
                   `ml-auto` 把它顶到最右，中间空余全归它。 -->
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
          <!-- Pixabay 还没配 key：就地给填写入口，不逼用户去设置里翻 -->
          <template v-if="store.source === 'pixabay' && !store.pixabayKey">
            <p class="text-[13px] text-dim">需要 Pixabay API key</p>
            <p class="empty-sub mt-1 max-w-[300px] text-[11.5px] leading-relaxed text-faint">
              在 pixabay.com 免费注册即可获取，key 只保存在本机，不会上传。
            </p>
            <div class="mt-3 flex w-full max-w-[260px] items-center gap-1.5 px-2">
              <NInput
                v-model:value="pixabayKeyInput"
                size="small"
                placeholder="粘贴 API key"
                @keyup.enter="savePixabayKey"
              />
              <button
                class="shrink-0 cursor-pointer rounded-md border border-accent/40 bg-accent-soft px-2.5 py-1 text-[11.5px] text-accent transition-colors hover:bg-accent/20"
                @click="savePixabayKey"
              >
                保存
              </button>
            </div>
          </template>
          <template v-else>
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
                            : store.source === "pixabay"
                              ? store.pixabayMedia === "video"
                                ? "Pixabay 没有匹配的视频：换个关键词，或点「筛选」放宽条件"
                                : store.pixabayFilterCount
                                  ? "当前筛选条件下没有结果：点「筛选」把条件放宽，或直接重置"
                                  : "Pixabay 没有匹配结果：换个关键词试试"
                              : store.source === "wallhaven"
                                ? // 连不上 ≠ 没结果：这两种情况在空态里必须分开，
                                  // 否则用户会一直换关键词，而真正的原因在网络
                                  store.wallhavenError
                                  ? store.wallhavenError
                                  : store.wallhavenFilterCount
                                    ? "当前筛选条件下没有结果：点「筛选」把条件放宽，或直接重置"
                                    : "Wallhaven 没有匹配结果：换个关键词试试"
                                : "此目录下暂无图片或视频，可到设置中更换壁纸目录"
              }}
            </p>
            <!-- 网络失败时给一个就地重试：用户不该为了再试一次去切页签 -->
            <div
              v-if="store.source === 'wallhaven' && store.wallhavenError"
              class="mt-3"
            >
              <button
                class="cursor-pointer rounded-md border border-accent/40 bg-accent-soft px-3 py-1 text-[11.5px] text-accent transition-colors hover:bg-accent/20 disabled:opacity-50"
                :disabled="store.wallhavenLoading"
                @click="store.reloadWallhaven()"
              >
                {{ store.wallhavenLoading ? "重试中…" : "重试" }}
              </button>
            </div>
          </template>
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
        <!-- Pixabay 的排序挂在同一位置：页脚整行本来就空着，
             而顶部那行要留给搜索框（分类已经占掉 104px） -->
        <NSelect
          v-else-if="store.showPixabayTools"
          :value="store.pixabayOrder"
          size="tiny"
          class="sort-select"
          :options="PIXABAY_ORDER_OPTIONS"
          @update:value="(v: string) => store.setPixabayOrder(v)"
        />
        <!-- Wallhaven 排序同样挂页脚 -->
        <NSelect
          v-else-if="store.showWallhavenTools"
          :value="store.wallhavenSort"
          size="tiny"
          class="sort-select"
          :options="WALLHAVEN_SORT_OPTIONS"
          @update:value="(v: WallhavenSort) => store.setWallhavenSort(v)"
        />
      </div>
    </div>

    <!-- Pixabay 筛选浮层。
         必须 Teleport 到 body —— 留在 aside 里会被 `.app-shell` 的
         overflow-hidden 裁掉（与 ContextMenu 同一套做法、同一个理由）。
         位置/最大高度在 `toggleFilterPanel()` 里按按钮实测，不写死。 -->
    <Teleport to="body">
      <Transition name="filter-pop">
        <div
          v-if="filterOpen && (store.showPixabayTools || store.showWallhavenTools)"
          ref="filterPanelEl"
          class="filter-panel app-no-drag fixed z-[88] overflow-y-auto rounded-xl border border-line-2 bg-[rgba(24,31,45,0.97)] p-2.5 shadow-[0_14px_36px_rgba(0,0,0,0.45)] backdrop-blur-xl"
          :style="filterPanelStyle"
          @mousedown.stop
        >
          <!-- Wallhaven 筛选：分类 / 分辨率 / 比例 / 颜色。
               全部是接口原生参数（分类是 3 位位掩码，颜色是十六进制），
               排序不在这里 —— 它挂在页脚，与 Pixabay 同一位置。 -->
          <template v-if="store.showWallhavenTools">
            <div class="fp-row">
              <span class="fp-label">分类</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.wallhavenCategory"
                :options="WALLHAVEN_CATEGORY_OPTIONS"
                @update:value="(v: WallhavenCategory) => store.setWallhavenCategory(v)"
              />
            </div>
            <div class="fp-row">
              <span class="fp-label">分辨率</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.wallhavenAtleast"
                :options="WALLHAVEN_ATLEAST_OPTIONS"
                @update:value="(v: WallhavenAtleast) => store.setWallhavenAtleast(v)"
              />
            </div>
            <div class="fp-row">
              <span class="fp-label">比例</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.wallhavenRatio"
                :options="WALLHAVEN_RATIO_OPTIONS"
                @update:value="(v: WallhavenRatio) => store.setWallhavenRatio(v)"
              />
            </div>

            <!-- 颜色：单选（接口 `colors` 只吃一个十六进制值）。
                 12 个色块排 6×2，每格 20px + 6px gap = 150px，塞得进面板。 -->
            <div class="fp-col">
              <span class="fp-label">颜色</span>
              <div class="fp-swatches mt-1.5 grid grid-cols-6 gap-1.5">
                <button
                  v-for="c in WALLHAVEN_COLORS"
                  :key="c.value"
                  class="color-dot cursor-pointer"
                  :class="{ 'is-on': store.wallhavenColor === c.value }"
                  :style="{ background: c.hex }"
                  :title="c.label"
                  @click="store.setWallhavenColor(c.value)"
                />
              </div>
            </div>

            <div class="fp-foot mt-2 flex items-center justify-between border-t border-line pt-2">
              <span class="text-[10.5px] text-faint">
                {{ store.wallhavenFilterCount ? `已启用 ${store.wallhavenFilterCount} 项` : "改动即时生效" }}
              </span>
              <button
                class="fp-reset shrink-0 cursor-pointer rounded px-1.5 py-0.5 text-[11px] text-dim transition-colors hover:bg-white/10 hover:text-tx"
                @click="store.resetWallhavenFilters()"
              >
                重置
              </button>
            </div>
          </template>

          <template v-else>
          <!-- 分类：图片与视频共用 -->
          <div class="fp-row">
            <span class="fp-label">分类</span>
            <NSelect
              size="small"
              class="fp-select"
              :menu-props="FILTER_MENU_PROPS"
              :value="store.pixabayCategory"
              :options="PIXABAY_CATEGORY_OPTIONS"
              @update:value="(v: string) => store.setPixabayCategory(v)"
            />
          </div>

          <!-- 视频模式：视频类型 + 方向 + 尺寸。
               内容类型 / 颜色这两项**视频接口根本不支持**（传了静默忽略），
               所以不渲染 —— 摆出来一个改了没反应的控件比不摆更糟。
               方向则是**后端按宽高本地筛**的（接口没有 orientation 参数），
               所以它在这里是真的有用，只是生效路径与图片不同。 -->
          <template v-if="store.pixabayMedia === 'video'">
            <div class="fp-row">
              <span class="fp-label">视频类型</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.pixabayVideoType"
                :options="VIDEO_TYPE_OPTIONS"
                @update:value="(v: PixabayVideoType) => store.setPixabayVideoType(v)"
              />
            </div>
            <div class="fp-row">
              <span class="fp-label">方向</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.pixabayVideoOrientation"
                :options="ORIENTATION_OPTIONS"
                @update:value="(v: PixabayOrientation) => store.setPixabayVideoOrientation(v)"
              />
            </div>
          </template>

          <!-- 图片模式：内容类型 / 方向 / 尺寸 / 颜色 -->
          <template v-else>
            <div class="fp-row">
              <span class="fp-label">内容类型</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.pixabayImageType"
                :options="IMAGE_TYPE_OPTIONS"
                @update:value="(v: PixabayImageType) => store.setPixabayImageType(v)"
              />
            </div>
            <div class="fp-row">
              <span class="fp-label">方向</span>
              <NSelect
                size="small"
                class="fp-select"
                :menu-props="FILTER_MENU_PROPS"
                :value="store.pixabayOrientation"
                :options="ORIENTATION_OPTIONS"
                @update:value="(v: PixabayOrientation) => store.setPixabayOrientation(v)"
              />
            </div>
          </template>

          <div class="fp-row">
            <span class="fp-label">尺寸</span>
            <NSelect
              size="small"
              class="fp-select"
              :menu-props="FILTER_MENU_PROPS"
              :value="store.pixabaySize"
              :options="SIZE_OPTIONS"
              @update:value="(v: PixabaySize) => store.setPixabaySize(v)"
            />
          </div>

          <!-- 颜色：多选色块。14 个色块排 7×2，每格 20px + 6px gap = 176px，
               正好塞进面板 268 - 20(内边距) - 52(标签) - 8(gap) = 188px。 -->
          <div v-if="store.pixabayMedia !== 'video'" class="fp-col">
            <span class="fp-label">颜色</span>
            <div class="fp-swatches mt-1.5 grid grid-cols-7 gap-1.5">
              <button
                v-for="c in PIXABAY_COLORS"
                :key="c.value"
                class="color-dot cursor-pointer"
                :class="{ 'is-on': store.pixabayColors.includes(c.value) }"
                :style="{ background: colorBg(c.value, c.hex) }"
                :title="c.label"
                @click="toggleColor(c.value)"
              />
            </div>
          </div>

          <div class="fp-foot mt-2 flex items-center justify-between border-t border-line pt-2">
            <span class="text-[10.5px] text-faint">
              {{ store.pixabayFilterCount ? `已启用 ${store.pixabayFilterCount} 项` : "改动即时生效" }}
            </span>
            <button
              class="fp-reset shrink-0 cursor-pointer rounded px-1.5 py-0.5 text-[11px] text-dim transition-colors hover:bg-white/10 hover:text-tx"
              @click="store.resetPixabayFilters()"
            >
              重置
            </button>
          </div>
          </template>
        </div>
      </Transition>
    </Teleport>
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

/* Pixabay 筛选按钮上的角标：只统计偏离默认值的筛选数。
   宽度随位数自适应（min-width 兜住一位数），字号比正文更小 —— 它是个提示，
   不是主信息，抢眼会盖过"筛选"二字。 */
.filter-badge {
  min-width: 14px;
  height: 14px;
  padding: 0 3px;
  font-size: 9.5px;
  font-weight: 600;
  line-height: 1;
  color: #0b1220;
  background: var(--color-accent);
}

/* 筛选浮层。宽度**必须与 `PANEL_WIDTH` 常量一致**（定位靠它算左边界），
   改一个不改另一个会让面板从右侧溢出屏幕。 */
.filter-panel {
  width: 268px;
}

/* 一行 = 左侧标签 + 右侧控件。
   标签定宽 52px：最长是「内容类型」「视频类型」= 4 字 × 11px ≈ 44px，
   留 8px 余量。控件 flex-1 拿到 268 - 20(面板内边距) - 52 - 8(gap) = 188px。 */
.fp-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 6px;
}

.fp-label {
  flex: 0 0 auto;
  width: 52px;
  font-size: 11px;
  color: var(--color-dim);
}

/* NSelect 内部输入框有基础宽度、不跟随外层收缩，必须显式撑满 */
.fp-select {
  flex: 1 1 auto;
  min-width: 0;
}
.fp-select :deep(.n-base-selection) {
  width: 100%;
}

/* 颜色色块：20px 圆点。选中用 2px 主色描边 + 留白，
   不改变尺寸（否则选中瞬间会顶动周围色块）。 */
.color-dot {
  width: 20px;
  height: 20px;
  border: 1px solid rgb(255 255 255 / 0.16);
  border-radius: 9999px;
  transition:
    box-shadow 0.15s ease,
    transform 0.15s ease;
}
.color-dot:hover {
  transform: scale(1.1);
}
.color-dot.is-on {
  box-shadow:
    0 0 0 2px rgba(24, 31, 45, 1),
    0 0 0 4px var(--color-accent);
}

.filter-pop-enter-active,
.filter-pop-leave-active {
  transition:
    opacity 0.12s ease,
    transform 0.12s ease;
  transform-origin: top right;
}
.filter-pop-enter-from,
.filter-pop-leave-to {
  opacity: 0;
  transform: scale(0.97) translateY(-2px);
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
   各段宽度由文字长度自然决定，不做等宽处理。
   9px 而不是 12px：Pixabay 那条从两段变三段后，800px 窗口下搜索框内部
   只剩 49px（约 3.5 个中文字），已经没法输入。收紧到 9px 后每段 42px、
   三段共省 18px，搜索框回到约 87px。本地那条（全部/图片/视频）同时受益。 */
.segmented-item {
  padding: 4px 9px;
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
