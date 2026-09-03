<script setup lang="ts">
// Lightbox：全屏预览，←/→ 切换、Esc 关闭、Space 1:1、Delete 删除（体验规格 0002 §4）
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { commands, type AssetSummary } from "@miaomory/contracts";
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

// 原图路径按需向后端取（AssetSummary 只带缩略图路径）
const imagePath = ref<string | null>(null);

async function loadImagePath() {
  imagePath.value = null;
  loadFailed.value = false;
  isOneToOne.value = false;
  const it = current.value;
  if (!it) return;
  const res = await commands.getAssetImage(it.asset_id);
  if (res.status === "ok") {
    imagePath.value = res.data;
  } else {
    loadFailed.value = true;
  }
}

watch(() => props.index, loadImagePath, { immediate: true });

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
    if (res.status === "ok") close();
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

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
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

    <div class="flex flex-1 items-center justify-center overflow-auto px-6 pb-6">
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
      <img
        v-else-if="imagePath"
        :src="assetSrc(imagePath)"
        alt="照片原图"
        :class="isOneToOne ? 'max-w-none' : 'max-h-full max-w-full object-contain'"
        @error="loadFailed = true"
      />
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
        class="ml-6 rounded-md border border-red-400/60 px-3 py-1.5 text-sm text-red-300 hover:bg-red-500/20 disabled:opacity-40"
        :disabled="deleting"
        @click="remove"
      >
        删除记录
      </button>
    </footer>
  </div>
</template>
