<script setup lang="ts">
// SvgIcon - 少量内联 SVG 图标（替代图标库依赖）
import { computed } from "vue";

const props = defineProps<{
  name:
    | "folder"
    | "import"
    | "refresh"
    | "play"
    | "pause"
    | "stop"
    | "volume"
    | "mute"
    | "trash"
    | "monitor"
    | "image"
    | "film"
    | "check"
    | "search"
    | "close"
    | "settings";
  size?: number;
  /** 实心渲染（播放三角等） */
  fill?: boolean;
}>();

const PATHS: Record<string, string[]> = {
  folder: ["M4 6a2 2 0 0 1 2-2h3.2a1 1 0 0 1 .8.4l1 1.3a1 1 0 0 0 .8.4H18a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2Z"],
  import: ["M12 4v10m0 0 4-4m-4 4-4-4", "M4 17v1a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-1"],
  refresh: ["M20.5 12a8.5 8.5 0 1 1-2.6-6.1", "M20.5 4v5h-5"],
  play: ["M7 4.5 19 12 7 19.5Z"],
  pause: ["M9 5v14", "M15 5v14"],
  stop: ["M7 7h10v10H7Z"],
  volume: ["M11 5 6.5 9H3v6h3.5L11 19Z", "M15.5 9a4 4 0 0 1 0 6", "M18.5 6.5a8 8 0 0 1 0 11"],
  mute: ["M11 5 6.5 9H3v6h3.5L11 19Z", "m16 10 4 4", "m20 10-4 4"],
  trash: ["M4 7h16", "M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2", "M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12"],
  monitor: ["M3 5h18v11H3Z", "M9 20h6", "M12 16v4"],
  image: ["M3 5h18v14H3Z", "M8.5 12a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3", "m5 18 5-5 3 3 3-3 3 3"],
  film: ["M3 5h18v14H3Z", "M8 5v14", "M16 5v14", "M3 12h18"],
  check: ["m5 13 4 4L19 7"],
  search: ["M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z", "m16.5 16.5 4 4"],
  close: ["m6 6 12 12", "m18 6-12 12"],
  settings: ["M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Z", "M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-1.7 2.94-.08-.02a1.7 1.7 0 0 0-1.77.49l-.06.06h-3.4l-.04-.08a1.7 1.7 0 0 0-1.48-1.04 1.7 1.7 0 0 0-1.5.95l-.04.08-2.94-1.7.02-.08a1.7 1.7 0 0 0-.49-1.77l-.06-.06v-3.4l.08-.04a1.7 1.7 0 0 0 1.04-1.48 1.7 1.7 0 0 0-.95-1.5l-.08-.04 1.7-2.94.08.02a1.7 1.7 0 0 0 1.77-.49l.06-.06h3.4l.04.08a1.7 1.7 0 0 0 1.48 1.04 1.7 1.7 0 0 0 1.5-.95l.04-.08 2.94 1.7-.02.08a1.7 1.7 0 0 0 .49 1.77l.06.06v3.4Z"],
};

const d = computed(() => PATHS[props.name] ?? []);
const strokeW = computed(() => (props.name === "pause" ? 2.6 : 1.7));
</script>

<template>
  <svg
    class="shrink-0"
    :width="size ?? 14"
    :height="size ?? 14"
    viewBox="0 0 24 24"
    :fill="fill ? 'currentColor' : 'none'"
    :stroke="fill ? 'none' : 'currentColor'"
    :stroke-width="strokeW"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
  >
    <path v-for="(p, i) in d" :key="i" :d="p" />
  </svg>
</template>
