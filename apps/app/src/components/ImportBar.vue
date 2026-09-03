<script setup lang="ts">
// 导入进度条（体验规格 0001：绝对数量 + 剩余时间估算 + 暂停/继续/停止）
import { computed } from "vue";
import type { ImportProgressEvent } from "@miaomory/contracts";
import { formatEta } from "../lib/ui";

const props = defineProps<{
  job: ImportProgressEvent;
  paused: boolean;
}>();

const emit = defineEmits<{ pause: []; resume: []; stop: [] }>();

const percent = computed(() =>
  props.job.total > 0 ? Math.round((props.job.done / props.job.total) * 100) : 0,
);
</script>

<template>
  <div class="border-b border-line bg-surface px-6 py-3" role="status">
    <div class="mx-auto flex max-w-5xl items-center gap-4">
      <div class="flex-1">
        <div class="mb-1 flex items-baseline justify-between text-sm">
          <span class="text-fg">
            正在导入
            <span class="font-medium tabular-nums">{{ job.done.toLocaleString() }}</span>
            / {{ job.total.toLocaleString() }}
            <span v-if="job.failed > 0" class="ml-2 text-danger">
              {{ job.failed }} 个无法读取
            </span>
          </span>
          <span class="text-muted">{{ formatEta(job.eta_seconds) }}</span>
        </div>
        <div
          class="h-1.5 overflow-hidden rounded-full bg-line"
          role="progressbar"
          :aria-valuenow="job.done"
          aria-valuemin="0"
          :aria-valuemax="job.total"
        >
          <div
            class="h-full rounded-full bg-accent transition-[width] duration-150"
            :style="{ width: `${percent}%` }"
          />
        </div>
      </div>
      <div class="flex shrink-0 gap-2">
        <button
          v-if="!paused"
          type="button"
          class="rounded-md border border-line px-3 py-1.5 text-sm text-fg transition-colors hover:bg-bg focus-visible:outline-2 focus-visible:outline-accent"
          @click="emit('pause')"
        >
          暂停
        </button>
        <button
          v-else
          type="button"
          class="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-fg transition-colors hover:opacity-90 focus-visible:outline-2 focus-visible:outline-accent"
          @click="emit('resume')"
        >
          继续
        </button>
        <button
          type="button"
          class="rounded-md px-3 py-1.5 text-sm text-muted transition-colors hover:text-fg focus-visible:outline-2 focus-visible:outline-accent"
          @click="emit('stop')"
        >
          停止
        </button>
      </div>
    </div>
  </div>
</template>
