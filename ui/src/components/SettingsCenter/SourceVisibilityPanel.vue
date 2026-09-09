<script setup lang="ts">
// 壁纸来源显示 — 控制左侧选项卡的展示/隐藏与排列顺序（本地默认强制开启）
import { ref } from "vue";
import { NIcon, NSwitch } from "naive-ui";
import { Eye, FolderHeart, GripVertical, Layers, Monitor } from "lucide-vue-next";
import { useWallpaperStore } from "@/stores/wallpaper";
import type { WallpaperSource } from "@/stores/wallpaper";

const store = useWallpaperStore();

// 来源选项配置：local 不允许关闭（开关禁用且始终开启）
const sourceRows = [
  {
    key: "local",
    label: "本地壁纸",
    desc: "展示本地壁纸目录中的图片（默认必开，不可关闭）",
    icon: Layers,
    locked: true,
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

function isVisible(key: (typeof sourceRows)[number]["key"]): boolean {
  if (key === "local") return true;
  return key === "system"
    ? store.sourceVisibility.system
    : store.sourceVisibility.favorites;
}

function onChange(key: "system" | "favorites", v: boolean) {
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
            <div class="mt-0.5 truncate text-[11px] text-faint">{{ rowByKey[key].desc }}</div>
          </div>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          <span class="mx-1 h-4 w-px bg-white/10"></span>
          <n-switch
            v-if="rowByKey[key].key !== 'local'"
            :value="rowByKey[key].key === 'system' ? store.sourceVisibility.system : store.sourceVisibility.favorites"
            size="small"
            @update:value="(v: boolean) => onChange(rowByKey[key].key as 'system' | 'favorites', v)"
          />
          <!-- 本地：始终开启且禁改 -->
          <n-switch v-else size="small" :value="true" disabled />
        </div>
      </div>
    </div>

    <div class="src-note mt-4 flex items-center gap-2">
      <NIcon :component="Layers" :size="13" class="note-icon shrink-0" />
      <span>本地壁纸是应用主源，始终展示；隐藏“系统 / 收藏”后对应选项卡将从左侧消失，数据不会删除。按住左侧手柄上下拖动可调整选项卡的展示顺序，自动保存</span>
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
}
.drag-handle {
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
</style>
