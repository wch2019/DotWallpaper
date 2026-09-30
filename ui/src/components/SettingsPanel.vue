<script setup lang="ts">
import { ref } from "vue";
import { useApp } from "../composables/useApp";
import SvgIcon from "./SvgIcon.vue";
const emit = defineEmits<{ close: [] }>();
const app = useApp();
const choosing = ref(false);
async function chooseDirectory() {
  if (choosing.value) return;
  choosing.value = true;
  try { await app.pickFolder(); } finally { choosing.value = false; }
}
function revealConfigPath() {
  return "~/Library/Application Support/com.dot.wallpaper/settings.json";
}
</script>
<template>
  <div class="dw-modal-backdrop" @click.self="emit('close')" @keydown.esc="emit('close')">
    <section class="dw-settings mac-card" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <header class="flex items-center justify-between border-b border-line px-5 py-4">
        <div><p class="dw-eyebrow">PREFERENCES</p><h2 id="settings-title" class="mt-1 text-base font-semibold">设置</h2></div>
        <button class="mac-btn !px-2" aria-label="关闭设置" @click="emit('close')"><SvgIcon name="close" :size="14" /></button>
      </header>
      <div class="flex flex-col gap-5 overflow-y-auto p-5">
        <section>
          <h3 class="text-xs font-semibold">媒体库</h3>
          <p class="mt-1 text-[11px] leading-relaxed text-faint">仅扫描你选择的文件夹；换目录不会删除原有文件。若系统无法读取，请在这里重新选择以恢复访问。</p>
          <code class="mt-3 block break-all rounded-lg bg-bg-2 p-3 text-[11px] text-dim">{{ app.libraryDir.value || '尚未选择目录' }}</code>
          <button class="mac-btn mt-3" :disabled="choosing" @click="chooseDirectory"><SvgIcon name="folder" :size="13" />{{ choosing ? '正在打开…' : app.libraryDir.value ? '更改或重新授权目录' : '选择壁纸目录' }}</button>
        </section>
        <section class="border-t border-line pt-4">
          <h3 class="text-xs font-semibold">默认播放</h3>
          <div class="mt-3 flex items-center justify-between"><span class="text-xs text-dim">图片显示方式</span><div class="mac-segmented"><button class="mac-segment" :class="app.fitMode.value==='fill'?'mac-segment-active':''" @click="app.setFitMode('fill')">填充</button><button class="mac-segment" :class="app.fitMode.value==='fit'?'mac-segment-active':''" @click="app.setFitMode('fit')">适应</button></div></div>
          <div class="mt-3 flex items-center justify-between"><span class="text-xs text-dim">视频默认静音</span><button class="mac-switch" :class="app.muted.value?'mac-switch-on':''" role="switch" :aria-checked="app.muted.value" @click="app.toggleMuted()"><span class="mac-switch-knob" /></button></div>
        </section>
        <section class="border-t border-line pt-4">
          <h3 class="text-xs font-semibold">文件与隐私</h3>
          <p class="mt-1 text-[11px] leading-relaxed text-faint">目录通过 macOS 系统文件选择器选取。无需屏幕录制或辅助功能权限。遇到读取失败时，可在上方重新选择目录；视频解码能力由系统支持的格式决定。</p>
          <p class="mt-3 text-[10px] text-faint">设置文件</p><code class="mt-1 block break-all rounded-lg bg-bg-2 p-2 text-[10px] text-dim">{{ revealConfigPath() }}</code>
          <p class="mt-2 text-[10px] text-faint">缓存目录：~/Library/Caches/com.dot.wallpaper</p>
        </section>
      </div>
      <footer class="flex justify-end border-t border-line px-5 py-3"><button class="mac-btn mac-btn-primary" @click="emit('close')">完成</button></footer>
    </section>
  </div>
</template>
