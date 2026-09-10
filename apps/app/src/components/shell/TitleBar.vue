<script setup lang="ts">
// 自绘窗口标题栏(替换系统原生框,2026-09-11 所有者裁定):
// 整条为拖动区 + 右侧最小化/最大化/关闭;窗口装饰已在 tauri.conf.json 关闭(decorations:false)。
import { onBeforeUnmount, onMounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";

const win = getCurrentWindow();
const maximized = ref(false);
let unlisten: (() => void) | null = null;

async function pollMaximized() {
  try {
    maximized.value = await win.isMaximized();
  } catch {
    /* 窗口可能已关闭 */
  }
}

onMounted(async () => {
  await pollMaximized();
  unlisten = await win.onResized(() => void pollMaximized());
});
onBeforeUnmount(() => unlisten?.());
</script>

<template>
  <div class="titlebar" data-tauri-drag-region>
    <span class="tb-title" data-tauri-drag-region>MiaoMory</span>
    <div class="tb-controls">
      <button class="tb-btn" aria-label="最小化" title="最小化" @click="win.minimize()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M5 12h14" /></svg>
      </button>
      <button class="tb-btn" :aria-label="maximized ? '向下还原' : '最大化'" :title="maximized ? '向下还原' : '最大化'" @click="win.toggleMaximize()">
        <svg v-if="!maximized" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="5.5" y="5.5" width="13" height="13" rx="1.5" /></svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="4.5" y="8.5" width="11" height="11" rx="1.5" /><path d="M8.5 5.5h9a1.5 1.5 0 0 1 1.5 1.5v9" /></svg>
      </button>
      <button class="tb-btn tb-close" aria-label="关闭" title="关闭" @click="win.close()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </div>
</template>
