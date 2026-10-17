<script setup lang="ts">
// 动态壁纸播放页（视频）
// 由后端创建的置底 WebviewWindow 以 ?view=video 加载本页面；
// 页面只渲染全屏循环视频，双击桌面空白处可停止动态壁纸。
//
// 关于循环连贯性（为什么不用 loop / 不用单元素回绕）：
//   1) HTML5 的 loop 属性在回到开头时会重新 seek 并解码，循环点几乎必然掉帧或闪黑；
//   2) 单元素"提前 seek 到 0"也只是把这次 seek 提前，仍会丢几帧 —— 表现为
//      "循环点画面一顿"；
//   3) 真正无感的方式是**双实例预载 + 淡出**：两个 <video> 上下叠放，下层始终
//      停在 0 帧且保持**不透明**（被上层完整盖住，看不见），上层接近结尾时下层
//      起播、同时把上层淡出。合成结果 = (1-α)·上层 + α·下层，**两层永远都有
//      画面，物理上不可能露出黑底**。
//   本文件采用方案 3，并在 WebView2 不支持交叉时回退到单元素回绕（方案 2）。
//
// 【交叉点闪屏的根因（CDP 探针实测，勿再猜）】
//   "每播放一个循环闪一次，有时候不闪" = **被盖住的下层被合成器遮挡裁剪了**。
//   Chromium 会把"被不透明层完全覆盖"的层裁掉不画，于是下层的视频帧根本没被
//   提交；等上层开始淡出、下层突然需要出画时，它还要 1~2 帧才被恢复提交 ——
//   这段空窗里合成结果退化成 `α·上层`（下面是空的/黑底），**整屏按 α 等比压暗**。
//   探针实测（用户真实视频，4×4 空间指纹）交叉点那一帧 16 格全部同比例掉到 ~0.90
//   （83.6 → 75.4），是"整体变暗"而不是"内容变了"。掉多少取决于下层几帧恢复：
//   有时只掉 2%（几乎看不出）→ 这就是"有时候不闪"。
//   修法见 `TOP_OPACITY`：**别让覆盖层恰好是 opacity: 1**。
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
/// 两个上下叠放、交替承担播放的视频元素
const videoA = ref<HTMLVideoElement | null>(null);
const videoB = ref<HTMLVideoElement | null>(null);

let unlisten: UnlistenFn | null = null;
let hintTimer: number | undefined;
/// 主循环定时器（交叉检测）
let loopTimer: number | undefined;
/// 交叉收尾定时器。
/// **必须可取消**：换源时若不清掉，它会在约 460ms 后去改一个已经换过角色的元素。
/// 目前换源会重建 `<video>`（`v-if`）使旧引用变成游离节点，所以实际不会出问题 ——
/// 但那是"靠副作用兜底"，不该依赖。换源路径显式取消它。
let crossTimer: number | undefined;
/// 是否正在交叉（上层淡出中）。挡住 `crossStep` 的重复触发 ——
/// 淡出期间 `front` **还没切换**，不挡的话每 25ms 就会再触发一次交叉。
let crossing = false;
/// 当前"可见层"（两层中在上面、正在播的那个）索引，0=A / 1=B
let front = 0;
/// 当前生效的视频地址（判断是否需要重置）
let currentSrc = "";
/// 单调递增的 z-index 计数器，见 `finishCross` 的注释。
///
/// **绝不能用固定的 1/2**：退场层仍保留着自己的 z-index，下一次交叉若把新的
/// 可见层也设成同一个值，二者 z-index 相等 → 绘制顺序退化成 DOM 顺序，
/// 而 DOM 里 B 永远在 A 之后 → "永远 B 压 A"：当 A 才是要升上来的那层时，
/// 画面方向就反了。每次取一个更大的值，才能保证"新可见层一定在最上层"。
/// 长期运行这个值会一直涨（10s 的片子一天约 8600），但 z-index 是 32 位整数、
/// 且只有两个元素 —— **不要为此加"归一化回收"逻辑**：归一化本身要在播放中改
/// z-index，为了一个永远不会溢出的计数器去动正在显示的层，得不偿失。
let zCounter = 2;
/// 是否已启用交叉循环（元数据就绪后确定）
let crossEnabled = false;

/// 交叉点提前量与淡出时长的**上限**（秒）。
///
/// 实际值按视频时长算（见 `onLoadedMetadata`）—— 短片子必须按比例缩小，
/// 否则 `lead` 会占掉整段视频的一大半。
///
/// 两个约束（改一个就要重算另一个）：
/// - **`lead` 必须明显大于 `fade`**：上层在淡出走完之前不能播到结尾，
///   否则它会冻在最后一帧、与新画面混在一起 → 看着像卡了一下。
/// - 也不能太大：下层是从 0 起播的，两层内容相差正好一个 `lead`，太大就成了"重影"。
const CROSS_LEAD_MAX = 0.5;
const CROSS_FADE_MAX = 0.4;
/// 交叉检测的轮询周期（毫秒）。
/// 越短，实际提前量越贴近 `lead`（抖动越小）；
/// 它和上面两个常量是绑定的 —— `lead - fade` 必须能吃掉一个轮询周期的抖动。
const CROSS_POLL_MS = 25;
/// 回退方案（交叉不可用）单元素提前回绕的提前量（秒）
const WRAP_LEAD_SECS = 0.28;
/// 预载层迟迟不就绪时，从"上层已播到结尾"起最多再等多久，超时就兜底回绕。
/// 只是防"永久冻帧"的安全网，正常路径永远走不到（预载层有一整圈的时间去就绪）。
const CROSS_STALL_RESCUE_MS = 800;
/// 上面那个安全网的计时起点（0 = 未在计时）
let stallSince = 0;

/// 本片实际使用的提前量 / 淡出时长（`onLoadedMetadata` 里按 duration 定）
let crossLead = CROSS_LEAD_MAX;
let crossFade = CROSS_FADE_MAX;

/// **上层（负责覆盖的那层）的 opacity。绝不能是 `1`。**
///
/// Chromium 的遮挡裁剪（occlusion culling）会把"被**不透明**层完全覆盖"的层
/// 直接跳过不画。上层写 `opacity: 1` 时它就是不透明层 → 下层被裁掉 → 下层的
/// 视频帧不被提交。等上层开始淡出、下层需要出画时，合成器要 1~2 帧才把它恢复，
/// 这段空窗里合成结果 = `α·上层`（下层贡献为 0）→ **整屏等比压暗一下**。
/// 探针实测：交叉点那一帧 16 格全部同比例掉到 ~0.90，掉多少取决于恢复快慢 ——
/// 恢复快就只掉 2%（看不出），于是表现为"**每循环闪一次，有时候不闪**"。
///
/// 只要**不是恰好 1**，这层就不算"不透明层"，下面的层就永远不会被裁掉。
/// 取 0.999（透出量 0.1%，肉眼不可见）：既要足够小于 1 躲开 `IsOpaque()`
/// 的精确比较，又要足够接近 1 让平时看不出来。
///
/// ⚠️ **不要"顺手"把它改成 1。** 那不是清洁化，是把闪屏放回来。
/// ⚠️ 也不要改成 0.99 之类更大的值 —— 没必要，透出量越大越可能被看出来。
const TOP_OPACITY = "0.999";

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

/// 复位到"初始叠放"状态：A 在上（可见、待播），B 在下（预载、停在 0 帧）。
///
/// **两层都必须是"看得见画面"的**（上层 `TOP_OPACITY`、下层 `1`）：
/// 下层被上层完整盖住、看不见，但它因此**始终参与合成**、视频帧始终有效 ——
/// 交叉时立刻就有画面可混。
///
/// 曾经的做法是"下层 `opacity: 0`，交叉时把它淡入"。**那是本 bug 的根源**：
/// 透明层里的视频帧不一定被合成器提交（可能被裁剪掉），淡入时它会先露出
/// 几帧空白，于是合成结果退化成 `(1-α)·旧层` —— **每个循环点整屏往黑里沉
/// 一下再回来**，也就是"每播放一个循环闪一次"。用 `requestVideoFrameCallback`
/// 等帧只能赌它什么时候回调，赌不中就闪 —— 所以"有时候不闪"。
///
/// ⚠️ 改完之后还剩**同一个洞的另一半**：上层写 `opacity: 1` 就成了"不透明层"，
/// 于是**下层被遮挡裁剪**、帧同样不被提交 —— 症状一模一样。详见 `TOP_OPACITY`。
/// 所以这里上层写的是 `TOP_OPACITY` 而**不是** `"1"`。
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
    // 上层 0.999（不能是 1，理由见 TOP_OPACITY）；下层 1（它在最底下，无人覆盖）
    el.style.opacity = i === 0 ? TOP_OPACITY : "1";
    el.style.zIndex = i === 0 ? "2" : "1";
  }
  front = 0;
  zCounter = 2;
  crossing = false;
  stallSince = 0;
}

/// 双实例交叉循环：上层接近结尾时，下层从 0 起播、同时把上层淡出。
///
/// **核心是"上层淡出、下层始终有画面"**（改动前请先读注释）：
/// 下层在交叉前就已经停在 0 帧并显示着它，两层都参与合成，所以
///     结果 = (1-α)·上层 + α·下层
/// 是真正的线性交叉：亮度恒定、无色偏、**任何时刻都不会露出黑底**。
///
/// 这个洞**被堵了两次**，两次症状完全一样（每循环闪一次 / 有时候不闪），
/// 但机理是同一个洞的两半 —— **"需要出画的那层，帧没被提交"**：
/// - 前半：下层写 `opacity: 0` → 透明层被跳过不画 → 帧没提交。
///   修法：下层写 `1`。
/// - 后半：上层写 `opacity: 1` → 它成了不透明层 → **下层被遮挡裁剪** → 帧没提交。
///   修法：上层写 `TOP_OPACITY`（0.999）。探针实测过，这才是"有时候不闪"的
///   那个时有时无的来源：掉多少取决于合成器几帧恢复，恢复快就只掉 2%。
///
/// **另一个坑（CDP 探针实测）**：交叉点上**不要**再 seek 一次下层。
/// 把 `currentTime` 设成"与当前相同"的值，Chrome 仍会执行一次真实 seek：
/// `readyState` 立刻掉到 1、`seeking` 变 true，紧随其后的 `play()` 要等 seek
/// 完成才真正开始推进 —— 淡出的头 100ms（片子越大越久）新层冻在第 0 帧不动，
/// 观感是"**画面卡一下 / 跳一下**"。复位只在 `finishCross` 里做，
/// 那里离下次交叉还有一整圈的时间，seek 是异步完成的、完全不影响画面。
function crossStep() {
  // 暂停时不推进循环：此时 currentTime 本就不动，但交叉淡出的定时器与
  // `front` 指针不该在冻结期间被改动，显式挡掉最稳妥。
  if (paused.value) return;
  // 上一次交叉还在淡出：`front` 尚未切换，不能重复触发
  if (crossing) return;
  const cur = elOf(front); // 可见层（在上，正在播）
  const next = elOf(1 - front); // 预载层（在下，停在 0 帧）
  if (!cur || !next) return;

  const d = cur.duration;
  if (!Number.isFinite(d) || d <= 0) return;

  // 还有足够剩余时间就什么都不做
  if (cur.currentTime < d - crossLead) return;

  // 预载层还没拿到数据 → 本轮不交叉，下一轮 25ms 后重试（条件恢复后自愈）。
  //
  // **不要在这里立刻退化调 `wrapStep()`**：那会对**正在显示的那层**做 seek，
  // 实测就是一次瞬时的内容跳变（整屏亮度 10ms 内从 239 跳到 16）——
  // 正是要修的症状。宁可让上层冻在末帧等下一轮，也不要制造一次跳变。
  //
  // 但"等"必须**有上限**：预载层整个循环都没就绪时（文件被删/解码失败），
  // 上层会播到结尾停住 —— 画面永久冻在最后一帧，比顿一下严重得多。
  // 所以只在上层**真的已经播到结尾**之后才开始计时，超时就兜底回绕一次。
  if (next.readyState < 2 /* HAVE_CURRENT_DATA */) {
    if (cur.currentTime >= d - 0.02) {
      if (stallSince === 0) stallSince = performance.now();
      else if (performance.now() - stallSince > CROSS_STALL_RESCUE_MS) {
        stallSince = 0;
        wrapStep();
      }
    }
    return;
  }
  stallSince = 0;

  // ⚠️ **不要在交叉点 seek 下层。**
  //
  // 下层在 `finishCross` 里就已经被复位到 0 帧，那之后它有一整圈的时间去完成
  // 这次 seek —— 到这里 `currentTime` 已经是 0 了。而把 `currentTime` 设成
  // **与当前相同**的值，Chrome 仍然会执行一次**真实 seek**：
  // 实测（CDP 探针）设之前 `readyState=4 / seeking=false`，设之后立刻变成
  // `readyState=1 / seeking=true`。于是紧随其后的 `play()` 必须等 seek 完成
  // 才真正开始推进 —— 淡出的头 100ms（片子越大越久）里新层**冻在第 0 帧不动**，
  // 观感就是"**画面卡一下 / 跳一下**"。
  // 这个 seek 是纯粹的自伤：它没有换来任何正确性，只把一次异步操作搬到了
  // 最关键的那 400ms 上。只在真的偏了才纠正（正常情况不会走到）。
  if (next.currentTime > 0.02) {
    try {
      next.currentTime = 0;
    } catch {
      /* 忽略：部分编码不支持精确 seek，仍可继续 */
    }
  }
  tryPlay(next);

  // ② 上层淡出，露出下面那层（`opacity: 1`，且因为上层只有 `TOP_OPACITY`
  //    而**没有被遮挡裁剪**，帧一直是提交着的）。
  //    合成结果 = (1-α)·上层 + α·下层 —— 线性、亮度恒定、不会往黑里沉。
  crossing = true;
  const fadeMs = Math.round(crossFade * 1000);
  cur.style.transition = `opacity ${fadeMs}ms linear`;
  cur.style.opacity = "0";

  // ③ 收尾：把角色对调（退场层复位成"下层预载"）。
  //    这个定时器**提前或延后都无害**：退场层此刻已经被夹在**不透明的**下层
  //    下面，它是什么透明度都看不见 —— 这是本方案比"淡入 + 固定延时硬切"稳的地方。
  if (crossTimer !== undefined) window.clearTimeout(crossTimer);
  crossTimer = window.setTimeout(() => {
    crossTimer = undefined;
    finishCross(cur, next);
  }, fadeMs + 60);
}

/// 交叉收尾：把刚退场的层复位成"下层预载"，并把角色对调。
function finishCross(cur: HTMLVideoElement, next: HTMLVideoElement) {
  if (!crossing) return;
  crossing = false;
  // **这里是全流程中唯一一次 seek，而且是刻意的。**
  // 退场层要退回 0 帧，好在下一次交叉时充当预载层；它现在被夹在上层（`TOP_OPACITY`）
  // 下面（不可见），而且距离下次交叉还有一整圈（≈ 视频时长）——
  // seek 有充足的时间异步完成。实测下一轮交叉时它确实已经是
  // `currentTime=0 / readyState=4 / seeking=false`。
  // 交叉点上**不要**再补一次 seek（见 `crossStep` 的注释：那是自伤）。
  try {
    cur.pause();
    cur.currentTime = 0;
  } catch {
    /* ignore */
  }
  // 预载层升到最上，成为新的可见层。此刻 `cur` 已被淡到 0，
  // 所以这次 z 变化在视觉上不可见（这正是铁律 ④ 允许"提前/延后都无害"的原因）。
  next.style.transition = "none";
  next.style.zIndex = String(++zCounter);
  // 新上层要落到 `TOP_OPACITY`（**不是 1**）—— 它马上要盖住下面那层，
  // 保持 1 会把下面那层遮挡裁剪掉，下一次交叉就又闪（见 `TOP_OPACITY`）。
  next.style.opacity = TOP_OPACITY;
  // 退场层复位成"下层预载"：先清过渡再恢复不透明，否则它会花 fadeMs
  // 再淡回来（虽然被盖住看不见，但会给下一次交叉留下一个"正在过渡"的状态）。
  // 它在 `TOP_OPACITY` 的 `next` 下面，这一步同样不可见。
  cur.style.transition = "none";
  cur.style.opacity = "1";
  front = 1 - front;
}

/// 回退方案：单元素提前回绕（交叉循环不可用时）
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
  crossing = false;
  stallSince = 0;
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
  // 交叉循环的几何约束是 `d - lead > fade`（上层在淡出走完之前不会播到结尾），
  // 满足它的最短片子约 0.9s，取 1.0s 留一点余量。
  //
  // **门槛不要随手调回 1.5**：低于门槛会退化成单元素回绕，而那是一次
  // **瞬时的内容跳变** —— 实测整屏亮度在 10ms 内从 239 直跳到 16，
  // 正是用户报的"画面跳一下/卡一下"。交叉循环只要能跑就跑。
  crossEnabled = !!b && Number.isFinite(d) && d > 1.0;
  if (crossEnabled) {
    // 短片子按比例缩小，否则 0.5s 的提前量会占掉整段视频的一大半。
    crossLead = Math.min(CROSS_LEAD_MAX, d * 0.35);
    crossFade = Math.min(CROSS_FADE_MAX, crossLead * 0.8);
  } else {
    crossLead = CROSS_LEAD_MAX;
    crossFade = CROSS_FADE_MAX;
  }
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
  /** 用户手动暂停（播放页不直接用它，仅保留字段含义） */
  paused: boolean;
  /** 有效暂停 = 手动 || 环境自动暂停（全屏 / 电池 / 锁屏）。**播放页只认这一位** */
  effective_paused: boolean;
}

/// 把静音 / 暂停应用到两个视频元素。
///
/// 静音走模板上的 `:muted` 绑定（改 ref 即生效，元素不会被重建）；
/// 暂停 / 恢复必须**显式**调 `pause()` / `play()` —— 这是命令式行为，绑定表达不了。
/// 恢复时只播可见层：下层是交叉循环的预载位，让它也播只是白耗解码。
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
  // **用 effective_paused 而不是 paused**：后者只表示用户手动暂停，
  // 环境自动暂停（全屏游戏 / 电池 / 锁屏）必须同样冻结画面。
  // 播放页因此不需要知道暂停是谁触发的 —— 后端已经合成好这一位。
  paused.value = !!st.effective_paused;
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
  // 只有可见层报错才算失败：下层是预载位，它出错不影响当前画面
  //（真到交叉时才会暴露，那时它已成为可见层）。
  if (v === elOf(1 - front)) return;
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
  // **页面根背景必须是黑的。**
  // `styles/main.css` 里 `body { background: transparent }`，而 WebView2 的
  // **默认窗口底色是白色** —— 画布透明时露出来的就是那层白。视频层任何一次
  // 重排/重建合成表面（循环点正好是最频繁的时刻）只要有一帧没铺上，
  // 露出来的就是**整屏白闪**，比黑闪刺眼得多。
  // 这里在播放页自己把根背景刷黑（只在本窗口生效，主界面不受影响）。
  document.documentElement.style.background = "#000";
  document.body.style.background = "#000";

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
      <!-- 两层上下叠放，**都不透明**（见 resetElements 的注释）：
           A 在上为可见层，B 在下为预载层（停在 0 帧，被 A 完整盖住）。
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
  /* 两层都常驻合成层（`will-change` 只保证"有独立层"，**不保证不被遮挡裁剪**）：
     被盖住的下层必须持续参与合成，否则交叉时它的视频帧还没被提交 → 闪。
     保证这一点的真正手段是上层用 `TOP_OPACITY`（0.999）而不是 1 —— 见其注释。 */
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
