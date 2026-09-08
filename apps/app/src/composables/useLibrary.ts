// 库数据（时间线 keyset 分页 + 工作区作用域）：单一数据实例，导入完成/切换工作区时重载
import { ref, watch } from "vue";
import { commands, type AssetSummary, type YearGroup } from "@miaomory/contracts";
import { selectedFolderId } from "./useWorkspaceSelection";

const PAGE_SIZE = 200;
const groups = ref<YearGroup[]>([]);
const cursor = ref<string | null>(null);
const exhausted = ref(false);
const loading = ref(false);
// 忙碌期间到达的 reset 记账：当前装载完成后补一次重载（快速切工作区不丢刷新）
let resetQueued = false;

async function loadPage(reset: boolean) {
  if (loading.value) {
    if (reset) resetQueued = true;
    return;
  }
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
    if (resetQueued) {
      resetQueued = false;
      void loadPage(true);
    }
  }
}

export function useLibrary() {
  // 工作区切换 → 重载（原型 v8 语义延伸：范围跟随侧栏选择）。
  // watch 随调用方组件的生命周期注册/销毁（与 useSearch 一致）——不能加单例守卫：
  // 守卫会让二次挂载后切换工作区不再刷新（watch 已随首次挂载的 effectScope 销毁）
  watch(selectedFolderId, () => void loadPage(true));
  return {
    groups,
    exhausted,
    loading,
    reload: () => loadPage(true),
    loadMore: () => loadPage(false),
  };
}

export type LibraryItem = AssetSummary & { year: number };
