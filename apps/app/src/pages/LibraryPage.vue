<script setup lang="ts">
// 照片库页（UI 对齐 v5）：空态功能卡 ↔ 内容；时间线（默认，年份吸顶）/瀑布流切换；
// 152px 定宽单元格恒定；选中侧栏文件夹时显示过滤 chip。原文件永远只读。
import { computed, onMounted, onUnmounted, ref } from "vue";
import type { AssetSummary } from "@miaomory/contracts";
import { useLibrary } from "../composables/useLibrary";
import { useImportJob } from "../composables/useImportJob";
import { useFolders } from "../composables/useFolders";
import { useWorkspaceSelection } from "../composables/useWorkspaceSelection";
import { useLightbox } from "../composables/useLightbox";
import PhotoTile from "../components/PhotoTile.vue";

const library = useLibrary();
const importJob = useImportJob(() => void library.reload());
const { folders } = useFolders();
const { selectedFolderId, selectFolder } = useWorkspaceSelection();
const lightbox = useLightbox();

const libView = ref<"timeline" | "wall">("timeline"); // v5 裁定：默认时间线

const allGroups = computed(() => library.groups.value);
const isLoading = computed(() => library.loading.value);
const isExhausted = computed(() => library.exhausted.value);
const hasAnyPhoto = computed(
  () => allGroups.value.some((g) => g.items.length > 0) || !isExhausted.value,
);
const selectedFolder = computed(() =>
  folders.value.find((f) => String(f.folder_id) === selectedFolderId.value),
);

/** 扁平化当前列表（灯箱导航顺序 = 展示顺序） */
const flatItems = computed<AssetSummary[]>(() =>
  allGroups.value.flatMap((g) => g.items),
);

const wallCols = computed<AssetSummary[][]>(() => {
  const photos = flatItems.value;
  const frameW = window.innerWidth - 214 - 32;
  const k = Math.max(2, Math.floor((Math.max(340, frameW) + 6) / 158));
  const cols: AssetSummary[][] = Array.from({ length: k }, () => []);
  photos.forEach((p, i) => cols[i % k].push(p));
  return cols;
});

function openPhoto(item: AssetSummary) {
  const idx = flatItems.value.findIndex((p) => p.asset_id === item.asset_id);
  lightbox.open(flatItems.value, Math.max(0, idx));
}

function clearFolder() {
  selectFolder("");
}

function onResize() {
  // 触发 wallCols 重算（读取 window.innerWidth）
  void frameW.value;
}
const frameW = ref(0);
function measure() {
  frameW.value = window.innerWidth;
}
onMounted(() => {
  measure();
  window.addEventListener("resize", onResize);
  void library.reload();
});
onUnmounted(() => window.removeEventListener("resize", onResize));
</script>

<template>
  <div class="page">
    <!-- 空库：功能优先，无标语（v2 裁定） -->
    <div v-if="!hasAnyPhoto && !importJob.job.value" class="empty-lib">
      <button class="empty-card" @click="importJob.pickFolder()">
        <div class="t">选择照片文件夹</div>
      </button>
    </div>

    <div v-else>
      <div class="lib-toolbar">
        <div class="seg" role="group" aria-label="库视图切换">
          <button :class="{ on: libView === 'timeline' }" @click="libView = 'timeline'">
            时间线
          </button>
          <button :class="{ on: libView === 'wall' }" @click="libView = 'wall'">瀑布流</button>
        </div>
      </div>

      <button v-if="selectedFolderId" class="filter-chip" @click="clearFolder">
        <span>文件夹：{{ selectedFolder?.label || selectedFolder?.path }}</span>
        <span aria-label="清除文件夹过滤" style="cursor: pointer">✕</span>
      </button>

      <!-- 时间线：年份吸顶 -->
      <div v-if="libView === 'timeline'">
        <template v-for="g in allGroups" :key="g.year">
          <div v-if="g.items.length" class="year-head">
            <span class="y">{{ g.year }}</span>
            <span class="c num">{{ g.items.length }} 张</span>
          </div>
          <div v-if="g.items.length" class="grid-152">
            <PhotoTile
              v-for="it in g.items"
              :key="it.asset_id"
              :item="it"
              @open="openPhoto"
            />
          </div>
        </template>
        <div v-if="isLoading" class="sp-empty">加载中…</div>
        <div
          v-else-if="!isExhausted"
          class="sp-empty"
          style="cursor: pointer"
          @click="library.loadMore"
        >
          加载更多
        </div>
      </div>

      <!-- 瀑布流：152px 定宽列，尺寸恒定（列数随 frame 宽度重算，单元格不缩放） -->
      <div v-else class="wall">
        <div v-for="(col, ci) in wallCols" :key="ci" class="wall-col">
          <PhotoTile
            v-for="it in col"
            :key="it.asset_id"
            :item="it"
            fluid
            @open="openPhoto"
          />
        </div>
      </div>
    </div>
  </div>
</template>
