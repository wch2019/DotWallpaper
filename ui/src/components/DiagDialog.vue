<script setup lang="ts">
// 动态壁纸诊断弹窗
//
// 目的：用户遇到"设了动态壁纸但桌面没画面"时，**不用去命令行跑脚本**，
// 点一下就能看到结论，并且能把明细一键复制/保存发给开发者。
//
// 数据全部来自后端 `diag_video_wallpaper`（原生采集，只读）：
// 上半部分是人话结论，下半部分是可展开的原始明细。
//
// 为什么不用 toast 直接弹结论：结论可能有好几条、还要能复制明细，
// toast 一闪而过反而没法看。这里做成常驻弹窗，用户随时可以回头再看。
import { onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  CircleCheckBig,
  CircleX,
  Copy,
  Download,
  LoaderCircle,
  RefreshCw,
  TriangleAlert,
  X,
} from "lucide-vue-next";

type Level = "ok" | "warn" | "fail";

interface DiagItem {
  level: Level;
  text: string;
}
interface DiagReport {
  level: Level;
  summary: DiagItem[];
  detail: string;
}

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "update:show", v: boolean): void }>();

const loading = ref(false);
const report = ref<DiagReport | null>(null);
const error = ref("");
const copied = ref(false);
const savedPath = ref("");

const LEVEL_TEXT: Record<Level, string> = {
  ok: "一切正常",
  warn: "有需要注意的地方",
  fail: "发现问题",
};

/** 每条结论前面那个小图标 */
const LEVEL_ICON = {
  ok: CircleCheckBig,
  warn: TriangleAlert,
  fail: CircleX,
} as const;

const LEVEL_CLASS: Record<Level, string> = {
  ok: "text-ok",
  warn: "text-amber-300",
  fail: "text-danger",
};

/** 复制/保存用的完整文本：结论 + 空行 + 明细 */
function fullText(): string {
  const r = report.value;
  if (!r) return "";
  const head = r.summary.map((s) => `- ${s.text}`).join("\n");
  return `DotWallpaper 动态壁纸诊断（${new Date().toLocaleString()}）\n\n【结论】\n${head}\n\n【明细】\n${r.detail}`;
}

async function run() {
  loading.value = true;
  error.value = "";
  copied.value = false;
  savedPath.value = "";
  try {
    report.value = await invoke<DiagReport>("diag_video_wallpaper");
  } catch (e) {
    report.value = null;
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}

async function onCopy() {
  const text = fullText();
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1600);
  } catch {
    // 剪贴板 API 在极少数环境下不可用：退化成"选中临时文本框再复制"
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
      copied.value = true;
      window.setTimeout(() => (copied.value = false), 1600);
    } finally {
      document.body.removeChild(ta);
    }
  }
}

async function onSave() {
  const text = fullText();
  if (!text) return;
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}`;
  const path = await save({
    defaultPath: `DotWallpaper-诊断-${stamp}.txt`,
    filters: [{ name: "文本文件", extensions: ["txt"] }],
  });
  if (!path) return;
  try {
    await invoke("save_text_file", { path, content: text });
    savedPath.value = String(path);
  } catch (e) {
    error.value = String(e);
  }
}

function close() {
  emit("update:show", false);
}

// Esc 关闭。挂在 window 上而不是给卡片挂 @keydown.esc ——
// 卡片没有焦点，键盘事件根本到不了它身上（这是最常见的无效写法）。
function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape" && props.show) close();
}
onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));

// 每次打开都重新采一遍（用户可能刚改过设置）
watch(
  () => props.show,
  (v) => {
    if (v) void run();
  }
);
</script>

<template>
  <Teleport to="body">
    <Transition name="diag-fade">
      <div
        v-if="show"
        class="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-4"
        @click.self="close"
      >
        <div
          class="flex max-h-[82vh] w-[min(94vw,560px)] flex-col overflow-hidden rounded-lg border border-line-2 bg-panel shadow-2xl"
        >
          <!-- 头部 -->
          <div class="flex items-start gap-2 border-b border-line px-3 py-2.5">
            <div class="mt-0.5 shrink-0" :class="report ? LEVEL_CLASS[report.level] : 'text-accent'">
              <LoaderCircle v-if="loading" :size="16" class="animate-spin" />
              <Activity v-else :size="16" />
            </div>
            <div class="min-w-0 flex-1">
              <div class="text-[12.5px] font-medium text-tx">动态壁纸诊断</div>
              <div class="mt-0.5 text-[10.5px] text-faint">
                <template v-if="loading">正在检查桌面层与窗口状态…</template>
                <template v-else-if="error">诊断没能完成</template>
                <template v-else-if="report">{{ LEVEL_TEXT[report.level] }}</template>
                <template v-else>检查桌面层与窗口状态</template>
              </div>
            </div>
            <button
              class="shrink-0 cursor-pointer rounded p-1 text-faint transition-colors hover:bg-panel-2 hover:text-tx"
              title="关闭"
              @click="close"
            >
              <X :size="14" />
            </button>
          </div>

          <!-- 主体 -->
          <div class="min-h-0 flex-1 overflow-y-auto px-3 py-2.5">
            <!-- 检查中 -->
            <div v-if="loading" class="flex items-center gap-2 py-6 text-[11px] text-dim">
              <LoaderCircle :size="14" class="animate-spin" />
              正在读取窗口树与显示器信息…
            </div>

            <!-- 出错 -->
            <div v-else-if="error" class="py-4">
              <div class="flex items-start gap-1.5 text-[11px] leading-relaxed text-danger">
                <CircleX :size="13" class="mt-0.5 shrink-0" />
                <span>{{ error }}</span>
              </div>
              <p class="mt-2 text-[10.5px] leading-relaxed text-faint">
                可以先点「重新检测」再试一次；如果一直失败，把这段文字发给开发者。
              </p>
            </div>

            <!-- 结论 -->
            <template v-else-if="report">
              <ul class="flex flex-col gap-1.5">
                <li
                  v-for="(item, i) in report.summary"
                  :key="i"
                  class="flex items-start gap-1.5 text-[11px] leading-relaxed text-dim"
                >
                  <component
                    :is="LEVEL_ICON[item.level]"
                    :size="13"
                    class="mt-0.5 shrink-0"
                    :class="LEVEL_CLASS[item.level]"
                  />
                  <span class="min-w-0 flex-1">{{ item.text }}</span>
                </li>
              </ul>

              <!-- 明细：默认折叠。用户不需要看，开发者需要。 -->
              <details class="mt-2.5 rounded border border-line bg-base-2/60">
                <summary
                  class="cursor-pointer select-none px-2 py-1.5 text-[10.5px] text-faint transition-colors hover:text-dim"
                >
                  查看详细数据（复制 / 保存时一并包含）
                </summary>
                <pre
                  class="max-h-52 overflow-auto border-t border-line px-2 py-1.5 text-[10px] leading-[1.5] whitespace-pre text-dim"
                  >{{ report.detail }}</pre
                >
              </details>
            </template>
          </div>

          <!-- 底部 -->
          <div class="flex items-center gap-1.5 border-t border-line px-3 py-2">
            <button
              class="flex cursor-pointer items-center gap-1 rounded border border-line-2 px-1.5 py-1 text-[10.5px] text-dim transition-colors hover:border-accent/50 hover:text-tx"
              :disabled="loading"
              title="重新检测一次"
              @click="run"
            >
              <RefreshCw :size="11" class="shrink-0" :class="{ 'animate-spin': loading }" />
              重新检测
            </button>
            <button
              class="flex cursor-pointer items-center gap-1 rounded border border-line-2 px-1.5 py-1 text-[10.5px] text-dim transition-colors hover:border-accent/50 hover:text-tx disabled:cursor-default disabled:opacity-40"
              :disabled="!report"
              :title="copied ? '已复制到剪贴板' : '复制结论与明细'"
              @click="onCopy"
            >
              <Copy :size="11" class="shrink-0" />
              {{ copied ? "已复制" : "复制" }}
            </button>
            <button
              class="flex cursor-pointer items-center gap-1 rounded border border-line-2 px-1.5 py-1 text-[10.5px] text-dim transition-colors hover:border-accent/50 hover:text-tx disabled:cursor-default disabled:opacity-40"
              :disabled="!report"
              title="保存为 txt 文件"
              @click="onSave"
            >
              <Download :size="11" class="shrink-0" />
              保存
            </button>
            <span
              v-if="savedPath"
              class="min-w-0 flex-1 truncate text-[10px] text-ok"
              :title="savedPath"
            >
              已保存
            </span>
            <div v-else class="flex-1"></div>
            <button
              class="shrink-0 cursor-pointer rounded border border-line-2 px-2 py-1 text-[10.5px] text-dim transition-colors hover:border-line-2 hover:text-tx"
              @click="close"
            >
              关闭
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.diag-fade-enter-active,
.diag-fade-leave-active {
  transition: opacity 0.16s ease;
}
.diag-fade-enter-from,
.diag-fade-leave-to {
  opacity: 0;
}
</style>
