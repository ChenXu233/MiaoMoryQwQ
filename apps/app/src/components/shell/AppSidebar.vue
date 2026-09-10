<script setup lang="ts">
// 侧边栏（UI 对齐 v2/v9）：桌面常驻挤压式，窄屏为抽屉（body.drawer）。
// 导航（首页/照片库）+ 文件夹区（计数 + 状态点 + 建索引徽标 + ＋导入）+
// 底部：左齿轮设置 / 右收起钮（body.rail，2026-09-09 所有者裁定）。
// 工作区右键菜单（重新向量化/重新检查）由 App.vue 全局右键接管（data-folder-id）。
import { computed } from "vue";
import { useRoute, navigate } from "../../lib/router";
import { useFolders } from "../../composables/useFolders";
import { useImportJob } from "../../composables/useImportJob";
import { useWorkspaceSelection } from "../../composables/useWorkspaceSelection";

const route = useRoute();
const { folders } = useFolders();
const { selectedFolderId, selectFolder } = useWorkspaceSelection();
const importJob = useImportJob();

const emit = defineEmits<{ toast: [msg: string] }>();

const folderItems = computed(() =>
  folders.value.filter((f) => f.path !== "" || f.asset_count > 0),
);

async function addFolder() {
  await importJob.pickFolder();
  if (importJob.importError.value) {
    emit("toast", "导入启动失败");
  }
}

function pickFolder(id: string) {
  selectFolder(id);
}

function statusDot(s: string): string {
  return s === "online" ? "bg-success" : s === "offline" ? "bg-muted" : "bg-danger";
}

function closeDrawer() {
  document.body.classList.remove("drawer");
}

function collapse() {
  document.body.classList.add("rail");
  try {
    localStorage.setItem("mm-rail", "1");
  } catch {
    /* 隐私模式等场景忽略 */
  }
  window.dispatchEvent(new CustomEvent("mm-rail-changed"));
}
</script>

<template>
  <aside class="sidebar">
    <button class="brand-mark" aria-label="照片库" @click="navigate('library')">
      <svg style="width: 19px; height: 19px" viewBox="0 0 24 24" fill="currentColor">
        <ellipse cx="12" cy="15.6" rx="5.4" ry="4.5" />
        <circle cx="5.2" cy="10" r="2.15" />
        <circle cx="9.5" cy="6.9" r="2.25" />
        <circle cx="14.5" cy="6.9" r="2.25" />
        <circle cx="18.8" cy="10" r="2.15" />
      </svg>
    </button>

    <button
      class="nav-item"
      :class="{ on: route === 'home' }"
      @click="navigate('home')"
    >
      <svg class="i" style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 10.5 9-7.5 9 7.5" /><path d="M5 9.5V20a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V9.5" /></svg>
      首页
    </button>
    <button
      class="nav-item"
      :class="{ on: route === 'library' }"
      @click="navigate('library')"
    >
      <svg class="i" style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7.5" height="7.5" rx="1.5" /><rect x="13.5" y="3" width="7.5" height="7.5" rx="1.5" /><rect x="3" y="13.5" width="7.5" height="7.5" rx="1.5" /><rect x="13.5" y="13.5" width="7.5" height="7.5" rx="1.5" /></svg>
      照片库
    </button>

    <div class="side-sec">
      <span class="t">文件夹</span>
      <button class="icon-btn" title="导入文件夹" aria-label="导入文件夹" @click="addFolder">
        <svg style="width: 14px; height: 14px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>
    <div class="folder-list">
      <div
        v-for="f in folderItems"
        :key="f.folder_id"
        class="folder-row"
        :data-folder-id="f.folder_id"
        :data-folder-label="f.label || f.path"
      >
        <button
          class="folder-item"
          :class="{ on: selectedFolderId === String(f.folder_id) }"
          :title="f.status !== 'online' ? `文件夹${f.status === 'offline' ? '离线' : '路径丢失'}：${f.path}（右键可重新向量化/重新检查）` : `${f.path}（右键可重新向量化/重新检查）`"
          @click="pickFolder(String(f.folder_id))"
        >
          <span
            class="h-2 w-2 shrink-0 rounded-full"
            :class="statusDot(f.status)"
          />
          <span class="n">{{ f.label || f.path }}</span>
          <span v-if="f.pending_index > 0" class="idx num" title="建立索引中">◌ {{ f.pending_index }}</span>
          <span class="c num">{{ f.asset_count }}</span>
        </button>
      </div>
    </div>

    <div class="side-foot">
      <button class="icon-btn" title="设置" aria-label="设置" @click="navigate('settings')">
        <svg style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3.2" /><path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.11-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.56-1.11 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.08a1.7 1.7 0 0 0 1.03-1.56V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.08a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.56 1.03Z" /></svg>
      </button>
      <span class="sp" style="flex: 1" />
      <!-- 收起侧边栏（桌面；收起后左上角悬浮钮恢复） -->
      <button class="icon-btn" title="收起侧边栏" aria-label="收起侧边栏" @click="collapse">
        <svg style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m11 7-5 5 5 5" /><path d="m18 7-5 5 5 5" /></svg>
      </button>
      <!-- 窄屏抽屉关闭钮（.drawer-close 默认隐藏，body.drawer 时显示） -->
      <button class="icon-btn drawer-close" aria-label="关闭侧边栏" @click="closeDrawer">
        <svg style="width: 17px; height: 17px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </aside>
</template>
