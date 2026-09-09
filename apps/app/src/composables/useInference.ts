// 推理后端信息与运行时分发（spec 0008）：状态展示、EP 切换、运行时下载/导入
import { ref } from "vue";
import { commands, events, type InferenceInfo } from "@miaomory/contracts";

const info = ref<InferenceInfo | null>(null);
const downloading = ref<{ received: number; total: number } | null>(null);
let started = false;

async function refresh() {
  info.value = await commands.inferenceInfo();
}

export function useInference() {
  if (!started) {
    started = true;
    void events.runtimeDownloadProgressEvent.listen((e) => {
      downloading.value = e.payload;
    });
    void events.runtimeReadyEvent.listen(() => {
      downloading.value = null;
      void refresh();
    });
    void refresh();
  }
  return {
    info,
    downloading,
    refresh,
    setEp: (ep: string) => commands.setInferenceEp(ep),
    downloadRuntime: () => commands.downloadRuntime("cuda"),
    importRuntime: (path: string) => commands.importRuntime(path),
  };
}
