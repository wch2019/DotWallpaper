<script setup lang="ts">
// 壁纸目录设置 — 当前目录显示 / 选择目录 / 恢复默认
// 必应下载目录为**派生只读**：恒等于 `<壁纸目录>\BingWallpaper`，不提供选择器
import { computed, ref, onMounted, watch } from "vue";
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

// ---- 在线源下载目录（只读派生） ----
// 路径不在前端拼：`<壁纸目录>\BingWallpaper` / `<壁纸目录>\Pixabay` /
// `<壁纸目录>\Wallhaven` 这几条规则只在 Rust 侧各存在一份，这里只负责展示后端给的最终路径。
// 遗留目录（老版本可单独配置）只提示不搬移。
const openingLegacy = ref(false);

/// 两个在线源的展示配置。**文件名规则写在这里是为了让用户知道下载下来叫什么**，
/// 与后端 `bing::download_wallpaper` / `pixabay::download_wallpaper` 的命名保持一致；
/// 改后端命名时这里要同步改（只是展示文案，改错不会报错，只会说错话）。
const onlineDirs = computed(() => [
  {
    key: "bing",
    path: store.bingDirInfo?.dir || "",
    pattern: "文件名：BingWallpaper_日期.jpg",
  },
  {
    key: "pixabay",
    path: store.pixabayDirInfo?.dir || "",
    // 图片与视频落在**同一个目录**，靠前缀区分（两个 id 命名空间可能撞号）。
    // 用户来这一栏就是确认"文件落在哪、叫什么"，两条规则都要写全。
    pattern: "文件名：Pixabay_图片id.jpg · PixabayVideo_视频id.mp4",
  },
  {
    key: "wallhaven",
    path: store.wallhavenDirInfo?.dir || "",
    // id 是 6 位字母数字；扩展名随原图（jpg / png / webp）
    pattern: "文件名：Wallhaven_图片id.jpg",
  },
]);

function refreshOnlineDirs() {
  void store.refreshBingDirInfo();
  void store.refreshPixabayDirInfo();
  void store.refreshWallhavenDirInfo();
}

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

onMounted(refreshOnlineDirs);
// 壁纸目录一变，派生出的下载目录跟着变；面板常驻，靠 watch 补刷新
watch(() => store.currentDir, refreshOnlineDirs);
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

    <!-- 这里原本有一条「选择目录后左侧列表自动刷新，无需重启」的提示，
         已删除：它讲的是点一下就能看到的结果，属于废话；而面板在 640 高的窗口里
         本来就差 45px 放不下，删掉后在线壁纸目录那段才能完整露出。
         真正需要保留的是底部那条「只浏览不落盘」—— 那是隐私承诺，不是废话。 -->

    <!-- 在线壁纸目录（各来源都是**只读派生**，规则只在 Rust 侧存在一份） -->
    <div class="panel-header pt-5">
      <NIcon :component="DownloadCloud" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">在线壁纸目录</div>
      <div class="panel-desc">固定为壁纸目录的子目录</div>
    </div>

    <div class="dir-card">
      <!-- 不再写「下载目录」这个小标签：上面的标题已经说了"在线壁纸目录"，
           重复一次白占 20px，而面板在 640 高的窗口里本来就紧。 -->
      <!-- 每个在线源一行。以后再加源，往 onlineDirs 里加一项即可，
           不用再复制一段卡片模板。 -->
      <div
        v-for="d in onlineDirs"
        :key="d.key"
        class="online-row"
      >
        <NIcon :component="ImageIcon" :size="16" class="dir-icon" />
        <div class="min-w-0 flex-1">
          <div class="online-path">{{ d.path || "读取中…" }}</div>
          <div class="online-pattern">{{ d.pattern }}</div>
        </div>
      </div>
    </div>

    <!-- 老版本遗留目录：只提示不搬移。用户在旧版本里单独指定过下载目录时，
         那些文件落在壁纸目录之外，「本地」选项卡扫不到 —— 给数量 + 打开入口。 -->
    <div v-if="store.bingDirInfo?.legacy_dir && store.bingDirInfo.legacy_count > 0" class="legacy-card">
      <div class="legacy-head">
        <NIcon :component="AlertTriangle" :size="14" class="legacy-icon" />
        <span class="legacy-title">发现旧版本的必应下载目录</span>
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
      <span>点击"设为壁纸"或卡片上的「下载」时，原图才会下载到对应目录 —— 只浏览不落盘</span>
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

/* ========== 在线源目录行 ==========
   两个在线源各一行，路径与文件名规则上下排；路径可换行（Windows 长路径很常见），
   所以用 break-all 而不是省略号 —— 用户是来确认"文件到底落在哪"的，截断就没用了。 */
.online-row {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.45rem 0;
}
.online-row + .online-row {
  border-top: 1px solid var(--color-line);
}
.online-path {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--color-tx);
  word-break: break-all;
  line-height: 1.4;
}
.online-pattern {
  margin-top: 0.15rem;
  font-size: 10.5px;
  color: var(--color-faint);
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
