<script setup lang="ts">
// 动态壁纸播放页（视频）
// 由后端创建的置底 WebviewWindow 以 ?view=video 加载本页面；
// 页面只渲染全屏循环视频，双击桌面空白处可停止动态壁纸。
//
// 关于循环连贯性（为什么不用 loop / 不用单元素回绕）：
//   1) HTML5 的 loop 属性在回到开头时会重新 seek 并解码，循环点几乎必然掉帧或闪黑；
//   2) 单元素"提前 seek 到 0"也只是把这次 seek 提前，仍会丢几帧 —— 表现为
//      "循环点画面一顿"；
//   3) 真正无感的方式是**双实例交叉播放**：两个 <video> 交替承担播放，
//      当 A 接近结尾时唤醒已预载好的 B 从 0 开始播放，两者交叉淡入淡出，
//      切换瞬间画面始终有内容，观感上完全连续。
//   本文件采用方案 3，并在 WebView2 不支持交叉时回退到单元素回绕（方案 2）。
import { onMounted, onUnmounted, ref } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

const src = ref("");
const failed = ref(false);
const errorMsg = ref("");
const hintVisible = ref(true);
/// 静音（绑定到两个 <video> 的 muted 属性，由主界面的静音开关控制）
const muted = ref(false);
/// 是否暂停：暂停时冻结当前帧（不销毁窗口、不回退到静态壁纸），恢复时从原处继续
const paused = ref(false);
/// 两个交替播放的视频元素
const videoA = ref<HTMLVideoElement | null>(null);
const videoB = ref<HTMLVideoElement | null>(null);

let unlisten: UnlistenFn | null = null;
let hintTimer: number | undefined;
/// 主循环定时器（交叉检测）
let loopTimer: number | undefined;
/// 交叉淡入的收尾定时器（淡入走完后隐藏旧前台）。
/// **必须可取消**：换源时若不清掉，它会在约 460ms 后去改一个已经换过角色的元素。
/// 目前换源会重建 `<video>`（`v-if`）使旧引用变成游离节点，所以实际不会出问题 ——
/// 但那是"靠副作用兜底"，不该依赖。换源路径显式取消它。
let crossTimer: number | undefined;
/// 当前正在"前台"显示的元素索引（0=A, 1=B）
let front = 0;
/// 当前生效的视频地址（判断是否需要重置）
let currentSrc = "";
/// 单调递增的 z-index 计数器。
///
/// **绝不能用固定的 1/2**：交叉结束后旧前台仍保留着自己的 z-index，
/// 下一次交叉若把新前台也设成同一个值，二者 z-index 相等，
/// 绘制顺序就退化成 DOM 顺序 —— 而 DOM 里 B 永远在 A 之后，
/// 于是"永远 B 压 A"：当 A 才是要淡入的新前台时，画面变成
/// 旧帧压在新帧上面淡出，方向和亮度全反了。症状是**每隔一次交叉就出问题**。
/// 每次交叉取一个更大的值，才能保证"新前台一定在最上层"。
/// 长期运行这个值会一直涨（10s 的片子一天约 8600），但 z-index 是 32 位整数、
/// 且只有两个元素 —— **不要为此加"归一化回收"逻辑**：归一化本身要在播放中改
/// z-index，为了一个永远不会溢出的计数器去动正在显示的层，得不偿失。
let zCounter = 2;
/// 交叉点提前量（秒）。
///
/// **必须明显大于 `CROSS_FADE_SECS`**：旧前台在淡入走完之前不能播到结尾，
/// 否则它会在仍然可见的时候冻在最后一帧，与新画面混在一起 → 看着像卡了一下。
/// 但也不能太大：新元素是从 0 开始播的，两层内容相差正好一个提前量，
/// 太大就成了"重影"。所以取"略大于淡入时长 + 一个轮询周期"。
const CROSS_LEAD_SECS = 0.5;
/// 交叉淡入时长（秒）
const CROSS_FADE_SECS = 0.4;
/// 交叉检测的轮询周期（毫秒）。
/// 越短，实际提前量越贴近 `CROSS_LEAD_SECS`（抖动越小）；
/// 它和上面两个常量是绑定的 —— `lead - fade` 必须能吃掉一个轮询周期的抖动。
const CROSS_POLL_MS = 25;
/// 是否已启用交叉循环（元数据就绪后确定）
let crossEnabled = false;

function elOf(i: number): HTMLVideoElement | null {
  return (i === 0 ? videoA.value : videoB.value) ?? null;
}

/// 起播。**失败时必须留下痕迹** ——
/// 被自动播放策略拒绝（取消静音后又没有用户手势）时画面会直接停住，
/// 一个裸的 `.catch(() => {})` 会让它看起来像"这个视频坏了"，无从排查。
/// 正常情况下不会走到这里：后端建窗口时用的是 wry 的默认参数，
/// 其中已含 `--autoplay-policy=no-user-gesture-required`（见 `create_video_window`）。
function tryPlay(el: HTMLVideoElement | null) {
  if (!el) return;
  void el.play().catch((err) => {
    console.warn(
      "[动态壁纸] play() 被拒绝，画面可能停住。若刚取消静音，可能是自动播放策略拦下了",
      err
    );
  });
}

/// 复位到"单元素前台"状态：A 显示并播放，B 隐藏备用
function resetElements() {
  const a = elOf(0);
  const b = elOf(1);
  for (const [i, el] of [
    [0, a],
    [1, b],
  ] as const) {
    if (!el) continue;
    el.loop = false;
    // 复位时清掉过渡：换源/重载后不需要任何动画，残留的 transition
    // 会让"瞬间切到初始态"变成一次可见的淡入。
    el.style.transition = "none";
    el.style.opacity = i === 0 ? "1" : "0";
    el.style.zIndex = i === 0 ? "1" : "0";
  }
  front = 0;
  zCounter = 2;
}

/// 双实例交叉循环：检测前台元素是否接近结尾，是则把后台元素拉到 0 播放并淡入。
///
/// **只淡入新前台，绝不淡出旧前台**（这是本函数的核心，改动前请先读注释）：
/// 旧实现让两层同时过渡（旧 1→0、新 0→1），而新层在上。带 alpha 的两层
/// 叠加合成出来的不是线性交叉：
///     结果 = α·新 + (1-α)·[(1-α)·旧] = α·新 + (1-α)²·旧
/// α=0.5 时整体亮度只剩约 **75%** —— 每个循环点整屏往黑里沉一下再回来。
/// 用户对它的描述正是"闪屏 + 循环不连贯"，而这两句说的是同一件事。
/// 让旧层保持完全不透明垫在下面、只淡入新层，才是真正的线性交叉：
///     结果 = α·新 + (1-α)·旧      （亮度恒定，无色偏、无明暗跳变）
function crossStep() {
  // 暂停时不推进循环：此时 currentTime 本就不动，但交叉淡入的定时器与
  // `front` 指针不该在冻结期间被改动，显式挡掉最稳妥。
  if (paused.value) return;
  const cur = elOf(front);
  const next = elOf(1 - front);
  if (!cur || !next) return;

  const d = cur.duration;
  if (!Number.isFinite(d) || d <= 0) return;

  // 还有足够剩余时间就什么都不做
  if (cur.currentTime < d - CROSS_LEAD_SECS) return;

  // 后台元素准备好并从头播放
  try {
    next.currentTime = 0;
  } catch {
    /* 忽略：部分编码不支持精确 seek，仍可继续 */
  }
  tryPlay(next);

  const fadeMs = Math.round(CROSS_FADE_SECS * 1000);
  // 新前台升到最上层（取一个比任何现存层都大的值，见 zCounter 注释）并淡入。
  next.style.transition = `opacity ${fadeMs}ms linear`;
  next.style.zIndex = String(++zCounter);
  next.style.opacity = "1";

  // 旧前台**保持 opacity 1**，只是被压在下层 —— 不要给它写 opacity。
  const oldEl = cur;

  // 淡入走完后：旧层已完全被不透明的上层盖住，此时再隐藏它不会有任何可见变化。
  if (crossTimer !== undefined) window.clearTimeout(crossTimer);
  crossTimer = window.setTimeout(() => {
    crossTimer = undefined;
    try {
      oldEl.pause();
      oldEl.currentTime = 0;
    } catch {
      /* ignore */
    }
    // 先把过渡清掉再改 opacity，否则它会花 fadeMs 再淡出一次
    // （虽然被盖住看不见，但会给下一次交叉留下一个"正在过渡"的状态）。
    oldEl.style.transition = "none";
    oldEl.style.opacity = "0";
    oldEl.style.zIndex = "0";
  }, fadeMs + 60);

  front = 1 - front;
}

/// 回退方案：单元素提前回绕（交叉循环不可用时）
const WRAP_LEAD_SECS = 0.28;
let wrapHandler: (() => void) | null = null;
function wrapStep() {
  if (paused.value) return;
  const cur = elOf(front);
  if (!cur) return;
  const d = cur.duration;
  if (!Number.isFinite(d) || d <= 0) return;
  if (cur.currentTime >= d - WRAP_LEAD_SECS) {
    try {
      cur.currentTime = 0;
    } catch {
      /* ignore */
    }
    tryPlay(cur);
  }
}

/// 启动循环驱动（交叉优先，失败回退单元素回绕）
function startLoop() {
  stopLoop();
  const a = elOf(0);
  const b = elOf(1);
  if (!a) return;
  if (b && crossEnabled) {
    loopTimer = window.setInterval(crossStep, CROSS_POLL_MS);
  } else {
    wrapHandler = wrapStep;
    a.addEventListener("timeupdate", wrapHandler);
    loopTimer = window.setInterval(wrapStep, 60);
  }
}

function stopLoop() {
  if (loopTimer !== undefined) {
    window.clearInterval(loopTimer);
    loopTimer = undefined;
  }
  // 收尾定时器也要取消：换源/停止后它已经没有意义，留着会去改游离节点
  if (crossTimer !== undefined) {
    window.clearTimeout(crossTimer);
    crossTimer = undefined;
  }
  const a = elOf(0);
  if (a && wrapHandler) {
    a.removeEventListener("timeupdate", wrapHandler);
  }
  wrapHandler = null;
}

/// 加载新路径：两个元素都指向同一地址，A 先播
function applyPath(path: string) {
  if (!path) return;
  const url = convertFileSrc(path);
  failed.value = false;
  errorMsg.value = "";
  crossEnabled = false;
  stopLoop();
  // currentSrc 必须**同步**记下，不能等 rAF 里再写：
  // 它现在是"是否需要换源"的判据（见 applyState），晚一帧写会让同一帧内
  // 到达的两条状态都判定为"路径变了"→ 视频被重载两次。
  currentSrc = path;
  // 先清空触发元素重建，再在下一帧回填地址
  src.value = "";
  requestAnimationFrame(() => {
    src.value = url;
    requestAnimationFrame(() => {
      resetElements();
      const a = elOf(0);
      // 暂停状态下换视频：只换源、不自动播放，保持冻结
      if (!paused.value) tryPlay(a);
    });
  });
}

/// 元数据就绪：此时才知道 duration，也才好决定能否交叉循环
function onLoadedMetadata() {
  const a = elOf(0);
  const b = elOf(1);
  if (!a) return;
  const d = a.duration;
  // duration 过短（< 1.5s）时交叉循环收益不明显，直接用回绕更稳
  crossEnabled = !!b && Number.isFinite(d) && d > 1.5;
  resetElements();
  startLoop();
  if (!paused.value) tryPlay(a);
}

// ---------- 状态收敛 ----------

/// 后端下发的完整状态。事件负载与 `get_video_wallpaper_state` 的返回值
/// **是同一个形状**（后端共用 `VideoWallpaperStateOut`），所以两条路径
/// 可以走同一个收敛函数，不会出现"某个字段只在其中一条路径生效"。
interface VideoState {
  enabled: boolean;
  path: string;
  monitors: number[];
  muted: boolean;
  paused: boolean;
}

/// 把静音 / 暂停应用到两个视频元素。
///
/// 静音走模板上的 `:muted` 绑定（改 ref 即生效，元素不会被重建）；
/// 暂停 / 恢复必须**显式**调 `pause()` / `play()` —— 这是命令式行为，绑定表达不了。
/// 恢复时只播前台元素：后台那个是交叉循环的预载位，让它也播只是白耗解码。
function applyPlayback() {
  if (paused.value) {
    elOf(0)?.pause();
    elOf(1)?.pause();
    return;
  }
  const cur = elOf(front);
  if (cur && cur.paused) tryPlay(cur);
}

/// 收敛一份状态。
///
/// **只有路径真的变了才换源**：静音/暂停也会推状态过来，若无条件重载，
/// 点一下静音桌面就会黑一下、视频还会从头开始 —— 这正是"一个事件承载全部状态"
/// 必须配的判据。
function applyState(st: VideoState) {
  muted.value = !!st.muted;
  paused.value = !!st.paused;
  // 已停止：`stop_video_wallpaper` 是直接关窗、不发事件，所以正常收不到这一支；
  // 但万一"停止"与"建窗"并发、窗口活了下来，冻结画面总好过继续播一个
  // 已经被取消的壁纸。
  if (!st.enabled) {
    elOf(0)?.pause();
    elOf(1)?.pause();
    return;
  }
  if (st.path && st.path !== currentSrc) {
    applyPath(st.path); // 内部按 paused 决定是否自动播放
    return;
  }
  applyPlayback();
}

function onVideoError(e: Event) {
  const v = e.target as HTMLVideoElement;
  // 只有前台元素报错才算失败，后台元素预载失败不影响前台播放
  if (v && v.style.opacity === "0") return;
  const code = v?.error?.code ?? -1;
  failed.value = true;
  errorMsg.value = `视频加载失败（错误码 ${code}）`;
}

function retryLoad() {
  if (!currentSrc) return;
  failed.value = false;
  errorMsg.value = "";
  applyPath(currentSrc);
}

async function stopWallpaper() {
  try {
    await invoke("stop_video_wallpaper");
  } catch {
    /* ignore */
  }
}

onMounted(async () => {
  hintTimer = window.setTimeout(() => (hintVisible.value = false), 3000);
  try {
    unlisten = await listen<VideoState>("video-wallpaper-state", (e) =>
      applyState(e.payload)
    );
  } catch (err) {
    // ⚠️ 这里**绝不能静默吞掉**。
    // 监听失败时，下面那次状态拉取只能兜住"窗口首次创建"这一种情况；
    // 之后主界面再换视频 / 静音 / 暂停，后端是"只 emit 事件、不重建窗口"
    //（为了不闪屏），于是完全收不到 —— 症状是"应用了一次动态壁纸，
    // 再应用其他的没有替换"或"点了暂停没反应"，而且界面上没有任何报错，极难定位。
    //
    // 最常见的失败原因是**权限**：capabilities 的 `windows` 字段必须能匹配到
    // `wallpaper-video-{index}` 这个 label。曾经写成精确的 "wallpaper-video"，
    // 而实际 label 是 "wallpaper-video-0/1/…"，一个都匹配不上 →
    // `core:event:allow-listen` 不生效 → listen() 直接 reject。
    // 正确写法是 glob：`"wallpaper-video-*"`。
    console.error(
      "[动态壁纸] 监听 video-wallpaper-state 失败，" +
        "之后换视频 / 静音 / 暂停都不会生效。" +
        "请检查 src-tauri/capabilities 的 windows 是否匹配 wallpaper-video-*",
      err
    );
  }
  // 挂载时主动拉一次兜底：事件可能早于本 listener 注册（窗口刚建、页面还在加载）
  try {
    const st = await invoke<VideoState>("get_video_wallpaper_state");
    if (st.enabled) applyState(st);
  } catch {
    /* ignore */
  }
});

onUnmounted(() => {
  if (hintTimer) window.clearTimeout(hintTimer);
  stopLoop();
  unlisten?.();
});
</script>

<template>
  <div class="video-wall" @dblclick="stopWallpaper">
    <template v-if="src && !failed">
      <!-- 双实例：A 为默认前台，B 为预载/交叉备用；两者地址相同。
           **刻意不写 `autoplay` 属性**：它在 `src` 变化时会自动开播，
           而"暂停"必须能跨换源保持（用户暂停着换下一段视频，不该自己动起来）。
           起播一律由 JS 显式 `play()`，且都判过 `paused`。
           静音用 `:muted` 绑定（Vue 会写 DOM 属性 muted），
           因此"取消静音"无需重建元素、也不会打断播放。 -->
      <video
        ref="videoA"
        class="layer"
        :src="src"
        :muted="muted"
        playsinline
        preload="auto"
        @loadedmetadata="onLoadedMetadata"
        @error="onVideoError"
      ></video>
      <video
        ref="videoB"
        class="layer"
        :src="src"
        :muted="muted"
        playsinline
        preload="auto"
        @error="onVideoError"
      ></video>
    </template>
    <div v-if="failed" class="video-error">
      {{ errorMsg }}
      <button @click="retryLoad">重试</button>
      <button @click="stopWallpaper">停止动态壁纸</button>
    </div>
    <div v-if="hintVisible" class="video-hint">双击停止动态壁纸</div>
  </div>
</template>

<style scoped>
.video-wall {
  position: fixed;
  inset: 0;
  overflow: hidden;
  background: #000;
}
.layer {
  position: absolute;
  inset: 0;
  display: block;
  width: 100%;
  height: 100%;
  object-fit: cover;
  will-change: opacity;
}
.video-hint {
  position: fixed;
  right: 16px;
  bottom: 16px;
  padding: 5px 12px;
  border-radius: 8px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.85);
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(4px);
  pointer-events: none;
  user-select: none;
}
.video-error {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  padding: 12px 16px;
  border-radius: 10px;
  font-size: 13px;
  color: #ff8f8f;
  background: rgba(20, 10, 10, 0.78);
  border: 1px solid rgba(255, 120, 120, 0.35);
  z-index: 10;
}
.video-error button {
  margin-left: 10px;
  padding: 3px 10px;
  border-radius: 6px;
  border: 1px solid rgba(127, 168, 255, 0.6);
  color: #7fa8ff;
  background: transparent;
  cursor: pointer;
}
</style>
