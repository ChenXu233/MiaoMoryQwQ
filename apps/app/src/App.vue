<script setup lang="ts">
// App 壳（UI 对齐 v2/v9）：环境柔光 + 侧栏（桌面常驻/窄屏抽屉）+ 内容区路由 +
// 底部 Dock（窄屏）+ 悬浮圆钮 + 导入任务卡 + 通知。业务逻辑全部在 composables。
import { onBeforeUnmount, onMounted, ref } from "vue";
import { useRoute } from "./lib/router";
import { useImportJob } from "./composables/useImportJob";
import { useLightbox } from "./composables/useLightbox";
import { useModelStatus } from "./composables/useModelStatus";
import AppSidebar from "./components/shell/AppSidebar.vue";
import BottomDock from "./components/shell/BottomDock.vue";
import ImportCard from "./components/shell/ImportCard.vue";
import EmbedCard from "./components/shell/EmbedCard.vue";
import SearchFloat from "./components/shell/SearchFloat.vue";
import Lightbox from "./components/Lightbox.vue";
import HomePage from "./pages/HomePage.vue";
import LibraryPage from "./pages/LibraryPage.vue";
import SettingsPage from "./pages/SettingsPage.vue";

const route = useRoute();
const importJob = useImportJob();
const lightbox = useLightbox();
useModelStatus(); // 单例：启动即检查模型并自动下载（规格 0004）
const toastMsg = ref<string | null>(null);
let toastTimer: number | undefined;
const rail = ref(false); // 侧边栏收起态（body.rail，2026-09-09 所有者裁定）

function toast(msg: string) {
  toastMsg.value = msg;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toastMsg.value = null), 2600);
}

function toggleDrawer() {
  document.body.classList.toggle("drawer");
}
function closeDrawer() {
  document.body.classList.remove("drawer");
}
function applyRail(on: boolean) {
  rail.value = on;
  document.body.classList.toggle("rail", on);
}
function expandRail() {
  applyRail(false);
  try {
    localStorage.removeItem("mm-rail");
  } catch {
    /* ignore */
  }
}
function onRailChanged() {
  applyRail(document.body.classList.contains("rail"));
}
function goBack() {
  history.back();
}
function onGlobalKey(e: KeyboardEvent) {
  if (e.key === "Escape" && document.body.classList.contains("drawer")) {
    closeDrawer();
  }
}
// 具名引用：removeEventListener 必须传同一函数对象（内联箭头两边各建一个 = 永远移不掉）
function onToast(e: Event) {
  toast((e as CustomEvent<string>).detail);
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKey);
  window.addEventListener("mm-rail-changed", onRailChanged);
  // 全局 toast 通道（灯箱等深层组件无 emit 链时用）
  window.addEventListener("mm-toast", onToast);
  let saved: string | null = null;
  try {
    saved = localStorage.getItem("mm-rail");
  } catch {
    /* ignore */
  }
  applyRail(saved === "1");
  void importJob.snapshot();
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onGlobalKey);
  window.removeEventListener("mm-rail-changed", onRailChanged);
  window.removeEventListener("mm-toast", onToast);
});
</script>

<template>
  <div>
    <!-- 环境柔光（玻璃折射来源） -->
    <div class="amb amb-a" />
    <div class="amb amb-b" />
    <div class="amb amb-c" />

    <AppSidebar @toast="toast" />

    <!-- 侧边栏收起后的恢复钮（左上悬浮） -->
    <button v-if="rail" class="rail-fab" aria-label="展开侧边栏" title="展开侧边栏" @click="expandRail">
      <svg style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="currentColor">
        <ellipse cx="12" cy="15.6" rx="5.4" ry="4.5" />
        <circle cx="5.2" cy="10" r="2.15" />
        <circle cx="9.5" cy="6.9" r="2.25" />
        <circle cx="14.5" cy="6.9" r="2.25" />
        <circle cx="18.8" cy="10" r="2.15" />
      </svg>
    </button>

    <div class="frame">
      <HomePage v-if="route === 'home'" />
      <LibraryPage v-else-if="route === 'library'" />
      <SettingsPage v-else />
    </div>

    <BottomDock />

    <!-- 窄屏悬浮圆钮：左返回 / 右呼出抽屉（v7/v9） -->
    <button class="fab fab-back" aria-label="返回上一页" @click="goBack">
      <svg style="width: 18px; height: 18px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m14.5 6-6 6 6 6" /></svg>
    </button>
    <button class="fab fab-menu" aria-label="呼出侧边栏" @click="toggleDrawer">
      <svg style="width: 18px; height: 18px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h10" /></svg>
    </button>
    <div class="scrim" @click="closeDrawer" />

    <SearchFloat v-if="route === 'library'" />

    <!-- 右下浮动任务卡栈：导入卡 + 向量索引进度卡 -->
    <div class="float-stack">
      <ImportCard />
      <EmbedCard />
    </div>

    <div v-if="importJob.failedBanner.value && !importJob.job.value" class="notice" role="status">
      <span>有文件无法导入——原文件未受影响。清单见「设置 → 无法导入的文件」。</span>
      <button
        type="button"
        aria-label="关闭"
        style="border: none; background: transparent; cursor: pointer; color: inherit"
        @click="importJob.dismissFailed"
      >
        ✕
      </button>
    </div>
    <div v-if="toastMsg" class="toast" role="status">{{ toastMsg }}</div>

    <Lightbox
      v-if="lightbox.index.value !== null"
      :items="lightbox.items.value"
      :index="lightbox.index.value"
      @closed="lightbox.close"
      @navigate="lightbox.navigate"
    />
  </div>
</template>
