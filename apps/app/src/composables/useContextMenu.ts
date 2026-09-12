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
let onDocClick: ((e: MouseEvent) => void) | null = null;
let onScroll: (() => void) | null = null;

export function useContextMenu() {
  /** 在鼠标位置显示菜单;同时挂全局关闭器（外点/滚动即关） */
  function openFor(e: MouseEvent, list: ContextItem[]) {
    close();
    items.value = list;
    // 视口边缘收拢,防止菜单溢出
    const x = Math.min(e.clientX, window.innerWidth - 180);
    const y = Math.min(e.clientY, window.innerHeight - list.length * 34 - 16);
    pos.value = { x, y };
    onDocClick = (ev) => {
      // 菜单内部点击必须放行：capture 关闭器先于按钮的 @click 触发,
      // 若在此卸载菜单 DOM,click 走到目标时节点已移除,动作永远不执行
      if ((ev.target as Element | null)?.closest?.(".ctx-menu")) return;
      close();
    };
    onScroll = () => close();
    window.addEventListener("click", onDocClick, { capture: true });
    window.addEventListener("scroll", onScroll, { capture: true });
  }
  function close() {
    pos.value = null;
    items.value = [];
    if (onDocClick) {
      window.removeEventListener("click", onDocClick, { capture: true } as EventListenerOptions);
      onDocClick = null;
    }
    if (onScroll) {
      window.removeEventListener("scroll", onScroll, { capture: true } as EventListenerOptions);
      onScroll = null;
    }
  }
  return { pos, items, openFor, close };
}
