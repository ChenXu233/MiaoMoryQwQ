<script setup lang="ts">
// App 壳（UI 对齐 v2/v9）：环境柔光 + 自绘标题栏 + 侧栏（桌面常驻/窄屏抽屉）+
// 内容区路由 + 底部 Dock（窄屏）+ 悬浮圆钮 + 任务卡栈 + 全局确认/右键菜单 + 通知。
import { onBeforeUnmount, onMounted, ref } from "vue";
import { commands } from "@miaomory/contracts";
import { useRoute } from "./lib/router";
import { useImportJob } from "./composables/useImportJob";
import { useLightbox } from "./composables/useLightbox";
import { useModelStatus } from "./composables/useModelStatus";
import { useEmbedJob } from "./composables/useEmbedJob";
import { useConfirm } from "./composables/useConfirm";
import { useContextMenu } from "./composables/useContextMenu";
import { useWorkspaceSelection } from "./composables/useWorkspaceSelection";
import TitleBar from "./components/shell/TitleBar.vue";
import UiConfirm from "./components/shell/UiConfirm.vue";
import UiContextMenu from "./components/shell/UiContextMenu.vue";
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
const { refresh: refreshEmbed } = useEmbedJob();
const { confirm } = useConfirm();
const { openFor: openContextMenu, close: closeContextMenu } = useContextMenu();
const { selectedFolderId, selectFolder } = useWorkspaceSelection();
const toastMsg = ref<string | null>(null);
let toastTimer: number | undefined;
const rail = ref(false); // 侧边栏收起态（body.rail，2026-09-09 所有者裁定）

function toast(msg: string) {
  toastMsg.value = msg;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toastMsg.value = null), 2600);
}

// ---- 右键动作（照片格/工作区行由 data-* 属性识别，2026-09-11 所有者裁定）----

async function reembedAsset(assetId: number) {
  const res = await commands.reindexAssets([assetId]);
  toast(res.status === "ok" ? "已重新向量化这张照片，进度见右下角" : `重建失败：${res.error}`);
}

async function deleteAsset(assetId: number) {
  const ok = await confirm({
    title: "删除这条记录？",
    message: "只从 MiaoMory 移除记录，你的原文件不会被改动。",
    okLabel: "删除",
    danger: true,
  });
  if (!ok) return;
  const res = await commands.deleteAssets([assetId]);
  if (res.status === "ok") {
    window.dispatchEvent(new CustomEvent("mm-library-changed"));
    toast("已删除记录");
  } else {
    toast(`删除失败：${res.error}`);
  }
}

async function reembedFolder(folderId: number, label: string) {
  const ok = await confirm({
    title: `重建「${label}」的语义向量？`,
    message: "重建期间这个工作区的语义搜索会暂时不可用，直到重建完成。",
    okLabel: "重建",
  });
  if (!ok) return;
  const res = await commands.reindexFolder(folderId);
  if (res.status === "ok") {
    await refreshEmbed();
    toast(`已开始重建（${res.data} 张），进度见右下角`);
  } else {
    toast(`重建失败：${res.error}`);
  }
}

async function recheckFolderById(folderId: number, label: string) {
  const res = await commands.recheckFolder(folderId);
  toast(
    res.status === "ok" && res.data.status === "online"
      ? `「${label}」已恢复在线`
      : `「${label}」仍不可访问`,
  );
}

async function deleteFolderById(folderId: number, label: string) {
  const ok = await confirm({
    title: `删除工作区「${label}」？`,
    message:
      "将移除这个工作区的全部记录、向量和缩略图，搜索结果里不再出现；磁盘上的原文件不会被改动。此操作不可撤销。",
    okLabel: "删除",
    danger: true,
  });
  if (!ok) return;
  const res = await commands.deleteFolder(folderId);
  if (res.status === "ok") {
    if (selectedFolderId.value === String(folderId)) selectFolder(""); // 回到全部照片
    window.dispatchEvent(new CustomEvent("mm-library-changed"));
    toast(`已删除「${label}」（${res.data.deleted} 条记录）`);
  } else {
    toast(`删除失败：${res.error}`);
  }
}

/** 右键拦截：输入区放行原生（复制/粘贴）；照片/工作区给上下文动作；其余一律屏蔽原生菜单 */
function onContextMenu(e: MouseEvent) {
  const el = e.target as HTMLElement;
  if (el.closest("input, textarea, [contenteditable='true']")) return;
  e.preventDefault();
  const tile = el.closest("[data-asset-id]");
  if (tile) {
    const id = Number(tile.getAttribute("data-asset-id"));
    if (Number.isFinite(id)) {
      openContextMenu(e, [
        { label: "重新向量化", action: () => void reembedAsset(id) },
        { label: "删除记录", danger: true, action: () => void deleteAsset(id) },
      ]);
      return;
    }
  }
  const folder = el.closest("[data-folder-id]");
  if (folder) {
    const fid = Number(folder.getAttribute("data-folder-id"));
    const label = folder.getAttribute("data-folder-label") ?? `工作区 #${fid}`;
    if (Number.isFinite(fid)) {
      openContextMenu(e, [
        { label: "重新向量化", action: () => void reembedFolder(fid, label) },
        { label: "重新检查", action: () => void recheckFolderById(fid, label) },
        { label: "删除工作区", danger: true, action: () => void deleteFolderById(fid, label) },
      ]);
      return;
    }
  }
  closeContextMenu();
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
  window.addEventListener("contextmenu", onContextMenu, true);
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
  window.removeEventListener("contextmenu", onContextMenu, true);
  window.removeEventListener("mm-toast", onToast);
});
</script>

<template>
  <div>
    <!-- 环境柔光（玻璃折射来源） -->
    <div class="amb amb-a" />
    <div class="amb amb-b" />
    <div class="amb amb-c" />

    <TitleBar />
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

    <UiConfirm />
    <UiContextMenu />

    <Lightbox
      v-if="lightbox.index.value !== null"
      :items="lightbox.items.value"
      :index="lightbox.index.value"
      @closed="lightbox.close"
      @navigate="lightbox.navigate"
    />
  </div>
</template>
