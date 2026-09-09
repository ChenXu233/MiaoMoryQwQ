<script setup lang="ts">
// 设置 · 数据与存储（spec 0006 §3.0 分类三）：数据位置、来源文件夹、存储占用、失败文件。
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { open as pickDirectory } from "@tauri-apps/plugin-dialog";
import { commands, type DataInfo, type FailedItem, type StorageUsage } from "@miaomory/contracts";
import { useFolders } from "../../composables/useFolders";
import { useEmbedJob } from "../../composables/useEmbedJob";
import { formatBytes } from "../../lib/format";

const { folders, recheck, relocate } = useFolders();
const { refresh: refreshEmbed } = useEmbedJob();

const info = ref<DataInfo | null>(null);
const usage = ref<StorageUsage | null>(null);
const failed = ref<FailedItem[] | null>(null);
const notice = ref<string | null>(null);
const error = ref<string | null>(null);
const changed = ref(false);
const folderNotice = ref<string | null>(null);
const folderError = ref<string | null>(null);

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

async function loadStatic() {
  info.value = await commands.dataInfo();
  const u = await commands.storageUsage();
  if (u.status === "ok") usage.value = u.data;
  const f = await commands.listFailedItems();
  failed.value = f.status === "ok" ? f.data : [];
}

async function openDataFolder() {
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

async function recheckFolder(folderId: number, label: string) {
  folderError.value = null;
  const f = await recheck(folderId);
  if (f) {
    folderNotice.value =
      f.status === "online" ? `「${label}」已恢复在线。` : `「${label}」仍不可访问。`;
  } else {
    folderNotice.value = null;
    folderError.value = "重检失败";
  }
}

async function relocateFolder(folderId: number, label: string) {
  folderError.value = null;
  const dir = await pickDirectory({ directory: true, multiple: false });
  if (typeof dir !== "string") return;
  const res = await relocate(folderId, dir);
  if (typeof res === "object") {
    folderNotice.value = `已重新指定「${label}」的位置。`;
  } else {
    folderError.value = res;
  }
}

// 重新向量化（2026-09-10 裁决：设置页集中入口）
async function reembedFolder(folderId: number, label: string) {
  folderError.value = null;
  const ok = await ask(
    `重建「${label}」的语义向量？重建期间这个工作区的语义搜索会暂时不可用，直到重建完成。`,
    { title: "重新向量化", kind: "warning", okLabel: "重建", cancelLabel: "取消" },
  );
  if (!ok) return;
  const res = await commands.reindexFolder(folderId);
  if (res.status === "ok") {
    await refreshEmbed();
    folderNotice.value = `已开始重建（${res.data} 张），进度见右下角索引卡。`;
  } else {
    folderError.value = res.error;
  }
}

async function reembedAll() {
  folderError.value = null;
  const ok = await ask(
    "重建全部语义向量？所有照片的语义搜索在重建完成前会暂时不可用。",
    { title: "重建全部向量", kind: "warning", okLabel: "重建", cancelLabel: "取消" },
  );
  if (!ok) return;
  const res = await commands.reindexAll();
  if (res.status === "ok") {
    await refreshEmbed();
    folderNotice.value = `已开始重建全部向量（${res.data} 张），进度见右下角索引卡。`;
  } else {
    folderError.value = res.error;
  }
}

function statusText(s: string): string {
  return s === "online" ? "在线" : s === "offline" ? "离线" : "找不到路径";
}

onMounted(() => void loadStatic());
</script>

<template>
  <!-- 数据位置 -->
  <div class="card">
    <div class="set-row" style="padding-top: 0">
      <div class="set-info">
        <div class="set-t">数据位置</div>
        <div class="set-d">所有数据都保存在这台电脑上，MiaoMory 不会上传任何内容。</div>
      </div>
      <span v-if="info" class="mode-badge">{{ modeLabel }}</span>
    </div>
    <div v-if="info" class="path-rows">
      <div class="prow">
        <span class="pinfo">
          <span class="pl">数据文件夹</span>
          <span class="pv">{{ info.data_folder }}</span>
        </span>
      </div>
      <div class="prow">
        <span class="pinfo">
          <span class="pl">索引</span>
          <span class="pv">{{ info.db_path }}</span>
        </span>
      </div>
      <div class="prow">
        <span class="pinfo">
          <span class="pl">模型</span>
          <span class="pv">{{ info.models_dir }}</span>
        </span>
      </div>
    </div>
    <p v-if="notice" role="status" class="set-d" style="color: var(--mm-success); margin-top: 10px">
      {{ notice }}
    </p>
    <p v-if="error" role="alert" class="set-d" style="color: var(--mm-danger); margin-top: 10px">
      {{ error }}
    </p>
    <div class="ops">
      <span class="hint">更改数据位置后重启 MiaoMory 生效</span>
      <button class="btn-glass btn-outline-glass" @click="openDataFolder">打开数据文件夹</button>
      <button
        class="btn-glass btn-primary"
        :disabled="changed || !(info?.can_change ?? false)"
        style="disabled: opacity 0.5"
        @click="changeLocation"
      >
        更改数据位置…
      </button>
    </div>
  </div>

  <!-- 来源文件夹 -->
  <div class="card">
    <div class="set-row" style="padding-top: 0">
      <div class="set-info">
        <div class="set-t">来源文件夹</div>
        <div class="set-d">文件夹不在线时，照片仍可浏览和搜索（用缩略图），只是看不到原图。</div>
      </div>
    </div>
    <div v-if="folders.length" class="path-rows">
      <div v-for="f in folders" :key="f.folder_id" class="prow">
        <span
          class="st-dot"
          :class="{ ok: f.status === 'online', no: f.status === 'missing', dl: f.status === 'offline' }"
          :aria-label="`文件夹${statusText(f.status)}`"
          :style="f.status === 'offline' ? 'background: var(--mm-muted)' : undefined"
        />
        <span class="pinfo">
          <span class="pv">{{ f.label || f.path }}</span>
          <span class="pl num">{{ f.asset_count }} 张 · {{ statusText(f.status) }}</span>
        </span>
        <button
          class="btn-glass btn-outline-glass"
          style="padding: 5px 12px; font-size: 12px"
          @click="recheckFolder(f.folder_id, f.label || f.path)"
        >
          重新检查
        </button>
        <button
          v-if="f.status !== 'online'"
          class="btn-glass btn-outline-glass"
          style="padding: 5px 12px; font-size: 12px"
          @click="relocateFolder(f.folder_id, f.label || f.path)"
        >
          重新指定位置…
        </button>
        <button
          v-if="f.asset_count > 0"
          class="btn-glass btn-outline-glass"
          style="padding: 5px 12px; font-size: 12px"
          title="清空该工作区的语义向量并重建"
          @click="reembedFolder(f.folder_id, f.label || f.path)"
        >
          重建向量
        </button>
      </div>
    </div>
    <p v-else class="set-d" style="margin-top: 10px">还没有导入过文件夹。</p>
    <p v-if="folderNotice" role="status" class="set-d" style="color: var(--mm-success); margin-top: 8px">
      {{ folderNotice }}
    </p>
    <p v-if="folderError" role="alert" class="set-d" style="color: var(--mm-danger); margin-top: 8px">
      {{ folderError }}
    </p>
  </div>

  <!-- 存储占用（裁定 25：只分析不清理） -->
  <div class="card">
    <div class="set-row" style="padding-top: 0">
      <div class="set-info">
        <div class="set-t">存储占用</div>
        <div class="set-d">索引与缩略图只为本机服务。</div>
      </div>
    </div>
    <div v-if="usage" class="path-rows">
      <div class="prow">
        <span class="pinfo">
          <span class="pl">照片记录</span>
          <span class="pv num">
            {{ usage.assets_count }} 条（已建索引 {{ usage.embedded_count }}）
          </span>
        </span>
      </div>
      <div class="prow">
        <span class="pinfo">
          <span class="pl">索引数据库</span>
          <span class="pv num">{{ formatBytes((usage.db_bytes ?? 0) + (usage.wal_bytes ?? 0)) }}</span>
        </span>
      </div>
      <div class="prow">
        <span class="pinfo">
          <span class="pl">缩略图</span>
          <span class="pv num">{{ formatBytes(usage.thumbs_bytes ?? 0) }}</span>
        </span>
      </div>
      <div class="prow">
        <span class="pinfo">
          <span class="pl">语义模型</span>
          <span class="pv num">{{ formatBytes(usage.models_bytes ?? 0) }}</span>
        </span>
      </div>
      <div v-for="ix in usage.per_index" :key="ix.index_id" class="prow">
        <span class="pinfo">
          <span class="pl">
            索引：{{ ix.display }}{{ ix.status !== "active" ? "（已停用）" : "" }}
          </span>
          <span class="pv num">
            {{ ix.count }} 条 · {{ formatBytes(ix.approx_bytes ?? 0) }}
          </span>
        </span>
      </div>
    </div>
    <div class="ops">
      <span class="hint">重建后语义搜索暂时不可用，完成后自动恢复</span>
      <button
        class="btn-glass btn-outline-glass"
        :disabled="!usage || usage.embedded_count === 0"
        @click="reembedAll"
      >
        重建全部向量
      </button>
    </div>
  </div>

  <!-- 无法导入的文件 -->
  <div class="card">
    <div class="set-row" style="padding-top: 0">
      <div class="set-info">
        <div class="set-t">无法导入的文件</div>
        <div class="set-d">这些文件的原文件没有被动过。</div>
      </div>
    </div>
    <div
      v-if="failed && failed.length"
      class="path-rows"
      style="max-height: 200px; overflow-y: auto"
    >
      <div v-for="it in failed" :key="it.asset_id" class="prow">
        <span class="pinfo">
          <span class="pv" style="font-size: 12px">{{ it.path }}</span>
          <span v-if="it.error_code" class="pl" style="color: var(--mm-danger)">
            {{ it.error_code }}
          </span>
        </span>
      </div>
    </div>
    <p v-else-if="failed" class="set-d" style="margin-top: 10px">没有失败记录。</p>
  </div>
</template>
