<script setup lang="ts">
// 设置 · 模型与推理（spec 0006 §3.0 分类二）：语义模型（状态/下载/本地导入）
// + 推理加速（EP 选择 / 运行时下载与导入，spec 0008）。
import { computed, onMounted, ref } from "vue";
import { open as pickDirectory, open as pickFile } from "@tauri-apps/plugin-dialog";
import { commands, type InferenceInfo } from "@miaomory/contracts";
import { useModelStatus } from "../../composables/useModelStatus";
import { useInference } from "../../composables/useInference";

const model = useModelStatus();
const { info: infInfo, downloading: rtDownloading, refresh: infRefresh, setEp, downloadRuntime, importRuntime } =
  useInference();

const error = ref<string | null>(null);
const epNotice = ref<string | null>(null);
const epError = ref<string | null>(null);
const importReport = ref<string | null>(null);

const modelState = computed<"missing" | "downloading" | "ready">(() => {
  if (model.modelReady.value) return "ready";
  if (model.downloading.value) return "downloading";
  return "missing";
});

const epNames: Record<string, string> = {
  cpu: "CPU",
  directml: "DirectML",
  cuda: "CUDA",
};

const selectedHint = computed(() => {
  const cur = infInfo.value?.options.find((o) => o.kind === infInfo.value?.current_ep);
  return cur?.hint ?? "";
});

const showCudaDownload = computed(
  () => infInfo.value?.current_ep === "cuda" && !infInfo.value?.runtime_ready,
);

async function chooseEp(kind: string) {
  epError.value = null;
  epNotice.value = null;
  const res = await setEp(kind);
  if (res.status === "ok") {
    epNotice.value = `已选择 ${epNames[kind] ?? kind}，重启 MiaoMory 后生效。`;
    void infRefresh();
  } else {
    epError.value = res.error;
  }
}

async function importRuntimeZip() {
  epError.value = null;
  const file = await pickFile({
    multiple: false,
    filters: [{ name: "运行时包", extensions: ["zip"] }],
  });
  if (typeof file !== "string") return;
  const res = await importRuntime(file);
  if (res.status === "ok") {
    epNotice.value =
      res.data === true
        ? "运行时包已导入并通过校验。"
        : "运行时包已导入（未能对照清单校验，来源：本地文件）。";
    void infRefresh();
  } else {
    epError.value = res.error;
  }
}

async function importModels() {
  error.value = null;
  importReport.value = null;
  const dir = await pickDirectory({ directory: true, multiple: false });
  if (typeof dir !== "string") return;
  const res = await commands.importModels(dir);
  if (res.status === "error") {
    error.value = res.error;
    return;
  }
  const { imported, skipped, mismatched } = res.data;
  if (mismatched.length) {
    importReport.value = `以下文件未通过校验，未做任何改动：${mismatched
      .map((m) => `${m.name}（${m.reason}）`)
      .join("、")}`;
  } else if (imported > 0) {
    importReport.value = `已导入 ${imported} 个文件${skipped ? `（跳过 ${skipped} 个已存在）` : ""}，索引已恢复。`;
  } else {
    importReport.value = "模型已就绪，无需导入。";
  }
  void model.refresh();
}

onMounted(() => void infRefresh());

function epLabel(o: InferenceInfo["options"][number]): string {
  return `${epNames[o.kind] ?? o.kind}${o.recommended ? " ·推荐" : ""}`;
}
</script>

<template>
  <!-- 语义模型 -->
  <div class="card">
    <div class="set-row" style="padding-top: 0">
      <span
        class="st-dot"
        :class="{ ok: modelState === 'ready', dl: modelState === 'downloading', no: modelState === 'missing' }"
      />
      <div class="set-info">
        <div class="set-t">
          {{
            modelState === "ready"
              ? "语义模型已就绪"
              : modelState === "downloading"
                ? "正在下载语义模型"
                : "语义模型未就绪"
          }}
        </div>
        <div class="set-d">导入索引与搜索共用一套中文图文语义模型（约 200MB，仅一次）。</div>
      </div>
      <button
        v-if="modelState === 'missing'"
        class="btn-glass btn-primary"
        @click="model.download()"
      >
        重试下载
      </button>
    </div>
    <div v-if="model.downloading.value" class="mbar">
      <i
        :style="{
          width:
            model.downloading.value.total > 0
              ? `${Math.round((model.downloading.value.received / model.downloading.value.total) * 100)}%`
              : '0%',
        }"
      />
    </div>
    <div v-if="model.downloading.value" class="set-d num">
      {{ model.downloading.value.file }}：
      {{ Math.round(model.downloading.value.received / 1e6) }} /
      {{ Math.round(model.downloading.value.total / 1e6) }} MB（已下载部分不会丢失）
    </div>
    <p class="set-d" style="margin: 10px 0 0">
      🔒 模型只在本机运行，照片不会上传；下载可中断，已下载部分不会丢失。
    </p>
    <div class="ops">
      <span v-if="importReport" role="status" class="hint" style="color: var(--mm-success)">
        {{ importReport }}
      </span>
      <span v-else-if="error" role="alert" class="hint" style="color: var(--mm-danger)">
        {{ error }}
      </span>
      <button class="btn-glass btn-outline-glass" @click="importModels">从本地导入模型…</button>
    </div>
  </div>

    <!-- 推理加速 -->
    <div class="card">
      <div class="set-row" style="padding-top: 0">
        <div class="set-info">
          <div class="set-t">推理加速</div>
          <div class="set-d">建索引与语义搜索使用的计算后端；默认 CPU，所有机器可用。</div>
        </div>
        <span v-if="infInfo" class="mode-badge">
          当前生效：{{ epNames[infInfo.effective_ep] ?? infInfo.effective_ep }}
        </span>
      </div>

      <!-- 运行时整体不可用（比降级更严重：语义搜索与建索引停用） -->
      <p
        v-if="infInfo?.runtime_missing"
        role="alert"
        class="set-d"
        style="color: var(--mm-danger); margin: 0 0 10px; font-weight: 550"
      >
        ⚠ 推理运行时不可用：{{ infInfo.runtime_missing }}。语义搜索与建索引自本次启动起停用，重装应用或恢复运行时文件后重启即可恢复。
      </p>

      <p
        v-if="infInfo?.degraded_reason"
        role="alert"
        class="set-d"
        style="color: var(--mm-danger); margin: 0 0 10px"
      >
        {{ infInfo.degraded_reason }}
      </p>

    <div class="seg" role="group" aria-label="推理后端">
      <button
        v-for="o in infInfo?.options ?? []"
        :key="o.kind"
        :class="{ on: infInfo?.current_ep === o.kind }"
        :disabled="infInfo?.current_ep === o.kind"
        @click="chooseEp(o.kind)"
      >
        {{ epLabel(o) }}
      </button>
    </div>
    <p class="set-d" style="margin: 8px 0 0">{{ selectedHint }}</p>

    <!-- CUDA 运行时下载 / 导入 -->
    <template v-if="showCudaDownload">
      <div v-if="rtDownloading" class="mbar" style="margin-top: 10px">
        <i
          :style="{
            width:
              rtDownloading.total > 0
                ? `${Math.round((rtDownloading.received / rtDownloading.total) * 100)}%`
                : '0%',
          }"
        />
      </div>
      <div v-if="rtDownloading" class="set-d num">
        正在下载运行时包：{{ Math.round(rtDownloading.received / 1e6) }} /
        {{ Math.round(rtDownloading.total / 1e6) }} MB（已下载部分不会丢失）
      </div>
      <div v-else class="ops" style="margin-top: 10px">
        <span class="hint">CUDA 运行时包（约 350MB，下载一次）</span>
        <button class="btn-glass btn-primary" @click="downloadRuntime()">下载运行时包</button>
        <button class="btn-glass btn-outline-glass" @click="importRuntimeZip">从本地导入…</button>
      </div>
    </template>

    <p v-if="epNotice" role="status" class="set-d" style="color: var(--mm-success); margin: 10px 0 0">
      {{ epNotice }}
    </p>
    <p v-if="epError" role="alert" class="set-d" style="color: var(--mm-danger); margin: 10px 0 0">
      {{ epError }}
    </p>
    <p class="set-d" style="margin: 10px 0 0">
      切换后端需要重启 MiaoMory；推理始终只在本机进行。
    </p>
  </div>
</template>
