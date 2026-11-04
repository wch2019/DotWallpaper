<script setup lang="ts">
// 首次启动引导：让用户选择壁纸存放目录（可跳过 = 使用默认目录）。
// 两个按钮都是有效选择，故禁用遮罩/ESC 关闭，强制二选一。
// 依赖 store 的 currentDir / markOnboarded，以及后端 pick_wallpaper_directory 命令。
//
// 容器改为自定义 Teleport overlay（参考 UpdateDialog）：NModal preset="card" 的 NCard
// body 内边距会让头图被挤在内部，无法做「贴顶贴左右边缘」的满宽沉浸头图；
// 这里用 .fr-card 的 overflow:hidden + 顶部圆角把头图四角裁成圆角，达到沉浸式效果。
// 头图纯 CSS 绘制（日落山景），不加载任何图片、不联网、不读用户文件 ——
// 首次用户此时还没授权目录，绝不能用真实壁纸做背景。
import { onMounted, ref } from "vue";
import { NButton, NIcon } from "naive-ui";
import { FolderOpen } from "lucide-vue-next";
import { invoke } from "@tauri-apps/api/core";
import { useWallpaperStore, DIR_STORAGE_KEY } from "@/stores/wallpaper";
import { toast } from "@/lib/naive-host";

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", v: boolean): void }>();

const store = useWallpaperStore();
const picking = ref(false);

// 默认目录的真实路径由 Rust 侧派生（USERPROFILE\Pictures）：前端读不到环境变量，
// 写 `C:\Users\<用户名>\Pictures` 只是占位文本，用户点确认时并不知道文件会落在哪。
// 命令不可用时（老后端 / HMR 期间）退回占位文本，避免卡片空白。
const fallbackDirHint = "C:\\Users\\<你的用户名>\\Pictures";
const defaultDirHint = ref("");

onMounted(async () => {
  try {
    defaultDirHint.value = (await invoke("get_default_wallpaper_dir")) as string;
  } catch {
    defaultDirHint.value = fallbackDirHint;
  }
});

async function finish() {
  store.markOnboarded();
  emit("update:show", false);
}

async function onUseDefault() {
  // 清空为默认目录（空字符串 = 预设目录），并清掉可能残留的自定义目录键
  store.currentDir = "";
  try {
    localStorage.removeItem(DIR_STORAGE_KEY);
  } catch {
    /* ignore */
  }
  toast("已使用默认目录", "success");
  await store.loadWallpapers();
  await finish();
}

async function onPickOther() {
  picking.value = true;
  try {
    const picked = (await invoke("pick_wallpaper_directory")) as string | null;
    if (!picked) return; // 用户在系统选择框里取消：停留在引导弹窗，不做任何改动
    store.currentDir = picked;
    try {
      localStorage.setItem(DIR_STORAGE_KEY, picked);
    } catch {
      /* ignore */
    }
    toast("已选择目录：" + picked, "success");
    await store.loadWallpapers();
    await finish();
  } catch (err: unknown) {
    toast("选择目录失败：" + ((err as Error)?.message || String(err)), "error");
  } finally {
    picking.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fr-fade">
      <div v-if="show" class="fr-overlay" role="dialog" aria-modal="true">
        <div class="fr-card">
          <!-- 全宽沉浸式头图：贴顶、贴左右边缘，靠 .fr-card 的 overflow:hidden 裁成圆角。
               纯 CSS 日落山景，不加载任何图片 / 不联网 / 不读用户文件。 -->
          <div class="fr-hero" aria-hidden="true">
            <div class="fr-hero-grid" />
            <div class="fr-sun" />
            <div class="fr-sun-glow" />
            <div class="fr-hills">
              <span class="fr-hill fr-hill-back" />
              <span class="fr-hill fr-hill-front" />
            </div>
            <div class="fr-hero-scrim" />
            <div class="fr-hero-copy">
              <span class="fr-eyebrow">DOTWALLPAPER · 首次使用</span>
              <h2 class="fr-hero-title">让桌面每天都有新风景</h2>
              <p class="fr-hero-sub">先选好壁纸的存放位置，之后随时能改。</p>
            </div>
          </div>

          <div class="fr-content">
            <p class="fr-desc">
              拖入的图片 / 视频，以及下载的必应 / Pixabay / Wallhaven 壁纸都会存到这里；
              之后可在「设置 → 壁纸目录」里更换。
            </p>

            <!-- 默认目录：图标 + 路径 + 推荐标记，比纯文本块更有「可选项」的感觉 -->
            <div class="fr-dir">
              <span class="fr-dir-icon"><n-icon :component="FolderOpen" :size="18" /></span>
              <span class="fr-dir-text">
                <span class="fr-dir-label">默认目录</span>
                <span class="fr-dir-path">{{ defaultDirHint || "读取中…" }}</span>
              </span>
              <span class="fr-dir-badge">推荐</span>
            </div>

            <!-- 隐私承诺：措辞必须精确 —— 启动时的其它初始化仍会读系统壁纸状态，
                 这里只承诺不扫描 / 不导入用户的壁纸文件，不能写成「不读取任何数据」。 -->
            <p class="fr-privacy">在你选定目录之前，不会扫描或导入你的壁纸文件</p>

            <div class="fr-decision">
              <!-- 按钮组：右对齐、主次分明；放大的冰蓝主按钮是整页的高饱和重心 -->
              <div class="fr-actions">
                <n-button size="large" :disabled="picking" @click="onUseDefault">
                  使用默认目录
                </n-button>
                <n-button
                  class="fr-btn-main"
                  type="primary"
                  size="large"
                  :loading="picking"
                  :disabled="picking"
                  @click="onPickOther"
                >
                  <template #icon><n-icon :component="FolderOpen" /></template>
                  选择其他目录…
                </n-button>
              </div>
              <!-- 关闭行为默认是隐藏到托盘：首启先讲清楚，免得用户以为点 × 是退出 -->
              <p class="fr-footnote">关闭窗口时会最小化到系统托盘，可在「设置 → 托盘」中更改</p>
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* 遮罩层：不绑定 click / ESC 关闭，强制用户在两个按钮间二选一 */
.fr-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(8, 10, 18, 0.55);
  backdrop-filter: blur(8px);
}

/* 卡片：overflow:hidden + 圆角，让顶部头图四角被裁成圆角，形成沉浸式满宽头图 */
.fr-card {
  width: min(520px, 92vw);
  border: 1px solid rgba(255, 255, 255, 0.09);
  border-radius: 16px;
  overflow: hidden;
  background: linear-gradient(165deg, rgba(30, 35, 52, 0.97), rgba(18, 22, 36, 0.99));
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
}

/* ---- 全宽沉浸式头图 ---- */
.fr-hero {
  position: relative;
  width: 100%;
  height: 200px;
  overflow: hidden;
  background:
    radial-gradient(120% 150% at 80% 8%, rgba(127, 168, 255, 0.4) 0%, transparent 48%),
    linear-gradient(140deg, #16264f 0%, #0f1832 52%, #1b2c54 100%);
}
/* 细网格，避免纯色块的廉价感 */
.fr-hero-grid {
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(255, 255, 255, 0.03) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255, 255, 255, 0.03) 1px, transparent 1px);
  background-size: 26px 26px;
}
/* 落日 + 光晕 */
.fr-sun {
  position: absolute;
  right: 70px;
  top: 46px;
  width: 62px;
  height: 62px;
  border-radius: 50%;
  background: radial-gradient(circle at 50% 50%, #fff0c4 0%, #ffd27a 52%, rgba(255, 200, 120, 0) 74%);
}
.fr-sun-glow {
  position: absolute;
  right: 34px;
  top: 6px;
  width: 150px;
  height: 150px;
  border-radius: 50%;
  background: radial-gradient(circle, rgba(255, 214, 140, 0.28) 0%, rgba(255, 214, 140, 0.05) 46%, transparent 72%);
}
/* 两层山脊，做出「桌面壁纸」式的日落山景 */
.fr-hills {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 132px;
}
.fr-hill {
  position: absolute;
  bottom: 0;
  border-radius: 50% 50% 0 0 / 100% 100% 0 0;
}
.fr-hill-back {
  left: -16%;
  width: 76%;
  height: 92px;
  background: rgba(96, 134, 214, 0.3);
}
.fr-hill-front {
  right: -18%;
  width: 82%;
  height: 124px;
  background: rgba(48, 80, 150, 0.58);
}
/* 底部压暗，保证标题文字可读 */
.fr-hero-scrim {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 116px;
  background: linear-gradient(to top, rgba(10, 16, 32, 0.62), transparent);
  pointer-events: none;
}
.fr-hero-copy {
  position: absolute;
  left: 24px;
  bottom: 18px;
  right: 24px;
  z-index: 1;
}
.fr-eyebrow {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 1.6px;
  color: #add0ff;
}
.fr-hero-title {
  margin: 8px 0 0;
  font-size: 27px;
  font-weight: 700;
  line-height: 1.25;
  letter-spacing: 0.3px;
  color: #f4f8ff;
  text-shadow: 0 2px 12px rgba(0, 0, 0, 0.35);
}
.fr-hero-sub {
  margin: 8px 0 0;
  font-size: 12.5px;
  line-height: 1.5;
  color: rgba(228, 236, 252, 0.82);
}

/* ---- 卡片正文 ---- */
.fr-content {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 20px 24px 22px;
}

.fr-desc {
  margin: 0;
  font-size: 13px;
  line-height: 1.7;
  color: var(--color-dim);
}

/* ---- 目录卡片 ---- */
.fr-dir {
  display: flex;
  align-items: center;
  gap: 0.8rem;
  padding: 0.85rem 1rem;
  border-radius: 12px;
  background: rgba(255, 255, 255, 0.035);
  border: 1px solid var(--color-line);
  transition: border-color 0.2s ease, background 0.2s ease;
}
.fr-dir:hover {
  border-color: rgba(127, 168, 255, 0.35);
  background: rgba(127, 168, 255, 0.05);
}
.fr-dir-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 9px;
  background: rgba(127, 168, 255, 0.12);
  color: var(--color-accent);
  flex-shrink: 0;
}
.fr-dir-text {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
  flex: 1;
}
.fr-dir-label {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.6px;
  text-transform: uppercase;
  color: var(--color-faint);
}
.fr-dir-path {
  font-size: 13px;
  font-weight: 600;
  line-height: 1.5;
  color: var(--color-tx);
  word-break: break-all;
}
.fr-dir-badge {
  flex-shrink: 0;
  padding: 2px 8px;
  border-radius: 999px;
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.5px;
  color: var(--color-accent);
  background: rgba(127, 168, 255, 0.1);
  border: 1px solid rgba(127, 168, 255, 0.22);
}

/* 隐私承诺：左侧一道细冰蓝竖线，不用第二个高饱和圆点 */
.fr-privacy {
  margin: -0.3rem 0 0;
  padding-left: 0.65rem;
  border-left: 2px solid var(--color-accent);
  font-size: 11.5px;
  line-height: 1.6;
  color: var(--color-dim);
}
.fr-decision {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 0.6rem;
}
.fr-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
}
.fr-btn-main {
  padding: 0 20px;
  font-weight: 600;
  letter-spacing: 0.2px;
}
.fr-footnote {
  margin: 0;
  font-size: 11px;
  line-height: 1.5;
  color: var(--color-faint);
}

/* 依次入场：让欢迎页有开场感，而不是一个静态表单 */
@keyframes fr-rise {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.fr-content > * {
  animation: fr-rise 0.4s ease both;
}
.fr-content > :nth-child(2) { animation-delay: 0.06s; }
.fr-content > :nth-child(3) { animation-delay: 0.12s; }
.fr-content > :nth-child(4) { animation-delay: 0.18s; }

/* 卡片整体淡入 + 轻微上浮 */
.fr-fade-enter-active,
.fr-fade-leave-active {
  transition: opacity 0.2s ease;
}
.fr-fade-enter-active .fr-card,
.fr-fade-leave-active .fr-card {
  transition: transform 0.22s ease, opacity 0.2s ease;
}
.fr-fade-enter-from,
.fr-fade-leave-to {
  opacity: 0;
}
.fr-fade-enter-from .fr-card,
.fr-fade-leave-to .fr-card {
  opacity: 0;
  transform: translateY(12px) scale(0.97);
}

@media (prefers-reduced-motion: reduce) {
  .fr-content > *,
  .fr-fade-enter-active,
  .fr-fade-leave-active {
    animation: none;
    transition: none;
  }
  .fr-fade-enter-from .fr-card,
  .fr-fade-leave-to .fr-card {
    opacity: 1;
    transform: none;
  }
}
</style>
