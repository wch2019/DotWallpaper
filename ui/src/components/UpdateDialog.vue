<script setup lang="ts">
// UpdateDialog - 通用更新弹窗（TitleBar 更新按钮与关于页检查更新共用）
// 状态全部来自 updater store，展示分支：
//   checking → 检查中 / release → 发现新版本 / downloading → 下载进度
//   noUpdate → 已是最新 / checkFailed → 检查失败
import { computed } from "vue";
import {
  Rocket,
  X,
  LoaderCircle,
  ArrowRight,
  CircleCheckBig,
  TriangleAlert,
  CalendarDays,
  RefreshCw,
} from "lucide-vue-next";
import { useUpdaterStore } from "@/stores/updater";

const store = useUpdaterStore();

type ViewKey = "checking" | "release" | "downloading" | "noUpdate" | "checkFailed" | "idle";

const view = computed<ViewKey>(() => {
  if (!store.dialogVisible) return "idle";
  if (store.checking) return "checking";
  if (store.downloading) return "downloading";
  if (store.hasUpdate) return "release";
  if (store.checked && store.error) return "checkFailed";
  if (store.checked) return "noUpdate";
  return "checking";
});

const isCentered = computed(() =>
  ["checking", "noUpdate", "checkFailed"].includes(view.value)
);

/** release 视图下存在错误（即下载失败） */
const releaseFailed = computed(() => view.value === "release" && !!store.error);

const headTitle = computed(() => {
  switch (view.value) {
    case "checking":
      return "正在检查更新";
    case "release":
      return `发现新版本 v${store.latestVersion}`;
    case "downloading":
      return `正在下载 v${store.latestVersion}`;
    case "noUpdate":
      return "已是最新版本";
    case "checkFailed":
      return "检查更新失败";
    default:
      return "";
  }
});

const headDesc = computed(() => {
  switch (view.value) {
    case "checking":
      return "正在与更新服务器通信，请稍候…";
    case "downloading":
      return `正在下载更新包（${formatBytes(store.downloaded)} / ${formatBytes(store.total)}）`;
    case "noUpdate":
      return "当前使用的就是最新版本，无需更新。";
    case "checkFailed":
      return errText(store.error);
    default:
      return "";
  }
});

/** 弹窗底部是否展示按钮区 */
const showFooter = computed(() =>
  ["release", "noUpdate", "checkFailed"].includes(view.value)
);

function onMask() {
  // 下载中禁止关闭（安装流程即将接管）
  if (view.value === "downloading") return;
  store.closeUpdateDialog();
}

async function handleCheckAgain() {
  const result = await store.checkUpdate();
  if (result) store.openUpdateDialog();
}

function handleInstall() {
  store.installUpdate();
}

function handleConfirmClose() {
  store.closeUpdateDialog();
}

/* ---------------- 说明文本轻量渲染 ---------------- */

type NoteLine = { kind: "h" | "li" | "p" | "blank"; text: string };
type NoteBlock = { kind: "code"; text: string } | { kind: "lines"; lines: NoteLine[] };

/** 去除常见 Markdown 装饰符，保留纯文本语义 */
function inlineClean(text: string): string {
  return text
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/\*\*/g, "")
    .replace(/~~([^~]+)~~/g, "$1");
}

function renderNotes(body: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  body.split("```").forEach((part, i) => {
    if (i % 2 === 1) {
      // 代码块
      const code = part.replace(/^\n+|\n+$/g, "");
      if (code) blocks.push({ kind: "code", text: code });
      return;
    }
    const lines: NoteLine[] = [];
    for (const raw of part.split(/\r?\n/)) {
      const t = raw.trim();
      if (!t) {
        lines.push({ kind: "blank", text: "" });
        continue;
      }
      if (/^#{1,6}\s+/.test(t)) {
        lines.push({ kind: "h", text: inlineClean(t.replace(/^#{1,6}\s*/, "")) });
      } else if (/^[-*]\s+/.test(t)) {
        lines.push({ kind: "li", text: inlineClean(t.replace(/^[-*]\s+/, "")) });
      } else if (/^\d+[.、)]\s+/.test(t)) {
        lines.push({ kind: "li", text: inlineClean(t.replace(/^\d+[.、)]\s+/, "")) });
      } else {
        lines.push({ kind: "p", text: inlineClean(t) });
      }
    }
    if (lines.length) blocks.push({ kind: "lines", lines });
  });
  return blocks;
}

const noteBlocks = computed<NoteBlock[]>(() => {
  if (!store.updateNotes) return [];
  return renderNotes(store.updateNotes);
});

const updateDateText = computed(() => {
  if (!store.updateDate) return "";
  const d = new Date(store.updateDate);
  if (Number.isNaN(d.getTime())) return store.updateDate;
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
});

function formatBytes(n: number): string {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 || v >= 100 ? 0 : 1)} ${units[i]}`;
}

function errText(e: unknown): string {
  if (!e) return "未知错误";
  const raw = (e as { message?: unknown })?.message ?? e;
  if (typeof raw === "string") return raw;
  return String(raw);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="upd-fade">
      <div
        v-if="store.dialogVisible"
        class="upd-overlay"
        @click.self="onMask"
        @keydown.esc="onMask"
      >
        <div class="upd-card">
          <!-- Header -->
          <div class="upd-head">
            <div class="upd-head-icon" :class="`view-${view}`">
              <LoaderCircle v-if="view === 'checking'" :size="17" class="spin" />
              <Rocket v-else-if="view === 'release' || view === 'downloading'" :size="17" />
              <CircleCheckBig v-else-if="view === 'noUpdate'" :size="17" />
              <TriangleAlert v-else :size="17" />
            </div>
            <div class="upd-head-text">
              <div class="upd-title">{{ headTitle }}</div>
              <div class="upd-desc" v-if="headDesc">{{ headDesc }}</div>
            </div>
            <button
              class="upd-close"
              :disabled="view === 'downloading'"
              title="关闭"
              @click="onMask"
            >
              <X :size="15" />
            </button>
          </div>

          <!-- Body -->
          <div class="upd-body" :class="{ center: isCentered }">
            <!-- 检查中 -->
            <div v-if="view === 'checking'" class="center-wrap">
              <LoaderCircle :size="30" class="spin accent" />
              <p class="center-text">正在检查更新…</p>
            </div>

            <!-- 已是最新 -->
            <div v-else-if="view === 'noUpdate'" class="center-wrap">
              <CircleCheckBig :size="34" class="ok" />
              <p class="center-text">当前已是最新版本</p>
              <p class="center-sub">启动后会定期自动检查，有新版本将第一时间提醒你</p>
            </div>

            <!-- 检查失败 -->
            <div v-else-if="view === 'checkFailed'" class="center-wrap">
              <TriangleAlert :size="34" class="warn" />
              <p class="center-text">{{ headDesc }}</p>
              <p class="center-sub">请检查网络连接后重试</p>
            </div>

            <!-- 发现新版本 / 下载中 -->
            <template v-else-if="view === 'release' || view === 'downloading'">
              <!-- 下载失败提示 -->
              <div v-if="releaseFailed" class="err-banner">
                <TriangleAlert :size="15" />
                <span>更新包下载失败：{{ errText(store.error) }}</span>
              </div>

              <div class="ver-line">
                <span class="ver-chip old">v{{ store.currentVersion || "当前" }}</span>
                <ArrowRight :size="15" class="ver-arrow" />
                <span class="ver-chip new">v{{ store.latestVersion }}</span>
              </div>

              <div v-if="updateDateText" class="meta-line">
                <CalendarDays :size="12" />
                <span>发布于 {{ updateDateText }}</span>
              </div>

              <!-- 下载进度 -->
              <div v-if="view === 'downloading'" class="dl-area">
                <div class="dl-bar">
                  <div class="dl-bar-fill" :style="{ width: store.progress + '%' }"></div>
                </div>
                <div class="dl-meta">
                  <span>{{ Math.round(store.progress) }}%</span>
                  <span class="dim-text">{{ formatBytes(store.downloaded) }} / {{ formatBytes(store.total) }}</span>
                </div>
                <p class="dl-hint">下载完成后将自动安装并重启应用</p>
              </div>

              <!-- 更新说明 -->
              <div v-else class="notes">
                <div v-if="noteBlocks.length === 0" class="notes-empty">本次更新未提供详细说明。</div>
                <template v-for="(block, bi) in noteBlocks" :key="bi">
                  <pre v-if="block.kind === 'code'" class="note-code">{{ block.text }}</pre>
                  <div v-else class="note-lines">
                    <template v-for="(line, li) in block.lines" :key="li">
                      <p v-if="line.kind === 'blank'" class="note-blank"></p>
                      <p v-else-if="line.kind === 'h'" class="note-h">{{ line.text }}</p>
                      <p v-else-if="line.kind === 'li'" class="note-li">• {{ line.text }}</p>
                      <p v-else class="note-p">{{ line.text }}</p>
                    </template>
                  </div>
                </template>
              </div>
            </template>
          </div>

          <!-- Footer -->
          <div v-if="showFooter" class="upd-foot">
            <template v-if="view === 'release'">
              <button v-if="!releaseFailed" class="btn ghost" @click="store.closeUpdateDialog">
                取消更新
              </button>
              <button
                v-else
                class="btn ghost"
                @click="store.closeUpdateDialog"
              >
                稍后再说
              </button>
              <button v-if="releaseFailed" class="btn primary" @click="handleInstall">
                <RefreshCw :size="14" />
                重试下载
              </button>
              <button v-else class="btn primary" @click="handleInstall">
                <Rocket :size="14" />
                立即更新
              </button>
            </template>

            <template v-else-if="view === 'noUpdate'">
              <button class="btn primary" @click="handleConfirmClose">知道了</button>
            </template>

            <template v-else-if="view === 'checkFailed'">
              <button class="btn ghost" @click="handleConfirmClose">取消</button>
              <button class="btn primary" @click="handleCheckAgain">
                <RefreshCw :size="14" />
                重新检查
              </button>
            </template>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.upd-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(8, 10, 18, 0.55);
  backdrop-filter: blur(8px);
}

.upd-card {
  width: min(500px, 100%);
  max-height: min(640px, 88vh);
  display: flex;
  flex-direction: column;
  padding: 20px;
  border: 1px solid rgba(255, 255, 255, 0.09);
  border-radius: 16px;
  background: linear-gradient(165deg, rgba(30, 35, 52, 0.97), rgba(18, 22, 36, 0.99));
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.45);
}

/* ---------- Header ---------- */
.upd-head {
  display: flex;
  align-items: center;
  gap: 12px;
}

.upd-head-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  border-radius: 10px;
  background: rgba(127, 168, 255, 0.12);
  color: var(--color-accent, #7fa8ff);
}

.upd-head-icon.view-noUpdate {
  background: rgba(72, 199, 142, 0.14);
  color: #48c78e;
}

.upd-head-icon.view-checkFailed {
  background: rgba(240, 173, 78, 0.14);
  color: #f0ad4e;
}

.upd-head-text {
  flex: 1;
  min-width: 0;
}

.upd-title {
  font-size: 14.5px;
  font-weight: 600;
  color: var(--color-tx, #e8ecf6);
  line-height: 1.35;
}

.upd-desc {
  margin-top: 2px;
  font-size: 11.5px;
  color: rgba(160, 170, 195, 0.9);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.upd-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  flex-shrink: 0;
  border-radius: 7px;
  color: rgba(160, 170, 195, 0.9);
  background: transparent;
  cursor: pointer;
  transition: all 0.16s ease;
}

.upd-close:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--color-tx, #e8ecf6);
}

.upd-close:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ---------- Body ---------- */
.upd-body {
  margin-top: 16px;
  overflow-y: auto;
  scrollbar-width: thin;
  scrollbar-color: rgba(255, 255, 255, 0.15) transparent;
}

.upd-body::-webkit-scrollbar {
  width: 6px;
}

.upd-body::-webkit-scrollbar-thumb {
  border-radius: 3px;
  background: rgba(255, 255, 255, 0.15);
}

.upd-body.center {
  display: flex;
  align-items: center;
  justify-content: center;
}

.center-wrap {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 26px 0 20px;
  text-align: center;
}

.center-wrap .accent {
  color: var(--color-accent, #7fa8ff);
}

.center-wrap .ok {
  color: #48c78e;
}

.center-wrap .warn {
  color: #f0ad4e;
}

.center-text {
  margin-top: 14px;
  font-size: 13px;
  color: var(--color-tx, #e8ecf6);
  word-break: break-all;
}

.center-sub {
  margin-top: 6px;
  font-size: 11.5px;
  color: rgba(160, 170, 195, 0.85);
}

/* 下载失败横幅 */
.err-banner {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  margin-bottom: 14px;
  border-radius: 9px;
  background: rgba(240, 173, 78, 0.1);
  border: 1px solid rgba(240, 173, 78, 0.28);
  color: #f0ad4e;
  font-size: 12px;
}

/* 版本行 */
.ver-line {
  display: flex;
  align-items: center;
  gap: 10px;
}

.ver-chip {
  padding: 4px 12px;
  border-radius: 999px;
  font-size: 13px;
  font-weight: 600;
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
}

.ver-chip.old {
  background: rgba(255, 255, 255, 0.06);
  color: rgba(160, 170, 195, 0.9);
  text-decoration: line-through;
  text-decoration-color: rgba(255, 255, 255, 0.25);
}

.ver-chip.new {
  background: linear-gradient(135deg, rgba(127, 168, 255, 0.28), rgba(99, 148, 255, 0.14));
  color: var(--color-accent, #7fa8ff);
  box-shadow: inset 0 0 0 1px rgba(127, 168, 255, 0.35);
}

.ver-arrow {
  color: rgba(160, 170, 195, 0.6);
}

.meta-line {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 10px;
  color: rgba(160, 170, 195, 0.75);
  font-size: 11.5px;
}

/* 下载进度 */
.dl-area {
  margin-top: 16px;
}

.dl-bar {
  height: 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}

.dl-bar-fill {
  height: 100%;
  border-radius: 999px;
  background: linear-gradient(90deg, #5f8cff, var(--color-accent, #7fa8ff));
  transition: width 0.2s ease;
}

.dl-meta {
  display: flex;
  justify-content: space-between;
  margin-top: 8px;
  font-size: 11.5px;
  color: var(--color-tx, #e8ecf6);
}

.dim-text {
  color: rgba(160, 170, 195, 0.75);
}

.dl-hint {
  margin-top: 8px;
  font-size: 11px;
  color: rgba(160, 170, 195, 0.7);
  text-align: center;
}

/* 更新说明 */
.notes {
  margin-top: 14px;
  padding-top: 13px;
  border-top: 1px dashed rgba(255, 255, 255, 0.08);
}

.notes-empty {
  padding: 6px 0;
  font-size: 12px;
  color: rgba(160, 170, 195, 0.75);
}

.note-h {
  margin: 8px 0 4px;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--color-tx, #e8ecf6);
}

.note-li {
  margin: 3px 0;
  padding-left: 4px;
  font-size: 12px;
  line-height: 1.7;
  color: rgba(200, 208, 228, 0.95);
}

.note-p {
  margin: 3px 0;
  font-size: 12px;
  line-height: 1.7;
  color: rgba(200, 208, 228, 0.95);
}

.note-blank {
  height: 6px;
}

.note-code {
  margin: 8px 0;
  padding: 10px 12px;
  border-left: 3px solid var(--color-accent, #7fa8ff);
  border-radius: 0 8px 8px 0;
  background: rgba(8, 10, 18, 0.5);
  font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
  font-size: 11.5px;
  line-height: 1.6;
  color: #9fc3ff;
  white-space: pre-wrap;
  word-break: break-all;
}

/* ---------- Footer ---------- */
.upd-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 18px;
  padding-top: 14px;
  border-top: 1px solid rgba(255, 255, 255, 0.07);
}

.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 16px;
  border-radius: 9px;
  font-size: 12.5px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.16s ease;
}

.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: rgba(200, 208, 228, 0.9);
  border: 1px solid rgba(255, 255, 255, 0.09);
}

.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
  color: var(--color-tx, #e8ecf6);
}

.btn.primary {
  background: linear-gradient(135deg, #5f8cff, #4f7dff);
  color: #fff;
  border: 1px solid rgba(255, 255, 255, 0.14);
  box-shadow: 0 4px 14px rgba(79, 125, 255, 0.3);
}

.btn.primary:hover {
  filter: brightness(1.08);
}

.btn.primary:active {
  transform: translateY(1px);
}

/* ---------- 动画 ---------- */
.upd-fade-enter-active,
.upd-fade-leave-active {
  transition: opacity 0.2s ease;
}

.upd-fade-enter-active .upd-card,
.upd-fade-leave-active .upd-card {
  transition: transform 0.2s ease, opacity 0.2s ease;
}

.upd-fade-enter-from,
.upd-fade-leave-to {
  opacity: 0;
}

.upd-fade-enter-from .upd-card,
.upd-fade-leave-to .upd-card {
  opacity: 0;
  transform: translateY(10px) scale(0.97);
}

.spin {
  animation: upd-spin 1s linear infinite;
}

@keyframes upd-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
