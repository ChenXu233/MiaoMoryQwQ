// 全局确认弹窗(自绘玻璃,替换系统原生 ask 对话框——2026-09-11 所有者裁定):
// useConfirm() 单例 promise 风格,与 @tauri-apps/plugin-dialog 的 ask 等价替换。
import { ref } from "vue";

export interface ConfirmOptions {
  title: string;
  message: string;
  okLabel?: string;
  cancelLabel?: string;
  /** 危险操作:确认按钮红色变体 */
  danger?: boolean;
}

const state = ref<(ConfirmOptions & { busy: boolean }) | null>(null);
let resolver: ((v: boolean) => void) | null = null;

export function useConfirm() {
  function confirm(opts: ConfirmOptions): Promise<boolean> {
    state.value = { ...opts, busy: false };
    return new Promise((resolve) => {
      resolver = resolve;
    });
  }
  function settle(v: boolean) {
    if (!resolver) {
      state.value = null;
      return;
    }
    const r = resolver;
    resolver = null;
    state.value = null;
    r(v);
  }
  return { state, confirm, settle };
}
