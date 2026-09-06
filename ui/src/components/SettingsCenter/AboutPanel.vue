<script setup lang="ts">
// 关于
import { NIcon } from "naive-ui";
import { Info } from "lucide-vue-next";
import dotCode from "@/assets/dotCode.png";
import { useUpdaterStore } from '@/stores/updater'

defineProps<{ appName: string; appVersion: string }>();


const updaterStore = useUpdaterStore()

const handleCheckUpdate = async () => {
  await updaterStore.checkUpdate()
}

const handleInstallUpdate = async () => {
  await updaterStore.installUpdate()
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Info" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">关于</div>
    </div>

    <div class="about-card mt-4 flex flex-col items-center rounded-lg bg-panel-2 px-6 py-6 text-center">
      <div class="flex h-12 w-12 items-center justify-center rounded-xl bg-accent-soft text-accent">
        <img :src="dotCode" alt="DotNote Logo"/>
      </div>
      <div class="mt-3 text-[16px] font-semibold text-tx">{{ appName }}</div>
      <div class="mt-0.5 text-[11px] text-faint">版本 v {{ updaterStore.currentVersion }}</div>
      <button
          :disabled="updaterStore.checking"
          @click="handleCheckUpdate"
      >
        {{
          updaterStore.checking
              ? '检查中...'
              : '检查更新'
        }}
      </button>

      <div v-if="updaterStore.hasUpdate">
        <div>
          发现新版本 {{ updaterStore.latestVersion }}
        </div>

        <div>
          {{ updaterStore.updateNotes }}
        </div>

        <button
            :disabled="updaterStore.downloading"
            @click="handleInstallUpdate"
        >
          {{
            updaterStore.downloading
                ? `正在更新 ${updaterStore.progress}%`
                : '立即更新'
          }}
        </button>
      </div>
      <div class="mt-2 text-[11px] text-faint">
        Windows 桌面壁纸管理工具 
      </div>
      <div class="mt-4 text-[10px] text-faint/70">
        © 2026 DotWallpaper. Made with ♥
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
</style>
