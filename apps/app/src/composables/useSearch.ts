// 语义搜索（单一实例）：防抖 + 工作区范围（v8）+ 网格/列表视图；首页与悬浮搜索共用
import { ref, watch } from "vue";
import { commands, type SearchHit } from "@miaomory/contracts";
import { selectedFolderId } from "./useWorkspaceSelection";

const query = ref("");
const searching = ref(false);
const results = ref<SearchHit[] | null>(null);
const elapsedLabel = ref("");
const searchError = ref(false);
const spView = ref<"grid" | "list">("grid");
let timer: number | undefined;
let seq = 0;

async function run(immediate: boolean) {
  window.clearTimeout(timer);
  const q = query.value.trim();
  if (!q) {
    results.value = null;
    return;
  }
  const fire = async () => {
    const mySeq = ++seq;
    searching.value = true;
    try {
      const t0 = performance.now();
      const res = await commands.searchAssets(q, 100, {
        taken_from: null,
        taken_to: null,
        kind: null,
        folder_id: selectedFolderId.value ? Number(selectedFolderId.value) : null,
      });
      const ms = Math.round(performance.now() - t0);
      if (mySeq !== seq) return; // 过期响应丢弃
      elapsedLabel.value = ms < 1000 ? `${(ms / 1000).toFixed(2)} 秒` : `${ms} ms`;
      if (res.status === "ok") {
        results.value = res.data.items;
        searchError.value = false;
      } else {
        results.value = [];
        searchError.value = true;
      }
    } finally {
      searching.value = false;
    }
  };
  if (immediate) void fire();
  else timer = window.setTimeout(fire, 300);
}

export function useSearch() {
  // 范围变化且已有结果：立即重搜（原型 v8「结果态下切换文件夹即时刷新」）
  watch(selectedFolderId, () => {
    if (results.value !== null) void run(true);
  });
  return {
    query,
    searching,
    results,
    elapsedLabel,
    searchError,
    spView,
    run,
    clear: () => {
      query.value = "";
      results.value = null;
      searchError.value = false;
    },
  };
}
