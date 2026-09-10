// DotWallpaper 壁纸工具 - Pinia 状态仓库（替代原 useWallpaper.ts 模块级 ref）
import { computed, ref } from "vue";
import { defineStore } from "pinia";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { confirmDanger, toast } from "../lib/naive-host";

// ---------- 类型 ----------
export type WallpaperKind = "local" | "current";

/// 壁纸来源选项卡：local = 本地壁纸（可增删），system = Windows 自带系统壁纸（只读），
/// favorites = 收藏夹（书签视图：跨本地/系统来源，仅标记不删文件）
export type WallpaperSource = "local" | "system" | "favorites";

/// 各来源选项卡的可见性：local 默认强制开启、不可关闭
export const SOURCE_VIS_KEY = "dot-wallpaper-source-visibility";
export type SourceVisibility = { local: true; system: boolean; favorites: boolean };
// 选项卡展示顺序（独立于可见性：顺序控制渲染次序，可见性控制是否显示）
export const SOURCE_ORDER_KEY = "dot-wallpaper-source-order";
export const SOURCE_ORDER_DEFAULT: WallpaperSource[] = ["local", "favorites", "system"];

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
export const FAVORITES_KEY = "dot-wallpaper-favorites"; // localStorage 收藏书签集合键
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
  const previewItem = ref<WallpaperItem | null>(null); // 正在预览的壁纸（选中态，非当前桌面）
  const desktopStyle = ref<{ style: number; tile: boolean } | null>(null); // 桌面壁纸样式
  const isApplying = ref(false); // 应用壁纸 loading
  const loadingMore = ref(false);
  const allCount = ref(0);
  const favorites = ref<Set<string>>(new Set()); // 收藏壁纸路径集合（localStorage 持久化）

  // 各来源选项卡的可见性：local 默认 true、不可关闭；system 默认 false（可在设置开启）；favorites 默认 true
  const sourceVisibility = ref<SourceVisibility>({ local: true, system: false, favorites: true });
  restoreSourceVisibility();

  function restoreSourceVisibility() {
    try {
      const raw = localStorage.getItem(SOURCE_VIS_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<SourceVisibility>;
        sourceVisibility.value.system = parsed.system ?? false;
        sourceVisibility.value.favorites = parsed.favorites ?? true;
      }
    } catch { /* ignore */ }
  }

  function persistSourceVisibility() {
    try {
      localStorage.setItem(SOURCE_VIS_KEY, JSON.stringify({
        system: sourceVisibility.value.system,
        favorites: sourceVisibility.value.favorites,
      }));
    } catch { /* ignore */ }
  }

  function setSourceVisibility(key: "system" | "favorites", visible: boolean) {
    sourceVisibility.value[key] = visible;
    persistSourceVisibility();
    // 当前正停留在被隐藏的选项卡时自动切回本地，避免留下不可达空栏
    if (!visible && source.value === key) {
      void setSource("local");
    }
  }

  // ---- 选项卡展示顺序（持久化；local 恒可见但仍可参与排序） ----
  const sourceOrder = ref<WallpaperSource[]>([...SOURCE_ORDER_DEFAULT]);
  restoreSourceOrder();

  function restoreSourceOrder() {
    try {
      const raw = localStorage.getItem(SOURCE_ORDER_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return;
      const list = parsed.filter(
        (s): s is WallpaperSource =>
          s === "local" || s === "system" || s === "favorites"
      );
      // 去重并补全缺失来源，保证数组恰好包含全部三个来源
      const seen = new Set<WallpaperSource>(list);
      for (const s of SOURCE_ORDER_DEFAULT) {
        if (!seen.has(s)) {
          list.push(s);
          seen.add(s);
        }
      }
      sourceOrder.value = list;
    } catch { /* ignore */ }
  }

  function persistSourceOrder() {
    try {
      localStorage.setItem(SOURCE_ORDER_KEY, JSON.stringify(sourceOrder.value));
    } catch { /* ignore */ }
  }

  // 将指定来源在展示顺序中上移 / 下移（dir: -1 上移，1 下移）
  function moveSourceOrder(key: WallpaperSource, dir: -1 | 1) {
    const idx = sourceOrder.value.indexOf(key);
    const target = idx + dir;
    if (idx < 0 || target < 0 || target >= sourceOrder.value.length) return;
    const next = [...sourceOrder.value];
    [next[idx], next[target]] = [next[target], next[idx]];
    sourceOrder.value = next;
    persistSourceOrder();
  }

  // 拖拽排序：将指定来源移动到目标来源所在的槽位（就地替换式插入）
  function moveSourceOrderTo(key: WallpaperSource, targetKey: WallpaperSource) {
    const from = sourceOrder.value.indexOf(key);
    const to = sourceOrder.value.indexOf(targetKey);
    if (from < 0 || to < 0 || from === to) return;
    const next = [...sourceOrder.value];
    next.splice(from, 1);
    const insertAt = next.indexOf(targetKey) + (from < to ? 1 : 0);
    next.splice(insertAt, 0, key);
    sourceOrder.value = next;
    persistSourceOrder();
  }

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
  // hasMore 必须是响应式普通 ref（与 allCount 等一致，pinia 访问时解包为 boolean）：
  // 若用 computed 包普通 let 变量，let 变化不会让 computed 失效（永远 false）；
  // 若 computed getter 返回 ref，store.hasMore 拿到的又是 Ref 对象而非 boolean，
  // 模板 !store.hasMore 恒为 false，底部“已加载全部”永不显示、fill 补页判断失真。
  const hasMore = ref(false);

  // ---- Getter ----
  // 右侧大预览目标：优先"正在预览"，无预览时回退当前桌面壁纸
  const previewTarget = computed<WallpaperItem | null>(
    () => previewItem.value ?? currentWallpaper.value
  );

  // ---- 右键菜单 ----
  function openContextMenu(item: WallpaperItem, x: number, y: number) {
    ctxItem.value = item;
    ctxX.value = Math.min(x, window.innerWidth - 216);
    ctxY.value = Math.min(y, window.innerHeight - 220);
    ctxVisible.value = true;
    // 系统壁纸 / 收藏夹为只读视图：右键菜单不提供删除入口
    // （收藏夹是书签视图，删除文件入口仍由本地/系统来源提供，避免误删）
    ctxReadOnly.value = source.value === "system" || source.value === "favorites";
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
    hasMore.value = false;
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

  // 点击卡片：仅选中/预览，不改动桌面（选中态与当前桌面解耦）
  function selectItem(item: WallpaperItem) {
    previewItem.value = item;
  }

  // 读取系统当前壁纸样式（填充/适应等），供预览与"设为壁纸"使用
  async function loadDesktopStyle() {
    try {
      const s = (await invoke("get_wallpaper_style")) as { style: number; tile: boolean };
      desktopStyle.value = {
        style: Number(s.style) || 10,
        tile: Boolean(s.tile),
      };
    } catch (err: unknown) {
      console.error("读取壁纸样式失败:", err);
      desktopStyle.value = null;
    }
  }

  // 将某张壁纸设为桌面壁纸；可选同步应用桌面样式（仅当与系统当前样式不一致时写注册表）
  async function setItemAsDesktop(
    item: WallpaperItem,
    style?: { style: number; tile: boolean }
  ): Promise<boolean> {
    if (isApplying.value) return false;
    const path = item.path || "";
    if (!path) {
      toast("壁纸路径无效", "warning");
      return false;
    }
    isApplying.value = true;
    try {
      if (
        style &&
        desktopStyle.value &&
        (style.style !== desktopStyle.value.style || style.tile !== desktopStyle.value.tile)
      ) {
        await invoke("set_desktop_style", { style: style.style, tile: style.tile });
        desktopStyle.value = { ...style };
      }
      const result = await doSetWallpaper(item);
      currentWallpaper.value = {
        key: "current_" + (result.path || ""),
        kind: "local",
        path: result.path,
      };
      toast("壁纸设置成功", "success");
      return true;
    } catch (err: unknown) {
      toast("设置失败：" + ((err as Error)?.message || String(err)), "error");
      return false;
    } finally {
      isApplying.value = false;
    }
  }

  // 将右侧正在预览（无预览时为当前桌面）的壁纸设为桌面（Ctrl+S / 设为壁纸按钮）
  async function applyPreviewAsDesktop(
    style?: { style: number; tile: boolean }
  ): Promise<boolean> {
    const target = previewTarget.value;
    if (!target || !target.path) {
      toast("当前无可预览壁纸", "warning");
      return false;
    }
    return setItemAsDesktop(target, style);
  }

  // ---- 壁纸源加载（分页） ----
  async function loadWallpapers() {
    gridItems.value = [];
    loadedCount = 0;
    allEntries = [];
    try {
      // 后端列表返回：原图路径 + 缩略图路径（缩略图可能为空 → 列表显示占位）
      if (source.value === "system") {
        allEntries = (await invoke("list_system_wallpapers")) as WallpaperEntryData[];
      } else if (source.value === "favorites") {
        // 收藏夹：按收藏路径在本地/系统两个来源中取交集，复用缩略图缓存
        allEntries = await loadFavoriteEntries();
      } else {
        allEntries = (await invoke("list_local_wallpapers", {
          directory: resolveDirArg(),
        })) as WallpaperEntryData[];
      }
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
    hasMore.value = loadedCount < allEntries.length;
  }

  async function loadMore() {
    if (loadingMoreLock || !hasMore.value) return;
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

  // ---- 收藏夹（书签标记：纯前端 localStorage 持久化，后端零改动） ----
  function loadFavorites() {
    try {
      const raw = localStorage.getItem(FAVORITES_KEY);
      const arr: unknown = raw ? JSON.parse(raw) : [];
      favorites.value = new Set(
        Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : []
      );
    } catch {
      favorites.value = new Set();
    }
  }

  function persistFavorites() {
    try {
      localStorage.setItem(FAVORITES_KEY, JSON.stringify([...favorites.value]));
    } catch {
      /* ignore */
    }
  }

  function isFavorite(path: string | undefined | null): boolean {
    return !!path && favorites.value.has(path);
  }

  // 收藏页数据：直接按收藏路径向后端查询，不经过当前壁纸目录，
  // 因此切换壁纸目录后收藏依然完整；已被外部删除的失效路径由后端过滤不展示
  async function loadFavoriteEntries(): Promise<WallpaperEntryData[]> {
    const favPaths = [...favorites.value];
    if (!favPaths.length) return [];
    return (await invoke<WallpaperEntryData[]>("list_wallpapers_by_paths", {
      paths: favPaths,
    })) as WallpaperEntryData[];
  }

  // 从当前已加载列表移除某路径（收藏页取消收藏时即时消失，避免整表重载闪烁）
  function dropFavoriteFromGrid(path: string) {
    allEntries = allEntries.filter((e) => e.path !== path);
    gridItems.value = gridItems.value.filter((it) => it.path !== path);
    loadedCount = gridItems.value.length;
    allCount.value = allEntries.length;
    hasMore.value = loadedCount < allEntries.length;
  }

  // 增删收藏书签；返回是否已收藏（true = 刚加入）
  function toggleFavorite(path: string | undefined | null): boolean {
    if (!path) return false;
    const had = favorites.value.has(path);
    if (had) favorites.value.delete(path);
    else favorites.value.add(path);
    persistFavorites();

    // 在收藏夹页取消收藏：立即移除卡片并清空对应预览，避免幽灵项
    if (had && source.value === "favorites") {
      if (previewItem.value?.path === path) previewItem.value = null;
      dropFavoriteFromGrid(path);
    }
    return !had;
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
      // 设置桌面不改变"正在预览"的选中态：蓝（预览）与绿（桌面）独立
      await setItemAsDesktop(item);
      return;
    }

    if (action === "toggle-favorite") {
      // 收藏 / 取消收藏：仅书签标记，不删文件
      const fav = toggleFavorite(item.path);
      if (fav) toast("已收藏：" + baseName(item.path || ""), "success");
      else toast("已取消收藏", "warning");
      return;
    }

    if (action === "reveal-folder") {
      // 在系统资源管理器中打开文件所在目录并定位（本地/系统壁纸通用）
      if (!item.path) {
        toast("当前壁纸无本地路径", "warning");
        return;
      }
      try {
        await invoke("reveal_in_explorer", { path: item.path });
      } catch (err: unknown) {
        toast("打开目录失败：" + ((err as Error)?.message || String(err)), "error");
      }
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
      // 文件已物理删除：同步清理收藏书签，避免收藏夹出现失效路径
      if (favorites.value.delete(item.path)) persistFavorites();
      toast("已删除壁纸：" + name, "success");
      await loadWallpapers();
      // 删除的正是当前桌面壁纸：同步刷新桌面真值
      if (currentWallpaper.value?.path === item.path) {
        await loadCurrentWallpaper();
      }
      // 删除的正是正在预览的壁纸：清空选中态，大预览回退到当前桌面
      if (previewItem.value?.path === item.path) {
        previewItem.value = null;
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
    previewItem,
    desktopStyle,
    isApplying,
    loadingMore,
    allCount,
    ctxVisible,
    ctxX,
    ctxY,
    ctxItem,
    ctxReadOnly,
    sourceVisibility,
    sourceOrder,
    // getters
    hasMore,
    previewTarget,
    // actions
    setSource,
    openContextMenu,
    closeContextMenu,
    selectItem,
    loadFavorites,
    isFavorite,
    toggleFavorite,
    setItemAsDesktop,
    applyPreviewAsDesktop,
    loadDesktopStyle,
    loadWallpapers,
    loadMore,
    loadCurrentWallpaper,
    restoreDir,
    pickAndApplyDirectory,
    saveDroppedPaths,
    handleContextAction,
    removeWallpaper,
    setSourceVisibility,
    persistSourceVisibility,
   moveSourceOrder,
    moveSourceOrderTo,
 };
});
