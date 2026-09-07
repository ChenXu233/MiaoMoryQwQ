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
function goBack() {
  history.back();
}
function onGlobalKey(e: KeyboardEvent) {
  if (e.key === "Escape" && document.body.classList.contains("drawer")) {
    closeDrawer();
  }
}

onMounted(() => {
  window.addEventListener("keydown", onGlobalKey);
  void importJob.snapshot();
});
onBeforeUnmount(() => window.removeEventListener("keydown", onGlobalKey));
</script>

<template>
  <div>
    <!-- 环境柔光（玻璃折射来源） -->
    <div class="amb amb-a" />
    <div class="amb amb-b" />
    <div class="amb amb-c" />

    <AppSidebar @toast="toast" />

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
    <ImportCard />

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
