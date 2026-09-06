<script setup lang="ts">
// CurrentPanel - 右栏：壁纸预览与设置
// 交互模型：
//   - 点击左侧卡片 -> store.selectItem（蓝框选中，仅预览，桌面不变）
//   - 右侧大预览始终展示"正在预览"的图（无选中时回退当前桌面壁纸）
//   - 点击"设为壁纸"才真正写入桌面（绿框 currentWallpaper 随之更新）
//   - 预览样式下拉可即时预览；设为壁纸时若与系统样式不同会同步写注册表
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { NButton, NIcon, NSelect } from "naive-ui";
import { Check, ChevronLeft, ChevronRight, LayoutGrid, Maximize, X } from "lucide-vue-next";
import {
  baseName,
  displaySrc,
  useWallpaperStore,
  type WallpaperItem,
} from "../stores/wallpaper";
import { invoke } from "@tauri-apps/api/core";
import { currentMonitor } from "@tauri-apps/api/window";

const store = useWallpaperStore();

const imgError = ref(false);

// ---------- 壁纸样式选项（与 Windows 壁纸模式一一对应） ----------
const STYLE_OPTIONS = [
  { key: "fill", label: "填充", style: 10, tile: false },
  { key: "fit", label: "适应", style: 6, tile: false },
  { key: "stretch", label: "拉伸", style: 22, tile: false },
  { key: "center", label: "居中", style: 0, tile: false },
  { key: "tile", label: "平铺", style: 10, tile: true },
] as const;
type StyleKey = (typeof STYLE_OPTIONS)[number]["key"];

// NSelect 选项：直接复用样式选项表
const styleOptions = STYLE_OPTIONS.map((o) => ({
  label: o.label,
  value: o.key,
}));

function styleKeyOf(s: { style: number; tile: boolean } | null): StyleKey {
  if (!s) return "fill";
  if (s.tile) return "tile";
  switch (s.style) {
    case 6: return "fit";
    case 22: return "stretch";
    case 0: return "center";
    default: return "fill";
  }
}

// 当前预览选用的样式：默认跟随系统桌面样式，用户可下拉切换（仅影响预览渲染；
// 真正写入桌面发生在点击"设为壁纸"且与系统不一致时）
const styleKey = ref<StyleKey>("fill");
watch(
  () => store.desktopStyle,
  (s) => {
    if (s) styleKey.value = styleKeyOf(s);
  },
  { immediate: true }
);
const selectedStyle = computed(() =>
  STYLE_OPTIONS.find((o) => o.key === styleKey.value)!
);
const styleText = computed(() => selectedStyle.value.label);

// ---------- 预览目标与图源 ----------
// 大预览展示"正在预览"（无选中时回退当前桌面壁纸）
const previewTarget = computed(() => store.previewTarget);
const previewSrc = computed(() =>
  previewTarget.value ? displaySrc(previewTarget.value) : ""
);

// ---------- 屏幕信息：真实屏幕宽高比 + 缩放 ----------
const screenInfo = ref<{
  logicalWidth: number;
  logicalHeight: number;
  scale: number;
} | null>(null);

async function loadScreenInfo() {
  let loaded = false;
  try {
    const screen = await invoke<Record<string, any>>("get_desktop_screen");
    screenInfo.value = {
      logicalWidth: Number(screen.logical_width),
      logicalHeight: Number(screen.logical_height),
      scale: Number(screen.scale_factor),
    };
    loaded = true;
  } catch (e) {
    console.error("加载屏幕信息失败，尝试 currentMonitor 降级:", e);
  }
  if (!loaded) {
    try {
      const mon = await currentMonitor();
      if (mon) {
        const { size, scaleFactor } = mon;
        screenInfo.value = {
          logicalWidth: size.width / scaleFactor,
          logicalHeight: size.height / scaleFactor,
          scale: scaleFactor,
        };
      } else {
        screenInfo.value = null;
      }
    } catch (err) {
      console.error("currentMonitor 降级失败，按 16:9 预览:", err);
      screenInfo.value = null;
    }
  }
  scheduleLayout();
}

const screenRatio = computed(() => {
  const s = screenInfo.value;
  if (s && s.logicalWidth > 0 && s.logicalHeight > 0) {
    return s.logicalWidth / s.logicalHeight;
  }
  return 16 / 9;
});

// ---------- 模拟屏尺寸：ResizeObserver 随容器自适应 ----------
const previewRef = ref<HTMLElement | null>(null);
const monitorSize = ref<{ width: number; height: number } | null>(null);

const PAD = 16; // 模拟屏四周留白
const BADGE_RESERVE = 46; // 顶部为右上角样式角标预留空间

let ro: ResizeObserver | null = null;
let rafId = 0;

function scheduleLayout() {
  if (rafId) cancelAnimationFrame(rafId);
  rafId = requestAnimationFrame(() => {
    rafId = 0;
    computeMonitorSize();
  });
}

// 在容器内计算「最大贴合」的模拟屏像素尺寸（保持宽高比，顶部预留角标位）
function computeMonitorSize() {
  const el = previewRef.value;
  if (!el) return;
  const availW = Math.max(40, el.clientWidth - PAD * 2);
  const availH = Math.max(40, el.clientHeight - PAD - BADGE_RESERVE);
  const ratio = screenRatio.value;
  let w = availW;
  let h = w / ratio;
  if (h > availH) {
    h = availH;
    w = h * ratio;
  }
  monitorSize.value = { width: Math.floor(w), height: Math.floor(h) };
}

const mockScreenStyle = computed(() => {
  const m = monitorSize.value;
  if (!m) return { width: "100%", height: "100%" };
  return { width: `${m.width}px`, height: `${m.height}px` };
});

// 模拟屏过窄时隐藏任务栏内容
const isTaskbarNarrow = computed(() => {
  const m = monitorSize.value;
  return !m || m.width < 220;
});

// 任务栏高度：约屏高 5%，限制在 14~26px
const taskbarStyle = computed(() => {
  const m = monitorSize.value;
  if (!m) return { height: "18px" };
  const h = Math.min(26, Math.max(14, Math.round(m.height * 0.05)));
  return { height: `${h}px` };
});

// ---------- 模拟任务栏时钟 ----------
const taskbarTime = ref("");
let clockTimer: number | undefined;

function tickClock() {
  const d = new Date();
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  taskbarTime.value = `${hh}:${mm}`;
}

function startClock() {
  tickClock();
  clockTimer = window.setInterval(tickClock, 30_000);
}

// ---------- 背景渲染：按壁纸模式映射 CSS background（对应 Windows 桌面实际效果） ----------
function styleToBg(style: { style: number; tile: boolean }, src: string) {
  let size = "cover";
  let repeat = "no-repeat";
  if (style.tile) {
    size = "auto"; // 平铺：原始尺寸重复
    repeat = "repeat";
  } else {
    switch (style.style) {
      case 0: size = "auto"; repeat = "no-repeat"; break; // 居中
      case 6: size = "contain"; repeat = "no-repeat"; break; // 适应
      case 10: size = "cover"; repeat = "no-repeat"; break; // 填充
      case 22: size = "100% 100%"; repeat = "no-repeat"; break; // 拉伸
      default: size = "cover"; repeat = "no-repeat"; break;
    }
  }
  return {
    backgroundImage: src ? `url("${src}")` : "none",
    backgroundSize: size,
    backgroundRepeat: repeat,
  };
}


// ---------- 快速切换（左右箭头）+ 淡入淡出过渡 ----------
const animPhase = ref<"fade-out" | "fade-in" | null>(null);
const animOpacity = ref(1);
let switchCooldown = false;

function quickSwitch(dir: "left" | "right") {
  if (animPhase.value || switchCooldown) return;
  const items = store.gridItems;
  const src = previewSrc.value;
  if (!src || items.length === 0) return;

  const currentIdx = items.findIndex((it) => previewSrc.value === displaySrc(it));
  let idx = currentIdx >= 0 ? currentIdx : (dir === "right" ? 0 : items.length - 1);
  let nextIdx = dir === "right" ? (idx + 1) % items.length : (idx - 1 + items.length) % items.length;
  const nextItem = items[nextIdx];
  const nextSrc = nextItem ? displaySrc(nextItem) : "";
  if (!nextSrc || nextSrc === src) return;

  // 先淡出
  switchCooldown = true;
  animPhase.value = "fade-out";
  animOpacity.value = 1;

  const fadeOutAnim = requestAnimationFrame(() => {
    animOpacity.value = 0;
    setTimeout(() => {
      // 切换
      store.selectItem(nextItem);
      animPhase.value = "fade-in";
      const fadeInAnim = requestAnimationFrame(() => {
        animOpacity.value = 1;
        setTimeout(() => {
          animPhase.value = null;
          switchCooldown = false;
        }, 280);
      });
      setTimeout(() => cancelAnimationFrame(fadeInAnim), 280);
    }, 280);
  });
  setTimeout(() => cancelAnimationFrame(fadeOutAnim), 280);
}

function getTransitionBg(style: { style: number; tile: boolean }, src: string): Record<string, string> {
  const bg = styleToBg(style, src);
  return { ...bg, opacity: animOpacity.value.toString() };
}

// ---------- 键盘快捷键：← → 切换壁纸 ----------
function onKey(e: KeyboardEvent) {
  if (e.key === "ArrowLeft") quickSwitch("left");
  if (e.key === "ArrowRight") quickSwitch("right");
}
const wallpaperBg = computed(() =>
  styleToBg(
    { style: selectedStyle.value.style, tile: selectedStyle.value.tile },
    previewSrc.value && !imgError.value ? previewSrc.value : ""
  )
);

// 图片可用性探测（new Image）：成功则渲染背景，失败展示占位
let probeSeq = 0;
watch(
  previewSrc,
  (src) => {
    imgError.value = false;
    if (!src) return;
    const seq = ++probeSeq;
    const probe = new Image();
    probe.onload = () => {
      if (seq === probeSeq) imgError.value = false;
    };
    probe.onerror = () => {
      if (seq === probeSeq) imgError.value = true;
    };
    probe.src = src;
  },
  { immediate: true }
);

// ---------- 名称/提示 ----------
const previewName = computed(() => {
  const t = previewTarget.value;
  if (!t) return "";
  return t.title || baseName(t.path || "") || "当前壁纸";
});
const previewPath = computed(() => previewTarget.value?.path || "");

// 路径文件扩展名徽标（如 .png / .jpg）
const previewNameType = computed(() => {
  const p = previewPath.value;
  if (!p) return "";
  const ext = p.split(".").pop();
  return ext && ext.length <= 6 ? `.${ext.toLowerCase()}` : "";
});

const currentName = computed(() => {
  const c = store.currentWallpaper;
  if (!c) return "未获取";
  return c.title || baseName(c.path || "") || "当前壁纸";
});

// 预览目标是否为已设置到桌面的当前壁纸（用于状态点颜色与文案）
const previewIsCurrent = computed(() => {
  const p = previewTarget.value;
  const c = store.currentWallpaper;
  return !!p && !!c && p.path === c.path;
});

const resInfo = computed(() => {
  const s = screenInfo.value;
  if (!s) return "--";
  return `${Math.round(s.logicalWidth)} × ${Math.round(s.logicalHeight)} · ${Math.round(s.scale * 100)}%`;
});

// 预览容器右键：作用于"正在预览"对象（在网格中的可删除；纯桌面项只读设壁纸）
function onPreviewContext(e: MouseEvent) {
  const t = previewTarget.value;
  if (!t || !t.path) return;
  e.preventDefault();
  const fromGrid = store.gridItems.some((it) => it.path === t.path);
  const item: WallpaperItem = {
    key: "preview_ctx",
    kind: fromGrid ? "local" : "current",
    path: t.path,
    title: t.title || baseName(t.path),
  };
  store.openContextMenu(item, e.clientX, e.clientY);
}

// ---------- 放大预览（全屏浮层） ----------
const zoomVisible = ref(false);

function openZoom() {
  if (!previewSrc.value) return;
  zoomVisible.value = true;
}

function closeZoom() {
  zoomVisible.value = false;
}

function onZoomKey(e: KeyboardEvent) {
  if (e.key === "Escape") closeZoom();
}

watch(zoomVisible, (v) => {
  if (v) window.addEventListener("keydown", onZoomKey);
  else window.removeEventListener("keydown", onZoomKey);
});

// zoom 浮层背景改由模板内 .zoom-bg 层承载（复用 getTransitionBg 淡入淡出，
// 使放大态左右切换与普通预览保持一致）

// ---------- 设为壁纸 ----------
function onApplyAsDesktop() {
  void store.applyPreviewAsDesktop({
    style: selectedStyle.value.style,
    tile: selectedStyle.value.tile,
  });
}

onMounted(() => {
  loadScreenInfo();
  ro = new ResizeObserver(() => scheduleLayout());
  if (previewRef.value) ro.observe(previewRef.value);
  computeMonitorSize();
  startClock();
  window.addEventListener("keydown", onKey);
});

onUnmounted(() => {
  ro?.disconnect();
  if (clockTimer !== undefined) window.clearInterval(clockTimer);
  if (rafId) cancelAnimationFrame(rafId);
  window.removeEventListener("keydown", onZoomKey);
  window.removeEventListener("keydown", onKey);
});
</script>

<template>
  <main class="panel-right flex min-w-0 flex-1 flex-col gap-2">

    <!-- 预览区域 -->
    <div
      ref="previewRef"
      class="preview-container relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-xl border border-line bg-panel/55"
      @contextmenu="onPreviewContext"
    >
      <!-- 样式角标 -->
      <div
          v-if="previewTarget && !imgError"
          class="style-badge absolute right-2 top-2 flex items-center gap-1 rounded-lg bg-black/60 px-2 py-1"
      >
        <NIcon :component="LayoutGrid" :size="12" class="text-accent" />
        <span class="text-[10.5px] text-white/80">{{ styleText }}</span>
      </div>

      <!-- 放大预览按钮 -->
      <button
          class="zoom-btn absolute bottom-2 right-2  flex h-7 w-7 items-center justify-center rounded-lg border border-white/10 bg-black/45 text-white/85 transition-colors hover:bg-black/65"
          title="放大预览（Esc 关闭）"
          @click="openZoom"
      >
        <NIcon :component="Maximize" :size="13" />
      </button>
      <!-- 空态 -->
      <div v-if="!previewTarget" class="preview-empty flex flex-col items-center text-center">
        <p class="text-[13px] text-dim">暂无壁纸预览</p>
        <p class="mt-1 text-[11px] text-faint">在左侧点击一张壁纸即可预览</p>
      </div>

      <!-- 图片加载失败占位 -->
      <div v-else-if="imgError" class="preview-empty flex flex-col items-center text-center">
        <p class="text-[12.5px] text-dim">预览不可用</p>
        <p class="mt-1 max-w-[85%] truncate text-[11px] text-faint" :title="previewPath">{{ previewPath }}</p>
      </div>

      <!-- 模拟屏 -->
      <div v-else-if="monitorSize" class="mock-screen" :style="mockScreenStyle">
        <!-- 壁纸层 -->
        <div class="mock-wall" :style="getTransitionBg(selectedStyle, previewSrc)"></div>

        <!-- 快速切换按钮 -->
        <button class="switch-btn switch-left" title="上一张 (←)" @click="quickSwitch('left')">
          <NIcon :component="ChevronLeft" />
        </button>
        <button class="switch-btn switch-right" title="下一张 (→)" @click="quickSwitch('right')">
          <NIcon :component="ChevronRight" />
        </button>

        <!-- 模拟任务栏 -->
        <div class="mock-taskbar" :class="{ narrow: isTaskbarNarrow }" :style="taskbarStyle">
          <div v-if="!isTaskbarNarrow" class="taskbar-inner">
            <div class="taskbar-start" title="开始">
              <span class="start-cell"></span>
              <span class="start-cell"></span>
              <span class="start-cell"></span>
              <span class="start-cell"></span>
            </div>
            <span class="taskbar-time">{{ taskbarTime }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- 底部控制卡：名称 + 样式 + 设壁纸 + 状态 -->
    <div class="dock-card flex flex-col gap-2 rounded-xl border border-line bg-panel/60 px-3 py-2.5">
      <!-- 工具行：显示样式 + 放大预览 -->
      <div class="flex items-center gap-2">
        <div class="style-label shrink-0 text-[10px] text-faint">显示样式</div>
        <NSelect
          v-model:value="styleKey"
          size="small"
          class="style-nselect min-w-0 flex-1"
          :options="styleOptions"
          title="预览样式；设为壁纸时若与桌面不同会同步应用"
        />
   
        <span class="ml-auto shrink-0 res-text rounded-md bg-panel-2 px-1.5 py-[3px] font-mono text-[10px] leading-none text-faint" title="屏幕分辨率与缩放">{{ resInfo }}</span>
      </div>

      <!-- 设为壁纸主按钮 -->
      <NButton
        type="primary"
        size="medium"
        class="apply-btn w-full"
        :disabled="!previewTarget || !previewTarget.path"
        :loading="store.isApplying"
        @click="onApplyAsDesktop"
      >
        <template #icon>
          <NIcon :component="Check" />
        </template>
        设为壁纸
      </NButton>

      <!-- 状态行 -->
      <div class="status-row flex items-center gap-4 border-t border-line/80 pt-2">
        <span class="flex min-w-0 items-center gap-1.5 text-[10.5px]">
          <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-ok/90"></span>
          <span class="shrink-0 text-faint">当前壁纸</span>
          <span class="min-w-0 truncate text-dim" :title="currentName + '（已设置到桌面）'">{{ currentName }}</span>
        </span>
        <span class="flex min-w-0 items-center gap-1.5 text-[10.5px]">
          <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent/90"></span>
          <span class="shrink-0 text-faint">预览</span>
          <span class="min-w-0 truncate text-tx" :title="previewName + '（当前选中，仅预览）'">{{ previewName || "—" }}</span>
        </span>
      </div>
    </div>

    <!-- 放大预览浮层（Esc 关闭） -->
    <Teleport to="body">
      <div
        v-if="zoomVisible"
        class="zoom-overlay fixed inset-0 z-[999] overflow-hidden"
        @click.self="closeZoom"
      >
        <!-- 壁纸背景层：独立承载背景图，切换时通过 animOpacity 淡入淡出 -->
        <div class="zoom-bg" :style="getTransitionBg(selectedStyle, previewSrc)"></div>
        <div class="zoom-mask pointer-events-none absolute inset-0 bg-black/30"></div>
        <button
          class="zoom-close absolute right-4 top-4 z-10 flex h-9 w-9 items-center justify-center rounded-full border border-white/15 bg-black/50 text-white/90 transition-colors hover:bg-black/70"
          title="关闭（Esc）"
          @click="closeZoom"
        >
          <NIcon :component="X" :size="18" />
        </button>
        <!-- 放大态左右切换（复用快速切换的淡入淡出） -->
        <button
          class="zoom-switch zoom-switch-left"
          title="上一张 (←)"
          @click="quickSwitch('left')"
        >
          <NIcon :component="ChevronLeft" :size="26" />
        </button>
        <button
          class="zoom-switch zoom-switch-right"
          title="下一张 (→)"
          @click="quickSwitch('right')"
        >
          <NIcon :component="ChevronRight" :size="26" />
        </button>
        <!-- 底部信息条 -->
        <div class="zoom-info absolute bottom-6 left-1/2 z-10 flex max-w-[80%] -translate-x-1/2 items-center gap-2 whitespace-nowrap rounded-full border border-white/10 bg-black/55 px-4 py-1.5 text-[12px] text-white/90 backdrop-blur">
          <span class="truncate">{{ previewName || "—" }}</span>
          <span class="h-3 w-px shrink-0 bg-white/15"></span>
          <span class="flex shrink-0 items-center gap-1 text-accent">
            <NIcon :component="LayoutGrid" :size="12" />
            {{ styleText }}
          </span>
        </div>
      </div>
    </Teleport>

  </main>
</template>


<style scoped>
.preview-container {
  position: relative;
  background-color: rgba(15, 20, 30, 0.5);
}

/* 模拟屏：显示器轮廓（带边框圆角与投影） */
.mock-screen {
  position: relative;
  flex: none;
  overflow: hidden;
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 10px;
  background: #000;
  box-shadow:
    0 12px 30px rgba(0, 0, 0, 0.45),
    inset 0 0 0 1px rgba(0, 0, 0, 0.55);
}

/* 壁纸层：铺满整个模拟屏，填充方式由 background-size/repeat 控制 */
.mock-wall {
  position: absolute;
  inset: 0;
  background-position: center center;
  transition: opacity 0.28s ease;
}

/* 模拟任务栏：半透明深色，覆盖屏幕底部 */
.mock-taskbar {
  position: absolute;
  right: 0;
  bottom: 0;
  left: 0;
  display: flex;
  align-items: center;
  background: rgba(16, 20, 28, 0.72);
  backdrop-filter: blur(8px);
  border-top: 1px solid rgba(255, 255, 255, 0.07);
}

/* 模拟屏过窄：任务栏保留细条，隐藏内容 */
.mock-taskbar.narrow {
  background: rgba(16, 20, 28, 0.45);
}
.mock-taskbar.narrow .taskbar-inner {
  display: none;
}

/* 快速切换按钮 */
.switch-btn {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 36px;
  height: 36px;
  border-radius: 50%;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(15, 20, 30, 0.65);
  backdrop-filter: blur(8px);
  color: rgba(255, 255, 255, 0.85);
  font-size: 20px;
  line-height: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s ease, transform 0.2s ease, background 0.2s ease;
  user-select: none;
}
.mock-screen:hover .switch-btn {
  opacity: 1;
}
.switch-left {
  left: 10px;
}
.switch-right {
  right: 10px;
}
.switch-btn:hover {
  background: rgba(25, 30, 45, 0.8);
  border-color: rgba(255, 255, 255, 0.25);
}
.switch-btn:active {
  transform: translateY(-50%) scale(0.92);
}

.taskbar-inner {
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: space-between;
  padding: 0 10px;
}

/* Windows 风格开始按钮：四宫格小窗 */
.taskbar-start {
  display: grid;
  grid-template-columns: repeat(2, 4px);
  gap: 1.5px;
  padding: 2px;
  border-radius: 3px;
  transition: background-color 0.15s ease;
}
.taskbar-start:hover {
  background: rgba(255, 255, 255, 0.12);
}
.start-cell {
  width: 4px;
  height: 4px;
  border-radius: 1px;
  background: rgba(255, 255, 255, 0.92);
}

.taskbar-time {
  font-size: 10px;
  color: rgba(255, 255, 255, 0.85);
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.3px;
  user-select: none;
}

.style-badge {
  backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.1);
}

/* 放大按钮与浮层 */
.zoom-btn {
  cursor: pointer;
}
.zoom-overlay {
  background: #05070c; /* 兜底底色：切换淡出时不透出下层应用 */
  animation: zoom-fade-in 0.18s ease;
}
/* 放大态壁纸背景层：独立承载背景图，opacity 过渡即左右切换淡入淡出 */
.zoom-bg {
  position: absolute;
  inset: 0;
  background-position: center center;
  transition: opacity 0.28s ease;
}
/* 放大态左右切换按钮 */
.zoom-switch {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  z-index: 10;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  border: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(15, 20, 30, 0.55);
  backdrop-filter: blur(8px);
  color: rgba(255, 255, 255, 0.9);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s ease, transform 0.2s ease, background 0.2s ease;
  user-select: none;
}
.zoom-overlay:hover .zoom-switch {
  opacity: 1;
}
.zoom-switch:hover {
  background: rgba(25, 30, 45, 0.8);
  border-color: rgba(255, 255, 255, 0.28);
}
.zoom-switch:active {
  transform: translateY(-50%) scale(0.92);
}
.zoom-switch-left {
  left: 18px;
}
.zoom-switch-right {
  right: 18px;
}
@keyframes zoom-fade-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
.zoom-close {
  cursor: pointer;
}
.zoom-info {
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
}

/* 底部控制卡 */
.dock-card {
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
  backdrop-filter: blur(8px);
}
.name-dot {
  box-shadow: 0 0 6px currentColor;
}
.status-row {
  min-width: 0;
}
.status-row > span {
  min-width: 0;
}

/* 预览样式下拉：由 Naive UI NSelect + 全局主题接管，此处仅保证弹性宽度 */
.style-nselect {
  flex: 1 1 auto;
  min-width: 0;
  max-width: 220px;
}
.style-nselect :deep(.n-base-selection) {
  width: 100%;
}
.style-nselect :deep(.n-base-selection-label) {
  font-size: 11.5px;
}

/* 分辨率/扩展名等小型信息徽标统一样式 */
.res-text {
  letter-spacing: 0.2px;
  font-variant-numeric: tabular-nums;
}
</style>
