<script setup lang="ts">
// 搜索框（体验规格 0003）：防抖 150ms、Ctrl/Cmd+K 聚焦、七状态
import { onBeforeUnmount, onMounted, ref } from "vue";

defineProps<{
  modelReady: boolean;
  searching: boolean;
  years: string[];
  year: string;
  kind: string;
}>();

const emit = defineEmits<{
  search: [query: string];
  downloadModels: [];
  refilter: [];
  updateYear: [v: string];
  updateKind: [v: string];
}>();

const input = ref("");
const el = ref<HTMLInputElement | null>(null);
let timer: number | undefined;

function onInput() {
  window.clearTimeout(timer);
  timer = window.setTimeout(() => emit("search", input.value), 150);
}

function onKeydown(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    el.value?.focus();
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

defineExpose({
  clear() {
    input.value = "";
    emit("search", "");
  },
});
</script>

<template>
  <div>
    <div v-if="modelReady" class="relative">
      <input
        ref="el"
        v-model="input"
        type="search"
        :disabled="searching"
        placeholder="搜索你的照片，比如：海边的日落"
        aria-label="语义搜索照片"
        class="w-full rounded-md border border-line bg-surface px-4 py-2 text-sm text-fg placeholder:text-muted focus-visible:outline-2 focus-visible:outline-accent"
        @input="onInput"
        @keydown.enter="emit('search', input)"
      />
      <span
        v-if="searching"
        class="absolute right-3 top-1/2 -translate-y-1/2 text-xs text-muted"
      >
        搜索中…
      </span>
      <div v-if="input" class="mt-2 flex gap-2 text-xs">
        <select
          :value="year"
          aria-label="按年份过滤"
          class="rounded border border-line bg-surface px-2 py-1 text-fg focus-visible:outline-accent"
          @change="emit('updateYear', ($event.target as HTMLSelectElement).value); emit('refilter')"
        >
          <option value="">全部时间</option>
          <option v-for="y in years" :key="y" :value="y">{{ y }}</option>
        </select>
        <select
          :value="kind"
          aria-label="按类型过滤"
          class="rounded border border-line bg-surface px-2 py-1 text-fg focus-visible:outline-accent"
          @change="emit('updateKind', ($event.target as HTMLSelectElement).value); emit('refilter')"
        >
          <option value="">全部类型</option>
          <option value="photo">照片</option>
        </select>
      </div>
    </div>
    <div
      v-else
      class="flex flex-wrap items-center gap-3 rounded-md border border-line bg-surface px-4 py-3 text-sm"
    >
      <span class="text-fg">搜索功能需要先下载识别模型（约 200MB，仅一次）。模型只在本机运行，照片不会上传。</span>
      <button
        type="button"
        class="rounded-md bg-accent px-3 py-1.5 text-sm text-accent-fg transition-colors hover:opacity-90 focus-visible:outline-2 focus-visible:outline-accent"
        @click="emit('downloadModels')"
      >
        下载识别模型
      </button>
    </div>
  </div>
</template>
