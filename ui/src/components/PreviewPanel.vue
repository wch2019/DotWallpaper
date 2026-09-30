<script setup lang="ts">
// PreviewPanel - 右侧：选中媒体预览 + 壁纸控制 + 各显示器状态
import { computed, onUnmounted, ref, watch } from "vue";
import { fitObject, phaseMetaOf, useApp } from "../composables/useApp";
import SvgIcon from "./SvgIcon.vue";

const app = useApp();

const item = computed(() => app.selected.value);
const src = computed(() => app.previewSrc.value);
const isVideo = computed(() => item.value?.kind === "video");
const objectFit = computed(() => fitObject(app.fitMode.value));

// 选中的是不在线的显示器：不能应用（后端找不到那块屏），只保留"解除占用"这条退出路径
const targetOffline = computed(() => app.selectedDisplayOffline.value);
const displayBadge = computed(
  () =>
    app.selectedDisplay.value?.name ??
    (app.selectedDisplayId.value ? "已断开显示器" : "未选显示器")
);

// 删除二次确认（避免原生 confirm）
const pendingDelete = ref(false);
let deleteTimer: number | undefined;
watch(item, () => {
  window.clearTimeout(deleteTimer);
  pendingDelete.value = false;
});
function onRequestDelete() {
  if (!item.value) return;
  if (!pendingDelete.value) {
    pendingDelete.value = true;
    deleteTimer = window.setTimeout(() => (pendingDelete.value = false), 4000);
    return;
  }
  window.clearTimeout(deleteTimer);
  pendingDelete.value = false;
  void app.removeMedia(item.value.path);
}
onUnmounted(() => window.clearTimeout(deleteTimer));

const displayStates = computed(() => {
  const rows = app.displays.value.map((d) => {
    const state = app.states.value.find((s) => s.displayId === d.id);
    return {
      id: d.id,
      name: d.name,
      primary: d.primary,
      temporary: d.temporary,
      offline: false,
      phase: state?.phase ?? "static",
      current: state?.assignment?.path?.split(/[\\/]/).pop() ?? "",
      error: state?.error ?? "",
    };
  });
  // 状态镜像里可能留着已不在线的显示器（拔出后后端推 error）。这条记录仍占着持久化
  // 分配和文件删除保护，必须可见可选中——否则用户没有任何"解除占用"的入口。
  for (const s of app.states.value) {
    if (app.displays.value.some((d) => d.id === s.displayId)) continue;
    rows.push({
      id: s.displayId,
      name: "已断开显示器",
      primary: false,
      temporary: true,
      offline: true,
      phase: s.phase,
      current: s.assignment?.path?.split(/[\\/]/).pop() ?? "",
      error: s.error ?? "",
    });
  }
  return rows;
});
</script>

<template>
  <aside class="dw-inspector flex shrink-0 flex-col gap-3 overflow-y-auto p-4">
    <!-- 预览卡片 -->
    <div class="dw-preview-card mac-card overflow-hidden">
      <div class="flex min-h-10 items-center gap-2 border-b border-line px-3">
        <SvgIcon :name="isVideo ? 'film' : 'image'" :size="12" class="text-faint" />
        <span class="min-w-0 flex-1">
          <span class="block truncate text-[12px] font-semibold text-tx" :title="item?.name">
            {{ item?.name ?? "选择一张壁纸" }}
          </span>
          <span class="block text-[10.5px] text-faint">{{ item ? "实时预览" : "从左侧媒体库开始" }}</span>
        </span>
        <span v-if="item" class="mac-badge text-faint">
          {{ item.kind === "video" ? "视频" : "图片" }}
        </span>
      </div>

      <div class="dw-preview-stage media-fallback relative aspect-video w-full">
        <video
          v-if="item && isVideo && src"
          :key="src"
          class="h-full w-full"
          :style="{ objectFit }"
          :src="src"
          :muted="app.muted.value"
          autoplay
          loop
          playsinline
          preload="metadata"
          controlslist="nodownload"
        ></video>
        <img
          v-else-if="item && src"
          :key="src"
          class="h-full w-full"
          :style="{ objectFit }"
          :src="src"
          :alt="item.name"
          draggable="false"
        />
        <div v-else class="absolute inset-0 flex flex-col items-center justify-center gap-1.5 text-center">
          <SvgIcon name="image" :size="22" class="text-faint" />
          <span class="text-[11px] text-faint">在左侧选择图片或视频</span>
        </div>

        <!-- 当前显示器阶段 -->
        <span
          v-if="item"
          class="absolute left-2 top-2 flex items-center gap-1 rounded-md bg-panel/85 px-1.5 py-[3px] text-[10px] backdrop-blur-sm"
          :class="phaseMetaOf(app.selectedStatePhase.value).tone"
        >
          {{ displayBadge }} ·
          {{ phaseMetaOf(app.selectedStatePhase.value).label }}
        </span>
      </div>

      <!-- 名称路径 -->
      <p
        v-if="item"
        class="truncate border-t border-line px-3 py-2 text-[10.5px] text-faint"
        :title="item.path"
      >
        {{ item.path }}
      </p>
    </div>

    <!-- 控制区 -->
    <div class="dw-control-card mac-card flex flex-col gap-3 p-3.5">
      <div class="flex items-center justify-between gap-2">
        <span class="text-[11px] text-faint">显示方式</span>
        <div class="mac-segmented" role="radiogroup" aria-label="显示方式">
          <button
            class="mac-segment cursor-pointer"
            :class="app.fitMode.value === 'fill' ? 'mac-segment-active' : ''"
            role="radio"
            :aria-checked="app.fitMode.value === 'fill'"
            @click="app.setFitMode('fill')"
          >
            填充
          </button>
          <button
            class="mac-segment cursor-pointer"
            :class="app.fitMode.value === 'fit' ? 'mac-segment-active' : ''"
            role="radio"
            :aria-checked="app.fitMode.value === 'fit'"
            @click="app.setFitMode('fit')"
          >
            适应
          </button>
        </div>
      </div>

      <div class="flex items-center justify-between gap-2">
        <span class="flex items-center gap-1.5 text-[11px] text-faint">
          <SvgIcon :name="app.muted.value ? 'mute' : 'volume'" :size="13" :class="app.muted.value ? 'text-dim' : 'text-accent'" />
          静音播放
        </span>
        <button
          class="mac-switch"
          :class="app.muted.value ? 'mac-switch-on' : ''"
          role="switch"
          :aria-checked="app.muted.value"
          @click="app.toggleMuted()"
        >
          <span class="mac-switch-knob"></span>
        </button>
      </div>

      <button
        class="mac-btn mac-btn-primary w-full"
        :disabled="!item || app.applying.value || !app.selectedDisplayId.value || targetOffline"
        @click="app.applySelected()"
      >
        <SvgIcon name="check" :size="12" />
        {{ app.applying.value ? "应用中…" : "应用壁纸" }}
      </button>
      <p v-if="targetOffline" class="-mt-2 text-[10.5px] text-warn">
        该显示器未在线：换一块在线的显示器才能应用；这里只能用下方「解除占用」清掉它的配置。
      </p>

      <div class="grid grid-cols-3 gap-1.5">
        <button
          class="mac-btn !px-0 text-[11.5px]"
          :disabled="!app.canControlVideo.value"
          title="暂停所选显示器的动态壁纸"
          @click="app.controlPlayback('pause')"
        >
          <SvgIcon name="pause" :size="12" />
          暂停
        </button>
        <button
          class="mac-btn !px-0 text-[11.5px]"
          :disabled="!app.canControlVideo.value"
          title="恢复播放"
          @click="app.controlPlayback('resume')"
        >
          <SvgIcon name="play" :size="11" fill />
          恢复
        </button>
        <button
          class="mac-btn !px-0 text-[11.5px]"
          :disabled="!app.canStopPlayback.value"
          :title="
            app.canForgetAssignment.value
              ? '该显示器不可用（已断开或壁纸恢复失败）：这里停止会清除它的壁纸配置，' +
                '同时解除对该壁纸文件的占用，之后才能删除文件'
              : '停止该显示器的壁纸'
          "
          @click="app.controlPlayback('stop')"
        >
          <SvgIcon name="stop" :size="11" />
          {{ app.canForgetAssignment.value ? "解除占用" : "停止" }}
        </button>
      </div>

      <!-- 准备中：暂停/恢复此刻不可用，需要说明而不是让用户对着灰按钮猜 -->
      <p v-if="app.selectedStatePhase.value === 'preparing'" class="-mt-2 text-[10.5px] text-warn">
        动态壁纸准备中：可以立即停止，暂停 / 恢复要等首帧就绪。
      </p>

      <button
        class="mac-btn mac-btn-danger w-full !bg-transparent !border-line"
        :disabled="!item"
        @click="onRequestDelete"
      >
        <SvgIcon name="trash" :size="12" />
        {{ pendingDelete ? "再次点击确认删除" : "删除文件" }}
      </button>
      <p v-if="pendingDelete" class="-mt-2 text-[10.5px] text-danger">
        将从磁盘永久删除该文件，4 秒内再次点击确认。
      </p>
    </div>

    <!-- 显示器状态 -->
    <div class="mac-card flex flex-col gap-1.5 p-3">
      <span class="text-[11px] font-medium text-faint">显示器状态</span>
      <div v-if="!displayStates.length" class="text-[11px] text-faint">未检测到显示器</div>
      <button
        v-for="d in displayStates"
        :key="d.id"
        class="flex w-full cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-panel-2"
        :class="d.id === app.selectedDisplayId.value ? 'bg-panel-2' : ''"
        @click="app.selectDisplay(d.id)"
      >
        <span class="h-1.5 w-1.5 shrink-0 rounded-full" :class="d.id === app.selectedDisplayId.value ? 'bg-accent' : 'bg-line-2'"></span>
        <span class="min-w-0 flex-1">
          <span class="block truncate text-[11.5px] text-tx">
            {{ d.name }}<span v-if="d.primary" class="text-faint"> · 主</span
            ><span v-if="d.offline" class="text-faint" :title="`稳定 ID：${d.id}`"> · 未在线</span
            ><span
              v-else-if="d.temporary"
              class="text-faint"
              title="该显示器未上报序列号：ID 仅在本次连接内有效，重新插拔后需重新指定壁纸"
              > · 临时 ID</span
            >
          </span>
          <span v-if="d.current" class="block truncate text-[10px] text-faint" :title="d.current">{{ d.current }}</span>
          <span v-if="d.error" class="block truncate text-[10px] text-danger" :title="d.error">{{ d.error }}</span>
        </span>
        <span class="mac-badge shrink-0" :class="phaseMetaOf(d.phase).tone">
          {{ phaseMetaOf(d.phase).label }}
        </span>
      </button>
    </div>
  </aside>
</template>
