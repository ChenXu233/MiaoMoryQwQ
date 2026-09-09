<script setup lang="ts">
// 向量化进度卡（右下浮动，与导入卡同区叠放，2026-09-10 裁决）：
// 按工作区分组：已向量化 N/M + 迷你进度条 + 正在处理的文件名逐张滚动 + 失败计数；
// 可收纳；全部完成后展示完成态数秒自动收场。数据源 useEmbedJob（单例）。
import { computed, ref } from "vue";
import { useEmbedJob } from "../../composables/useEmbedJob";

const { rows, totalDone, totalAll, hasPending, active, failedCount, doneNotice } = useEmbedJob();
const collapsed = ref(false);

const pct = computed(() =>
  totalAll.value > 0 ? Math.round((totalDone.value / totalAll.value) * 100) : 100,
);

function rowPct(done: number, total: number): number {
  return total > 0 ? Math.round((done / total) * 100) : 100;
}
</script>

<template>
  <aside
    v-if="hasPending || doneNotice"
    class="import-card embed-card"
    :class="{ collapsed, done: !hasPending }"
    role="status"
  >
    <div class="row1">
      <span class="label">
        <svg v-if="hasPending" style="width: 13px; height: 13px; flex: none" viewBox="0 0 24 24" fill="currentColor"><path d="M13 2 4.5 13.5H11L9.5 22 19 10h-6.5L13 2Z" /></svg>
        <svg v-else style="width: 13px; height: 13px; flex: none; color: var(--mm-success)" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m4.5 12.5 5 5 10-11" /></svg>
        {{ hasPending ? "正在建立索引" : "索引已就绪" }}
        <span v-if="hasPending" class="fn num">{{ totalDone }} / {{ totalAll }}</span>
      </span>
      <span class="hgroup">
        <span v-if="hasPending" class="pct num">{{ pct }}%</span>
        <button
          class="icon-btn fold"
          :title="collapsed ? '展开' : '收起'"
          :aria-label="collapsed ? '展开索引进度卡' : '收起索引进度卡'"
          @click="collapsed = !collapsed"
        >
          <svg v-if="collapsed" style="width: 14px; height: 14px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m7 11 5-5 5 5" /><path d="m7 18 5-5 5 5" /></svg>
          <svg v-else style="width: 14px; height: 14px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m7 13 5 5 5-5" /><path d="m7 6 5 5 5-5" /></svg>
        </button>
      </span>
    </div>

    <template v-if="!collapsed">
      <div v-for="r in rows" :key="r.folder_id" class="eg-row">
        <div class="eg-head">
          <span class="eg-name">{{ r.label || `工作区 #${r.folder_id}` }}</span>
          <span v-if="r.status !== 'online' && r.done < r.total" class="eg-off">源离线 · 暂停</span>
          <span class="num eg-frac">{{ r.done }} / {{ r.total }}</span>
        </div>
        <div class="bar"><i :style="{ width: rowPct(r.done, r.total) + '%' }" /></div>
      </div>
      <div class="row2">
        <span v-if="hasPending && active?.file_name" class="eg-file">{{ active.file_name }}</span>
        <span v-if="failedCount > 0" class="failed-n">{{ failedCount }} 张源文件不可读</span>
      </div>
    </template>
  </aside>
</template>
