<script setup lang="ts">
// App 编排：空态引导 / 导入进度 / 时间轴浏览（状态矩阵见 docs/spec/0001、0002）
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, events, type ImportProgressEvent } from "@miaomory/contracts";
import EmptyGuide from "./components/EmptyGuide.vue";
import ImportBar from "./components/ImportBar.vue";
import Timeline from "./components/Timeline.vue";

type Phase = "loading" | "empty" | "browsing";

const phase = ref<Phase>("loading");
const job = ref<ImportProgressEvent | null>(null);
const paused = ref(false);
const finishNotice = ref<string | null>(null);
const failedBanner = ref(false);
const importError = ref<string | null>(null);
const timeline = ref<InstanceType<typeof Timeline> | null>(null);

const showGuide = computed(() => phase.value === "empty" && job.value === null);
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
      window.setTimeout(() => (finishNotice.value = null), 6000);
    }),
  );

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
      <span v-if="finishNotice" class="text-sm text-success">{{ finishNotice }}</span>
      <button
        v-if="phase === 'browsing' && job === null"
        type="button"
        class="rounded-md border border-line px-3 py-1.5 text-sm transition-colors hover:bg-surface focus-visible:outline-2 focus-visible:outline-accent"
        @click="pickFolder"
      >
        导入文件夹
      </button>
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

    <main class="min-h-0 flex-1">
      <EmptyGuide v-if="showGuide" @pick-folder="pickFolder" />
      <div v-else-if="phase === 'loading'" class="p-6 text-sm text-muted">加载中…</div>
      <Timeline v-show="phase === 'browsing'" ref="timeline" />
    </main>
  </div>
</template>
