<script setup lang="ts">
// 壁纸目录设置 — 当前目录显示 / 选择目录 / 恢复默认
import { ref } from "vue";
import { NButton, NIcon, useMessage } from "naive-ui";
import { FolderOpen, FolderSync, Trash2 } from "lucide-vue-next";
import { useWallpaperStore, DIR_STORAGE_KEY } from "@/stores/wallpaper";
import { toast } from "@/lib/naive-host";

const store = useWallpaperStore();
const message = useMessage();
const loading = ref(false);

function resolveDirArg(): string | null {
  return store.currentDir && store.currentDir.trim()
    ? store.currentDir.trim()
    : null;
}

function displayDir() {
  if (store.currentDir && store.currentDir.trim()) return store.currentDir.trim();
  return "默认（C:\\Users\\<用户名>\\Pictures）";
}

function displayHint() {
  if (store.currentDir && store.currentDir.trim()) return "自定义目录，修改后左侧列表自动刷新";
  return "使用系统预设的图片壁纸目录";
}

async function onPickDirectory() {
  loading.value = true;
  try {
    await store.pickAndApplyDirectory();
  } catch (err: unknown) {
    message.error(String(err));
  } finally {
    loading.value = false;
  }
}

async function onRestoreDefault() {
  try {
    store.currentDir = "";
    try {
      localStorage.removeItem(DIR_STORAGE_KEY);
    } catch { /* ignore */ }
    toast("已恢复默认目录", "success");
    await store.loadWallpapers();
  } catch (err: unknown) {
    message.error(String(err));
  }
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="FolderOpen" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">壁纸目录</div>
      <div class="panel-desc">管理本地壁纸文件存放位置</div>
    </div>

    <div class="dir-card">
      <div class="dir-label">当前目录</div>
      <div class="dir-value">
        <NIcon :component="FolderSync" :size="16" class="dir-icon" />
        <span class="dir-path">{{ displayDir() }}</span>
        <span class="dir-hint">{{ displayHint() }}</span>
      </div>
    </div>

    <div class="dir-actions">
      <NButton
        type="primary"
        size="small"
        @click="onPickDirectory"
        :disabled="loading"
      >
        <template #icon>
          <NIcon :component="FolderOpen" />
        </template>
        {{ loading ? "正在选择..." : "选择目录" }}
      </NButton>

      <NButton
        size="small"
        secondary
        @click="onRestoreDefault"
        :disabled="!store.currentDir"
      >
        <template #icon>
          <NIcon :component="Trash2" :size="15" />
        </template>
        恢复默认目录
      </NButton>
    </div>

    <div class="dir-note">
      <NIcon :component="FolderOpen" :size="13" class="note-icon" />
      <span>选择目录后，左侧壁纸列表将自动刷新，切换目录无需重启应用</span>
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

/* ========== 目录信息卡片 ========== */
.dir-card {
  margin-top: 0.85rem;
  border-radius: 12px;
  border: 1px solid var(--color-line);
  padding: 1rem 1.1rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
}
.dir-label {
  font-size: 10px;
  font-weight: 600;
  color: var(--color-faint);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.dir-value {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
}
.dir-icon {
  margin-top: 2px;
  color: var(--color-accent);
  flex-shrink: 0;
}
.dir-path {
  font-size: 13px;
  font-weight: 600;
  color: var(--color-tx);
  word-break: break-all;
  line-height: 1.4;
}
.dir-hint {
  font-size: 11px;
  color: var(--color-dim);
  margin-top: 0.2rem;
}

/* ========== 操作按钮组 ========== */
.dir-actions {
  margin-top: 1rem;
  display: flex;
  gap: 0.75rem;
}

/* ========== 底部提示 ========== */
.dir-note {
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
.dir-note span {
  font-size: 11.5px;
  color: var(--color-dim);
}
</style>
