// 全局右键菜单(2026-09-11 所有者裁定:替换 webview 原生右键,全量自定义):
// openFor 定位并显示动作列表;拦截逻辑在 App.vue 的 contextmenu 捕获阶段。
import { ref } from "vue";

export interface ContextItem {
  label: string;
  danger?: boolean;
  action: () => void;
}

const pos = ref<{ x: number; y: number } | null>(null);
const items = ref<ContextItem[]>([]);
let cleanup: (() => void) | null = null;

export function useContextMenu() {
  /** 在鼠标位置显示菜单;返回前先注册一次性全局关闭器 */
  function openFor(e: MouseEvent, list: ContextItem[]) {
    close();
    items.value = list;
    // 视口边缘收拢,防止菜单溢出
    const x = Math.min(e.clientX, window.innerWidth - 180);
    const y = Math.min(e.clientY, window.innerHeight - list.length * 34 - 16);
    pos.value = { x, y };
    cleanup = () => close();
    window.addEventListener("click", cleanup, { capture: true, once: true });
    window.addEventListener("scroll", cleanup, { capture: true, once: true });
    window.addEventListener("blur", cleanup, { once: true });
  }
  function close() {
    pos.value = null;
    items.value = [];
    cleanup = null;
  }
  return { pos, items, openFor, close };
}
