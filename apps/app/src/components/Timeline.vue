<script setup lang="ts">
// 时间轴：按年分组 + 虚拟滚动网格（体验规格 0002）+ Lightbox
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { useVirtualizer } from "@tanstack/vue-virtual";
import { commands, type AssetSummary, type YearGroup } from "@miaomory/contracts";
import { assetSrc, errorCopy, formatDate } from "../lib/ui";
import Lightbox from "./Lightbox.vue";

const PAGE_SIZE = 200;
const CELL = 200;
const GAP = 8;
const HEADER = 56;

const groups = ref<YearGroup[]>([]);
const cursor = ref<string | null>(null);
const exhausted = ref(false);
const loadError = ref(false);
const initialLoading = ref(true);
const loading = ref(false);

const container = ref<HTMLElement | null>(null);
const columnCount = ref(4);
const lightboxIndex = ref<number | null>(null);
const brokenThumbs = ref(new Set<number>());

const flatItems = computed<AssetSummary[]>(() => groups.value.flatMap((g) => g.items));

type Entry = { kind: "year"; year: number } | { kind: "row"; items: AssetSummary[] };

const entries = computed<Entry[]>(() => {
  const out: Entry[] = [];
  for (const g of groups.value) {
    out.push({ kind: "year", year: g.year });
    for (let i = 0; i < g.items.length; i += columnCount.value) {
      out.push({ kind: "row", items: g.items.slice(i, i + columnCount.value) });
    }
  }
  return out;
});

const virtualizer = useVirtualizer(
  computed(() => ({
    count: entries.value.length,
    getScrollElement: () => container.value,
    estimateSize: (i: number) => {
      const e = entries.value[i];
      return e ? (e.kind === "year" ? HEADER : CELL + GAP) : CELL + GAP;
    },
    overscan: 6,
    getItemKey: (i: number) => {
      const e = entries.value[i];
      if (!e) return `i${i}`;
      return e.kind === "year"
        ? `y${e.year}`
        : `r${e.items.map((it) => it.asset_id).join(",")}`;
    },
  })),
);

const totalSize = computed(() => virtualizer.value.getTotalSize());
const virtualRows = computed(() => virtualizer.value.getVirtualItems());

function measureColumns() {
  const el = container.value;
  if (!el) return;
  columnCount.value = Math.max(2, Math.floor((el.clientWidth - GAP) / (CELL + GAP)));
}

let observer: ResizeObserver | null = null;
onMounted(() => {
  measureColumns();
  observer = new ResizeObserver(measureColumns);
  if (container.value) observer.observe(container.value);
  void reload();
});
onBeforeUnmount(() => observer?.disconnect());

async function reload() {
  groups.value = [];
  cursor.value = null;
  exhausted.value = false;
  await loadMore(true);
}

async function loadMore(reset = false) {
  if (loading.value || (exhausted.value && !reset)) return;
  loading.value = true;
  loadError.value = false;
  try {
    const res = await commands.listTimeline(reset ? null : cursor.value, PAGE_SIZE, null);
    if (res.status === "error") {
      loadError.value = true;
      return;
    }
    const page = res.data;
    groups.value = reset ? page.groups : mergeGroups(groups.value, page.groups);
    cursor.value = page.next_cursor;
    exhausted.value = page.next_cursor == null;
  } finally {
    initialLoading.value = false;
    loading.value = false;
  }
}

function mergeGroups(oldG: YearGroup[], newG: YearGroup[]): YearGroup[] {
  const out = [...oldG];
  for (const g of newG) {
    const last = out[out.length - 1];
    if (last && last.year === g.year) last.items.push(...g.items);
    else out.push({ ...g });
  }
  return out;
}

function onScroll() {
  const el = container.value;
  if (!el) return;
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 800) void loadMore();
}

function thumbOf(item: AssetSummary): string | null {
  if (!item.thumb_path || brokenThumbs.value.has(item.asset_id)) return null;
  return assetSrc(item.thumb_path);
}

function yearOf(e: Entry | undefined): number {
  return e && e.kind === "year" ? e.year : 0;
}

function rowItems(e: Entry | undefined): AssetSummary[] {
  return e && e.kind === "row" ? e.items : [];
}

function onOpen(item: AssetSummary) {
  const idx = flatItems.value.findIndex((it) => it.asset_id === item.asset_id);
  if (idx >= 0) lightboxIndex.value = idx;
}

function onClosed() {
  lightboxIndex.value = null;
  void reload(); // 删除等操作后刷新
}

defineExpose({ reload });
</script>

<template>
  <div ref="container" class="h-full overflow-y-auto px-6 pb-16" @scroll.passive="onScroll">
    <div v-if="initialLoading" class="grid grid-cols-4 gap-2 pt-6">
      <div v-for="i in 12" :key="i" class="animate-pulse rounded-md bg-line" style="height: 200px" />
    </div>

    <template v-else>
      <div
        v-if="loadError"
        class="mx-auto mt-6 max-w-5xl rounded-md border border-line bg-surface px-4 py-3 text-sm text-fg"
      >
        加载失败，数据库暂不可用。
        <button type="button" class="ml-2 text-accent underline" @click="loadMore(true)">重试</button>
      </div>

      <div :style="{ position: 'relative', height: `${totalSize}px` }">
        <div
          v-for="vrow in virtualRows"
          :key="String(vrow.key)"
          :style="{
            position: 'absolute',
            top: 0,
            left: 0,
            width: '100%',
            transform: `translateY(${vrow.start}px)`,
          }"
        >
          <div
            v-if="entries[vrow.index]?.kind === 'year'"
            class="sticky top-0 z-10 flex h-14 items-center bg-bg/90 text-lg font-semibold text-fg backdrop-blur"
          >
            {{ yearOf(entries[vrow.index]) }}
          </div>
          <div
            v-else-if="entries[vrow.index]?.kind === 'row'"
            class="flex"
            :style="{ gap: `${GAP}px`, paddingTop: `${GAP}px` }"
          >
            <button
              v-for="item in rowItems(entries[vrow.index])"
              :key="item.asset_id"
              type="button"
              class="group relative shrink-0 overflow-hidden rounded-md bg-line focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
              :style="{ width: `${CELL}px`, height: `${CELL}px` }"
              :aria-label="`查看照片（${formatDate(item.taken_at)}）`"
              @click="onOpen(item)"
            >
              <img
                v-if="thumbOf(item)"
                :src="thumbOf(item) ?? undefined"
                alt=""
                loading="lazy"
                decoding="async"
                class="h-full w-full object-cover transition-opacity duration-150 group-hover:opacity-90"
                @error="brokenThumbs.add(item.asset_id)"
              />
              <span v-else class="flex h-full w-full items-center justify-center px-2 text-center text-xs text-muted">
                {{ errorCopy("write_failed") }}
              </span>
            </button>
          </div>
        </div>
      </div>

      <div
        v-if="!loadError && groups.length === 0 && !initialLoading"
        class="pt-16 text-center text-sm text-muted"
      >
        还没有照片。
      </div>
    </template>

    <Lightbox
      v-if="lightboxIndex !== null"
      :items="flatItems"
      :index="lightboxIndex"
      @closed="onClosed"
      @navigate="(i: number) => (lightboxIndex = i)"
    />
  </div>
</template>
