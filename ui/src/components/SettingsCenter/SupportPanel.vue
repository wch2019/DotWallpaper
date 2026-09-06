<script setup lang="ts">
// 交流打赏 — 加好友 & 打赏 双视图，切换卡片
import { ref } from "vue";
import { NIcon } from "naive-ui";
import { Check, Copy, HeartHandshake, MessageCircle, Wallet, UserPlus } from "lucide-vue-next";
import wxAvatar from "@/assets/readme/wechat-avatar.jpg";
import wxPay from "@/assets/readme/wechat-pay.jpg";
import alipay from "@/assets/readme/alipay-pay.jpg";
import { toast } from "@/lib/naive-host";

const wechatId = "XHDotcode";
const activeView = ref<"friend" | "reward">("reward");
const copied = ref(false);

async function copyWechat() {
  const text = wechatId;
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  }
  copied.value = true;
  toast("微信号已复制，去微信搜索添加吧", "success");
  window.setTimeout(() => (copied.value = false), 2000);
}

function switchView(view: "friend" | "reward") {
  activeView.value = view;
}
</script>

<template>
  <div class="panel-body">
    <div class="panel-header">
      <NIcon :component="HeartHandshake" :size="18" class="panel-icon text-accent" />
      <div class="panel-title">交流打赏</div>
      <div class="panel-desc">觉得好用可以支持一下作者</div>
    </div>

    <!-- 视图切换栏 -->
    <div class="view-tabs">
      <button
        class="view-tab"
        :class="{ active: activeView === 'reward' }"
        @click="switchView('reward')"
      >
        <NIcon :component="Wallet" :size="14" />
        <span>打赏</span>
      </button>
      <button
          class="view-tab"
          :class="{ active: activeView === 'friend' }"
          @click="switchView('friend')"
      >
        <NIcon :component="UserPlus" :size="14" />
        <span>加好友</span>
      </button>
    </div>

    <!-- ==================== 加好友视图 ==================== -->
    <Transition name="fadeSlide" mode="out-in">
      <div v-if="activeView === 'friend'" key="friend" class="view-content">
        <div class="friend-card">
          <div class="friend-topline">
            <span class="brand-dot wechat-dot"></span>
            <span class="brand-name">微信 · WeChat</span>
          </div>

          <div class="friend-body">
            <div class="friend-avatar">
              <img :src="wxAvatar" alt="微信头像" />
            </div>
            <div class="friend-info">
              <div class="friend-name">DotWallpaper 开发者</div>
              <div class="friend-desc">欢迎交流，一起探讨壁纸工具的使用与改进</div>

              <button class="copy-btn" :class="{ copied }" @click="copyWechat">
                <span class="copy-label">微信号</span>
                <span class="copy-value">{{ wechatId }}</span>
                <span class="copy-icon">
                  <NIcon v-if="copied" :component="Check" :size="14" />
                  <NIcon v-else :component="Copy" :size="14" />
                </span>
              </button>
              <div class="copy-tip" :class="{ copied }">
                {{ copied ? "已复制到剪贴板" : "点击复制微信号" }}
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- ==================== 打赏视图 ==================== -->
    <Transition name="fadeSlide" mode="out-in">
      <div v-if="activeView === 'reward'" key="reward" class="view-content">
        <div class="reward-row">
          <!-- 微信打赏 -->
          <div class="reward-card wechat-card">
            <div class="card-topline">
              <span class="brand-dot wechat-dot"></span>
              <span class="brand-name">微信 · WeChat</span>
            </div>
            <div class="reward-body">
              <div class="qr-frame">
                <img :src="wxPay" alt="微信打赏码" />
              </div>
              <div class="qr-label">
                <NIcon :component="MessageCircle" :size="12" />
                <span>微信扫一扫，请作者喝杯咖啡</span>
              </div>
            </div>
          </div>

          <!-- 支付宝打赏 -->
          <div class="reward-card alipay-card">
            <div class="card-topline">
              <span class="brand-dot alipay-dot"></span>
              <span class="brand-name">支付宝 · Alipay</span>
            </div>
            <div class="reward-body">
              <div class="qr-frame">
                <img :src="alipay" alt="支付宝打赏码" />
              </div>
              <div class="qr-label">
                <NIcon :component="Wallet" :size="12" />
                <span>支付宝扫一扫，感谢支持</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <div class="panel-foot">您的支持是持续更新的最大动力</div>
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

/* ========== 视图切换栏 ========== */
.view-tabs {
  display: flex;
  gap: 4px;
  margin-top: 0.75rem;
}
.view-tab {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  padding: 0.4rem 0.9rem;
  border: 1px solid var(--color-line);
  border-radius: 8px;
  background: transparent;
  color: var(--color-dim);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.18s ease;
}
.view-tab:hover {
  background: rgba(127, 168, 255, 0.08);
  color: var(--color-tx);
}
.view-tab.active {
  background: rgba(127, 168, 255, 0.14);
  border-color: rgba(127, 168, 255, 0.5);
  color: var(--color-accent);
  font-weight: 600;
}

/* ========== 视图内容过渡 ========== */
.fadeSlide-enter-active,
.fadeSlide-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.fadeSlide-enter-from {
  opacity: 0;
  transform: translateY(6px);
}
.fadeSlide-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}

.view-content {
  margin-top: 0.75rem;
}

/* ========== 品牌点 & 条 ========== */
.brand-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.wechat-dot {
  background: #43d19e;
  box-shadow: 0 0 8px rgba(67, 209, 158, 0.7);
}
.alipay-dot {
  background: #1677ff;
  box-shadow: 0 0 8px rgba(22, 119, 255, 0.7);
}
.brand-name {
  font-size: 12.5px;
  font-weight: 600;
  color: var(--color-tx);
}

/* ========== 加好友卡片 ========== */
.friend-card {
  border-radius: 14px;
  border: 1px solid var(--color-line);
  overflow: hidden;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
}
.friend-card:hover {
  border-color: rgba(127, 168, 255, 0.35);
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
}
.friend-topline {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.65rem 1rem;
  border-bottom: 1px solid var(--color-line);
  background: linear-gradient(160deg, rgba(67, 209, 158, 0.07), rgba(23, 30, 46, 0.9) 42%);
}
.friend-body {
  padding: 1.5rem 1.25rem;
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}
.friend-avatar {
  width: 72px;
  height: 72px;
  border-radius: 50%;
  overflow: hidden;
  border: 2px solid rgba(67, 209, 158, 0.45);
  box-shadow: 0 0 0 4px rgba(67, 209, 158, 0.1), 0 6px 18px rgba(0, 0, 0, 0.35);
}
.friend-avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.friend-name {
  margin-top: 0.75rem;
  font-size: 13.5px;
  font-weight: 600;
  color: var(--color-tx);
}
.friend-desc {
  margin-top: 0.3rem;
  font-size: 11.5px;
  color: var(--color-dim);
}

/* ========== 复制按钮 ========== */
.copy-btn {
  margin-top: 1rem;
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 1rem;
  border-radius: 999px;
  border: 1px solid rgba(127, 168, 255, 0.35);
  background: rgba(127, 168, 255, 0.08);
  cursor: pointer;
  transition: all 0.18s ease;
}
.copy-btn:hover {
  background: rgba(127, 168, 255, 0.16);
  border-color: rgba(127, 168, 255, 0.7);
  transform: translateY(-1px);
}
.copy-btn.copied {
  border-color: rgba(67, 209, 158, 0.7);
  background: rgba(67, 209, 158, 0.14);
}
.copy-label {
  font-size: 10px;
  color: var(--color-faint);
}
.copy-value {
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.3px;
  color: var(--color-accent);
  user-select: all;
}
.copy-icon {
  display: flex;
  color: var(--color-accent);
}
.copy-btn.copied .copy-icon {
  color: var(--color-ok);
}
.copy-tip {
  margin-top: 0.5rem;
  font-size: 10px;
  color: var(--color-faint);
  transition: color 0.2s ease;
}
.copy-tip.copied {
  color: var(--color-ok);
}

/* ========== 打赏视图 ========== */
.reward-row {
  display: flex;
  gap: 0.75rem;
}

.reward-card {
  flex: 1 1 0;
  border-radius: 14px;
  border: 1px solid var(--color-line);
  overflow: hidden;
  transition: border-color 0.2s ease, box-shadow 0.2s ease;
}
.reward-card:hover {
  border-color: rgba(127, 168, 255, 0.35);
  box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
}

.card-topline {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.65rem 1rem;
  border-bottom: 1px solid var(--color-line);
}
.wechat-card {
  background: linear-gradient(160deg, rgba(67, 209, 158, 0.07), rgba(23, 30, 46, 0.9) 42%);
}
.alipay-card {
  background: linear-gradient(160deg, rgba(22, 119, 255, 0.07), rgba(23, 30, 46, 0.9) 42%);
}

.reward-body {
  padding: 1rem 1rem 1.25rem;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
}

/* ========== 二维码 ========== */
.qr-frame {
  padding: 6px;
  border-radius: 12px;
  background: #ffffff;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
  transition: transform 0.2s ease;
}
.qr-frame:hover {
  transform: scale(1.03);
}
.qr-frame img {
  display: block;
  width: 200px;
  height: 200px;
  border-radius: 7px;
  object-fit: contain;
}
.qr-label {
  display: flex;
  align-items: center;
  gap: 0.35rem;
  font-size: 10.5px;
  color: var(--color-dim);
}

/* ========== 底部 ========== */
.panel-foot {
  margin-top: 0.8rem;
  text-align: center;
  font-size: 10.5px;
  color: var(--color-faint);
}
</style>
