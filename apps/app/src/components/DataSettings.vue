<script setup lang="ts">
// 设置对话框：数据位置（规格/体验 0006，ADR-0011）。极简浮层，Esc 关闭，焦点圈保持在浮层内。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { commands, type DataInfo } from "@miaomory/contracts";
import { open as pickDirectory } from "@tauri-apps/plugin-dialog";
import AppButton from "./AppButton.vue";

const emit = defineEmits<{ close: [] }>();

const info = ref<DataInfo | null>(null);
const notice = ref<string | null>(null);
const error = ref<string | null>(null);
const changed = ref(false);
const card = ref<HTMLDivElement | null>(null);

onMounted(() => {
  void commands.dataInfo().then((d) => (info.value = d));
  card.value?.focus();
  window.addEventListener("keydown", onKeydown, true);
});

onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown, true));

function onKeydown(e: KeyboardEvent) {
  if (e.key === "Escape") {
    e.stopPropagation();
    emit("close");
    return;
  }
  if (e.key === "Tab" && card.value) {
    // 简易焦点圈：Tab 在浮层内循环
    const items = Array.from(
      card.value.querySelectorAll<HTMLElement>("button:not(:disabled)"),
    );
    if (items.length === 0) return;
    const active = document.activeElement as HTMLElement | null;
    const idx = items.indexOf(active ?? items[0]);
    e.preventDefault();
    const next = e.shiftKey
      ? items[(idx - 1 + items.length) % items.length]
      : items[(idx + 1) % items.length];
    next.focus();
  }
}

const modeLabel = computed(() => {
  switch (info.value?.mode) {
    case "env":
      return "开发模式 · 数据位置由环境变量指定";
    case "portable":
      return "口袋模式 · 数据随应用携带";
    case "rooted":
      return "口袋模式 · 自定义数据位置";
    default:
      return "标准模式 · 数据在文档目录";
  }
});

const modeEmphasized = computed(
  () => info.value?.mode === "portable" || info.value?.mode === "rooted",
);

async function openFolder() {
  error.value = null;
  const res = await commands.openDataFolder();
  if (res.status === "error") error.value = res.error;
}

async function changeLocation() {
  error.value = null;
  const dir = await pickDirectory({ directory: true, multiple: false });
  if (typeof dir !== "string") return;
  const res = await commands.setDataLocation(dir);
  if (res.status === "ok") {
    changed.value = true;
    notice.value = "已记录新位置，重启 MiaoMory 后生效。";
  } else {
    error.value = res.error;
  }
}
</script>

<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
    <div
      ref="card"
      role="dialog"
      aria-modal="true"
      aria-label="设置"
      tabindex="-1"
      class="w-full max-w-lg rounded-lg border border-line bg-surface p-5 shadow-lg focus-visible:outline-2 focus-visible:outline-accent"
    >
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold text-fg">设置</h2>
        <button
          type="button"
          class="rounded-md px-2 py-1 text-sm text-muted transition-colors hover:bg-bg hover:text-fg focus-visible:outline-2 focus-visible:outline-accent"
          @click="emit('close')"
        >
          关闭
        </button>
      </div>

      <section class="mt-4" aria-label="数据位置">
        <div class="flex items-center gap-2">
          <span class="text-sm font-medium text-fg">数据位置</span>
          <span
            v-if="info"
            class="rounded px-1.5 py-0.5 text-xs"
            :class="modeEmphasized ? 'bg-accent/10 font-medium text-accent' : 'bg-bg text-muted'"
          >
            {{ modeLabel }}
          </span>
        </div>
        <p class="mt-1 text-xs leading-5 text-muted">
          所有数据都保存在这台电脑上，MiaoMory 不会上传任何内容。
        </p>

        <div v-if="info" class="mt-3 space-y-2 rounded-md border border-line bg-bg p-3">
          <div>
            <div class="text-xs text-muted">数据文件夹</div>
            <div class="select-text break-all text-sm text-fg">{{ info.data_folder }}</div>
          </div>
          <div class="grid gap-1 text-xs text-muted sm:grid-cols-2">
            <div class="select-text break-all">索引：{{ info.db_path }}</div>
            <div class="select-text break-all">模型：{{ info.models_dir }}</div>
          </div>
        </div>

        <p v-if="notice" role="status" class="mt-3 text-sm text-success">{{ notice }}</p>
        <p v-if="error" role="alert" class="mt-3 text-sm text-danger">{{ error }}</p>

        <div class="mt-4 flex items-center justify-end gap-2">
          <button
            type="button"
            class="rounded-md border border-line px-3 py-1.5 text-sm text-fg transition-colors hover:bg-bg focus-visible:outline-2 focus-visible:outline-accent"
            @click="openFolder"
          >
            打开数据文件夹
          </button>
          <AppButton :disabled="changed || !(info?.can_change ?? false)" @click="changeLocation">
            更改数据位置…
          </AppButton>
        </div>
      </section>
    </div>
  </div>
</template>
