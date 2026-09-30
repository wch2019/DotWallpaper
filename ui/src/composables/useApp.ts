// useApp —— 全局状态单例（替代原 pinia store）
// 模块级 ref/computed + 导出单例：任何组件调用 useApp() 都拿到同一份状态。
import { computed, ref } from "vue";
import { api, errMessage, localSrc, onNativeEvent } from "../lib/api";
import type {
  DisplayInfo,
  DisplayWallpaperState,
  FitMode,
  MediaItem,
  PlaybackAction,
  PlaybackPhase,
} from "../types";

// ---------- 提示 ----------
export type ToastKind = "info" | "success" | "error" | "warning";
export interface Toast {
  id: number;
  text: string;
  kind: ToastKind;
}

// ---------- 单例状态 ----------
const libraryDir = ref("");
const media = ref<MediaItem[]>([]);
const displays = ref<DisplayInfo[]>([]);
const states = ref<DisplayWallpaperState[]>([]);
const selectedPath = ref("");
const selectedDisplayId = ref("");
const fitMode = ref<FitMode>("fill");
const muted = ref(true);
const onboardingCompleted = ref(false);
let onboardingFolderChosen = false;
const snapshotReady = ref(false);
const loadingMedia = ref(false);
const mediaError = ref("");
const applying = ref(false);
const dragging = ref(false);
const toasts = ref<Toast[]>([]);
let toastSeq = 0;

// ---------- 私有工具 ----------
function upsertState(next: DisplayWallpaperState) {
  const idx = states.value.findIndex((s) => s.displayId === next.displayId);
  if (idx >= 0) states.value.splice(idx, 1, next);
  else states.value.push(next);
}

function dismiss(id: number) {
  toasts.value = toasts.value.filter((t) => t.id !== id);
}

function toast(text: string, kind: ToastKind = "info") {
  const id = ++toastSeq;
  toasts.value.push({ id, text, kind });
  window.setTimeout(() => dismiss(id), kind === "error" ? 4200 : 2400);
}

function errToast(prefix: string, err: unknown) {
  toast(`${prefix}：${errMessage(err)}`, "error");
}

// ---------- 后端事件绑定（模块级单例，幂等） ----------
let eventsBound = false;
let settingsEventBound = false;

/// 重取快照：显示器列表只在快照里刷新，热插拔后必须主动重取，否则界面拿着过期列表
/// （拔掉的屏还在下拉里、重连的屏的 ID 又变了）。定义在模块作用域，事件监听才用得到。
async function loadSnapshot() {
  try {
    const snap = await api.getAppSnapshot();
    libraryDir.value = snap.libraryDir ?? "";
    fitMode.value = snap.defaultFitMode ?? "fill";
    muted.value = snap.defaultMuted ?? true;
    onboardingCompleted.value = snap.onboardingCompleted ?? false;
    displays.value = snap.displays ?? [];
    states.value = snap.states ?? [];
    snapshotReady.value = true;
    if (!selectedDisplayId.value) {
      const primary = displays.value.find((d) => d.primary);
      selectedDisplayId.value = (primary ?? displays.value[0])?.id ?? "";
    }
  } catch (err) {
    errToast("读取应用状态失败", err);
  }
}

/// 后端 publish → "wallpaper-state"：Preparing→Playing/Error、热插拔、
/// 休眠恢复等所有状态变化实时同步到前端，无需手动刷新。
function bindBackendEvents() {
  if (eventsBound) return;
  eventsBound = true;
  onNativeEvent<DisplayWallpaperState>("wallpaper-state", (s) => {
    if (!s?.displayId) return;
    const prev = states.value.find((x) => x.displayId === s.displayId)?.phase;
    upsertState(s);
    // 列表里有未知显示器 = 刚热插拔过（重连会推 Preparing）；error 多半是那块屏刚被拔出。
    // 两种情况下前端的显示器列表都已过期，重取一次让界面自我修正。
    if (s.phase === "error" || !displays.value.some((d) => d.id === s.displayId)) {
      void loadSnapshot();
    }
    const name =
      displays.value.find((d) => d.id === s.displayId)?.name ?? s.displayId;
    if (s.phase === "playing" && prev === "preparing") {
      toast(`动态壁纸已启动：${name}`, "success");
    } else if (s.phase === "error" && s.error) {
      toast(s.error, "error");
    }
  });
  onNativeEvent<{ path?: string; thumb?: string }>("thumbnail-ready", ({ path, thumb }) => {
    if (!path || !thumb) return;
    const idx = media.value.findIndex((m) => m.path === path);
    if (idx >= 0 && media.value[idx].thumb !== thumb) {
      media.value.splice(idx, 1, { ...media.value[idx], thumb });
    }
  });
}

bindBackendEvents();

// ---------- 组合函数 ----------
export function useApp() {
  // ---- 派生状态 ----
  const selected = computed<MediaItem | null>(
    () => media.value.find((m) => m.path === selectedPath.value) ?? null
  );

  const previewSrc = computed(() => localSrc(selected.value?.path));

  const selectedDisplay = computed<DisplayInfo | null>(
    () => displays.value.find((d) => d.id === selectedDisplayId.value) ?? null
  );

  const selectedState = computed<DisplayWallpaperState | null>(
    () => states.value.find((s) => s.displayId === selectedDisplayId.value) ?? null
  );

  const selectedStatePhase = computed<PlaybackPhase>(
    () => selectedState.value?.phase ?? "static"
  );

  // 播放控制可用性：该显示器已有非静态壁纸会话
  const canControlPlayback = computed(() => {
    const s = selectedState.value;
    return !!s && s.phase !== "static" && s.phase !== "error";
  });

  // 暂停 / 恢复仅对视频壁纸有意义。准备中要排除：后端此时拒绝这两个动作（停止可以），
  // 按钮亮着却必然报错等于骗用户点。
  const canControlVideo = computed(() => {
    const s = selectedState.value;
    return (
      !!s &&
      canControlPlayback.value &&
      s.phase !== "preparing" &&
      s.assignment?.kind === "video"
    );
  });

  // 选中的显示器不在线：状态镜像里有它的记录，但它已经不在显示器列表里（刚被拔出）。
  // 这种屏没有"换一张壁纸"的界面路径，解除占用是唯一的退出路径。
  const selectedDisplayOffline = computed(
    () => !!selectedDisplayId.value && !selectedDisplay.value
  );

  // 错误态（多为显示器已断开）下没有会话可停，但用户需要一条退出路径：该文件仍占着
  // 持久化分配和删除保护，而后端的停止对无会话显示器是幂等的——顺带清除配置、解除占用。
  // 已断开显示器上的静态壁纸同理（phase 仍是 static，光靠 error 判断会漏掉）。
  const canForgetAssignment = computed(() => {
    const s = selectedState.value;
    if (!s?.assignment) return false;
    return s.phase === "error" || (selectedDisplayOffline.value && s.phase === "static");
  });

  const canStopPlayback = computed(
    () => canControlPlayback.value || canForgetAssignment.value
  );

  const hasMedia = computed(() => media.value.length > 0);

  const dirName = computed(() => {
    const p = libraryDir.value;
    if (!p) return "未选择";
    const parts = p.split(/[\\/]/);
    return parts[parts.length - 1] || p;
  });

  // ---- 数据加载 ----
  async function loadMedia(keepSelection = true) {
    loadingMedia.value = true;
    mediaError.value = "";
    try {
      media.value = await api.listMedia();
      if (!keepSelection || !media.value.some((m) => m.path === selectedPath.value)) {
        selectedPath.value = media.value[0]?.path ?? "";
      }
    } catch (err) {
      media.value = [];
      mediaError.value = errMessage(err);
      errToast("读取媒体列表失败", err);
    } finally {
      loadingMedia.value = false;
    }
  }

  async function refreshAll() {
    await loadSnapshot();
    // A migrated library may need a new macOS directory grant. Let the first-run
    // picker render before touching Documents rather than blocking on TCC I/O.
    if (onboardingCompleted.value) await loadMedia();
  }

  if (!settingsEventBound) {
    settingsEventBound = true;
    onNativeEvent("settings-changed", () => { void refreshAll(); });
  }

  // ---- 目录 / 导入 / 删除 ----
  async function saveSettings(): Promise<boolean> {
    try {
      await api.updateSettings({
        defaultFitMode: fitMode.value,
        defaultMuted: muted.value,
        onboardingCompleted: onboardingCompleted.value,
      });
      return true;
    } catch (err) {
      errToast("保存设置失败", err);
      return false;
    }
  }

  async function pickFolder(): Promise<boolean> {
    try {
      // 目录由原生选择器授权并由 Rust 持久化；成功后这里只更新界面
      const dir = await api.pickLibraryDirectory();
      if (!dir) return false;
      libraryDir.value = dir;
      if (onboardingCompleted.value) {
        await loadMedia(false);
      } else {
        // The native picker has already persisted the selected directory.
        // Do not hold the setup modal open while a media scan waits on TCC I/O.
        onboardingFolderChosen = true;
      }
      toast(`已切换到 ${dir}`, "success");
      return true;
    } catch (err) {
      errToast("选择文件夹失败", err);
      return false;
    }
  }

  async function importPaths(paths: string[]) {
    if (!paths.length) return;
    try {
      const res = await api.importMedia(paths);
      const saved = res?.saved?.length ?? 0;
      const skipped = res?.skipped?.length ?? 0;
      if (saved) {
        toast(`已导入 ${saved} 个文件${skipped ? `，跳过 ${skipped} 个` : ""}`, "success");
        await loadMedia();
        if (res.saved[0]) selectedPath.value = res.saved[0];
      } else {
        toast(skipped ? "没有可导入的图片或视频" : "后端未导入任何文件", "warning");
      }
    } catch (err) {
      errToast("导入失败", err);
    }
  }

  async function removeMedia(path: string) {
    if (!path) return;
    try {
      await api.deleteMedia(path);
      toast("已删除", "success");
      if (selectedPath.value === path) selectedPath.value = "";
      await loadMedia();
    } catch (err) {
      errToast("删除失败", err);
    }
  }

  // ---- 壁纸应用与控制 ----
  async function applySelected() {
    const item = selected.value;
    if (!item) {
      toast("请先选择一个媒体", "warning");
      return;
    }
    if (!selectedDisplayId.value) {
      toast("请先选择显示器", "warning");
      return;
    }
    applying.value = true;
    try {
      const state = await api.applyWallpaper({
        displayId: selectedDisplayId.value,
        path: item.path,
        kind: item.kind,
        fitMode: fitMode.value,
        muted: muted.value,
      });
      upsertState(state);
      // 这里不保存设置：显示方式/静音在 setFitMode/toggleMuted 时已各自落盘，
      // 逐屏的壁纸分配由后端在应用成功后写；失败的切换不该留下任何配置
      const name = selectedDisplay.value?.name ?? "显示器";
      if (state.phase === "preparing") {
        toast(`正在准备动态壁纸（${name}），首帧就绪后自动切换…`, "info");
      } else if (state.phase === "error" && state.error) {
        toast(state.error, "error");
      } else {
        toast(`已应用到 ${name}`, "success");
      }
    } catch (err) {
      errToast("应用失败", err);
      // 拔屏与点击之间存在竞态：这里报错多半是列表过期，静默重取一次自我修正
      void loadSnapshot();
    } finally {
      applying.value = false;
    }
  }

  async function controlPlayback(action: PlaybackAction) {
    const id = selectedDisplayId.value;
    if (!id) return;
    // 错误态（多为显示器已断开）下的"停止"实际语义是放弃这份配置：不说明的话，
    // 用户只会看到角标和删除保护莫名其妙地消失
    const forget = action === "stop" && canForgetAssignment.value;
    try {
      const state = await api.controlPlayback(id, action);
      upsertState(state);
      if (forget) toast("已解除占用：该显示器的壁纸配置已清除，文件可以删除了", "success");
    } catch (err) {
      errToast(action === "pause" ? "暂停失败" : action === "resume" ? "恢复失败" : "停止失败", err);
    }
  }

  function selectItem(path: string) {
    selectedPath.value = path;
  }

  function selectDisplay(id: string) {
    selectedDisplayId.value = id;
  }

  function setFitMode(mode: FitMode) {
    fitMode.value = mode;
    void saveSettings();
  }

  function toggleMuted() {
    muted.value = !muted.value;
    void saveSettings();
  }

  async function completeOnboarding(): Promise<boolean> {
    onboardingCompleted.value = true;
    if (!await saveSettings()) {
      onboardingCompleted.value = false;
      return false;
    }
    if (onboardingFolderChosen) {
      onboardingFolderChosen = false;
      void loadMedia(false);
    }
    return true;
  }

  function setDragging(v: boolean) {
    dragging.value = v;
  }

  return {
    // state
    libraryDir,
    media,
    displays,
    states,
    selectedPath,
    selectedDisplayId,
    fitMode,
    muted,
    onboardingCompleted,
    snapshotReady,
    loadingMedia,
    mediaError,
    applying,
    dragging,
    toasts,
    // getters
    selected,
    previewSrc,
    selectedDisplay,
    selectedState,
    selectedStatePhase,
    canControlPlayback,
    canControlVideo,
    canStopPlayback,
    canForgetAssignment,
    selectedDisplayOffline,
    hasMedia,
    dirName,
    // actions
    loadSnapshot,
    loadMedia,
    refreshAll,
    pickFolder,
    importPaths,
    removeMedia,
    applySelected,
    controlPlayback,
    selectItem,
    selectDisplay,
    setFitMode,
    toggleMuted,
    completeOnboarding,
    setDragging,
    toast,
    dismiss,
  };
}

export type AppStore = ReturnType<typeof useApp>;

/// 显示器分辨率文案（实际像素宽高）。
/// macOS 的 logicalBounds 是桌面坐标点：Retina 屏 3840×2400 会显示为 1920×1200，
/// 所以界面展示实际像素尺寸，窗口定位仍继续使用 logicalBounds。
export function boundsText(d: DisplayInfo): string {
  const pixelW = Math.round(d.pixelWidth ?? 0);
  const pixelH = Math.round(d.pixelHeight ?? 0);
  if (pixelW && pixelH) {
    const scale = d.scaleFactor > 1 ? ` · ${d.scaleFactor}x` : "";
    return `${pixelW} × ${pixelH}${scale}`;
  }
  const [, , w, h] = d.logicalBounds ?? [0, 0, 0, 0];
  if (!w || !h) return "";
  return `${Math.round(w)} × ${Math.round(h)}`;
}

/// 阶段 → 中文标签 + 语义色（供状态徽章使用）
export const phaseMeta: Record<PlaybackPhase, { label: string; tone: string }> = {
  static: { label: "静态", tone: "text-faint" },
  preparing: { label: "准备中", tone: "text-warn" },
  playing: { label: "播放中", tone: "text-ok" },
  paused: { label: "已暂停", tone: "text-dim" },
  error: { label: "错误", tone: "text-danger" },
};

/// 安全的阶段元信息取值：后端返回未知阶段时回退为「静态」
export function phaseMetaOf(phase: string): { label: string; tone: string } {
  return phaseMeta[phase as PlaybackPhase] ?? phaseMeta.static;
}

/// 用于预览层的 object-fit 映射
export function fitObject(mode: FitMode): "cover" | "contain" {
  return mode === "fill" ? "cover" : "contain";
}

export { localSrc };
