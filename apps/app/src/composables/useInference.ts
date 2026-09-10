// 推理后端信息与运行时分发（spec 0008）：状态展示、EP 切换、运行时下载/导入
import { ref } from "vue";
import { commands, events, type InferenceInfo } from "@miaomory/contracts";

const info = ref<InferenceInfo | null>(null);
const downloading = ref<{ received: number; total: number } | null>(null);
const rtError = ref<string | null>(null);
let started = false;

async function refresh() {
  // 命令异常（如清单损坏）不应变成未处理 rejection：保持现值即可
  try {
    info.value = await commands.inferenceInfo();
  } catch {
    /* 后端已在 runtime_missing 里带出原因 */
  }
}

export function useInference() {
  if (!started) {
    started = true;
    void events.runtimeDownloadProgressEvent.listen((e) => {
      downloading.value = e.payload;
    });
    void events.runtimeReadyEvent.listen(() => {
      downloading.value = null;
      rtError.value = null;
      void refresh();
    });
    // 失败是终态：清除「下载中」并带出原因（否则进度事件之后再无信号，卡永久下载中）
    void events.runtimeDownloadFailedEvent.listen((e) => {
      downloading.value = null;
      rtError.value = e.payload.error;
    });
    void refresh();
  }
  return {
    info,
    downloading,
    rtError,
    refresh,
    setEp: (ep: string) => commands.setInferenceEp(ep),
    downloadRuntime: () => {
      rtError.value = null;
      return commands.downloadRuntime("cuda");
    },
    importRuntime: (path: string) => commands.importRuntime(path),
  };
}
