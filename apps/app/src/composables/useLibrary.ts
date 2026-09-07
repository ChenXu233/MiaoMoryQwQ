// 库数据（时间线 keyset 分页 + 工作区作用域）：单一实例，导入完成/切换工作区时重载
import { ref, watch } from "vue";
import { commands, type AssetSummary, type YearGroup } from "@miaomory/contracts";
import { selectedFolderId } from "./useWorkspaceSelection";

const PAGE_SIZE = 200;
const groups = ref<YearGroup[]>([]);
const cursor = ref<string | null>(null);
const exhausted = ref(false);
const loading = ref(false);
let started = false;

async function loadPage(reset: boolean) {
  if (loading.value) return;
  loading.value = true;
  try {
    if (reset) {
      cursor.value = null;
      exhausted.value = false;
    }
    if (exhausted.value) return;
    const res = await commands.listTimeline(
      reset ? null : cursor.value,
      PAGE_SIZE,
      selectedFolderId.value ? Number(selectedFolderId.value) : null,
    );
    if (res.status !== "ok") return;
    if (reset) groups.value = res.data.groups;
    else groups.value = [...groups.value, ...res.data.groups];
    cursor.value = res.data.next_cursor;
    if (!res.data.next_cursor) exhausted.value = true;
  } finally {
    loading.value = false;
  }
}

export function useLibrary() {
  if (!started) {
    started = true;
    // 工作区切换 → 重载（原型 v8 语义延伸：范围跟随侧栏选择）
    watch(selectedFolderId, () => void loadPage(true));
  }
  return {
    groups,
    exhausted,
    loading,
    reload: () => loadPage(true),
    loadMore: () => loadPage(false),
  };
}

export type LibraryItem = AssetSummary & { year: number };
