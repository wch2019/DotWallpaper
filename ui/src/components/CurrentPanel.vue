<script setup lang="ts">
// CurrentPanel - 右栏：当前壁纸预览（按真实屏幕比例与壁纸模式渲染的「模拟屏」）+ 设为当前操作
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { NButton, NIcon } from "naive-ui";
import { MonitorDown, LayoutGrid, Monitor, Maximize } from "lucide-vue-next";
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

// 壁纸样式信息（style: 0=居中 6=适应 10=填充 22=拉伸；tile: 平铺优先）
const wallpaperStyle = ref<{ style: number; tile: boolean } | null>(null);
// 主屏幕逻辑分辨率与缩放；null 表示取不到，模拟屏按 16:9 兜底
const screenInfo = ref<{
  logicalWidth: number;
  logicalHeight: number;
  scale: number;
} | null>(null);

// 加载壁纸样式与屏幕信息
async function loadWallpaperInfo() {
  try {
    const style = await invoke<Record<string, any>>("get_wallpaper_style");
    wallpaperStyle.value = {
      style: style.style as number,
      tile: style.tile as boolean,
    };
  } catch (e) {
    console.error("加载壁纸样式失败:", e);
  }

  // 屏幕信息：优先 Rust 命令；失败降级 currentMonitor；再失败按 16:9
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
        screenInfo.value = null; // 无监视器：按 16:9 兜底
      }
    } catch (err) {
      console.error("currentMonitor 降级失败，按 16:9 预览:", err);
      screenInfo.value = null;
    }
  }

  scheduleLayout();
}

// 模拟屏宽高比：真实屏幕逻辑分辨率比例，缺省 16:9
const screenRatio = computed(() => {
  const s = screenInfo.value;
  if (s && s.logicalWidth > 0 && s.logicalHeight > 0) {
    return s.logicalWidth / s.logicalHeight;
  }
  return 16 / 9;
});

// ---- 模拟屏尺寸：ResizeObserver 随容器自适应 ----
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

// ---- 模拟任务栏时间 ----
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

// 壁纸样式文本
const styleText = computed(() => {
  if (!wallpaperStyle.value) return "";
  const { style, tile } = wallpaperStyle.value;
  if (tile) return "平铺";
  switch (style) {
    case 0: return "居中";
    case 6: return "适应";
    case 10: return "填充";
    case 22: return "拉伸";
    default: return "未知";
  }
});

// 分辨率和缩放显示
const resInfo = computed(() => {
  const s = screenInfo.value;
  if (!s) return "--";
  return `${Math.round(s.logicalWidth)} × ${Math.round(s.logicalHeight)} · ${Math.round(s.scale * 100)}%`;
});

const currentSrc = computed(() =>
  store.currentWallpaper ? displaySrc(store.currentWallpaper) : ""
);

const pathHint = computed(() => {
  if (!store.currentWallpaper) return "尚未获取当前壁纸";
  return store.currentWallpaper.path || "";
});

const nameHint = computed(() => {
  if (!store.currentWallpaper) return "";
  return (
    store.currentWallpaper.title ||
    baseName(store.currentWallpaper.path || "") ||
    "当前壁纸"
  );
});

// 背景渲染：按壁纸模式映射 CSS background（对应 Windows 桌面实际效果）
const wallpaperBg = computed(() => {
  let size = "cover";
  let repeat = "no-repeat";
  const s = wallpaperStyle.value;
  if (s) {
    if (s.tile) {
      size = "auto"; // 平铺：原始尺寸重复
      repeat = "repeat";
    } else {
      switch (s.style) {
        case 0: size = "auto"; repeat = "no-repeat"; break; // 居中
        case 6: size = "contain"; repeat = "no-repeat"; break; // 适应
        case 10: size = "cover"; repeat = "no-repeat"; break; // 填充
        case 22: size = "100% 100%"; repeat = "no-repeat"; break; // 拉伸
        default: size = "cover"; repeat = "no-repeat"; break;
      }
    }
  }
  return {
    backgroundImage:
      currentSrc.value && !imgError.value ? `url("${currentSrc.value}")` : "none",
    backgroundSize: size,
    backgroundRepeat: repeat,
  };
});

// 图片可用性探测（new Image）：成功则渲染背景，失败展示占位
let probeSeq = 0;
watch(
  currentSrc,
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

function onCurrentContext(e: MouseEvent) {
  if (!store.currentWallpaper) return;
  e.preventDefault();
  const item: WallpaperItem = {
    key: "current",
    kind: "current",
    path: store.currentWallpaper.path,
    title: "当前壁纸",
  };
  store.openContextMenu(item, e.clientX, e.clientY);
}

onMounted(() => {
  loadWallpaperInfo();
  ro = new ResizeObserver(() => scheduleLayout());
  if (previewRef.value) ro.observe(previewRef.value);
  computeMonitorSize();
  startClock();
});

onUnmounted(() => {
  ro?.disconnect();
  if (clockTimer !== undefined) window.clearInterval(clockTimer);
  if (rafId) cancelAnimationFrame(rafId);
});
</script>

<template>
  <main class="panel-right flex min-w-0 flex-1 flex-col gap-3">
    <!-- 面板头部 -->
    <div class="panel-header mb-0.5 flex items-center justify-between gap-2">
      <h2 class="text-[13px] font-semibold text-tx">当前壁纸</h2>
      <NButton size="small" secondary @click="store.setCurrentAsDesktop">
        <template #icon>
          <NIcon :component="MonitorDown" />
        </template>
        设为当前
      </NButton>
    </div>

    <!-- 壁纸预览：模拟真实桌面屏幕 -->
    <div
      ref="previewRef"
      class="preview-container relative flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-xl border border-line bg-panel/55"
      @contextmenu="onCurrentContext"
    >
      <!-- 空态：尚未选择壁纸 -->
      <div v-if="!store.currentWallpaper" class="preview-empty flex flex-col items-center text-center">
        <p class="text-[13px] text-dim">暂无壁纸预览</p>
        <p class="mt-1 text-[11px] text-faint">在左侧选择一张壁纸即可预览</p>
      </div>

      <!-- 图片加载失败占位 -->
      <div v-else-if="imgError" class="preview-empty flex flex-col items-center text-center">
        <p class="text-[12.5px] text-dim">预览不可用</p>
        <p class="mt-1 text-[11px] text-faint">{{ pathHint }}</p>
      </div>

      <!-- 模拟屏：按真实屏幕宽高比绘制显示器轮廓，内部按壁纸模式渲染 -->
      <div v-else-if="monitorSize" class="mock-screen" :style="mockScreenStyle">
        <!-- 壁纸层：铺满屏幕，填充方式由 wallpaperBg 控制 -->
        <div class="mock-wall" :style="wallpaperBg"></div>

        <!-- 模拟任务栏：半透明深色条（开始按钮 + 当前时间） -->
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

      <!-- 样式信息标签 -->
      <div
        v-if="wallpaperStyle && !imgError && store.currentWallpaper"
        class="style-badge absolute right-2 top-2 flex items-center gap-1 rounded-lg bg-black/60 px-2 py-1"
      >
        <NIcon :component="LayoutGrid" :size="12" class="text-accent" />
        <span class="text-[10.5px] text-white/80">{{ styleText }}</span>
      </div>
    </div>

    <!-- 底部信息条 -->
    <div class="info-bar flex items-center gap-2 rounded-lg bg-panel/40 px-3 py-2">
      <div class="flex min-w-0 items-center gap-2">
        <NIcon :component="Monitor" :size="12" class="shrink-0 text-dim" />
        <span class="res-text shrink-0 text-[10.5px] text-faint">{{ resInfo }}</span>
        <span
          v-if="nameHint"
          class="file-name truncate text-[10.5px] text-dim"
          :title="nameHint"
        >{{ nameHint }}</span>
      </div>
      <NIcon :component="Maximize" :size="12" class="ml-auto shrink-0 text-dim" />
    </div>
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

.info-bar {
  backdrop-filter: blur(8px);
  border: 1px solid rgba(255, 255, 255, 0.05);
}

.file-name {
  max-width: 260px;
}
</style>
