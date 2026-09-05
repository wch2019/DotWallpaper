// DotWallpaper 壁纸工具 - Pinia 状态仓库（替代原 useWallpaper.ts 模块级 ref）
import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirmDanger, toast } from "../lib/naive-host";

// ---------- 类型 ----------
export type WallpaperKind = "local" | "current";

/// 壁纸来源选项卡：local = 本地壁纸（可增删），system = Windows 自带系统壁纸（只读）
export type WallpaperSource = "local" | "system";

export interface WallpaperItem {
  key: string;
  kind: WallpaperKind; // 类型：本地 / 当前壁纸（右键目标）
  path?: string; // 本地绝对路径（原图）
  title?: string;
  thumb?: string; // 缩略图绝对路径（列表加载用；无缩略图时为空）
  applying?: boolean; // 是否正在设置中
}

/// 后端列表命令返回条目：原图路径 + 缩略图路径（可能为空）
interface WallpaperEntryData {
  path: string;
  thumb: string;
}

/// 后端后台缩略图完成事件 payload：原图路径 + 缩略图路径
interface ThumbnailUpdatedPayload {
  path: string;
  thumb: string;
}

// ---------- 常量 ----------
export const DIR_STORAGE_KEY = "dot-wallpaper-dir"; // localStorage 持久化键
export const PAGE_SIZE = 12; // 每页加载张数

// 后台缩略图事件监听全局只注册一次（应用单页生命周期内复用）
let thumbnailListenerRegistered = false;

// ---------- 纯工具函数 ----------
export function shuffle<T>(arr: T[]): T[] {
  const a = [...arr];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1];
}

// 图片能用于展示的地址：本地路径走 convertFileSrc
export function displaySrc(item: WallpaperItem): string {
  return item.path ? convertFileSrc(item.path) : "";
}

// 列表缩略图 URL：基于后端生成的缩略图缓存路径（无缩略图时返回空串，
// 由调用方显示占位；不回退原图，保证大图不进入列表加载链路）
export function thumbSrc(item: WallpaperItem): string {
  return item.thumb ? convertFileSrc(item.thumb) : "";
}

// 徽标文案（当前仅本地）
export function kindBadgeText(kind: WallpaperKind): string {
  return kind === "local" ? "本地" : "";
}

// ---------- Pinia Store ----------
export const useWallpaperStore = defineStore("wallpaper", () => {
  // ---- 状态 ----
  const source = ref<WallpaperSource>("local"); // 当前选项卡来源
  const currentDir = ref(""); // 自定义壁纸目录（空 = 预设目录）
  const gridItems = ref<WallpaperItem[]>([]); // 左栏当前列表
  const currentWallpaper = ref<WallpaperItem | null>(null); // 右侧当前壁纸
  const loadingMore = ref(false);
  const allCount = ref(0);

  // 右键菜单状态
  const ctxVisible = ref(false);
  const ctxX = ref(0);
  const ctxY = ref(0);
  const ctxItem = ref<WallpaperItem | null>(null);
  const ctxReadOnly = ref(false); // 当前右键目标是否为只读来源（系统壁纸禁止删除）

  // 分页私有状态
  let loadedCount = 0;
  let allEntries: WallpaperEntryData[] = [];
  let loadingMoreLock = false;
  let hasMoreFlag = false;

  // ---- Getter ----
  const hasMore = computed(() => hasMoreFlag);

  // ---- 右键菜单 ----
  function openContextMenu(item: WallpaperItem, x: number, y: number) {
    ctxItem.value = item;
    ctxX.value = Math.min(x, window.innerWidth - 200);
    ctxY.value = Math.min(y, window.innerHeight - 180);
    ctxVisible.value = true;
    // 系统壁纸来源只读：右键菜单不提供删除入口
    ctxReadOnly.value = source.value === "system";
  }

  function closeContextMenu() {
    ctxVisible.value = false;
    ctxItem.value = null;
    ctxReadOnly.value = false;
  }

  // ---- 后台缩略图渐进更新 ----
  // 后端列表命令不再等待全量缩略图生成：缺失项由后台线程池渐进生成，
  // 每完成一张推送 "thumbnail-updated" { path, thumb }。命中当前列表条目时
  // 仅更新该条 thumb（响应式触发对应 <img> :src 重算加载新图），不整列表刷新；
  // 尚未被懒加载放行的条目无需处理，放行时自然取到最新 thumb。
  async function registerThumbnailListener() {
    if (thumbnailListenerRegistered) return;
    thumbnailListenerRegistered = true;
    await listen<ThumbnailUpdatedPayload>("thumbnail-updated", (ev) => {
      const { path, thumb } = ev.payload;
      if (!path || !thumb) return;
      const item = gridItems.value.find((it) => it.path === path);
      if (item && item.thumb !== thumb) {
        item.thumb = thumb;
      }
    });
  }
  void registerThumbnailListener();

  // ---- 选项卡切换 ----
  async function setSource(next: WallpaperSource) {
    if (source.value === next) return;
    source.value = next;
    gridItems.value = [];
    loadedCount = 0;
    allEntries = [];
    hasMoreFlag = false;
    allCount.value = 0;
    await loadWallpapers();
  }

  // ---- 设置壁纸（核心） ----
  async function doSetWallpaper(item: WallpaperItem): Promise<{ path: string }> {
    const payload = item.path || "";
    const result = (await invoke("set_wallpaper", {
      path: payload,
      dir: resolveDirArg(),
    })) as { path: string } | string;
    return typeof result === "string" ? { path: result } : result;
  }

  function resolveDirArg(): string | null {
    return currentDir.value && currentDir.value.trim()
      ? currentDir.value.trim()
      : null;
  }

  async function applyItem(item: WallpaperItem): Promise<boolean> {
    if (item.applying) return false;
    item.applying = true;
    try {
      const result = await doSetWallpaper(item);
      toast("壁纸设置成功", "success");
      currentWallpaper.value = {
        key: "current_" + (result.path || ""),
        kind: "local",
        path: result.path,
      };
      return true;
    } catch (err: unknown) {
      toast("设置失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      item.applying = false;
    }
  }

  // 将右侧当前壁纸设为桌面（Ctrl+S / 按钮）
  async function setCurrentAsDesktop() {
    if (!currentWallpaper.value) {
      toast("当前无壁纸", "warning");
      return;
    }
    const cur = currentWallpaper.value;
    try {
      const result = (await invoke("set_wallpaper", {
        path: cur.path || "",
        dir: resolveDirArg(),
      })) as { path: string } | string;
      const path = typeof result === "string" ? result : result.path;
      currentWallpaper.value = { key: "current_" + path, kind: "local", path };
      toast("已设置为当前壁纸 (Ctrl+S)", "success");
    } catch (err: unknown) {
      toast("设置失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  // ---- 壁纸源加载（分页） ----
  async function loadWallpapers() {
    gridItems.value = [];
    loadedCount = 0;
    allEntries = [];
    try {
      // 后端列表返回：原图路径 + 缩略图路径（缩略图可能为空 → 列表显示占位）
      allEntries =
        source.value === "system"
          ? ((await invoke("list_system_wallpapers")) as WallpaperEntryData[])
          : ((await invoke("list_local_wallpapers", {
              directory: resolveDirArg(),
            })) as WallpaperEntryData[]);
      allCount.value = allEntries.length;
      appendBatch();
    } catch (err: unknown) {
      toast("加载壁纸失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  function appendBatch() {
    const batch = allEntries.slice(loadedCount, loadedCount + PAGE_SIZE);
    batch.forEach((e) => {
      gridItems.value.push({
        key: "local_" + e.path,
        kind: "local" as WallpaperKind,
        path: e.path,
        title: baseName(e.path),
        thumb: e.thumb || undefined,
      });
      loadedCount++;
    });
    hasMoreFlag = loadedCount < allEntries.length;
  }

  async function loadMore() {
    if (loadingMoreLock || !hasMoreFlag) return;
    loadingMoreLock = true;
    loadingMore.value = true;
    try {
      await new Promise((r) => setTimeout(r, 200)); // 防抖
      appendBatch();
    } finally {
      loadingMoreLock = false;
      loadingMore.value = false;
    }
  }

  // 获取当前桌面壁纸文件（用于右侧展示）
  async function loadCurrentWallpaper() {
    try {
      const path = (await invoke("get_current_wallpaper")) as string;
      if (path) {
        currentWallpaper.value = { key: "current_" + path, kind: "local", path };
      } else {
        currentWallpaper.value = null;
      }
    } catch (err: unknown) {
      toast("获取当前壁纸失败：" + ((err as Error)?.message || String(err)), "error");
      currentWallpaper.value = null;
    }
  }

  // ---- 目录持久化与选择 ----
  function restoreDir() {
    try {
      const saved = localStorage.getItem(DIR_STORAGE_KEY);
      if (saved) currentDir.value = saved;
    } catch {
      /* ignore */
    }
  }

  async function pickAndApplyDirectory() {
    try {
      const picked = (await invoke("pick_wallpaper_directory")) as string | null;
      if (picked) {
        currentDir.value = picked;
        try {
          localStorage.setItem(DIR_STORAGE_KEY, picked);
        } catch {
          /* ignore */
        }
        toast("已选择目录：" + picked, "success");
        await loadWallpapers();
      }
    } catch (err: unknown) {
      toast("选择目录失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  // ---- 外部拖入保存 ----
  // 将 Tauri 原生拖放事件给出的本地文件路径复制到壁纸目录并刷新列表
  async function saveDroppedPaths(paths: string[]) {
    if (!paths.length) return;
    try {
      const res = (await invoke("save_dropped_paths", {
        paths,
        dir: resolveDirArg(),
      })) as { saved: string[]; skipped: string[] };

      if (res.saved.length) {
        toast(
          `已保存 ${res.saved.length} 张壁纸` +
            (res.skipped.length ? `，跳过 ${res.skipped.length} 个` : ""),
          "success"
        );
        await loadWallpapers();
      } else if (res.skipped.length) {
        toast("没有可保存的图片：" + res.skipped[0], "warning");
      }
    } catch (err: unknown) {
      toast("保存失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  // ---- 右键菜单动作 ----
  async function handleContextAction(action: string, item: WallpaperItem | null) {
    closeContextMenu();
    if (!item) return;

    if (action === "set-wallpaper") {
      if (item.kind === "current") {
        try {
          const result = (await invoke("set_wallpaper", {
            path: item.path || "",
            dir: resolveDirArg(),
          })) as { path: string } | string;
          const path = typeof result === "string" ? result : result.path;
          currentWallpaper.value = { key: "current_" + path, kind: "local", path };
          toast("已重新设置当前壁纸", "success");
        } catch (err: unknown) {
          toast("设置失败：" + ((err as Error)?.message || String(err)), "error");
        }
        return;
      }
      await applyItem(item);
      return;
    }

    if (action === "delete-wallpaper" && item.kind !== "current") {
      // 系统壁纸来源只读：纵深防御，禁止进入删除流程
      if (source.value === "system") return;
      await removeWallpaper(item);
    }
  }

  // 右键删除：本地磁盘永久删除壁纸文件
  async function removeWallpaper(item: WallpaperItem): Promise<void> {
    if (!item.path) return;
    // 只读保护：系统壁纸来源不允许删除（同时后端 delete_wallpaper 亦拦截系统路径）
    if (source.value === "system") return;
    const name = baseName(item.path);

    const confirmed = await confirmDanger({
      title: "删除壁纸",
      content: `确定要删除壁纸「${name}」吗？\n该文件会从本地磁盘永久删除，无法恢复。`,
      positiveText: "删除",
    });
    if (!confirmed) return;

    try {
      await invoke("delete_wallpaper", { path: item.path });
      toast("已删除壁纸：" + name, "success");
      await loadWallpapers();
      // 若删除的正是当前桌面壁纸，同步刷新右侧预览
      if (currentWallpaper.value?.path === item.path) {
        await loadCurrentWallpaper();
      }
    } catch (err: unknown) {
      toast("删除失败：" + ((err as Error)?.message || String(err)), "error");
    }
  }

  return {
    // state
    source,
    currentDir,
    gridItems,
    currentWallpaper,
    loadingMore,
    allCount,
    ctxVisible,
    ctxX,
    ctxY,
    ctxItem,
    ctxReadOnly,
    // getters
    hasMore,
    // actions
    setSource,
    openContextMenu,
    closeContextMenu,
    applyItem,
    setCurrentAsDesktop,
    loadWallpapers,
    loadMore,
    loadCurrentWallpaper,
    restoreDir,
    pickAndApplyDirectory,
    saveDroppedPaths,
    handleContextAction,
    removeWallpaper,
  };
});
