<script setup lang="ts">
// TopBar - 顶部工具栏：壁纸目录 + 选择文件夹 / 导入文件 + 显示器选择
import { computed, ref } from "vue";
import { boundsText, useApp } from "../composables/useApp";
import { api, errMessage } from "../lib/api";
import SvgIcon from "./SvgIcon.vue";

const app = useApp();
const emit = defineEmits<{ "open-settings": [] }>();
const fileInput = ref<HTMLInputElement | null>(null);

const isNativeBridge = typeof window !== "undefined" && !!window.DotWallpaperNative;

const displayLabel = computed(() => {
  const d = app.selectedDisplay.value;
  if (!d) {
    // 选中的是不在线的显示器（右侧状态列表仍可选中它来解除占用）：不能显示成空白下拉
    return app.selectedDisplayId.value ? "该显示器已断开" : "未检测到显示器";
  }
  const res = boundsText(d);
  const tags = [
    d.primary ? "主" : null,
    d.mirrored ? "镜像" : null,
    d.temporary ? "临时 ID" : null,
  ]
    .filter(Boolean)
    .join(" · ");
  return [d.name, res, tags].filter(Boolean).join("  ");
});

// 无 EDID 序列号时 ID 会随拔插变化，需明确告知用户配置可能无法自动恢复
const displayTitle = computed(() => {
  const d = app.selectedDisplay.value;
  if (!d?.temporary) return displayLabel.value;
  return `${displayLabel.value}\n该显示器未上报序列号，此 ID 仅在本次连接内有效：重新插拔后需要重新指定壁纸。`;
});

const displayCount = computed(() => app.displays.value.length);

// 浏览器开发模式没有绝对路径权限；原生 WKWebView 则使用 Swift 提供的文件选择器
function onFileChange(e: Event) {
  const input = e.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  input.value = "";
  if (!files.length) return;
  const paths = files
    .map((f) => (f as File & { path?: string }).path || f.name)
    .filter((p) => p.startsWith("/") || /^[a-zA-Z]:[\\/]/.test(p));
  if (!paths.length) {
    app.toast("当前 WebView 未提供文件绝对路径，请把图片或视频直接拖入窗口", "warning");
    return;
  }
  void app.importPaths(paths);
}

async function clickImport() {
  if (!isNativeBridge) {
    fileInput.value?.click();
    return;
  }
  try {
    const paths = await api.pickMediaFiles();
    if (paths.length) await app.importPaths(paths);
  } catch (err) {
    app.toast(`导入失败：${errMessage(err)}`, "error");
  }
}
</script>

<template>
  <header
    class="dw-toolbar flex h-12 shrink-0 items-center gap-2 px-3.5"
  >
    <!-- 目录入口：权限和路径操作统一放在设置中 -->
    <button class="flex min-w-0 flex-1 items-center gap-2 text-left" title="在设置中查看或更改壁纸目录" @click="emit('open-settings')">
      <span class="dw-toolbar-mark" aria-hidden="true">
        <SvgIcon name="folder" :size="14" class="text-accent" />
      </span>
      <span
        class="min-w-0 truncate text-[12px] text-tx"
        :title="app.libraryDir.value || '未选择壁纸目录'"
        >{{ app.dirName.value }}</span
      >
    </button>

    <!-- 操作：窄窗口下收敛为图标按钮，靠 tooltip 说明 -->
    <button class="mac-btn !px-1.5" title="导入图片或视频" @click="clickImport">
      <SvgIcon name="import" :size="14" />
    </button>
    <button
      class="mac-btn !px-1.5"
      title="重新加载列表（⌘R）"
      :disabled="app.loadingMedia.value"
      @click="app.refreshAll()"
    >
      <SvgIcon
        name="refresh"
        :size="13"
        :class="app.loadingMedia.value ? 'animate-spin' : ''"
      />
    </button>

    <span v-if="displayCount" class="dw-toolbar-divider mx-1 h-5 w-px shrink-0"></span>

    <button class="mac-btn !px-1.5" title="设置" aria-label="设置" @click="emit('open-settings')">
      <SvgIcon name="settings" :size="14" />
    </button>

    <!-- 显示器选择 -->
    <label class="flex min-w-0 items-center gap-1.5">
      <SvgIcon name="monitor" :size="13" class="text-faint" />
      <select
        class="mac-select"
        :value="app.selectedDisplayId.value"
        :title="displayTitle"
        :disabled="!displayCount"
        @change="app.selectDisplay(($event.target as HTMLSelectElement).value)"
      >
        <option
          v-if="app.selectedDisplayId.value && !app.selectedDisplay.value"
          :value="app.selectedDisplayId.value"
          disabled
        >
          {{ displayLabel }}
        </option>
        <option v-for="d in app.displays.value" :key="d.id" :value="d.id">
          {{ d.name }}{{ d.primary ? "（主）" : "" }}{{ d.temporary ? "（临时）" : "" }} {{ boundsText(d) }}
        </option>
      </select>
    </label>

    <input
      ref="fileInput"
      type="file"
      multiple
      accept="image/*,video/*"
      class="hidden"
      @change="onFileChange"
    />
  </header>
</template>
