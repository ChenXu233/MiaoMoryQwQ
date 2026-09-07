<script setup lang="ts">
// 设置对话框：数据位置（规格/体验 0006，ADR-0011）+ 来源文件夹/存储占用/失败文件（P5 切片 A1）。
// 极简浮层，Esc 关闭，焦点圈保持在浮层内。
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import {
  commands,
  events,
  type DataInfo,
  type FolderInfo,
  type FailedItem,
  type StorageUsage,
} from "@miaomory/contracts";
import { open as pickDirectory } from "@tauri-apps/plugin-dialog";
import AppButton from "./AppButton.vue";

const emit = defineEmits<{ close: [] }>();

const info = ref<DataInfo | null>(null);
const notice = ref<string | null>(null);
const error = ref<string | null>(null);
const changed = ref(false);
const card = ref<HTMLDivElement | null>(null);

// —— 文件夹（工作区）——
const folders = ref<FolderInfo[]>([]);
const folderNotice = ref<string | null>(null);
const folderError = ref<string | null>(null);
// —— 存储占用（裁决 25：只分析不清理）——
const usage = ref<StorageUsage | null>(null);
// —— 失败文件 ——
const failed = ref<FailedItem[] | null>(null);

function fmtBytes(n: number): string {
  if (!n) return "0 B";
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

async function loadFolders() {
  const res = await commands.listFolders();
  if (res.status === "ok") folders.value = res.data;
}

async function loadUsage() {
  const res = await commands.storageUsage();
  if (res.status === "ok") usage.value = res.data;
}

async function loadFailedItems() {
  const res = await commands.listFailedItems();
  failed.value = res.status === "ok" ? res.data : [];
}

let unlisteners: Array<() => void> = [];

onMounted(async () => {
  void commands.dataInfo().then((d) => (info.value = d));
  void loadFolders();
  void loadUsage();
  void loadFailedItems();
  card.value?.focus();
  window.addEventListener("keydown", onKeydown, true);
  unlisteners.push(await events.folderStatusChangedEvent.listen(() => void loadFolders()));
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown, true);
  unlisteners.forEach((off) => off());
});

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

// —— 文件夹操作 ——
function statusText(s: string): string {
  return s === "online" ? "在线" : s === "offline" ? "离线" : "找不到路径";
}

async function recheck(f: FolderInfo) {
  folderError.value = null;
  const res = await commands.recheckFolder(f.folder_id);
  if (res.status === "ok") {
    folderNotice.value =
      res.data.status === "online"
        ? `「${res.data.label || res.data.path}」已恢复在线。`
        : `「${res.data.label || res.data.path}」仍不可访问。`;
    void loadFolders();
  } else {
    folderError.value = res.error;
  }
}

async function relocate(f: FolderInfo) {
  folderError.value = null;
  const dir = await pickDirectory({ directory: true, multiple: false });
  if (typeof dir !== "string") return;
  const res = await commands.relocateFolder(f.folder_id, dir);
  if (res.status === "ok") {
    folderNotice.value = `已重新指定「${f.label || f.path}」的位置。`;
    void loadFolders();
  } else {
    folderError.value = res.error;
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

      <!-- 来源文件夹（工作区）：状态点 + 重检 + 重指 -->
      <section class="mt-5 border-t border-line pt-4" aria-label="来源文件夹">
        <div class="text-sm font-medium text-fg">来源文件夹</div>
        <p class="mt-1 text-xs leading-5 text-muted">
          文件夹不在线时，照片仍可浏览和搜索（用缩略图），只是看不到原图。
        </p>
        <div v-if="folders.length" class="mt-3 space-y-1">
          <div
            v-for="f in folders"
            :key="f.folder_id"
            class="flex items-center gap-2 rounded-md border border-line bg-bg px-3 py-2"
          >
            <span
              class="h-2 w-2 shrink-0 rounded-full"
              :class="{
                'bg-success': f.status === 'online',
                'bg-muted': f.status === 'offline',
                'bg-danger': f.status === 'missing',
              }"
              :aria-label="`文件夹${statusText(f.status)}`"
            />
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm text-fg">{{ f.label || f.path }}</span>
              <span class="block truncate text-xs text-muted">
                {{ f.asset_count }} 张 · {{ statusText(f.status) }}
              </span>
            </span>
            <button
              type="button"
              class="rounded-md border border-line px-2 py-1 text-xs text-fg hover:bg-surface focus-visible:outline-2 focus-visible:outline-accent"
              @click="recheck(f)"
            >
              重新检查
            </button>
            <button
              v-if="f.status !== 'online'"
              type="button"
              class="rounded-md border border-line px-2 py-1 text-xs text-fg hover:bg-surface focus-visible:outline-2 focus-visible:outline-accent"
              @click="relocate(f)"
            >
              重新指定位置…
            </button>
          </div>
        </div>
        <p v-else class="mt-3 text-xs text-muted">还没有导入过文件夹。</p>
        <p v-if="folderNotice" role="status" class="mt-2 text-sm text-success">{{ folderNotice }}</p>
        <p v-if="folderError" role="alert" class="mt-2 text-sm text-danger">{{ folderError }}</p>
      </section>

      <!-- 存储占用：只分析，不清理（裁决 25） -->
      <section class="mt-5 border-t border-line pt-4" aria-label="存储占用">
        <div class="text-sm font-medium text-fg">存储占用</div>
        <div v-if="usage" class="mt-3 grid gap-1 rounded-md border border-line bg-bg p-3 text-xs text-muted sm:grid-cols-2">
          <div>照片记录：{{ usage.assets_count }} 条（已建索引 {{ usage.embedded_count }}）</div>
          <div class="select-text">索引数据库：{{ fmtBytes((usage.db_bytes ?? 0) + (usage.wal_bytes ?? 0)) }}</div>
          <div class="select-text">缩略图：{{ fmtBytes(usage.thumbs_bytes ?? 0) }}</div>
          <div class="select-text">语义模型：{{ fmtBytes(usage.models_bytes ?? 0) }}</div>
        </div>
      </section>

      <!-- 失败文件：常驻入口（不再只在导入完成横幅里一闪而过） -->
      <section class="mt-5 border-t border-line pt-4" aria-label="失败文件">
        <div class="text-sm font-medium text-fg">无法导入的文件</div>
        <div v-if="failed && failed.length" class="mt-3 max-h-40 space-y-1 overflow-y-auto rounded-md border border-line bg-bg p-3">
          <div v-for="it in failed" :key="it.asset_id" class="truncate text-xs text-muted select-text">
            {{ it.path }}
            <span v-if="it.error_code" class="text-danger">（{{ it.error_code }}）</span>
          </div>
        </div>
        <p v-else-if="failed" class="mt-3 text-xs text-muted">没有失败记录。</p>
      </section>
    </div>
  </div>
</template>
