<script setup lang="ts">
// 托盘运行设置 — 关闭窗口时的行为：直接退出 / 最小化到托盘（后台运行）+ 开机自启动
import { ref, onMounted } from "vue";
import { NIcon, NRadio, NRadioGroup, NSwitch } from "naive-ui";
import { Power, LogOut } from "lucide-vue-next";
import { isEnabled, enable, disable } from "@tauri-apps/plugin-autostart";
import { CLOSE_BEHAVIOR_KEY, AUTOSTART_SHOW_KEY, type AutostartWindowMode } from "@/stores/wallpaper";
import { toast } from "@/lib/naive-host";
import { invoke } from "@tauri-apps/api/core";

const behavior = ref<"exit" | "tray">("tray");
const autoStart = ref(false);
const autoStartBusy = ref(false);
// 开机启动后的行为：静默后台（默认）还是打开主界面。仅在自启动开启时有意义。
const startMode = ref<AutostartWindowMode>("silent");

// 从 localStorage 读取"开机后是否打开主界面"（未配置时默认静默后台运行）
function loadStartMode() {
  try {
    startMode.value = localStorage.getItem(AUTOSTART_SHOW_KEY) === "window" ? "window" : "silent";
  } catch {
    startMode.value = "silent";
  }
}

function onStartModeChange(value: AutostartWindowMode) {
  startMode.value = value;
  try {
    localStorage.setItem(AUTOSTART_SHOW_KEY, value);
  } catch {
    /* ignore */
  }
  toast(
    value === "window"
      ? "开机后将自动打开主界面"
      : "开机后将静默驻留托盘，不打开主窗口",
    "success"
  );
}

// 从 localStorage 读取关闭行为（未配置时默认最小化到托盘，贴合后台运行诉求）
function loadBehavior() {
  try {
    behavior.value = localStorage.getItem(CLOSE_BEHAVIOR_KEY) === "exit" ? "exit" : "tray";
  } catch {
    behavior.value = "tray";
  }
}

function onChange(value: "exit" | "tray") {
  behavior.value = value;
  try {
    localStorage.setItem(CLOSE_BEHAVIOR_KEY, value);
  } catch {
    /* ignore */
  }
  // 同步给后端兜底：后端 CloseRequested 时据此决定隐藏到托盘还是退出
  void invoke("set_close_behavior", { hideToTray: value === "tray" }).catch(() => {
    /* ignore */
  });
  toast(
    value === "tray"
      ? "已开启托盘后台运行：关闭窗口将最小化到系统托盘"
      : "已设为关闭窗口直接退出",
    "success"
  );
}

// 读取系统开机自启动状态（由 autostart 插件管理，Windows 写注册表 Run 项）
async function loadAutoStart() {
  try {
    autoStart.value = await isEnabled();
  } catch {
    autoStart.value = false;
  }
}

async function onAutoStartChange(value: boolean) {
  if (autoStartBusy.value) return;
  autoStartBusy.value = true;
  try {
    if (value) {
      await enable();
    } else {
      await disable();
    }
    autoStart.value = value;
    toast(value ? "已开启开机自启动：开机后静默后台运行" : "已关闭开机自启动", "success");
  } catch {
    autoStart.value = !value;
    toast("切换开机自启动失败，请稍后重试", "error");
  } finally {
    autoStartBusy.value = false;
  }
}

onMounted(() => {
  loadBehavior();
  loadStartMode();
  loadAutoStart();
});
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="Power" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">后台运行</div>
      <div class="panel-desc">开机自启动与关闭行为</div>
    </div>

    <div class="tray-card">
      <div class="autostart-row">
        <div class="autostart-info">
          <div class="autostart-title">
            <NIcon :component="Power" :size="13" class="note-icon" />
            <span>开机自启动</span>
          </div>
          <div class="option-hint">登录 Windows 后自动启动（写入系统启动项）</div>
        </div>
        <NSwitch :value="autoStart" :loading="autoStartBusy" @update:value="onAutoStartChange" />
      </div>

      <!-- 开机后行为：只在开启自启动时有意义，未开启时置灰不可选 -->
      <div class="autostart-when">
        <div class="tray-label">开机后</div>
        <NRadioGroup
          v-model:value="startMode"
          :disabled="!autoStart"
          @update:value="onStartModeChange"
        >
          <div class="tray-options">
            <div class="tray-option">
              <NRadio value="silent">静默后台运行</NRadio>
              <div class="option-hint">只在系统托盘显示图标，不打开主窗口</div>
            </div>
            <div class="tray-option">
              <NRadio value="window">打开主界面</NRadio>
              <div class="option-hint">启动后自动弹出主窗口，和手动双击图标一样</div>
            </div>
          </div>
        </NRadioGroup>
      </div>
    </div>

    <div class="tray-card">
      <div class="tray-label">关闭窗口</div>
      <NRadioGroup v-model:value="behavior" @update:value="onChange">
        <div class="tray-options">
          <div class="tray-option">
            <NRadio value="tray">最小化到托盘（后台运行）</NRadio>
            <div class="option-hint">窗口隐藏到系统托盘，左键单击托盘图标重新打开，右键菜单可退出</div>
          </div>
          <div class="tray-option">
            <NRadio value="exit">直接退出程序</NRadio>
            <div class="option-hint">点击关闭按钮立即结束进程</div>
          </div>
        </div>
      </NRadioGroup>
    </div>

    <div class="tray-note">
      <NIcon :component="LogOut" :size="13" class="note-icon" />
      <span>建议同时开启「开机自启动 + 最小化到托盘」，实现开机常驻、关闭不打扰；需要完全退出时在托盘图标右键菜单中选择「退出」</span>
    </div>
  </div>
</template>

<style scoped>
/* ========== 头部 ========== */
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

/* ========== 行为选项卡片 ========== */
.tray-card {
  margin-top: 0.85rem;
  border-radius: 12px;
  border: 1px solid var(--color-line);
  padding: 1rem 1.1rem;
}
.tray-label {
  font-size: 10px;
  font-weight: 600;
  color: var(--color-faint);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.tray-options {
  margin-top: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.9rem;
}
.tray-option {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}
.option-hint {
  font-size: 11px;
  color: var(--color-dim);
  padding-left: 1.65rem;
}

/* ========== 开机自启动行 ========== */
.autostart-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}
.autostart-info {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
.autostart-title {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 13px;
  font-weight: 600;
  color: var(--color-tx);
}

/* ========== 开机后行为 ========== */
.autostart-when {
  margin-top: 0.9rem;
  padding-top: 0.85rem;
  border-top: 1px solid var(--color-line);
}

/* ========== 底部提示 ========== */
.tray-note {
  margin-top: 1.2rem;
  display: flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.6rem 0.8rem;
  border-radius: 8px;
  background: rgba(127, 168, 255, 0.06);
  border: 1px solid rgba(127, 168, 255, 0.12);
}
.note-icon {
  color: var(--color-accent);
  flex-shrink: 0;
}
.tray-note span {
  font-size: 11.5px;
  color: var(--color-dim);
}
</style>
