<script setup lang="ts">
// 导入任务卡（左下浮动，UI 对齐 v2）：进度/百分比/ETA/失败计数/暂停/停止
import { computed } from "vue";
import { useImportJob } from "../../composables/useImportJob";
import { formatEta } from "../../lib/format";

const { job, paused, pause, resume, stop } = useImportJob();

const pct = computed(() => {
  if (!job.value || job.value.total === 0) return 0;
  return Math.round((job.value.done / job.value.total) * 100);
});
</script>

<template>
  <aside v-if="job" class="import-card" :class="{ paused }" role="status">
    <div class="row1">
      <span class="label">
        正在导入
        <span v-if="job.total > 0" class="fn num">{{ job.done }} / {{ job.total }}</span>
      </span>
      <span class="pct num">{{ pct }}%</span>
    </div>
    <div class="bar"><i :style="{ width: pct + '%' }" /></div>
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
  </aside>
</template>
