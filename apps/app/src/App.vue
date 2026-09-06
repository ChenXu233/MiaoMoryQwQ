<script setup lang="ts">
// App 编排：空态引导 / 导入进度 / 时间轴浏览（状态矩阵见 docs/spec/0001、0002）
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
  commands,
  events,
  type AssetSummary,
  type ImportProgressEvent,
  type SearchHit,
} from "@miaomory/contracts";
import EmptyGuide from "./components/EmptyGuide.vue";
import ImportBar from "./components/ImportBar.vue";
import SearchBar from "./components/SearchBar.vue";
import Timeline from "./components/Timeline.vue";
import Lightbox from "./components/Lightbox.vue";
import DataSettings from "./components/DataSettings.vue";
import { assetSrc, errorCopy } from "./lib/ui";

type Phase = "loading" | "empty" | "browsing";

const phase = ref<Phase>("loading");
const job = ref<ImportProgressEvent | null>(null);
const paused = ref(false);
const finishNotice = ref<string | null>(null);
const failedBanner = ref(false);
const importError = ref<string | null>(null);
const timeline = ref<InstanceType<typeof Timeline> | null>(null);
const searchbar = ref<InstanceType<typeof SearchBar> | null>(null);

// ---- 搜索状态（规格 0003 七状态）----
const modelReady = ref(false);
const modelFilesMissing = ref<string[]>([]);
const downloading = ref<{ file: string; received: number; total: number } | null>(null);
const searching = ref(false);
const searchResults = ref<SearchHit[] | null>(null);
const pendingIndexing = ref(0);
const searchError = ref(false);
const lightboxIndex = ref<number | null>(null);
const availableYears = ref<string[]>([]);
const filterYear = ref("");
const filterKind = ref("");

const showGuide = computed(() => phase.value === "empty" && job.value === null);
const showSettings = ref(false);
const settingsBtn = ref<HTMLButtonElement | null>(null);
let unlisteners: Array<() => void> = [];

onMounted(async () => {
  // 事件订阅（导入进行中才有事件；应用重启后任务不复活）
  unlisteners.push(
    await events.importProgressEvent.listen((e) => {
      job.value = e.payload;
      paused.value = false;
      finishNotice.value = null;
    }),
  );
  unlisteners.push(
    await events.importPausedEvent.listen(() => {
      paused.value = true;
    }),
  );
  unlisteners.push(
    await events.importResumedEvent.listen(() => {
      paused.value = false;
    }),
  );
  unlisteners.push(
    await events.importFinishedEvent.listen((e) => {
      job.value = null;
      paused.value = false;
      failedBanner.value = e.payload.failed_count > 0;
      finishNotice.value =
        e.payload.failed_count > 0
          ? `导入完成，${e.payload.failed_count} 个文件无法读取`
          : "导入完成";
      phase.value = "browsing";
      void timeline.value?.reload();
      window.setTimeout(() => (finishNotice.value = null), 6000);
    }),
  );

  // 模型状态 + 订阅模型/嵌入事件
  unlisteners.push(
    await events.modelDownloadProgressEvent.listen((e) => {
      downloading.value = e.payload;
    }),
  );
  unlisteners.push(
    await events.modelReadyEvent.listen(() => {
      downloading.value = null;
      void refreshModelStatus();
    }),
  );
  unlisteners.push(
    await events.embedProgressEvent.listen(() => {
      void refreshModelStatus();
    }),
  );
  await refreshModelStatus();

  // 初始态：看库里有没有照片
  const snap = await commands.importSnapshot();
  if (snap.status === "ok" && snap.data.total > 0 && snap.data.done < snap.data.total) {
    job.value = snap.data; // 极小概率：任务仍在跑（本次会话内）
    phase.value = "browsing";
    return;
  }
  const page = await commands.listTimeline(null, 1);
  if (page.status === "ok" && page.data.groups.length > 0) {
    phase.value = "browsing";
  } else {
    phase.value = "empty";
  }
});

onBeforeUnmount(() => unlisteners.forEach((off) => off()));

async function refreshModelStatus() {
  const status = await commands.modelStatus();
  modelReady.value = status.ready && status.loaded;
  modelFilesMissing.value = status.files_missing;
}

async function onSearch(query: string) {
  searchError.value = false;
  const q = query.trim();
  if (!q) {
    searchResults.value = null;
    return;
  }
  searching.value = true;
  try {
    const res = await commands.searchAssets(
      q,
      100,
      {
        taken_from: filterYear.value
          ? Number(filterYear.value) * 31_536_000 - 31_536_000
          : null,
        taken_to: filterYear.value ? Number(filterYear.value) * 31_536_000 + 3.15e7 : null,
        kind: filterKind.value || null,
      },
    );
    if (res.status === "ok") {
      searchResults.value = res.data.items;
      pendingIndexing.value = res.data.pending_indexing;
      availableYears.value = res.data.available_years;
    } else {
      searchResults.value = [];
      searchError.value = true;
    }
  } finally {
    searching.value = false;
  }
}

function refilter() {
  void onSearch(searchbar.value ? "" : "");
  // 触发同查询重搜：直接用当前输入重新拉取
}

function downloadModels() {
  void commands.downloadModels();
}

const searchItems = computed<AssetSummary[]>(() =>
  (searchResults.value ?? []).map((h) => h.summary),
);

function openResult(item: AssetSummary) {
  const idx = searchItems.value.findIndex((it) => it.asset_id === item.asset_id);
  if (idx >= 0) lightboxIndex.value = idx;
}

async function pickFolder() {
  importError.value = null;
  const dir = await open({ directory: true, multiple: false });
  if (typeof dir !== "string") return;
  const res = await commands.importFolder(dir);
  if (res.status === "ok") {
    job.value = { job_id: res.data, total: 0, done: 0, failed: 0, eta_seconds: null };
    phase.value = "browsing"; // 导入中也能看已导入部分
    void timeline.value?.reload();
  } else {
    importError.value = res.error;
  }
}

function onPause() {
  void commands.pauseImport();
}
function onResume() {
  void commands.resumeImport();
}
function onStop() {
  void commands.stopImport();
}
</script>

<template>
  <div class="flex h-full flex-col bg-bg text-fg">
    <header class="flex items-center justify-between px-6 py-3">
      <h1 class="text-lg font-semibold">MiaoMory</h1>
      <div class="flex items-center gap-2">
        <span v-if="finishNotice" class="text-sm text-success">{{ finishNotice }}</span>
        <button
          ref="settingsBtn"
          type="button"
          class="rounded-md px-2.5 py-1.5 text-sm text-muted transition-colors hover:bg-surface hover:text-fg focus-visible:outline-2 focus-visible:outline-accent"
          @click="showSettings = true"
        >
          设置
        </button>
        <button
          v-if="phase === 'browsing' && job === null"
          type="button"
          class="rounded-md border border-line px-3 py-1.5 text-sm transition-colors hover:bg-surface focus-visible:outline-2 focus-visible:outline-accent"
          @click="pickFolder"
        >
          导入文件夹
        </button>
      </div>
    </header>

    <div
      v-if="failedBanner && job === null"
      class="mx-auto w-full max-w-5xl rounded-md border border-line bg-surface px-4 py-2 text-sm"
    >
      有文件无法导入。
      <button
        type="button"
        class="ml-2 text-accent underline"
        @click="failedBanner = false"
      >
        知道了
      </button>
    </div>

    <ImportBar
      v-if="job"
      :job="job"
      :paused="paused"
      @pause="onPause"
      @resume="onResume"
      @stop="onStop"
    />

    <main class="flex min-h-0 flex-1 flex-col gap-3 px-6 pb-4 pt-3">
      <EmptyGuide v-if="showGuide" @pick-folder="pickFolder" />
      <template v-else-if="phase !== 'loading'">
        <div class="mx-auto w-full max-w-5xl">
          <SearchBar
            ref="searchbar"
            :model-ready="modelReady"
            :searching="searching"
            :years="availableYears"
            :kind="filterKind"
            :year="filterYear"
            @search="onSearch"
            @download-models="downloadModels"
            @refilter="refilter"
          />
          <div v-if="downloading" class="mt-2 text-xs text-muted" role="status">
            正在下载识别模型 {{ downloading.file }}：{{ Math.round(downloading.received / 1e6) }} /
            {{ Math.round(downloading.total / 1e6) }} MB（已下载部分不会丢失）
          </div>
          <div v-if="pendingIndexing > 0 && modelReady" class="mt-2 text-xs text-muted" role="status">
            还有 {{ pendingIndexing }} 张照片正在建立索引，结果稍后会更完整。
          </div>
        </div>

        <!-- 搜索结果 -->
        <div v-if="searchResults !== null" class="min-h-0 flex-1 overflow-y-auto">
          <div v-if="searchError" class="mx-auto max-w-5xl text-sm text-danger">
            {{ errorCopy("search_unavailable") }}
          </div>
          <div
            v-else-if="searchResults.length === 0"
            class="pt-10 text-center text-sm text-muted"
          >
            没有找到相关照片。试试更具体的词，比如「火锅」「雪山」。
          </div>
          <div v-else class="grid grid-cols-4 gap-2 md:grid-cols-6">
            <button
              v-for="item in searchItems"
              :key="item.asset_id"
              type="button"
              class="aspect-square overflow-hidden rounded-md bg-line focus-visible:outline-2 focus-visible:outline-accent"
              @click="openResult(item)"
            >
              <img
                v-if="item.thumb_path"
                :src="assetSrc(item.thumb_path)"
                alt=""
                loading="lazy"
                class="h-full w-full object-cover hover:opacity-90"
              />
              <span
                v-if="(searchResults?.find((h) => h.summary.asset_id === item.asset_id)?.matched ?? '') !== 'both'"
                class="absolute bottom-1 left-1 rounded bg-black/60 px-1 text-[10px] text-white"
                :aria-label="`命中来源：${(searchResults?.find((h) => h.summary.asset_id === item.asset_id)?.matched) === 'semantic' ? '语义' : '文件名'}`"
              >
                {{ (searchResults?.find((h) => h.summary.asset_id === item.asset_id)?.matched) === 'semantic' ? '语义' : '文件名' }}
              </span>
            </button>
          </div>
        </div>

        <!-- 时间轴浏览 -->
        <Timeline v-show="searchResults === null" class="min-h-0 flex-1" ref="timeline" />
      </template>
    </main>

    <DataSettings
      v-if="showSettings"
      @close="
        () => {
          showSettings = false;
          settingsBtn?.focus();
        }
      "
    />

    <Lightbox
      v-if="lightboxIndex !== null"
      :items="searchItems"
      :index="lightboxIndex"
      @closed="
        () => {
          lightboxIndex = null;
          void timeline?.reload();
        }
      "
      @navigate="(i: number) => (lightboxIndex = i)"
    />
  </div>
</template>
