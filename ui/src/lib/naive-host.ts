// Naive UI 全局宿主：将 useMessage / useDialog 实例注入到非组件上下文
import type { DialogApi, MessageApi } from "naive-ui";

export const naiveHost: {
  message: MessageApi | null;
  dialog: DialogApi | null;
} = {
  message: null,
  dialog: null,
};

export function bindNaiveHost(message: MessageApi, dialog: DialogApi) {
  naiveHost.message = message;
  naiveHost.dialog = dialog;
}

/** 轻提示（非组件上下文可用） */
export function toast(message: string, kind: "default" | "success" | "error" | "warning" = "default") {
  if (!naiveHost.message) return;
  if (kind === "default") naiveHost.message.create(message, { duration: 2600 });
  else if (kind === "success") naiveHost.message.success(message, { duration: 2600 });
  else if (kind === "error") naiveHost.message.error(message, { duration: 3200 });
  else naiveHost.message.warning(message, { duration: 3200 });
}

/**
 * 持续显示的加载提示，返回销毁函数 —— 供耗时操作前后配对调用：
 *   const done = toastLoading("正在导入…"); try { … } finally { done(); }
 * `duration: 0` = 不自动消失（必须显式销毁）。
 */
export function toastLoading(message: string): () => void {
  if (!naiveHost.message) return () => {};
  const inst = naiveHost.message.loading(message, { duration: 0 });
  return () => inst.destroy();
}

/** 危险操作确认（返回用户是否确认） */
export function confirmDanger(options: {
  title: string;
  content: string;
  positiveText?: string;
}): Promise<boolean> {
  return new Promise((resolve) => {
    if (!naiveHost.dialog) {
      resolve(false);
      return;
    }
    naiveHost.dialog.warning({
      title: options.title,
      content: options.content,
      positiveText: options.positiveText ?? "确认",
      negativeText: "取消",
      onPositiveClick: () => resolve(true),
      onNegativeClick: () => resolve(false),
      onClose: () => resolve(false),
      onMaskClick: () => resolve(false),
    });
  });
}
