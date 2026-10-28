<script setup lang="ts">
// 关于 - 应用信息 + 检查更新（发现新版本后弹出通用 UpdateDialog）
import { NButton, NIcon } from "naive-ui";
import { Copyright, Github, Info, RefreshCw, Rocket, ArrowRight } from "lucide-vue-next";
import { openUrl } from "@tauri-apps/plugin-opener";
import dotCode from "@/assets/dotCode.png";
import { useUpdaterStore } from "@/stores/updater";
import { isStoreBuild } from "@/utils/updater";
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

const GITHUB_URL = "https://github.com/wch2019/DotWallpaper";
// 官方许可页（标题 "Content License - Pixabay"）。注意**不要**指向 /service/terms/：
// 那是法律全文，用户看不懂；license-summary 才是给人读的摘要。
const PIXABAY_LICENSE_URL = "https://pixabay.com/service/license-summary/";

/// 完整条款说明 —— 卡片上放不下，挂到 `title` 里供需要的人悬停查看。
/// 措辞按官方原文校准：Content License **不要求署名**（"Use Content without having to
/// attribute the author"），可免费用 / 可改 / 无需署名，但不得以独立文件形式再分发。
/// 真正要求"标明来源"的是 API Terms，不是许可本身 —— 别混着写。
const LICENSE_FULL_TEXT =
  "在线图片分别来自必应每日壁纸、Pixabay 与 Wallhaven。" +
  "Pixabay 图片遵循 Content License：可免费使用与修改、无需署名，但不得以原图形式单独再分发。" +
  "Wallhaven 图片由用户上传，本应用只请求其公开的 SFW 内容，且只在设为桌面壁纸时单张下载。" +
  "在线图片只在你设为桌面壁纸时下载到本地，不做永久外链。";

function openGitHub() {
  openUrl(GITHUB_URL);
}

// 许可页 / 来源站都在系统浏览器里打开（WebView 内跳转会把整个界面替换掉且回不来）
function openLicUrl(url: string) {
  openUrl(url).catch((e: unknown) => {
    toast(`打开链接失败：${errText(e)}`, "error");
  });
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
          <p class="mt-0.5 text-[11px] text-faint">简单、轻量的 Windows 壁纸切换工具，让你快速浏览并设置喜欢的桌面壁纸。</p>
        </div>
      </div>

      <!-- 项目主页 -->
      <button
        class="flex w-full cursor-pointer items-center gap-3 rounded-lg bg-panel-2 px-4 py-4 text-left transition-colors hover:opacity-85"
        @click="openGitHub"
      >
        <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
          <Github :size="20" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="text-[12px] font-medium text-tx">项目主页</div>
          <p class="mt-0.5 text-[11px] text-faint">在 GitHub 查看源码、提交反馈，获取最新动态。</p>
        </div>
        <ArrowRight :size="14" class="shrink-0 text-faint" />
      </button>

      <!-- 图片来源与许可：**合规要求，不是装饰**。
           Pixabay 的 Content License 本身不要求署名，但 API Terms 明确要求
           「展示搜索结果时必须让用户知道图片来自哪里」，商店审核也会看这一条。

           形态与「项目主页」一致（图标 + 标题 + 副标题 + 箭头，整卡可点）：
           许可全文本来就该在许可页看，应用内只需要"说清来源 + 给出处"。
           之前写了两段完整条款，卡片高 125px，把「检查更新」挤到折叠线以下 ——
           用户来「关于」是看版本和更新的，不是来读协议的。
           完整说明挂 `title`，需要的人悬停能看到。 -->
      <button
        class="flex w-full cursor-pointer items-center gap-3 rounded-lg bg-panel-2 px-4 py-4 text-left transition-colors hover:opacity-85"
        :title="LICENSE_FULL_TEXT"
        @click="openLicUrl(PIXABAY_LICENSE_URL)"
      >
        <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
          <Copyright :size="20" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="text-[12px] font-medium text-tx">图片来源与许可</div>
          <p class="mt-0.5 text-[11px] text-faint">
            在线图片来自必应、Pixabay 与 Wallhaven · 查看 Pixabay Content License
          </p>
        </div>
        <ArrowRight :size="14" class="shrink-0 text-faint" />
      </button>

      <!-- 检查更新 -->
      <div v-if="!isStoreBuild" class="rounded-lg bg-panel-2 px-4 py-4">
        <div class="flex items-center gap-3">
          <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-accent-soft text-accent">
            <RefreshCw :size="20" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="text-[12px] font-medium text-tx">更新检查</div>
            <p class="mt-0.5 text-[11px] text-faint">检查 DotWallpaper 是否有新版本可用。</p>
          </div>
          <span class="shrink-0">
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

        <!-- 检查失败（成功时已有 toast 轻提示，不再常驻占用空间） -->
        <p
          v-else-if="updaterStore.checked && updaterStore.error"
          class="mt-2.5 text-[11px]"
          style="color: #f0ad4e"
        >
          检查更新失败：{{ errText(updaterStore.error) }}
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
