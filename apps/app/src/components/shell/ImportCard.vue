<script setup lang="ts">
// 导入任务卡（右下浮动，可收纳，2026-09-09 所有者裁定）：
// 展开 = 进度/百分比/ETA/失败计数/暂停/停止；收起 = 一行胶囊，点击恢复
import { computed, ref } from "vue";
import { useImportJob } from "../../composables/useImportJob";
import { formatEta } from "../../lib/format";

const { job, paused, pause, resume, stop } = useImportJob();
const collapsed = ref(false);

const pct = computed(() => {
  if (!job.value || job.value.total === 0) return 0;
  return Math.round((job.value.done / job.value.total) * 100);
});
</script>

<template>
  <aside
    v-if="job"
    class="import-card"
    :class="{ paused, collapsed }"
    role="status"
  >
    <div class="row1">
      <span class="label">
        正在导入
        <span v-if="job.total > 0" class="fn num">{{ job.done }} / {{ job.total }}</span>
      </span>
      <span class="hgroup">
        <span class="pct num">{{ pct }}%</span>
        <button
          class="icon-btn fold"
          :title="collapsed ? '展开' : '收起'"
          :aria-label="collapsed ? '展开导入卡片' : '收起导入卡片'"
          @click="collapsed = !collapsed"
        >
          <svg v-if="collapsed" style="width: 14px; height: 14px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m7 11 5-5 5 5" /><path d="m7 18 5-5 5 5" /></svg>
          <svg v-else style="width: 14px; height: 14px" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m7 13 5 5 5-5" /><path d="m7 6 5 5 5-5" /></svg>
        </button>
      </span>
    </div>
    <div class="bar"><i :style="{ width: pct + '%' }" /></div>
    <template v-if="!collapsed">
      <div class="row2 num">
        <span>{{ formatEta(job.eta_seconds) }}</span>
        <span v-if="job.failed > 0" class="failed-n">{{ job.failed }} 个无法读取</span>
      </div>
      <div class="ops">
        <button class="btn-glass btn-outline-glass" style="padding: 5px 12px; font-size: 12px" @click="paused ? resume() : pause()">
          {{ paused ? "继续" : "暂停" }}
        </button>
        <button class="btn-glass btn-text" style="padding: 5px 12px; font-size: 12px" @click="stop">
          停止
        </button>
      </div>
    </template>
  </aside>
</template>
