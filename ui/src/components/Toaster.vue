<script setup lang="ts">
// Toaster - 顶部轻提示（替代 Naive UI message）
import { useApp } from "../composables/useApp";
import SvgIcon from "./SvgIcon.vue";

const app = useApp();

function toneClass(kind: string) {
  if (kind === "success") return "border-ok/30 text-ok";
  if (kind === "error") return "border-danger/40 text-danger";
  if (kind === "warning") return "border-warn/30 text-warn";
  return "border-line-2 text-tx";
}

function iconKind(kind: string): "check" | "close" {
  return kind === "error" ? "close" : "check";
}
</script>

<template>
  <Teleport to="body">
    <div class="pointer-events-none fixed left-1/2 top-3 z-[120] flex -translate-x-1/2 flex-col items-center gap-1.5">
      <TransitionGroup name="toast">
        <div
          v-for="t in app.toasts.value"
          :key="t.id"
          class="pointer-events-auto flex max-w-[520px] items-center gap-2 rounded-lg border bg-panel/95 px-3 py-1.5 text-[12px] shadow-[0_10px_30px_var(--color-drop)] backdrop-blur-xl"
          :class="toneClass(t.kind)"
        >
          <SvgIcon :name="iconKind(t.kind)" :size="13" />
          <span class="truncate">{{ t.text }}</span>
          <button class="ml-1 cursor-pointer text-faint hover:text-tx" title="关闭" @click="app.dismiss(t.id)">
            <SvgIcon name="close" :size="11" />
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 0.18s ease, transform 0.18s ease;
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}
.toast-leave-active {
  position: absolute;
}
</style>
