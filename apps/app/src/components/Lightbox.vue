<script setup lang="ts">
// Lightbox：全屏预览，←/→ 切换、Esc 关闭、Space 1:1、Delete 删除（体验规格 0002 §4）。
// 源离线降级（P5 切片 A1）：原图加载失败 → 标记文件夹 offline → 缩略图 + 状态条 + 重新检查；
// 在线时角标「原图 · 大小 · 来自文件夹」（UI 永不把缩略图冒充原图）。
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  commands,
  events,
  type AssetDetail,
  type AssetSummary,
} from "@miaomory/contracts";
import { assetSrc, errorCopy, formatDate } from "../lib/ui";

const props = defineProps<{
  items: AssetSummary[];
  index: number;
}>();

const emit = defineEmits<{ closed: []; navigate: [index: number] }>();

const current = computed(() => props.items[props.index]);
const isOneToOne = ref(false);
const loadFailed = ref(false);
const deleting = ref(false);

// 单张重新向量化（2026-09-10 裁决）：清向量入队重嵌，进度见右下角索引卡
const reembedding = ref(false);

async function reembed() {
  const it = current.value;
  if (!it || reembedding.value) return;
  reembedding.value = true;
  try {
    const res = await commands.reindexAssets([it.asset_id]);
    if (res.status === "ok") {
      window.dispatchEvent(
        new CustomEvent("mm-toast", { detail: "已重新向量化这张照片，进度见右下角" }),
      );
    } else {
      window.dispatchEvent(new CustomEvent("mm-toast", { detail: `重建失败：${res.error}` }));
    }
  } finally {
    reembedding.value = false;
  }
}

// 原图路径按需向后端取（AssetSummary 只带缩略图路径）
const imagePath = ref<string | null>(null);
// 源离线：原图 <img> 加载失败后置位；此时回退显示缩略图 + 状态条
const sourceOffline = ref(false);
const detail = ref<AssetDetail | null>(null);
let unlistenFolder: (() => void) | null = null;

function fmtSize(bytes: number): string {
  if (!bytes) return "";
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function loadDetail() {
  detail.value = null;
  const it = current.value;
  if (!it) return;
  const res = await commands.assetDetail(it.asset_id);
  if (res.status === "ok") detail.value = res.data;
}

async function loadImagePath() {
  imagePath.value = null;
  loadFailed.value = false;
  sourceOffline.value = false;
  isOneToOne.value = false;
  const it = current.value;
  if (!it) return;
  void loadDetail();
  const res = await commands.getAssetImage(it.asset_id);
  if (res.status === "ok") {
    imagePath.value = res.data;
  } else {
    loadFailed.value = true;
  }
}

// 原图 <img> 加载失败：被动检测源离线——上报后端标记文件夹，界面回退缩略图 + 状态条
async function onOriginalError() {
  if (sourceOffline.value || loadFailed.value) return;
  sourceOffline.value = true;
  imagePath.value = null;
  const it = current.value;
  if (!it) return;
  const res = await commands.reportOriginalMissing(it.asset_id);
  if (res.status === "ok") void loadDetail();
}

// 重新检查：文件夹恢复在线 → 重新加载原图
async function recheckSource() {
  const d = detail.value;
  if (!d) return;
  const res = await commands.recheckFolder(d.folder_id);
  if (res.status === "ok" && res.data.status === "online") {
    await loadImagePath();
  }
}

watch(() => props.index, loadImagePath, { immediate: true });

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  unlistenFolder = await events.folderStatusChangedEvent.listen((e) => {
    if (detail.value?.folder_id === e.payload.folder_id) {
      detail.value = { ...detail.value, folder_status: e.payload.status };
    }
  });
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenFolder?.();
});

function close() {
  emit("closed");
}

function step(delta: number) {
  const next = props.index + delta;
  if (next < 0 || next >= props.items.length) return;
  // 通知父级换 index：父级以 v-model 方式处理（见 App/Timeline）
  emit("navigate", next);
}

async function remove() {
  const it = current.value;
  if (!it || deleting.value) return;
  deleting.value = true;
  try {
    const ok = await ask("删除这条记录？只从 MiaoMory 移除，不会删除你的原文件。", {
      title: "删除确认",
      kind: "warning",
      okLabel: "删除",
      cancelLabel: "取消",
    });
    if (!ok) return;
    const res = await commands.deleteAssets([it.asset_id]);
    if (res.status === "ok") {
      window.dispatchEvent(new CustomEvent("mm-library-changed"));
      close();
    }
  } finally {
    deleting.value = false;
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") close();
  else if (e.key === "ArrowLeft") step(-1);
  else if (e.key === "ArrowRight") step(1);
  else if (e.key === " ") {
    e.preventDefault();
    isOneToOne.value = !isOneToOne.value;
  } else if (e.key === "Delete") void remove();
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex flex-col bg-black/90"
    role="dialog"
    aria-modal="true"
    aria-label="照片预览"
  >
    <header class="flex items-center justify-between px-6 py-3 text-sm text-white/90">
      <span>{{ current ? formatDate(current.taken_at) : "" }}</span>
      <button
        type="button"
        class="rounded-md px-3 py-1.5 transition-colors hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-white"
        aria-label="关闭预览"
        @click="close"
      >
        关闭（Esc）
      </button>
    </header>

    <!-- 源离线状态条（缩略图回退时显示；照片没丢，只是源不在线） -->
    <div
      v-if="sourceOffline && detail"
      class="mx-auto flex w-full max-w-3xl items-center gap-3 rounded-md border border-white/20 bg-white/10 px-4 py-2 text-sm text-white/90"
      role="status"
    >
      <span class="flex-1">
        正在显示缩略图 · 原件所在文件夹不在线：{{
          detail.folder_label || detail.folder_path
        }}
      </span>
      <button
        type="button"
        class="rounded-md border border-white/30 px-3 py-1 text-sm hover:bg-white/10 focus-visible:outline-2 focus-visible:outline-white"
        @click="recheckSource"
      >
        重新检查
      </button>
    </div>

    <div class="relative flex flex-1 items-center justify-center overflow-auto px-6 pb-6">
      <div v-if="loadFailed" class="max-w-md text-center text-white/90">
        <p class="mb-4">{{ errorCopy("read_failed") }}</p>
        <button
          type="button"
          class="rounded-md border border-white/30 px-3 py-1.5 text-sm hover:bg-white/10"
          @click="close"
        >
          关闭
        </button>
      </div>
      <template v-else>
        <img
          v-if="imagePath"
          :src="assetSrc(imagePath)"
          alt="照片原图"
          :class="isOneToOne ? 'max-w-none' : 'max-h-full max-w-full object-contain'"
          @error="onOriginalError"
        />
        <!-- 离线回退：缩略图放大显示（不冒充原图） -->
        <img
          v-else-if="sourceOffline && current.thumb_path"
          :src="assetSrc(current.thumb_path)"
          alt="照片缩略图（原件不在线）"
          class="max-h-full max-w-full object-contain opacity-90"
        />
      </template>

      <!-- 在线原图角标：明确当前看的是原图 -->
      <span
        v-if="imagePath && detail"
        class="absolute bottom-2 right-8 rounded bg-black/60 px-2 py-1 text-xs text-white/90"
      >
        原图{{ detail.size_bytes ? ` · ${fmtSize(detail.size_bytes)}` : "" }} · 来自
        {{ detail.folder_label || detail.folder_path }}
      </span>
      <span
        v-else-if="sourceOffline"
        class="absolute bottom-2 right-8 rounded bg-black/60 px-2 py-1 text-xs text-white/70"
      >
        缩略图
      </span>
    </div>

    <footer class="flex items-center justify-center gap-3 px-6 pb-5 text-white/90">
      <button
        type="button"
        class="rounded-md border border-white/30 px-3 py-1.5 text-sm hover:bg-white/10 disabled:opacity-40"
        :disabled="index <= 0"
        aria-label="上一张"
        @click="step(-1)"
      >
        ←
      </button>
      <span class="min-w-20 text-center text-sm tabular-nums">
        {{ index + 1 }} / {{ items.length }}
      </span>
      <button
        type="button"
        class="rounded-md border border-white/30 px-3 py-1.5 text-sm hover:bg-white/10 disabled:opacity-40"
        :disabled="index >= items.length - 1"
        aria-label="下一张"
        @click="step(1)"
      >
        →
      </button>
      <button
        type="button"
        class="ml-6 rounded-md border border-white/30 px-3 py-1.5 text-sm hover:bg-white/10"
        @click="isOneToOne = !isOneToOne"
      >
        {{ isOneToOne ? "适应窗口" : "1:1" }}
      </button>
      <button
        type="button"
        class="ml-6 rounded-md border border-white/30 px-3 py-1.5 text-sm hover:bg-white/10 disabled:opacity-40"
        :disabled="reembedding"
        title="重建这张照片的语义向量"
        @click="reembed"
      >
        重新向量化
      </button>
      <button
        type="button"
        class="rounded-md border border-red-400/60 px-3 py-1.5 text-sm text-red-300 hover:bg-red-500/20 disabled:opacity-40"
        :disabled="deleting"
        @click="remove"
      >
        删除记录
      </button>
    </footer>
  </div>
</template>
