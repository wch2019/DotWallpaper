<script setup lang="ts">
// 壁纸效果 — 模糊 + 遮罩（MVP：滑块实时联动右栏预览，设为壁纸时由后端合成）
import { computed } from "vue";
import { NColorPicker, NIcon, NSlider, NSwitch } from "naive-ui";
import { Palette, Sparkles, Wand2 } from "lucide-vue-next";
import { useWallpaperStore } from "@/stores/wallpaper";

const store = useWallpaperStore();

const enabled = computed(() => store.wallpaperEffect.enabled);
const blur = computed(() => store.wallpaperEffect.blur);
const opacity = computed(() => store.wallpaperEffect.opacity);
const color = computed(() => store.wallpaperEffect.color);

function setEnabled(v: boolean) {
  store.setEffect({ enabled: v });
}
function setBlur(v: number) {
  store.setEffect({ blur: v });
}
function setOpacity(v: number) {
  store.setEffect({ opacity: v });
}
function setColor(v: string) {
  store.setEffect({ color: v });
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Wand2" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">壁纸效果</div>
      <div class="panel-desc">模糊 + 遮罩，设为壁纸时自动合成</div>
    </div>

    <div class="mt-4 flex flex-col gap-2.5">
      <!-- 启用开关 -->
      <div
        class="effect-row flex items-center justify-between gap-2 rounded-lg bg-panel-2 px-3 py-3"
      >
        <div class="flex min-w-0 flex-1 items-center gap-2.5">
          <span
            class="fx-icon flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
            :class="enabled ? 'bg-accent-soft text-accent' : 'bg-white/5 text-faint'"
          >
            <NIcon :component="Sparkles" :size="16" />
          </span>
          <div class="min-w-0">
            <div class="text-[12.5px] font-medium text-tx">启用效果</div>
            <!-- 说明文案在窄面板里会截断 → 悬停看全文 -->
            <div
              class="mt-0.5 truncate text-[11px] text-faint"
              title="开启后右栏预览实时模拟，设为壁纸时对原图合成模糊与遮罩"
            >
              开启后右栏预览实时模拟，设为壁纸时对原图合成模糊与遮罩
            </div>
          </div>
        </div>
        <n-switch :value="enabled" size="small" @update:value="setEnabled" />
      </div>

      <!-- 模糊强度 -->
      <div class="slider-row rounded-lg bg-panel-2 px-3 py-3" :class="{ disabled: !enabled }">
        <div class="flex items-center justify-between">
          <span class="text-[12.5px] font-medium text-tx">模糊强度</span>
          <span class="slider-value rounded bg-white/5 px-1.5 py-px font-mono text-[11px] text-accent">
            {{ blur }}px
          </span>
        </div>
        <div class="mt-2">
          <n-slider
            :value="blur"
            :min="0"
            :max="30"
            :step="1"
            :disabled="!enabled"
            tooltip
            @update:value="setBlur"
          />
        </div>
        <div class="mt-1 flex justify-between text-[10px] text-faint">
          <span>0 · 原图</span>
          <span>30 · 强模糊</span>
        </div>
      </div>

      <!-- 遮罩不透明度 -->
      <div class="slider-row rounded-lg bg-panel-2 px-3 py-3" :class="{ disabled: !enabled }">
        <div class="flex items-center justify-between">
          <span class="text-[12.5px] font-medium text-tx">遮罩不透明度</span>
          <span class="slider-value rounded bg-white/5 px-1.5 py-px font-mono text-[11px] text-accent">
            {{ opacity }}%
          </span>
        </div>
        <div class="mt-2">
          <n-slider
            :value="opacity"
            :min="0"
            :max="80"
            :step="1"
            :disabled="!enabled"
            tooltip
            @update:value="setOpacity"
          />
        </div>
        <div class="mt-1 flex justify-between text-[10px] text-faint">
          <span>0 · 无遮罩</span>
          <span>80 · 深色</span>
        </div>
      </div>

      <!-- 遮罩颜色 -->
      <div class="slider-row rounded-lg bg-panel-2 px-3 py-3" :class="{ disabled: !enabled }">
        <div class="flex items-center justify-between">
          <span class="flex items-center gap-1.5 text-[12.5px] font-medium text-tx">
            <NIcon :component="Palette" :size="13" class="text-faint" />
            遮罩颜色
          </span>
          <n-color-picker
            :value="color"
            :disabled="!enabled"
            :show-alpha="false"
            size="small"
            style="width: 132px"
            @update:value="setColor"
          />
        </div>
        <div class="mt-1.5 text-[10px] text-faint">点击色块取色，预览与合成同步使用该颜色</div>
      </div>
    </div>

    <div class="fx-note mt-4 flex items-center gap-2">
      <NIcon :component="Sparkles" :size="13" class="note-icon shrink-0" />
      <span>
        支持自定义遮罩颜色；参数保存在本地并立即生效，右栏预览与放大预览会同步显示。设为壁纸时后端按“模糊 → 遮罩 → 填充”合成新图，不修改你的原图。
      </span>
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
.fx-icon {
  transition: all 0.2s;
}
.slider-row {
  transition: opacity 0.2s;
}
.slider-row.disabled {
  opacity: 0.5;
}
.slider-value {
  font-variant-numeric: tabular-nums;
}
.fx-note {
  padding: 0.6rem 0.8rem;
  border-radius: 8px;
  background: rgba(127, 168, 255, 0.06);
  border: 1px solid rgba(127, 168, 255, 0.12);
}
.note-icon {
  color: var(--color-accent);
}
.fx-note span {
  font-size: 11.5px;
  color: var(--color-dim);
  line-height: 1.5;
}
</style>
