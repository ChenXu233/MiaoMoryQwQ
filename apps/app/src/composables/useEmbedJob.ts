// 向量化任务（单一实例）：EmbedProgressEvent 逐张推进 + embed_status 快照兜底。
// 进度卡按工作区分组（done/total + 正在处理的文件名滚动 + 失败计数）；
// 节流派发 mm-library-changed 让网格呼吸点随嵌入逐批熄灭。
import { computed, ref, watch } from "vue";
import {
  commands,
  events,
  type EmbedProgressEvent,
  type FolderEmbedStatus,
} from "@miaomory/contracts";

const groups = ref<Map<number, FolderEmbedStatus>>(new Map());
/** 刚完成向量化的一张（进度卡文件名滚动展示；file_name 为空 = 复制/校准事件） */
const active = ref<EmbedProgressEvent | null>(null);
/** 会话累计解码失败（事件口径，重启清零） */
const failedCount = ref(0);
const doneNotice = ref(false);
let started = false;
let gridThrottle = 0;
const GRID_REFRESH_MS = 3000;
let doneTimer: number | undefined;

const rows = computed(() =>
  Array.from(groups.value.values())
    .filter((r) => r.total > 0)
    .sort((a, b) => a.folder_id - b.folder_id),
);
const totalDone = computed(() => rows.value.reduce((s, r) => s + r.done, 0));
const totalAll = computed(() => rows.value.reduce((s, r) => s + r.total, 0));
/** 还有未完成的向量（含源离线卡住的） */
const hasPending = computed(() => rows.value.some((r) => r.done < r.total));

async function snapshot() {
  const res = await commands.embedStatus();
  if (res.status !== "ok") return;
  const next = new Map<number, FolderEmbedStatus>();
  for (const r of res.data) next.set(r.folder_id, { ...r });
  groups.value = next;
}

export function useEmbedJob() {
  if (!started) {
    started = true;
    void events.embedProgressEvent.listen((e) => {
      const p = e.payload;
      active.value = p;
      failedCount.value = p.failed;
      // 逐张推进：事件带 folder_id 时对应分组 done+1（快照基线 + 本地增量）
      if (p.folder_id > 0) {
        const g = groups.value.get(p.folder_id);
        if (g && g.done < g.total) {
          groups.value.set(p.folder_id, { ...g, done: g.done + 1 });
        }
      }
      // 节流刷新网格（呼吸点熄灭）；完成时立即刷一次
      const now = Date.now();
      if (now - gridThrottle > GRID_REFRESH_MS) {
        gridThrottle = now;
        window.dispatchEvent(new CustomEvent("mm-library-changed"));
      }
    });
    void snapshot();
  }

  watch(hasPending, (pending, prev) => {
    if (!pending && prev) {
      // 全部完成：立即刷新网格 + 展示完成态数秒
      window.dispatchEvent(new CustomEvent("mm-library-changed"));
      doneNotice.value = true;
      window.clearTimeout(doneTimer);
      doneTimer = window.setTimeout(() => (doneNotice.value = false), 6000);
    }
  });

  return {
    rows,
    totalDone,
    totalAll,
    hasPending,
    active,
    failedCount,
    doneNotice,
    /** 重建等操作后手动重拉快照（done 归零重新计数） */
    refresh: snapshot,
  };
}
