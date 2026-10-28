// DotWallpaper 壁纸工具 - 来源选项卡可见性与顺序状态（stores/wallpaper.ts 拆分子模块）
import { ref } from "vue";
import {
  SOURCE_VIS_KEY,
  SOURCE_ORDER_KEY,
  SOURCE_ORDER_DEFAULT,
  type WallpaperSource,
  type SourceVisibility,
} from "./types";

// 来源选项卡可见性与展示顺序（组合式模块：localStorage 持久化）
export function useSourceState() {
  // 各来源选项卡的可见性。
  //
  // **默认只开「本地 + 收藏」**：system / bing / pixabay 一律默认关闭，
  // 由用户自己在「设置 → 壁纸来源」里打开。
  // 理由：这三个来源都不是"装好就能用"的 —— 系统壁纸是只读的固定几张，
  // 必应要联网拉图，Pixabay 还得先填 API key。默认全展开会让新用户第一眼
  // 看到一堆空列表/加载失败，以为应用坏了。
  //
  // local 恒为 true 且不可关闭（应用主源）。
  const sourceVisibility = ref<SourceVisibility>({
    local: true,
    system: false,
    favorites: true,
    bing: false,
    pixabay: false,
  });

  /// 读取持久化的可见性。
  ///
  /// ⚠️ 这里的 `?? false` 是**新的默认口径**，与上面的初始值必须保持一致：
  /// 老版本写下的对象里可能没有某个键（该来源是后加的），缺键就当"未开启"。
  /// 用户主动改过的值照原样保留 —— 默认值只影响"从没设置过"的情况。
  function restoreSourceVisibility() {
    try {
      const raw = localStorage.getItem(SOURCE_VIS_KEY);
      if (raw) {
        const parsed = JSON.parse(raw) as Partial<SourceVisibility>;
        sourceVisibility.value.system = parsed.system ?? false;
        sourceVisibility.value.favorites = parsed.favorites ?? true;
        sourceVisibility.value.bing = parsed.bing ?? false;
        sourceVisibility.value.pixabay = parsed.pixabay ?? false;
      }
    } catch { /* ignore */ }
  }

  function persistSourceVisibility() {
    try {
      localStorage.setItem(SOURCE_VIS_KEY, JSON.stringify({
        system: sourceVisibility.value.system,
        favorites: sourceVisibility.value.favorites,
        bing: sourceVisibility.value.bing,
        pixabay: sourceVisibility.value.pixabay,
      }));
    } catch { /* ignore */ }
  }

  // 仅更新可见性并持久化；"隐藏当前选项卡时自动切回本地"的联动由主 store 的 setSourceVisibility 处理
  function updateSourceVisibility(
    key: "system" | "favorites" | "bing" | "pixabay",
    visible: boolean
  ) {
    sourceVisibility.value[key] = visible;
    persistSourceVisibility();
  }

  // ---- 选项卡展示顺序（持久化；local 恒可见但仍可参与排序） ----
  const sourceOrder = ref<WallpaperSource[]>([...SOURCE_ORDER_DEFAULT]);

  function restoreSourceOrder() {
    try {
      const raw = localStorage.getItem(SOURCE_ORDER_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return;
      const list = parsed.filter(
        (s): s is WallpaperSource =>
          s === "local" ||
          s === "system" ||
          s === "favorites" ||
          s === "bing" ||
          s === "pixabay"
      );
      // 去重并补全缺失来源，保证数组恰好包含全部来源
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

  restoreSourceVisibility();
  restoreSourceOrder();
  return {
    sourceVisibility,
    sourceOrder,
    updateSourceVisibility,
    persistSourceVisibility,
    moveSourceOrder,
    moveSourceOrderTo,
  };
}
