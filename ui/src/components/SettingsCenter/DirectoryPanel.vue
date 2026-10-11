<script setup lang="ts">
// 壁纸目录设置 — 当前目录显示 / 选择目录 / 恢复默认
// 必应下载目录为**派生只读**：恒等于 `<壁纸目录>\BingWallpaper`，不提供选择器
import { ref, onMounted, watch } from "vue";
import { NButton, NIcon, useMessage } from "naive-ui";
import { FolderOpen, FolderSync, Trash2, Image as ImageIcon, DownloadCloud, AlertTriangle } from "lucide-vue-next";
import { useWallpaperStore, DIR_STORAGE_KEY } from "@/stores/wallpaper";
import { toast } from "@/lib/naive-host";
import { invoke } from "@tauri-apps/api/core";

const store = useWallpaperStore();
const message = useMessage();
const loading = ref(false);

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

// ---- 必应下载目录（只读派生） ----
// 路径不在前端拼：`<壁纸目录>\BingWallpaper` 这条规则只在 Rust 侧存在一份，
// 这里只负责展示后端给的最终路径。遗留目录（老版本可单独配置）只提示不搬移。
const openingLegacy = ref(false);

async function onOpenLegacyDir() {
  const dir = store.bingDirInfo?.legacy_dir;
  if (!dir) return;
  openingLegacy.value = true;
  try {
    await invoke("reveal_in_explorer", { path: dir });
  } catch (err: unknown) {
    message.error(String(err));
  } finally {
    openingLegacy.value = false;
  }
}

onMounted(() => { void store.refreshBingDirInfo(); });
// 壁纸目录一变，派生出的下载目录跟着变；面板常驻，靠 watch 补刷新
watch(() => store.currentDir, () => { void store.refreshBingDirInfo(); });
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

    <div class="divider" />

    <!-- 必应壁纸目录（只读派生：<壁纸目录>\BingWallpaper） -->
    <div class="panel-header pt-5">
      <NIcon :component="DownloadCloud" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">必应壁纸目录</div>
      <div class="panel-desc">在线壁纸下载保存位置</div>
    </div>

    <div class="dir-card">
      <div class="dir-label">下载目录</div>
      <div class="dir-value">
        <NIcon :component="ImageIcon" :size="16" class="dir-icon" />
        <span class="dir-path">{{ store.bingDirInfo?.dir || "读取中…" }}</span>
      </div>
      <div class="dir-hint">固定为「壁纸目录」下的 BingWallpaper 子目录，随上方目录自动跟随</div>
    </div>

    <!-- 老版本遗留目录：只提示不搬移。用户在旧版本里单独指定过下载目录时，
         那些文件落在壁纸目录之外，「本地」选项卡扫不到 —— 给数量 + 打开入口。 -->
    <div v-if="store.bingDirInfo?.legacy_dir && store.bingDirInfo.legacy_count > 0" class="legacy-card">
      <div class="legacy-head">
        <NIcon :component="AlertTriangle" :size="14" class="legacy-icon" />
        <span class="legacy-title">发现旧版本的下载目录</span>
      </div>
      <div class="legacy-path">{{ store.bingDirInfo.legacy_dir }}</div>
      <div class="legacy-text">
        该目录里还有 {{ store.bingDirInfo.legacy_count }} 张必应壁纸，它们不在当前壁纸目录内，
        因此不会出现在「本地」选项卡中。手动移到上方目录后即可正常管理。
      </div>
      <NButton size="tiny" secondary :disabled="openingLegacy" @click="onOpenLegacyDir">
        <template #icon>
          <NIcon :component="FolderOpen" :size="13" />
        </template>
        打开该目录
      </NButton>
    </div>

    <div class="dir-note">
      <NIcon :component="DownloadCloud" :size="13" class="note-icon" />
      <span>点击"设为壁纸"或卡片上的「下载」时，必应原图将下载到此目录，文件名为 BingWallpaper_日期.jpg</span>
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

/* ========== 旧版本遗留目录提示 ========== */
.legacy-card {
  margin-top: 0.9rem;
  padding: 0.75rem 0.85rem;
  border-radius: 10px;
  background: rgba(255, 184, 92, 0.07);
  border: 1px solid rgba(255, 184, 92, 0.22);
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
  align-items: flex-start;
}
.legacy-head {
  display: flex;
  align-items: center;
  gap: 0.35rem;
}
.legacy-icon {
  color: #ffb85c;
  flex-shrink: 0;
}
.legacy-title {
  font-size: 12px;
  font-weight: 600;
  color: #ffd39a;
}
.legacy-path {
  font-size: 11.5px;
  color: var(--color-tx);
  word-break: break-all;
  line-height: 1.4;
}
.legacy-text {
  font-size: 11px;
  color: var(--color-dim);
  line-height: 1.5;
}
</style>
