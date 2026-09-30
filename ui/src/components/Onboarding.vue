<script setup lang="ts">
import { ref } from 'vue';
import { useApp } from '../composables/useApp';
import SvgIcon from './SvgIcon.vue';

const app = useApp();
const busy = ref(false);

async function choose() {
  if (busy.value) return;
  busy.value = true;
  try {
    if (await app.pickFolder()) await app.completeOnboarding();
  } finally {
    busy.value = false;
  }
}

async function finishLater() {
  if (busy.value) return;
  busy.value = true;
  try {
    await app.completeOnboarding();
  } finally {
    busy.value = false;
  }
}
</script>
<template>
  <div class="dw-modal-backdrop dw-onboarding-backdrop">
    <section class="dw-onboarding mac-card" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
      <div class="dw-onboarding-body">
        <div class="dw-onboarding-art"><div class="dw-onboarding-orbit orbit-one"/><div class="dw-onboarding-orbit orbit-two"/><SvgIcon name="image" :size="36" class="text-accent"/></div>
        <p class="dw-eyebrow mt-5">WALLPAPER ENGINE · MAC</p>
        <h1 id="welcome-title" class="mt-2 text-2xl font-semibold tracking-tight">让桌面，成为一幅正在发生的风景。</h1>
        <p class="mt-2 max-w-md text-sm leading-relaxed text-dim">先选一个壁纸文件夹。图片与视频只在本机读取，你也可以稍后再配置。</p>
        <div class="mt-6 rounded-xl border border-line bg-bg-2/70 p-4 text-left">
          <div class="flex gap-3"><span class="dw-step-dot">1</span><div><p class="text-xs font-medium">选择壁纸目录</p><p class="mt-1 text-[11px] text-faint">随时可在设置中更改</p></div></div>
          <div class="my-3 ml-[9px] h-5 border-l border-line-2"/>
          <div class="flex gap-3"><span class="dw-step-dot">2</span><div><p class="text-xs font-medium">仅授予所选文件夹访问</p><p class="mt-1 text-[11px] text-faint">不需要屏幕录制或辅助功能权限</p></div></div>
        </div>
      </div>
      <footer class="dw-onboarding-actions">
        <button class="mac-btn" :disabled="busy" @click="finishLater">稍后设置</button>
        <button class="mac-btn mac-btn-primary" :disabled="busy" @click="choose"><SvgIcon name="folder" :size="13"/>{{ busy?'正在打开…':'选择壁纸目录' }}</button>
      </footer>
    </section>
  </div>
</template>
