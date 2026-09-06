<script setup lang="ts">
// 快捷键使用
import { NIcon } from "naive-ui";
import { Keyboard } from "lucide-vue-next";

interface Shortcut {
  desc: string;
  keys: { name: string; type: string }[];
}

const shortcuts: Shortcut[] = [
  { desc: "打开拖放图片到壁纸目录引导", keys: [{ name: "Ctrl", type: "mod" }, { name: "I", type: "key" }] },
  { desc: "将正在预览的壁纸设为桌面", keys: [{ name: "Ctrl", type: "mod" }, { name: "S", type: "key" }] },
  { desc: "重新加载壁纸列表", keys: [{ name: "Ctrl", type: "mod" }, { name: "R", type: "key" }] },
  { desc: "预览上一张 / 下一张壁纸", keys: [{ name: "←", type: "arrow" }, { name: "/ ", type: "sep" }, { name: "→", type: "arrow" }] },
  { desc: "关闭当前弹窗 / 放大预览", keys: [{ name: "Esc", type: "key" }] },
];

function keyClass(type: string) {
  switch (type) {
    case "mod": return "mod-key";
    case "arrow": return "arrow-key";
    default: return "key";
  }
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Keyboard" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">快捷键使用</div>
      <div class="panel-desc">以下快捷键在应用聚焦时生效</div>
    </div>

    <div class="shortcut-list mt-4 flex flex-col gap-2">
      <div
        v-for="(sc, idx) in shortcuts"
        :key="idx"
        class="sc-row flex items-center justify-between rounded-lg bg-panel-2 px-3 py-2.5"
      >
        <span class="text-[12px] text-dim">{{ sc.desc }}</span>
        <span class="key-group flex items-center gap-1">
          <span
            v-for="(k, ki) in sc.keys"
            :key="ki"
            :class="['kbd', keyClass(k.type)]"
          >
            {{ k.name }}
          </span>
        </span>
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
.panel-desc {
  margin-left: auto;
  font-size: 11.5px;
  color: var(--color-faint);
}

.kbd {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 22px;
  height: 22px;
  padding: 0 6px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-bottom-width: 2px;
  background: rgba(30, 39, 57, 0.9);
  color: rgba(231, 235, 243, 0.9);
  font-size: 11px;
  line-height: 1;
}
.mod-key {
  min-width: 36px;
}
.arrow-key {
  font-size: 12px;
}
</style>
