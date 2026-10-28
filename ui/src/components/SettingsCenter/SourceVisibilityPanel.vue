<script setup lang="ts">
// 壁纸来源显示 — 控制左侧选项卡的展示/隐藏与排列顺序（本地默认强制开启）
import { ref } from "vue";
import { NIcon, NInput, NSelect, NSwitch } from "naive-ui";
import { Eye, Film, FolderHeart, Globe, GripVertical, KeyRound, Layers, Monitor, Sparkles } from "lucide-vue-next";
import { toast } from "@/lib/naive-host";
import { useWallpaperStore } from "@/stores/wallpaper";
import type { WallpaperSource } from "@/stores/wallpaper";
import { PIXABAY_VIDEO_QUALITIES } from "@/stores/wallpaper";
import type { PixabayVideoQuality } from "@/stores/wallpaper";

const store = useWallpaperStore();

/// 视频清晰度选项（NSelect 要可变数组，常量保持字面量类型）
const QUALITY_OPTIONS = PIXABAY_VIDEO_QUALITIES.map((q) => ({
  label: q.label,
  value: q.value,
}));

// 来源选项配置：local 不允许关闭（开关禁用且始终开启）
// 注意：sourceOrder 中若有历史遗留的未知 key，rowByKey[key] 会取不到配置，
// 模板中已用 rowByKey[key] && 做空值保护
const sourceRows = [
  {
    key: "local",
    label: "本地壁纸",
    desc: "展示本地壁纸目录中的图片（默认必开，不可关闭）",
    icon: Layers,
    locked: true,
  },
  {
    key: "bing",
    label: "必应壁纸",
    desc: "展示必应每日壁纸（在线图源，设为桌面时自动下载到本地）",
    icon: Globe,
    locked: false,
  },
  {
    key: "pixabay",
    label: "Pixabay 图库",
    desc: "展示 Pixabay 在线图库（需自填 API key，设为桌面时自动下载）",
    icon: Sparkles,
    locked: false,
  },
  {
    key: "system",
    label: "系统壁纸",
    desc: "展示 Windows 自带的系统壁纸（只读）",
    icon: Monitor,
    locked: false,
  },
  {
    key: "favorites",
    label: "收藏",
    desc: "展示你收藏的壁纸书签",
    icon: FolderHeart,
    locked: false,
  },
] as const;

// 各来源的可见性取值（local 恒为 true）
function visValue(key: WallpaperSource): boolean {
  return key === "local" ? true : store.sourceVisibility[key];
}

function isVisible(key: WallpaperSource): boolean {
  return visValue(key);
}

function onChange(key: Exclude<WallpaperSource, "local">, v: boolean) {
  store.setSourceVisibility(key, v);
}

// 来源配置按 key 索引（排序行复用同一份 label/icon）
const rowByKey = Object.fromEntries(
  sourceRows.map((row) => [row.key, row])
) as Record<(typeof sourceRows)[number]["key"], (typeof sourceRows)[number]>;

// ---- 手动拖拽排序（Pointer Events 自绘拖拽，绕开 WebView2 原生 DnD 禁用光标问题） ----
const dragKey = ref<WallpaperSource | null>(null);
const overKey = ref<WallpaperSource | null>(null);
let dragActive = false;
let lastTargetKey: WallpaperSource | null = null;

function onHandlePointerDown(e: PointerEvent, key: WallpaperSource) {
  if (e.button !== 0) return;
  e.preventDefault();
  // 捕获指针，保证移出组件/窗口边缘时仍能收到 move/up
  const el = e.currentTarget as HTMLElement;
  el.setPointerCapture?.(e.pointerId);
  dragActive = true;
  dragKey.value = key;
  overKey.value = null;
  lastTargetKey = null;
  window.addEventListener("pointermove", onDragPointerMove);
  window.addEventListener("pointerup", onDragPointerUp);
  window.addEventListener("pointercancel", onDragPointerUp);
}

function onDragPointerMove(e: PointerEvent) {
  if (!dragActive || !dragKey.value) return;
  // 命中测试指针当前所在行，跨行即实时落位
  const hit = document
    .elementFromPoint(e.clientX, e.clientY)
    ?.closest(".src-row") as HTMLElement | null;
  const key = (hit?.dataset.key as WallpaperSource | undefined) ?? null;
  if (key && key !== dragKey.value && key !== lastTargetKey) {
    lastTargetKey = key;
    overKey.value = key;
    store.moveSourceOrderTo(dragKey.value, key);
  }
}

function onDragPointerUp() {
  dragActive = false;
  dragKey.value = null;
  overKey.value = null;
  lastTargetKey = null;
  window.removeEventListener("pointermove", onDragPointerMove);
  window.removeEventListener("pointerup", onDragPointerUp);
  window.removeEventListener("pointercancel", onDragPointerUp);
}

// ---- Pixabay API key ----
// 这里是**唯一**可以修改/清除已保存 key 的地方。
// 侧栏空态里那个就地填写框只在"还没填过"时出现，填过之后就再也不显示了，
// 所以没有这块用户就永远换不了 key（填错了只能删 localStorage）。
const keyInput = ref("");

function savePixabayKey() {
  const v = keyInput.value.trim();
  if (!v) {
    toast("请先粘贴 API key", "warning");
    return;
  }
  store.setPixabayKey(v);
  keyInput.value = "";
  if (store.source === "pixabay") void store.reloadPixabay();
  toast("已保存，正在加载…", "success");
}

function clearPixabayKey() {
  store.setPixabayKey("");
  keyInput.value = "";
  // setPixabayKey 只改状态不重载列表（它不是 UI 动作），清除后必须自己刷一次，
  // 否则当前停在该选项卡时会留着一屏用已失效 key 拉回来的结果。
  if (store.source === "pixabay") void store.reloadPixabay();
  toast("已清除 Pixabay API key", "success");
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Eye" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">壁纸来源</div>
      <div class="panel-desc">控制左侧选项卡的显示</div>
    </div>

    <div class="src-list mt-4 flex flex-col gap-2">
      <div
        v-for="key in store.sourceOrder"
        :key="key"
        class="src-row flex items-center justify-between gap-2 rounded-lg bg-panel-2 px-3 py-3"
        :data-key="key"
        :class="{
          'is-dragging': dragKey === key,
          'drag-over': overKey === key && dragKey !== key,
        }"
      >
        <div class="flex min-w-0 flex-1 items-center gap-2.5">
          <NIcon
            :component="GripVertical"
            :size="14"
            class="drag-handle shrink-0"
            :class="{ 'cursor-grabbing': dragActive && dragKey === key }"
            @pointerdown="onHandlePointerDown($event, key)"
          />
          <span
            class="src-icon flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
            :class="isVisible(rowByKey[key].key) ? 'bg-accent-soft text-accent' : 'bg-white/5 text-faint'"
          >
            <NIcon :component="rowByKey[key].icon" :size="16" />
          </span>
          <div class="min-w-0">
            <div class="flex items-center gap-1.5 text-[12.5px] font-medium text-tx">
              {{ rowByKey[key].label }}
              <span
                v-if="rowByKey[key].locked"
                class="rounded bg-white/5 px-1 py-px text-[10px] text-faint"
              >
                默认
              </span>
            </div>
            <!-- 来源说明在窄面板里会截断 → 悬停看全文 -->
            <div class="mt-0.5 truncate text-[11px] text-faint" :title="rowByKey[key].desc">{{ rowByKey[key].desc }}</div>
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <span class="mx-1 h-4 w-px bg-white/10"></span>
          <n-switch
            v-if="key !== 'local'"
            :value="visValue(key)"
            size="small"
            @update:value="(v: boolean) => onChange(key as Exclude<WallpaperSource, 'local'>, v)"
          />
          <!-- 本地：始终开启且禁改 -->
          <n-switch v-else size="small" :value="true" disabled />
        </div>
      </div>
    </div>

    <div class="src-note mt-4 flex items-center gap-2">
      <NIcon :component="Layers" :size="13" class="note-icon shrink-0" />
      <span>本地壁纸是应用主源，始终展示。「系统 / 必应 / Pixabay」默认关闭，打开开关后对应选项卡才会出现在左侧；关掉只是隐藏选项卡，已下载的数据不会删除。按住左侧手柄上下拖动可调整选项卡的展示顺序，自动保存</span>
    </div>

    <!-- Pixabay API key：**唯一**能修改/清除已保存 key 的地方。
         侧栏空态里那个就地填写框只在"还没填过"时出现，填过之后就不显示了，
         所以没有这块，用户填错 key 之后就只能去删 localStorage。 -->
    <div class="pixabay-key mt-3 rounded-lg bg-panel-2 px-3 py-3">
      <div class="flex items-center gap-2">
        <NIcon :component="KeyRound" :size="14" class="shrink-0 text-accent" />
        <span class="text-[12.5px] font-medium text-tx">Pixabay API key</span>
        <span
          class="ml-auto text-[11px]"
          :class="store.pixabayKey ? 'text-ok' : 'text-faint'"
        >
          {{ store.pixabayKey ? "已配置" : "未配置" }}
        </span>
      </div>
      <p class="mt-1.5 text-[11px] leading-relaxed text-faint">
        在 pixabay.com 免费注册即可获取。key 只保存在本机，不会上传、也不会写入日志。
        免费 key 只能拿到 1280px 图片，申请 full access 后自动升级为原图。
      </p>
      <div class="mt-2 flex items-center gap-1.5">
        <NInput
          v-model:value="keyInput"
          type="password"
          show-password-on="click"
          size="small"
          placeholder="粘贴 API key"
          @keyup.enter="savePixabayKey"
        />
        <button class="key-btn key-btn-primary" @click="savePixabayKey">保存</button>
        <button v-if="store.pixabayKey" class="key-btn" @click="clearPixabayKey">清除</button>
      </div>

      <!-- 视频清晰度：只在「Pixabay → 视频」时生效。
           放在 key 同一张卡里 —— 同属"Pixabay 这个源的配置"，
           单开一块会让设置页多一段标题、多一层视觉层级。
           两个选项的代价（体积）必须写出来：这是用户唯一需要权衡的地方。 -->
      <div
        class="mt-2.5 flex items-center gap-2 border-t border-line/70 pt-2.5"
        title="搜索 Pixabay 视频时优先拿哪个清晰度；素材没有 4K 版时会自动降到 1080p"
      >
        <NIcon :component="Film" :size="13" class="shrink-0 text-dim" />
        <span class="shrink-0 text-[11.5px] text-dim">视频清晰度</span>
        <NSelect
          :value="store.pixabayVideoQuality"
          size="small"
          class="quality-select"
          :options="QUALITY_OPTIONS"
          @update:value="(v: string) => store.setPixabayVideoQuality(v as PixabayVideoQuality)"
        />
      </div>
    </div>
  </div>
</template>

<style scoped>
.panel-header {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.panel-icon {
  flex-shrink: 0;
}
.panel-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-tx);
}
.panel-desc {
  margin-left: auto;
  font-size: 11.5px;
  color: var(--color-faint);
}
.src-icon {
  transition: all 0.2s;
}
.src-note {
  padding: 0.6rem 0.8rem;
  border-radius: 8px;
  background: rgba(127, 168, 255, 0.06);
  border: 1px solid rgba(127, 168, 255, 0.12);
}
.note-icon {
  color: var(--color-accent);
}
.src-note span {
  font-size: 11.5px;
  color: var(--color-dim);
  line-height: 1.5;
}
.src-list {
  user-select: none;
}.drag-handle {
  color: var(--color-faint);
  opacity: 0.6;
  cursor: grab;
  touch-action: none;
  transition: opacity 0.2s;
}
.src-row:hover .drag-handle {
  opacity: 1;
}
.src-row.is-dragging {
  opacity: 0.45;
  border: 1px dashed rgba(127, 168, 255, 0.5);
}
.src-row.drag-over {
  border: 1px solid var(--color-accent);
  box-shadow: 0 0 0 2px rgba(127, 168, 255, 0.25);
}

/* Pixabay key 的按钮：shrink-0 必须有 —— 输入框是 flex:1，按钮被压缩就会裁字 */
.key-btn {
  flex: 0 0 auto;
  padding: 3px 10px;
  font-size: 11.5px;
  white-space: nowrap;
  color: var(--color-dim);
  cursor: pointer;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid var(--color-line);
  border-radius: 6px;
  transition: all 0.2s;
}
.key-btn:hover {
  color: var(--color-tx);
  background: rgba(255, 255, 255, 0.1);
}
.key-btn-primary {
  color: var(--color-accent);
  background: rgba(127, 168, 255, 0.12);
  border-color: rgba(127, 168, 255, 0.4);
}
.key-btn-primary:hover {
  color: var(--color-accent);
  background: rgba(127, 168, 255, 0.2);
}

/* 视频清晰度下拉：定宽而不是 flex-1 ——
   它是"左标签 + 右控件"的一行，拉满会把标签和控件拉得很远，读起来散。
   152px 够放下「1080p（推荐）」这种最长标签，且不会挤到下一行。 */
.quality-select {
  flex: 0 0 auto;
  width: 152px;
}
/* NSelect 内部输入框有基础宽度、不跟随外层收缩，必须显式撑满 */
.quality-select :deep(.n-base-selection) {
  width: 100%;
}
</style>
