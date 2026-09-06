<script setup lang="ts">
// 关于 - 应用信息 + 检查更新（发现新版本后弹出通用 UpdateDialog）
import { NButton, NIcon } from "naive-ui";
import { Info, RefreshCw, Rocket, ArrowRight } from "lucide-vue-next";
import dotCode from "@/assets/dotCode.png";
import { useUpdaterStore } from "@/stores/updater";
import { toast } from "@/lib/naive-host";

defineProps<{ appName: string; appVersion: string }>();

const updaterStore = useUpdaterStore();

function errText(e: unknown): string {
  if (!e) return "未知错误";
  const raw = (e as { message?: unknown })?.message ?? e;
  return typeof raw === "string" ? raw : String(raw);
}

/** 手动检查更新：发现新版本 → 打开通用弹窗；无更新 / 失败 → 轻提示 */
async function handleCheckUpdate() {
  const result = await updaterStore.checkUpdate();
  if (updaterStore.error) {
    toast(`检查更新失败：${errText(updaterStore.error)}`, "error");
    return;
  }
  if (result) {
    updaterStore.openUpdateDialog();
  } else {
    toast("当前已是最新版本", "success");
  }
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Info" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">关于</div>
    </div>

    <div class="about-layout mt-4 flex flex-col gap-3">
      <!-- 应用信息 -->
      <div class="app-card flex items-center gap-3 rounded-lg bg-panel-2 px-4 py-4">
        <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
          <img :src="dotCode" alt="DotWallpaper Logo" class="h-6 w-6" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="text-[14px] font-semibold text-tx">{{ appName }}</span>
            <span class="rounded-md bg-white/5 px-1.5 py-0.5 text-[10.5px] text-faint">v{{ appVersion }}</span>
          </div>
          <p class="mt-0.5 text-[11px] text-faint">Windows 桌面壁纸管理工具</p>
        </div>
      </div>

      <!-- 检查更新 -->
      <div class="upd-card-panel rounded-lg bg-panel-2 px-4 py-4">
        <div class="flex items-center gap-2">
          <RefreshCw :size="13" class="shrink-0 text-accent" />
          <span class="text-[12px] font-medium text-tx">更新检查</span>
          <span class="ml-auto">
            <NButton
              size="small"
              secondary
              type="primary"
              :loading="updaterStore.checking"
              :disabled="updaterStore.checking || updaterStore.downloading"
              @click="handleCheckUpdate"
            >
              <template #icon>
                <RefreshCw :size="13" />
              </template>
              {{ updaterStore.checking ? "检查中…" : "检查更新" }}
            </NButton>
          </span>
        </div>

        <!-- 发现新版本：入口引导打开通用弹窗 -->
        <div
          v-if="updaterStore.hasUpdate"
          class="mt-3 flex items-center gap-2 rounded-lg px-3 py-2.5"
          style="border: 1px solid rgba(127, 168, 255, 0.25); background: rgba(127, 168, 255, 0.08)"
        >
          <Rocket :size="14" class="shrink-0 text-accent" />
          <div class="min-w-0 flex-1 text-[11.5px] leading-snug text-tx">
            发现新版本 v{{ updaterStore.latestVersion }}
            <span v-if="updaterStore.currentVersion" class="text-faint">
              （当前 v{{ updaterStore.currentVersion }}）
            </span>
          </div>
          <button
            class="flex shrink-0 cursor-pointer items-center gap-0.5 text-[11.5px] font-medium text-accent hover:opacity-80"
            @click="updaterStore.openUpdateDialog()"
          >
            查看详情
            <ArrowRight :size="12" />
          </button>
        </div>

        <!-- 无更新 -->
        <p
          v-else-if="updaterStore.checked && !updaterStore.error"
          class="mt-2.5 text-[11px] text-faint"
        >
          当前已是最新版本，应用启动后会自动检查更新。
        </p>

        <!-- 检查失败 -->
        <p
          v-else-if="updaterStore.checked && updaterStore.error"
          class="mt-2.5 text-[11px]"
          style="color: #f0ad4e"
        >
          检查更新失败：{{ errText(updaterStore.error) }}
        </p>

        <!-- 默认提示 -->
        <p v-else class="mt-2.5 text-[11px] text-faint">
          点击右侧按钮检查 DotWallpaper 是否有新版本可用。
        </p>
      </div>

      <div class="mt-1 text-center text-[10.5px] text-faint/70">
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
