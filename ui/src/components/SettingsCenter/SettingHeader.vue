<script setup lang="ts">
// SettingHeader - 弹窗外壳：齿轮按钮 + 遮罩弹窗骨架
// 左侧导航由 #nav slot 注入，右侧内容由默认 slot 注入
// 右侧关闭按钮固定在内容区顶部栏，滚动区独立，按钮不会随内容滚走
import { watch, onUnmounted } from "vue";
import { NIcon } from "naive-ui";
import { Settings, X } from "lucide-vue-next";

const props = defineProps<{
  open: boolean;
  appName: string;
  appVersion: string;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  close: [];
}>();

function openDialog() {
  emit("update:open", true);
}

function closeDialog() {
  emit("update:open", false);
}

// Esc 关闭（弹窗自身不聚焦，需 window 级监听）
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && props.open) closeDialog();
}

watch(
  () => props.open,
  (v) => {
    if (v) window.addEventListener("keydown", onKey);
    else window.removeEventListener("keydown", onKey);
  },
);

onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <!-- 齿轮按钮 -->
  <button
    class="settings-btn flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-dim transition-colors hover:bg-white/10 hover:text-tx"
    title="设置"
    @click="openDialog"
  >
    <NIcon :component="Settings" :size="13" />
  </button>

  <!-- 弹窗 -->
  <Teleport to="body">
    <Transition name="fade">
      <div
        v-if="open"
        class="settings-overlay fixed inset-0 z-[998] flex items-center justify-center bg-black/45 backdrop-blur-[2px]"
        @click.self="closeDialog"
      >
        <div
          class="settings-dialog flex h-[480px] w-[720px] overflow-hidden rounded-2xl border border-line-2 bg-panel shadow-[0_24px_60px_rgba(0,0,0,0.5)]"
        >
          <!-- 左导航 -->
          <div class="nav-side flex w-[168px] shrink-0 flex-col border-r border-line p-2">
            <div
              class="nav-title flex items-center gap-1.5 px-2 pb-2 pt-1 text-[12px] font-semibold tracking-wide text-tx"
            >
              <NIcon :component="Settings" :size="12" class="text-accent" />
              设置
            </div>
            <nav class="nav-list flex flex-col gap-0.5">
              <slot name="nav" />
            </nav>
            <div class="nav-footer mt-auto px-2 pb-1 pt-3">
              <span class="text-[10px] text-faint">v {{ appVersion }}</span>
            </div>
          </div>

          <!-- 右内容：顶部固定栏 + 独立滚动区 -->
          <div class="content-side relative flex min-w-0 flex-1 flex-col">
            <!-- 固定顶栏（关闭按钮在此，不随内容滚动） -->
            <div
              class="content-bar flex h-11 shrink-0 items-center justify-between border-b border-line px-4"
            >
              <span class="text-[12px] font-medium tracking-wide text-faint">
                {{ appName }}
              </span>
              <button
                class="close-btn flex h-7 w-7 cursor-pointer items-center justify-center rounded-lg text-faint transition-colors hover:bg-white/10 hover:text-tx"
                title="关闭（Esc）"
                @click="closeDialog"
              >
                <NIcon :component="X" :size="15" />
              </button>
            </div>

            <!-- 滚动内容区 -->
            <div class="content-scroll min-h-0 flex-1 overflow-y-auto p-5">
              <slot />
            </div>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.settings-dialog {
  backdrop-filter: blur(16px);
}
.nav-side {
  background: rgba(18, 24, 38, 0.9);
}
.content-side {
  background: rgba(14, 19, 28, 0.82);
}
.content-bar {
  background: rgba(18, 24, 38, 0.55);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
