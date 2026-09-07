// 模型状态与下载（启动自动下载已由后端兜底；此处提供状态展示与手动重试）
import { ref } from "vue";
import { commands, events } from "@miaomory/contracts";

const modelReady = ref(false);
const modelFilesMissing = ref<string[]>([]);
const downloading = ref<{ file: string; received: number; total: number } | null>(null);
let started = false;

async function refresh() {
  const status = await commands.modelStatus();
  modelReady.value = status.ready && status.loaded;
  modelFilesMissing.value = status.files_missing;
}

export function useModelStatus() {
  if (!started) {
    started = true;
    void events.modelDownloadProgressEvent.listen((e) => {
      downloading.value = e.payload;
    });
    void events.modelReadyEvent.listen(() => {
      downloading.value = null;
      void refresh();
    });
    void events.embedProgressEvent.listen(() => void refresh());
    // 规格 0004 修订（PR #5）：启动发现模型缺失即自动后台下载，零点击
    void refresh().then(() => {
      if (!modelReady.value && downloading.value === null) {
        void commands.downloadModels();
      }
    });
  }
  return { modelReady, modelFilesMissing, downloading, refresh, download: () => void commands.downloadModels() };
}
