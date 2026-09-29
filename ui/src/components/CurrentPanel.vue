<script setup lang="ts">
// CurrentPanel - 右栏：壁纸预览与设置
// 交互模型：
//   - 点击左侧卡片 -> store.selectItem（蓝框选中，仅预览，桌面不变）
//   - 右侧大预览始终展示"正在预览"的图（无选中时回退当前桌面壁纸）
//   - 点击"设为壁纸"才真正写入桌面（绿框 currentWallpaper 随之更新）
//   - 预览样式下拉可即时预览；设为壁纸时若与系统样式不同会同步写注册表
import { computed, h, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { NButton, NColorPicker, NIcon, NSelect, NSlider, NSwitch } from "naive-ui";
import { AlertTriangle, Check, ChevronDown, ChevronLeft, ChevronRight, LayoutGrid, Maximize, Monitor, Pause, Play, Sparkles, Square, Star, Volume2, VolumeX, X } from "lucide-vue-next";
import {
  baseName,
  displaySrc,
  isRemoteSrc,
  isWebviewPlayable,
  useWallpaperStore,
  type WallpaperItem,
} from "../stores/wallpaper";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "../lib/naive-host";
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

// ---------- 视频预览（动态壁纸） ----------
// 选中视频项时右侧改为 <video muted loop autoplay> 实时播放，模拟桌面实际效果；
// 视频预览不参与壁纸效果（模糊/遮罩只作用于静态图片合成）、样式角标与放大浮层
const isVideoPreview = computed(() => previewTarget.value?.kind === "video");
const videoPlayable = computed(
  () => isVideoPreview.value && isWebviewPlayable(previewTarget.value?.path)
);
const videoRef = ref<HTMLVideoElement | null>(null);
const videoFailed = ref(false);

// 预览视频源或选中项变化时重置失败态并重新起播（浏览器自动播放需 muted 配合）
watch(
  () => [previewSrc.value, isVideoPreview.value] as const,
  async () => {
    videoFailed.value = false;
    if (!videoPlayable.value) return;
    await nextTick();
    const v = videoRef.value;
    if (v) {
      try {
        v.currentTime = 0;
      } catch {
        /* ignore */
      }
      void v.play().catch(() => {
        /* 自动播放被拦截时保留首帧，用户可手动点播放 */
      });
    }
  },
  { immediate: true }
);

function onPreviewVideoError() {
  videoFailed.value = true;
}

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
  // 优先用"正在预览的显示器"的实际比例 —— 多显示器时各屏可能宽高比不同
  //（如 16:9 主屏 + 21:9 带鱼屏），预览必须跟着切换，
  // 否则在带鱼屏上看到的预览比例是错的。
  const m = store.activeMonitorInfo;
  if (m && m.width > 0 && m.height > 0) {
    return m.width / m.height;
  }
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
const BADGE_RESERVE = 46; // 顶部为左上角角标位（样式角标组 / 视频类型角标）预留空间
// 底部为状态浮层（壁纸名 / 预览名）预留：浮层是绝对定位，
// 不留空间会盖住模拟屏底部的任务栏
const STATUS_RESERVE = 26;

let ro: ResizeObserver | null = null;
let rafId = 0;

function scheduleLayout() {
  if (rafId) cancelAnimationFrame(rafId);
  rafId = requestAnimationFrame(() => {
    rafId = 0;
    computeMonitorSize();
  });
}

// 在容器内计算「最大贴合」的模拟屏像素尺寸（保持宽高比，上下各预留浮层位）
function computeMonitorSize() {
  const el = previewRef.value;
  if (!el) return;
  const availW = Math.max(40, el.clientWidth - PAD * 2);
  // 顶部让开样式/视频角标，底部让开状态浮层（两者都是绝对定位，会盖住模拟屏）
  const availH = Math.max(
    40,
    el.clientHeight - PAD - BADGE_RESERVE - STATUS_RESERVE
  );
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

// ---------- 壁纸模糊遮罩效果（预览实时联动设置面板） ----------
// 预览层与放大层都叠加与后端合成一致的 filter + 黑色遮罩，滑块即改即看
const effectEnabled = computed(() => store.wallpaperEffect.enabled);
const effectBlur = computed(() => store.wallpaperEffect.blur);
const effectOpacity = computed(() => store.wallpaperEffect.opacity);
const effectColor = computed(() => store.wallpaperEffect.color);

function withEffect(base: Record<string, string>): Record<string, string> {
  if (!effectEnabled.value) return base;
  const s: Record<string, string> = { ...base };
  if (effectBlur.value > 0) s.filter = `blur(${effectBlur.value}px)`;
  return s;
}

// 前端预览近似后端逐像素混合：rgba(r,g,b,a) 与 (out*keep + c*a)/255 视觉等价
const effectMaskStyle = computed(() => {
  if (!effectEnabled.value || effectOpacity.value <= 0) {
    return { display: "none" };
  }
  const hex = effectColor.value.replace("#", "");
  const r = parseInt(hex.slice(0, 2), 16);
  const g = parseInt(hex.slice(2, 4), 16);
  const b = parseInt(hex.slice(4, 6), 16);
  const alpha = (effectOpacity.value / 100).toFixed(3);
  return { backgroundColor: `rgba(${r}, ${g}, ${b}, ${alpha})` };
});

// ---------- 底部设置区的折叠状态 ----------
// 设计目标：预览区永远优先。低频设置（效果滑块 / 播放范围）默认收起，
// 让底部卡高度从最坏 457px 压到约 162px，小窗口下预览区才不会被挤没。
//
// 折叠与"启用"是两件事：
//   - 开关（switch）控制效果是否生效；
//   - 箭头（chevron）控制参数是否展开。
// 但参数在效果关闭时毫无意义，故：打开开关 → 自动展开一次（刚开启多半要调）；
// 关闭开关 → 自动收起。用户手动收起后不会被再次强行展开。
const fxExpanded = ref(false);
const scopeExpanded = ref(false);

function onToggleEffect(v: boolean) {
  store.setEffect({ enabled: v });
  fxExpanded.value = v;
}

// ---------- 键盘快捷键：← → 切换壁纸 ----------
function onKey(e: KeyboardEvent) {
  if (e.key === "ArrowLeft") quickSwitch("left");
  if (e.key === "ArrowRight") quickSwitch("right");
}
const wallpaperBg = computed(() =>
  // 样式预览只对静态图片有意义；视频不走背景图渲染
  styleToBg(
    { style: selectedStyle.value.style, tile: selectedStyle.value.tile },
    !isVideoPreview.value && previewSrc.value && !imgError.value
      ? previewSrc.value
      : ""
  )
);

// 图片可用性探测（new Image）：成功则渲染背景，失败展示"预览不可用"占位。
//
// **视频必须直接跳过**：把 .mp4 交给 new Image() 必然触发 onerror（图片解码器
// 不认视频容器），会把 imgError 置为 true，进而抢在视频分支之前渲染出
// "预览不可用" —— 这正是"桌面能正常播放、右侧预览却说不可用"的原因。
// 视频的可播性由 video 元素自身的 @error（videoFailed）与格式白名单负责。
let probeSeq = 0;
watch(
  previewSrc,
  (src) => {
    imgError.value = false;
    // 视频不走图片探测；同时清掉可能残留的失败态
    if (!src || isVideoPreview.value) return;
    const seq = ++probeSeq;
    const probe = new Image();
    probe.onload = () => {
      if (seq === probeSeq) imgError.value = false;
    };
    probe.onerror = () => {
      // 期间若已切到视频，丢弃这次探测结果，避免误置失败态
      if (seq === probeSeq && !isVideoPreview.value) imgError.value = true;
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

// 路径文件扩展名徽标（如 .png / .jpg）；必应在线壁纸的 path 是远程 URL，不展示扩展名
const previewNameType = computed(() => {
  const p = previewPath.value;
  if (!p || isRemoteSrc(p)) return "";
  const ext = p.split(".").pop();
  return ext && ext.length <= 6 ? `.${ext.toLowerCase()}` : "";
});

const currentName = computed(() => {
  // 动态壁纸优先：视频播放时"当前壁纸"应显示正在播放的视频，而非静态壁纸
  if (store.videoWallpaper.enabled && store.videoWallpaper.path) {
    return baseName(store.videoWallpaper.path) + "（动态壁纸）";
  }
  const c = store.currentWallpaper;
  if (!c) return "未获取";
  return c.title || baseName(c.path || "") || "当前壁纸";
});

// 预览目标是否为已设置到桌面的当前壁纸（用于状态点颜色与文案）
// 必应来源预览项持有远程 URL，需经 store 的下载记录比对，不能直接比 path
const previewIsCurrent = computed(() => {
  const p = previewTarget.value;
  if (!p) return false;
  return store.isCurrentItem(p);
});

// 正在预览的壁纸是否已收藏（书签状态展示与快捷切换）
const previewIsFav = computed(() => store.isFavorite(previewTarget.value?.path));

function onTogglePreviewFav() {
  const p = previewTarget.value?.path;
  if (!p) return;
  const fav = store.toggleFavorite(p);
  if (fav) toast("已收藏", "success");
  else toast("已取消收藏", "warning");
}

// 预览屏的"分辨率 · 缩放"。展示位在**预览区底部状态栏**（原先挂在顶栏，
// 顶栏改为仅多屏渲染后并入状态栏 —— 它与"当前壁纸/预览"同属"我在看的是什么"）。
// 缩放**始终显示**（含 100%）：一致的格式比省几个字符重要，
// 缺一项会让人以为信息没读到（曾去掉过，用户要求恢复）。
// 必须跟着 activeMonitor 走：多显示器时各屏分辨率可能不同，
// 否则在 4K 屏上预览却看到主屏的 1080p 数值，会误判壁纸是否被降采样。
const resInfo = computed(() => {
  const m = store.activeMonitorInfo;
  if (m) {
    const scale = Math.round((m.scale || 1) * 100);
    return `${m.width} × ${m.height} · ${scale}%`;
  }
  const s = screenInfo.value;
  if (!s) return "--";
  return `${Math.round(s.logicalWidth)} × ${Math.round(s.logicalHeight)} · ${Math.round(s.scale * 100)}%`;
});

// ---------- 动态壁纸（视频） ----------
// 视频来源：壁纸目录扫描进列表（本地来源内即含视频项），选中后在右侧实时预览、
// 点"设为动态壁纸"启用（后端创建置底 WebView 窗口挂 WorkerW 播放）。
// 停止会销毁置底窗口并复位状态（播放页也支持双击停止）。
async function onStopVideoWallpaper() {
  try {
    await store.stopVideoWallpaper();
    toast("已停止动态壁纸", "success");
  } catch (err: unknown) {
    toast("停止失败：" + ((err as Error)?.message || String(err)), "error");
  }
}

// 暂停 / 恢复。与"停止"是两回事：暂停只冻结当前帧（画面留在最后一帧），
// 恢复从暂停处继续；停止会销毁置底窗口、桌面退回静态壁纸。
async function onToggleVideoPause() {
  await store.setVideoPaused(!store.videoWallpaper.paused);
}

// 静音开关：只影响桌面动态壁纸的音频（应用内的小预览与放大浮层**一律静音**，
// 否则点着列表挑视频会一路出声）。
//
// **成功时不给 toast**：图标本身就是反馈，而这个开关会连着点几下，
// 每次都弹一条只会糊住界面；失败仍由 store 统一报错。
async function onToggleVideoMuted() {
  await store.setVideoMuted(!store.videoMuted);
}

// ---- 显示器选择 ----
// 语义：selected 为空数组 = 全部显示器（用"全部"开关表达，UI 更直观）
type MonitorItem = (typeof store.availableMonitors)[number];
const monitorList = computed<MonitorItem[]>(() => store.availableMonitors);
// "全部显示器"开关：无任何单独选中即视为全部
const allMonitors = computed(() => store.selectedMonitors.length === 0);
// 单台显示器时无需展示选择区（没得选）
const showMonitorPicker = computed(() => monitorList.value.length > 1);

// 某台显示器当前是否生效
function monitorSelected(index: number): boolean {
  if (allMonitors.value) return true;
  return store.selectedMonitors.includes(index);
}

// 点击某台显示器：切换其选中状态（不能全部取消，至少保留一台）
async function onToggleMonitor(index: number) {
  const cur = allMonitors.value
    ? monitorList.value.map((m) => m.index) // 从"全部"开始点 → 只留这一台
    : [...store.selectedMonitors];
  const next = cur.includes(index)
    ? cur.filter((i) => i !== index)
    : [...cur, index].sort((a, b) => a - b);
  if (!next.length) {
    toast("至少需要保留一台显示器", "warning");
    return;
  }
  // 全部选中 → 归一化为"全部"语义（空数组），与后端约定一致
  const normalized = next.length === monitorList.value.length ? [] : next;
  const ok = await store.applyVideoMonitors(normalized);
  if (ok) toast("生效显示器已更新", "success");
}

// 切回"全部显示器"
async function onSelectAllMonitors() {
  if (allMonitors.value) return;
  const ok = await store.applyVideoMonitors([]);
  if (ok) toast("已应用到全部显示器", "success");
}

// ---------- 顶部工具条：显示器选择（**仅多屏渲染，且仅切换预览效果**）----------
// 语义：只决定右侧"模拟屏"按哪台显示器的宽高比/分辨率渲染，**不参与任何设置行为**。
// 真正决定"设到哪台"的入口是下方「动态壁纸」面板里的播放范围选择
//（selectedMonitors），静态壁纸则一律走原生全局设置。

// 显示器下拉项：值用 m.index（与 store.activeMonitor 同一套坐标）
const monitorOptions = computed(() =>
  store.availableMonitors.map((m) => ({ label: monitorLabel(m), value: m.index }))
);

// 下拉项自定义渲染：左侧通俗名，右侧靠边补"分辨率 · 缩放"。
// 两侧分层是为了把"是哪个屏"与"什么规格"分开，避免糊成一长串（那样就又回到"抽象"了）。
// 注意：renderLabel 在**收起态也会被调用**（Naive UI 用它渲染选中项），
// 所以这段右侧信息在未展开时同样可见 —— 这正是想要的效果。
function renderMonitorOption(option: { label: string; value: number }) {
  const m = store.availableMonitors.find((x) => x.index === option.value);
  return h("div", { class: "flex w-full items-center gap-3" }, [
    h("span", { class: "min-w-0 flex-1 truncate" }, option.label),
    h(
      "span",
      { class: "shrink-0 font-mono text-[10.5px] opacity-60" },
      m ? monitorSpec(m) : ""
    ),
  ]);
}

// 右侧规格文本："3840×2160 · 150%"。
// 缩放**始终显示**（含 100%）：一致的格式比省几个字符重要 ——
// 缺一项会让人以为信息没读到。与状态栏的 resInfo 保持同一格式。
function monitorSpec(m: { resolution: string; scale: number }): string {
  return `${m.resolution} · ${Math.round((m.scale || 1) * 100)}%`;
}

// 显示器短标签：`<名称> · <方位>`，**每块屏一律带方位**（主屏就是"主屏"）。
// 曾经对主屏省略方位后缀、并给它加"（主）"后缀 —— 同一份下拉里主屏那行
// 比别人短一截、后缀风格还不同，视觉上长短不齐，故统一成这一种写法。
// 名称与方位都是后端直接给的展示字符串，这里只做拼接，不解析。
function monitorLabel(m: {
  label: string;
  index: number;
  position_hint: string | null;
}): string {
  const base = m.label?.trim() || `显示器 ${m.index + 1}`;
  const hint = m.position_hint?.trim();
  return hint ? `${base} · ${hint}` : base;
}

// 悬停提示：给出完整信息（型号 + 分辨率 + 缩放 + 方位）。
// 用在「动态壁纸 → 播放范围」的显示器条目上 —— 那里只显示名称，
// 规格与缩放靠悬停补齐（下拉项已自带分辨率，不需要重复）。
function monitorTitle(m: {
  label: string;
  resolution: string;
  scale: number;
  position_hint: string | null;
}): string {
  // 缩放始终显示，与下拉项里的 monitorSpec、状态栏的 resInfo 保持同一格式
  const scaleText = ` · ${Math.round((m.scale || 1) * 100)}%`;
  // 方位放最后。label 里已不含"（主）"这类标记，故这里对**每块屏都补方位**，
  // 不再跳过主屏 —— 否则主屏的提示会缺一项。
  const pos = m.position_hint ? ` · ${m.position_hint}` : "";
  return `${m.label} · ${m.resolution}${scaleText}${pos}`;
}

// 显示器列表是异步拉取的（loadMonitors），到货后才需要重算预览比例
//（各屏宽高比可能不同，下拉切换后模拟屏要跟着变）。
// 注意：下拉控件自身不需要"测量"类补测，比原来的分段滑块简单很多。
watch(
  () => store.availableMonitors.length,
  async () => {
    await nextTick();
    requestAnimationFrame(() => requestAnimationFrame(() => {
      computeMonitorSize();
    }));
  }
);

// 切换显示器后预览比例/分辨率角标都要跟着变（模拟屏是"这块屏"的样子）
watch(
  () => store.activeMonitor,
  () => scheduleLayout()
);

// 预览容器右键：作用于"正在预览"对象（在网格中的可删除；纯桌面项只读设壁纸）
function onPreviewContext(e: MouseEvent) {
  const t = previewTarget.value;
  if (!t || !t.path) return;
  e.preventDefault();
  const fromGrid = store.gridItems.find(
    (it) => it.path === t.path && it.kind === t.kind
  );
  // 命中网格项则直接复用（保留 kind/date/title，必应项右键时仍按在线壁纸处理）；
  // 不在网格中的（纯当前桌面壁纸）构造只读 current 项
  const item: WallpaperItem = fromGrid ?? {
    key: "preview_ctx",
    kind: "current",
    path: t.path,
    title: t.title || baseName(t.path),
  };
  store.openContextMenu(item, e.clientX, e.clientY);
}

// ---------- 放大预览（全屏浮层） ----------
const zoomVisible = ref(false);

// 放大可用性：视频必须"可播放"才允许放大 —— 不支持的格式（mkv/mov）放进去
// 只会是一块黑屏，不如直接不给按钮，与预览区的"不支持的格式"提示保持一致。
const canZoom = computed(
  () => !!previewSrc.value && (!isVideoPreview.value || videoPlayable.value)
);

function openZoom() {
  if (!canZoom.value) return;
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
  const t = previewTarget.value;
  if (!t) return;
  // 静态壁纸统一走 Windows 原生全局设置：`SPI_SETDESKWALLPAPER` 只接受单张图，
  // 系统本身就没有"每屏不同壁纸"的接口，因此静态图始终作用于所有屏幕。
  // 需要按屏区分的是**动态壁纸（视频）**——那条路径由右侧显示器选择控制播放范围。
  void store.applyPreviewAsDesktop({
    style: selectedStyle.value.style,
    tile: selectedStyle.value.tile,
  });
}

// 视频项的"设为壁纸"= 启用动态壁纸；按钮文案随目标类型变化
const applyLabel = computed(() => {
  if (!isVideoPreview.value) return "设为壁纸";
  return store.isCurrentVideo(previewTarget.value!) ? "重新应用动态壁纸" : "设为动态壁纸";
});

// 视频项禁用条件：路径或播放能力不足时不允许设置（避免置底窗口黑屏）
const applyDisabled = computed(() => {
  const t = previewTarget.value;
  if (!t || !t.path) return true;
  if (t.kind === "video") return !isWebviewPlayable(t.path);
  return false;
});

onMounted(() => {
  loadScreenInfo();
  void store.loadMonitors();
  ro = new ResizeObserver(() => scheduleLayout());
  if (previewRef.value) ro.observe(previewRef.value);
  computeMonitorSize();
  startClock();
  window.addEventListener("keydown", onKey);
  // 显示器列表是异步拉取的，可能比本帧更晚；到货后各屏宽高比可能不同，
  // 需要在下一帧重算模拟屏尺寸（首屏先按当前比例画一个）。
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      computeMonitorSize();
    })
  );
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

    <!-- 顶部工具条：**只在多屏时存在**，整条只放"看哪台屏"这一件事。
         单屏时整条不渲染 —— 没有可选项，留一条空带子只会白吃垂直空间，
         还与下方预览区/控制卡不成比例。去掉后预览区自然上移占满，反而更干净。
         放在预览区**上方**而非容器内，因为它是"面板级"的上下文，
         不是预览内容的一部分 —— 与左侧列表的筛选栏（属于列表内部）在语义层级上不同。
         显示器用下拉而非平铺：台数不定且型号名可能很长，平铺在 3 台以上会挤爆一行。
         **样式控件不在这里** —— 它已移到预览区左上角的角标位（见下方 .style-overlay）：
         样式是直接作用在预览画面上的，贴着画面比放在顶栏更贴合语义。
         显示器下拉**仅切换预览效果**，不参与任何设置行为（静态壁纸走原生全局，
         动态壁纸的播放范围由下方「动态壁纸」面板决定）—— 详见其 title 提示。 -->
    <div
      v-if="store.hasMultipleMonitors"
      class="top-bar flex flex-none items-center gap-2"
    >
      <!-- "显示器"标签改用图标：同样承担"这里选的是哪块屏"的语义，
           但不占三个字的宽度，窄窗口下把宽度让给下拉里的显示器名。
           提示必须挂在这个图标上而非 NSelect：NSelect 内部会给选中项 overlay
           设自己的 title（显示选中文本），会盖掉外部传入的 title，
           挂在图标上才稳定可见。 -->
      <span
        class="flex shrink-0 cursor-help items-center text-faint"
        title="切换预览的显示器（仅影响预览画面，不会改动任何桌面设置）"
      >
        <NIcon :component="Monitor" :size="13" />
      </span>
      <NSelect
        :value="store.activeMonitor"
        size="small"
        class="monitor-nselect monitor-field"
        :options="monitorOptions"
        :render-label="renderMonitorOption"
        @update:value="(v: number) => store.setActiveMonitor(v)"
      />
    </div>

    <!-- 预览区域 -->
    <!-- min-h 保底：预览是本应用的核心，绝不能因为下方设置变多而被压没。
         下方 dock-card 可收缩 + 内部滚动，空间不足时由它让步。 -->
    <div
      ref="previewRef"
      class="preview-container relative flex min-h-[200px] flex-1 items-center justify-center overflow-hidden rounded-xl border border-line bg-panel/55"
      @contextmenu="onPreviewContext"
    >
      <!-- 左上角角标位（两种内容互斥，同一坐标）：
           图片 → 样式角标组；视频 → 类型角标。
           放在预览区上而非顶栏，是因为样式直接作用于这块画面，"贴着画面"比"隔着顶栏"更贴合语义。
           视频不适用样式（按填充铺满屏幕），所以视频时整组隐藏、把位置让给类型角标。 -->
      <!-- 样式角标组 -->
      <div
        v-if="!isVideoPreview && previewTarget && !imgError"
        class="corner-badge style-overlay absolute left-2 top-2 z-[5] flex items-center gap-1 rounded-lg bg-black/60 px-1.5 py-1"
      >
        <span class="shrink-0 px-0.5 text-[10px] text-white/40">样式</span>
        <button
          v-for="opt in STYLE_OPTIONS"
          :key="opt.key"
          class="style-chip shrink-0 rounded-md px-1.5 py-[3px] text-[10.5px] leading-none transition-colors"
          :class="
            opt.key === styleKey
              ? 'bg-accent/30 text-white'
              : 'text-white/55 hover:bg-white/10 hover:text-white/85'
          "
          :title="opt.label"
          @click="styleKey = opt.key"
        >
          {{ opt.label }}
        </button>
      </div>

      <!-- 视频类型角标：标明当前预览的是动态壁纸（或格式不支持）。 -->
      <div
        v-if="isVideoPreview && !videoFailed"
        class="corner-badge absolute left-2 top-2 flex items-center gap-1 rounded-lg bg-black/60 px-2 py-1"
      >
        <NIcon
          :component="videoPlayable ? Play : AlertTriangle"
          :size="12"
          :class="videoPlayable ? 'text-accent' : 'text-amber-300'"
        />
        <span class="text-[10.5px] text-white/80">
          {{ videoPlayable ? "动态壁纸" : "不支持的格式" }}
        </span>
      </div>

      <!-- 放大预览按钮：图片与视频都支持（视频走浮层内的 <video> 分支）。
           放右上角，与左上角角标位分列两侧、互不遮挡。 -->
      <button
        v-if="canZoom"
        class="zoom-btn absolute right-2 top-2 flex h-7 w-7 items-center justify-center rounded-lg border border-white/10 bg-black/45 text-white/85 transition-colors hover:bg-black/65"
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

      <!-- 视频：格式不兼容（mkv/mov）或解码失败 -->
      <div
        v-else-if="isVideoPreview && (!videoPlayable || videoFailed)"
        class="preview-empty flex flex-col items-center px-6 text-center"
      >
        <NIcon :component="AlertTriangle" :size="20" class="mb-1.5 text-amber-300" />
        <p class="text-[12.5px] text-dim">
          {{ videoPlayable ? "视频解码失败" : "该格式无法预览" }}
        </p>
        <p class="mt-1 max-w-[85%] text-[11px] leading-relaxed text-faint">
          桌面动态壁纸由 WebView 渲染，仅支持 mp4 / webm。请将该视频转换为 mp4 后重试。
        </p>
        <p class="mt-1.5 max-w-[85%] truncate text-[10.5px] text-faint/70" :title="previewPath">{{ previewPath }}</p>
      </div>

      <!-- 图片加载失败占位。
           必须加 !isVideoPreview：imgError 是"图片"探测的结果，
           顺序上又排在模拟屏之前，一旦为 true 会连视频预览一起吞掉，
           导致视频显示成"预览不可用"。 -->
      <div
        v-else-if="imgError && !isVideoPreview"
        class="preview-empty flex flex-col items-center text-center"
      >
        <p class="text-[12.5px] text-dim">预览不可用</p>
        <p class="mt-1 max-w-[85%] truncate text-[11px] text-faint" :title="previewPath">{{ previewPath }}</p>
      </div>

      <!-- 模拟屏 -->
      <div v-else-if="monitorSize" class="mock-screen" :style="mockScreenStyle">
        <!-- 视频壁纸层：静音循环自动播放，按桌面实际效果铺满（object-fit: cover） -->
        <video
          v-if="isVideoPreview"
          ref="videoRef"
          class="mock-wall mock-wall-video"
          :src="previewSrc"
          muted
          loop
          autoplay
          playsinline
          preload="auto"
          @error="onPreviewVideoError"
        ></video>
        <!-- 静态壁纸层（启用效果时叠加高斯模糊） -->
        <div v-else class="mock-wall" :style="withEffect(getTransitionBg(selectedStyle, previewSrc))"></div>
        <!-- 壁纸效果遮罩层（仅静态壁纸预览：视频不做后端合成，效果开关不作用于视频） -->
        <div v-if="effectEnabled && !isVideoPreview" class="effect-mask" :style="effectMaskStyle"></div>

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

      <!-- 状态浮层：原来占底部卡一整行（约 29px），移到预览区底部后不再吃垂直空间。
           语义也更顺 —— 信息直接标注在"正在看的东西"上。
           用绝对定位，故 computeMonitorSize 里预留了 STATUS_RESERVE 避免盖住任务栏。 -->
      <div class="preview-status absolute inset-x-0 bottom-0 flex items-center gap-3 px-3 py-1.5">
        <span class="flex min-w-0 items-center gap-1.5 text-[10.5px]">
          <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-ok/90"></span>
          <span class="shrink-0 text-faint">当前壁纸</span>
          <span class="min-w-0 truncate text-dim" :title="currentName + '（已设置到桌面）'">{{ currentName }}</span>
        </span>
        <span class="flex min-w-0 items-center gap-1.5 text-[10.5px]">
          <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-accent/90"></span>
          <span class="shrink-0 text-faint">预览</span>
          <span class="min-w-0 truncate text-tx" :title="previewName + '（当前选中，仅预览）'">{{ previewName || "—" }}</span>
          <!-- 收藏状态：星标展示 + 快捷收藏/取消收藏（必应在线壁纸不支持收藏） -->
          <button
            v-if="previewTarget?.path && previewTarget?.kind !== 'bing'"
            class="flex shrink-0 cursor-pointer items-center rounded p-0.5 transition-colors hover:bg-white/10"
            :title="previewIsFav ? '已收藏，点击取消收藏' : '收藏该壁纸 (Ctrl+F)'"
            @click.stop="onTogglePreviewFav"
          >
            <NIcon
              :component="Star"
              :fill="previewIsFav ? 'currentColor' : 'none'"
              :size="13"
              :class="previewIsFav ? 'text-amber-300' : 'text-faint'"
            />
          </button>
        </span>

        <!-- 分辨率/缩放：原先在顶栏，单屏顶栏整条取消后并入这里。
             与左边两组名称同属"我正在看的这块东西是什么"，语义一致。
             shrink-0 保证它不被压缩 —— 空间不够时由两组名称先截断
             （名称本身都有 title 提示，截断不丢信息）。 -->
        <span
          class="res-text ml-auto shrink-0 font-mono text-[10px] leading-none text-faint"
          title="预览屏的分辨率与缩放"
        >{{ resInfo }}</span>
      </div>
    </div>

    <!-- 底部控制卡：设壁纸 + 折叠的设置区
         高度策略：可收缩（shrink）+ 内部滚动（overflow-y-auto）。
         flex 分配原理：预览区 flex-1（basis 0）无法被压缩，本卡 basis 为内容高度，
         故空间不足时**全部压缩量都落在本卡**，预览区保住 min-h-[200px]，
         本卡超出部分自己滚动 —— 而不是把预览挤没。 -->
    <div class="dock-card flex min-h-0 shrink flex-col gap-2 overflow-y-auto rounded-xl border border-line bg-panel/60 px-3 py-2.5">
      <!-- 设为壁纸主按钮（视频项 = 启用动态壁纸）。
           "样式"控件既不在本卡也不在顶栏 —— 它在预览区左上角的角标位，
           故本卡首行就是主操作，视觉重心最明确。 -->
      <NButton
        type="primary"
        size="medium"
        class="apply-btn w-full"
        :disabled="applyDisabled"
        :loading="store.isApplying"
        @click="onApplyAsDesktop"
      >
        <template #icon>
          <NIcon :component="isVideoPreview ? Play : Check" />
        </template>
        {{ applyLabel }}
      </NButton>

      <!-- 壁纸效果：折叠区（仅静态壁纸适用：后端效果合成只处理图片，视频不参与）
           常驻一行只放"标题 + 开关 + 展开箭头"，3 个滑块收进展开区 -->
      <div v-if="!isVideoPreview" class="effect-settings border-t border-line/80 pt-2">
        <div class="flex items-center justify-between gap-2">
          <span class="flex items-center gap-1.5 text-[10.5px] text-faint">
            <NIcon :component="Sparkles" :size="12" class="text-accent" />
            壁纸效果
          </span>
          <div class="flex shrink-0 items-center gap-1.5">
            <n-switch
              :value="effectEnabled"
              size="small"
              @update:value="(v: boolean) => onToggleEffect(v)"
            />
            <!-- 展开箭头：效果关闭时参数无意义，故禁用 -->
            <button
              class="acc-chevron flex h-5 w-5 items-center justify-center rounded transition-colors"
              :class="
                effectEnabled
                  ? 'cursor-pointer text-dim hover:bg-white/10 hover:text-tx'
                  : 'cursor-not-allowed text-faint/40'
              "
              :disabled="!effectEnabled"
              :title="fxExpanded ? '收起效果参数' : '展开效果参数'"
              @click="fxExpanded = !fxExpanded"
            >
              <NIcon
                :component="ChevronDown"
                :size="13"
                :class="fxExpanded && effectEnabled ? 'rotate-180' : ''"
                style="transition: transform 0.18s ease"
              />
            </button>
          </div>
        </div>
        <div v-if="effectEnabled && fxExpanded" class="mt-2 flex flex-col gap-2">
          <div class="fx-field">
            <div class="flex items-center justify-between">
              <span class="text-[10.5px] text-dim">模糊强度</span>
              <span class="fx-value font-mono text-[10px] text-accent">{{ effectBlur }}px</span>
            </div>
            <n-slider
              :value="effectBlur"
              :min="0"
              :max="30"
              :step="1"
              size="small"
              @update:value="(v: number) => store.setEffect({ blur: v })"
            />
          </div>
          <div class="fx-field">
            <div class="flex items-center justify-between">
              <span class="text-[10.5px] text-dim">遮罩不透明度</span>
              <span class="fx-value font-mono text-[10px] text-accent">{{ effectOpacity }}%</span>
            </div>
            <n-slider
              :value="effectOpacity"
              :min="0"
              :max="80"
              :step="1"
              size="small"
              @update:value="(v: number) => store.setEffect({ opacity: v })"
            />
          </div>
          <div class="fx-field flex items-center justify-between">
            <span class="text-[10.5px] text-dim">遮罩颜色</span>
            <n-color-picker
              :value="effectColor"
              :show-alpha="false"
              size="small"
              style="width: 100px"
              @update:value="(v: string) => store.setEffect({ color: v })"
            />
          </div>
        </div>
      </div>

      <!-- 动态壁纸（视频）：常驻区。第一行放"状态点 + 文件名 + 展开箭头"，
           第二行是 停止 / 暂停 / 静音 **三个带边框的按钮同占一行**（理由见下方注释）。
           播放范围收进展开区 —— 但**播放控制全部留在常驻区**，
           因为停止是高频且紧急的操作，藏进折叠区会让用户找不到。
           选片入口在首页标题栏。 -->
      <div v-if="store.videoWallpaper.enabled" class="effect-settings border-t border-line/80 pt-2">
        <div class="flex items-center gap-1.5">
          <!-- 状态点：播放中=绿，暂停=琥珀。暂停时画面是静止的，
               光看文件名分不出"卡住了"还是"我按了暂停"，这个点负责回答。 -->
          <span
            class="flex shrink-0 items-center gap-1.5 text-[10.5px] text-faint"
            :title="
              store.videoWallpaper.paused
                ? '动态壁纸已暂停（画面冻结在当前帧）'
                : '动态壁纸播放中'
            "
          >
            <span
              class="h-1.5 w-1.5 shrink-0 rounded-full"
              :class="store.videoWallpaper.paused ? 'bg-amber-400/90' : 'bg-ok/90'"
            ></span>
            动态壁纸
          </span>
          <span
            class="min-w-0 flex-1 truncate text-[10.5px] text-tx"
            :title="store.videoWallpaper.path"
          >
            {{ baseName(store.videoWallpaper.path) }}
          </span>
          <!-- 展开播放范围（仅多显示器时有内容，单屏时隐藏箭头） -->
          <button
            v-if="showMonitorPicker"
            class="acc-chevron flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-dim transition-colors hover:bg-white/10 hover:text-tx"
            :title="scopeExpanded ? '收起播放范围' : '展开播放范围'"
            @click="scopeExpanded = !scopeExpanded"
          >
            <NIcon
              :component="ChevronDown"
              :size="13"
              :class="scopeExpanded ? 'rotate-180' : ''"
              style="transition: transform 0.18s ease"
            />
          </button>
        </div>

        <!-- 播放控制：停止 / 暂停 / 静音 **同占一行**（常驻，不折叠）。
             行 1 只留"状态点 + 文件名 + 展开箭头" —— 那里再塞按钮会把文件名
             挤到只剩百来像素，所以三个按钮单独占这一行。
             每个按钮都带边框：并排时若只靠文字颜色区分，停止（危险、不可逆）
             与暂停（可逆）看起来会一样重；边框把各自的点击热区显式框出来，
             误点概率明显下降。
             边框用 `border-line-2`（白 14%）而不是 `border-line`（白 7%）——
             后者做按钮描边太淡，在深色面板上几乎看不见。
             图标语义刻意不统一，这跟所有播放器一致：
               暂停/继续按钮显示**动作**（暂停图标 = 点了会暂停），
               静音按钮显示**状态**（喇叭带斜杠 = 现在没有声音），
               因为喇叭图标的"当前状态"含义比"点了会怎样"更被广泛理解。
             后果差别（停止会销毁置底窗口、暂停只冻结当前帧、静音只影响音频）
             由 title 承载 —— 行内宽度要留给按钮本身，放不下小字。 -->
        <div class="mt-1.5 flex items-center gap-1.5">
          <!-- 停止：破坏性操作，边框与文字都用危险色，与另两个拉开距离 -->
          <button
            class="flex flex-1 cursor-pointer items-center justify-center gap-1 rounded border border-danger/40 px-1.5 py-1 text-[10.5px] text-danger transition-colors hover:border-danger/70 hover:bg-danger/10"
            title="停止动态壁纸（关闭置底窗口，桌面回到静态壁纸）"
            @click="onStopVideoWallpaper"
          >
            <NIcon :component="Square" :size="11" class="shrink-0" />
            <span>停止</span>
          </button>
          <!-- 暂停 / 恢复 -->
          <button
            class="flex flex-1 cursor-pointer items-center justify-center gap-1 rounded border px-1.5 py-1 text-[10.5px] transition-colors"
            :class="
              store.videoWallpaper.paused
                ? 'border-amber-400/50 bg-amber-400/10 text-amber-300 hover:border-amber-400/80'
                : 'border-line-2 text-dim hover:border-white/25 hover:bg-white/10 hover:text-tx'
            "
            :title="
              store.videoWallpaper.paused
                ? '从暂停处继续播放'
                : '暂停（冻结当前画面，不关闭动态壁纸）'
            "
            @click="onToggleVideoPause"
          >
            <NIcon
              :component="store.videoWallpaper.paused ? Play : Pause"
              :size="11"
              class="shrink-0"
            />
            <span>{{ store.videoWallpaper.paused ? "继续" : "暂停" }}</span>
          </button>
          <!-- 静音 / 取消静音 -->
          <button
            class="flex flex-1 cursor-pointer items-center justify-center gap-1 rounded border px-1.5 py-1 text-[10.5px] transition-colors"
            :class="
              store.videoMuted
                ? 'border-accent/50 bg-accent-soft text-accent hover:border-accent/80'
                : 'border-line-2 text-dim hover:border-white/25 hover:bg-white/10 hover:text-tx'
            "
            :title="
              store.videoMuted
                ? '取消静音，让桌面播放原声'
                : '静音播放（只影响桌面，应用内预览本来就静音）'
            "
            @click="onToggleVideoMuted"
          >
            <NIcon
              :component="store.videoMuted ? VolumeX : Volume2"
              :size="11"
              class="shrink-0"
            />
            <span>{{ store.videoMuted ? "取消静音" : "静音" }}</span>
          </button>
        </div>

        <!-- 动态壁纸播放范围：多显示器时才有意义。
             注意与顶部那条区分：顶部只切预览效果，这里才是真正决定"播到哪几台"。 -->
        <div v-if="showMonitorPicker && scopeExpanded" class="mt-2.5">
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-[10.5px] text-faint">播放范围</span>
            <button
              class="cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] transition-colors"
              :class="allMonitors ? 'text-faint' : 'text-accent hover:bg-accent/10'"
              :disabled="allMonitors"
              @click="onSelectAllMonitors"
            >
              全部显示器
            </button>
          </div>
          <div class="flex flex-col gap-1">
            <button
              v-for="m in monitorList"
              :key="m.index"
              class="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-left transition-colors"
              :class="
                monitorSelected(m.index)
                  ? 'bg-accent/12 text-tx'
                  : 'text-dim hover:bg-white/5'
              "
              @click="onToggleMonitor(m.index)"
            >
              <span
                class="flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[3px] border transition-colors"
                :class="
                  monitorSelected(m.index)
                    ? 'border-accent bg-accent text-white'
                    : 'border-line'
                "
              >
                <NIcon v-if="monitorSelected(m.index)" :component="Check" :size="10" />
              </span>
              <span class="min-w-0 flex-1 truncate text-[10.5px]" :title="monitorTitle(m)">
                {{ monitorLabel(m) }}
              </span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- 放大预览浮层（Esc 关闭） -->
    <Teleport to="body">
      <div
        v-if="zoomVisible"
        class="zoom-overlay fixed inset-0 z-[999] overflow-hidden"
        @click.self="closeZoom"
      >
        <!-- 视频：直接放一个铺满的 <video> 实时播放，与桌面上的动态壁纸观感一致。
             `:key` 强制按源重建元素 —— 换源后 autoplay 才可靠，
             也避免残留上一段视频的最后一帧。
             opacity 复用 animOpacity，使放大态左右切换的淡入淡出与图片一致。 -->
        <video
          v-if="isVideoPreview"
          :key="previewSrc"
          class="zoom-video"
          :src="previewSrc"
          :style="{ opacity: animOpacity }"
          autoplay
          muted
          loop
          playsinline
        ></video>
        <!-- 图片：独立背景层承载，切换时通过 animOpacity 淡入淡出 -->
        <template v-else>
          <div class="zoom-bg" :style="withEffect(getTransitionBg(selectedStyle, previewSrc))"></div>
          <!-- 壁纸效果遮罩层（放大态同样叠加） -->
          <div v-if="effectEnabled" class="zoom-effect-mask" :style="effectMaskStyle"></div>
        </template>
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
          <!-- 视频不适用填充样式（按填充铺满），故此处标类型而非样式 -->
          <span class="flex shrink-0 items-center gap-1 text-accent">
            <NIcon :component="isVideoPreview ? Play : LayoutGrid" :size="12" />
            {{ isVideoPreview ? "动态壁纸" : styleText }}
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

/* ---- 顶部工具条的"显示器"下拉（仅多屏时渲染）---- */
/* 注意 width:auto 对 NSelect 无效（内部输入框有基础宽度），
   必须由外层 flex 决定宽度，再配合 .monitor-nselect 里的 100% + min-width。 */
.monitor-field {
  flex: 1 1 auto;
  min-width: 140px;
  /* 上限是"防贪婪"而非定宽：正常型号名（如 DELL U2720Q）自然宽约 200px，
     够不到 260px，此值不生效；只有型号名特别长时才封顶。
     最小窗口（800px）时右栏约 351px，"显示器"标签约 30px + 间距 8px，
     260px 的字段仍有余量，不会截断。 */
  max-width: 260px;
}
.monitor-nselect :deep(.n-base-selection) {
  width: 100%;
}
/* 选中项内容撑满，使右侧"分辨率 · 缩放"能真正贴到控件右缘。
   Naive UI 的 overlay 自带左右 padding，靠 w-full + flex-1 把标签推左、
   规格推右，两侧留白对称。 */
.monitor-nselect :deep(.n-base-selection-overlay) {
  display: flex;
  align-items: center;
}
.monitor-nselect :deep(.n-base-selection-overlay__wrapper) {
  width: 100%;
}
/* 下拉项内容：Naive UI 把 renderLabel 的产物与"选中勾"并排塞进
   `__content`，故我的 w-full 只占了勾以外剩下的宽度 ——
   结果是"靠右"看起来没贴到菜单右缘。把 __content 改成撑满的 flex 行，
   勾自然被推到最右，我的 div 占满其余空间。 */
.monitor-nselect :deep(.n-base-select-option__content) {
  display: flex;
  width: 100%;
  align-items: center;
}
/* 我的标签容器占满，使右规格与"勾"相邻对齐 */
.monitor-nselect :deep(.n-base-select-option__content > div:first-child) {
  flex: 1 1 auto;
  min-width: 0;
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

/* 视频壁纸层：动态壁纸按填充方式铺满（对齐置底窗口 object-fit: cover 的实际效果）；
   静态壁纸的样式下拉对视频不生效，故在此固定为 cover */
.mock-wall-video {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  background: #000;
}

/* 壁纸效果遮罩层：模拟后端合成的黑色半透明叠加（仅预览，不拦截点击） */
.effect-mask,
.zoom-effect-mask {
  position: absolute;
  inset: 0;
  pointer-events: none;
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

/* 预览区左上角角标外壳：样式角标组与视频类型角标共用同一套视觉语言
   （半透明黑底 + 模糊 + 细边），保证压在任意壁纸上都清晰可读。
   两者坐标相同且互斥：图片显示样式角标组，视频显示类型角标。 */
.corner-badge {
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
/* 放大态视频层：**必须与桌面实际渲染一致，用 object-fit: cover**。
   动态壁纸在置底窗口（VideoWallpaper.vue）与右侧小预览里都是 cover 铺满，
   放大态若用 contain 会露出黑边、与真实桌面观感不符 ——
   放大预览的意义是"看到它到底长什么样"，不是"看到完整原始帧"。
   opacity 过渡与 .zoom-bg 对齐，使视频左右切换也有同样的淡入淡出。 */
.zoom-video {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  background: #000;
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
  /* 空间不足时内部滚动而不是撑破布局（预览区有 min-h 保底）。
     overscroll-behavior 防止滚到底后把滚动链传给外层容器。
     滚动条外观统一交给 main.css 的 ::-webkit-scrollbar —— 这里**绝不能**写
     scrollbar-width / scrollbar-color，否则 Chromium 会忽略伪元素样式，
     退回系统原生滚动条（正是"不好看"的元凶）。 */
  overscroll-behavior: contain;
}

/* 预览区底部状态浮层：半透明底 + 模糊，压在壁纸上仍可读 */
.preview-status {
  background: linear-gradient(
    to top,
    rgba(5, 8, 14, 0.72),
    rgba(5, 8, 14, 0)
  );
  pointer-events: none; /* 不拦截预览区的右键菜单与切换按钮 */
}
.preview-status > span {
  min-width: 0;
}
/* 星标按钮需要重新开启点击（父级 pointer-events: none） */
.preview-status button {
  pointer-events: auto;
}

/* 折叠区箭头按钮（通用） */
.acc-chevron:disabled {
  pointer-events: none;
}

/* 样式角标按钮：全局 `*` 设了 `cursor: default`，所有交互元素必须显式给 pointer
   （与 .zoom-btn / .acc-chevron 同一处理方式）。压在壁纸上用半透明底 + 白字，
   实心面板色会在照片上糊成一块，故选中/未选都走白色系透明度。 */
.style-chip {
  cursor: pointer;
}

/* 分辨率/扩展名等小型信息徽标统一样式 */
.res-text {
  letter-spacing: 0.2px;
  font-variant-numeric: tabular-nums;
}
</style>
