<script setup lang="ts">
// SettingsCenter - 设置中心主入口（SettingsCenter 目录）
// 左侧导航由 #nav slot 注入（由本组件遍历 NAV_ITEMS 生成），
// 右侧内容按 activeTab 动态渲染各子面板，便于后续扩展选项卡。
import { ref, computed, defineComponent, h } from "vue";
import { NIcon } from "naive-ui";
import { Keyboard, FolderOpen, HeartHandshake, Info, LayoutGrid, Power, Wand2 } from "lucide-vue-next";
import SettingHeader from "./SettingHeader.vue";
import DirectoryPanel from "./DirectoryPanel.vue";
import SourceVisibilityPanel from "./SourceVisibilityPanel.vue";
import EffectPanel from "./EffectPanel.vue";
import ShortcutsPanel from "./ShortcutsPanel.vue";
import SupportPanel from "./SupportPanel.vue";
import AboutPanel from "./AboutPanel.vue";
import TrayPanel from "./TrayPanel.vue";

type TabKey = "directory" | "source" | "effect" | "tray" | "shortcuts" | "support" | "about";

const open = ref(false);
const activeTab = ref<TabKey>("directory");

const NAV_ITEMS: { key: TabKey; label: string; icon: any }[] = [
  { key: "directory", label: "壁纸目录", icon: FolderOpen },
  { key: "source", label: "壁纸来源", icon: LayoutGrid },
  { key: "effect", label: "壁纸效果", icon: Wand2 },
  { key: "tray", label: "后台运行", icon: Power },
  { key: "shortcuts", label: "快捷键使用", icon: Keyboard },
  { key: "support", label: "交流打赏", icon: HeartHandshake },
  { key: "about", label: "关于", icon: Info },
];

const appName = "DotWallpaper";
const appVersion = __APP_VERSION__;

function onNav(key: TabKey) {
  activeTab.value = key;
}

const currentPanel = computed(() => {
  switch (activeTab.value) {
    case "directory":
      return defineComponent({ render: () => h(DirectoryPanel) });
    case "source":
      return defineComponent({ render: () => h(SourceVisibilityPanel) });
    case "effect":
      return defineComponent({ render: () => h(EffectPanel) });
    case "tray":
      return defineComponent({ render: () => h(TrayPanel) });
    case "support":
      return defineComponent({ render: () => h(SupportPanel) });
    case "about":
      return defineComponent({
        render: () => h(AboutPanel, { appName, appVersion }),
      });
    default:
      return defineComponent({ render: () => h(ShortcutsPanel) });
  }
});
</script>

<template>
  <SettingHeader
    v-model:open="open"
    :app-name="appName"
    :app-version="appVersion"
  >
    <!-- 左导航 -->
    <template #nav>
      <button
        v-for="item in NAV_ITEMS"
        :key="item.key"
        class="nav-item flex cursor-pointer items-center gap-2 rounded-lg px-2.5 py-2 text-left text-[12.5px] transition-colors"
        :class="activeTab === item.key ? 'nav-item-active' : 'text-dim hover:bg-white/5 hover:text-tx'"
        @click="onNav(item.key)"
      >
        <NIcon :component="item.icon" :size="14" class="shrink-0" />
        <span>{{ item.label }}</span>
      </button>
    </template>

    <!-- 右内容 -->
    <component :is="currentPanel" :key="activeTab" />
  </SettingHeader>
</template>

<style scoped>
/* 导航列表容器的布局由 SettingHeader 的 <nav class="nav-list ..."> 自带 Tailwind 工具类，
   本来就没有也不需要 .nav-list 规则。原先这里留了一条空的 .nav-list（无任何声明、纯占位），已删除。 */
.nav-item {
  color: var(--color-dim);
}
.nav-item-active {
  background: rgba(127, 168, 255, 0.12);
  color: var(--color-accent);
  font-weight: 500;
}
</style>
