// 导入任务（事件 + 快照兜底 + 暂停/恢复/停止 + 选夹启动）
import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, events, type ImportProgressEvent } from "@miaomory/contracts";

const job = ref<ImportProgressEvent | null>(null);
const paused = ref(false);
const finishNotice = ref<string | null>(null);
const failedBanner = ref(false);
const importError = ref<string | null>(null);
let started = false;

async function startFolder(folder: string): Promise<boolean> {
  importError.value = null;
  const res = await commands.importFolder(folder);
  if (res.status === "ok") {
    job.value = { job_id: res.data, total: 0, done: 0, failed: 0, eta_seconds: null };
    paused.value = false;
    return true;
  }
  importError.value = res.error;
  return false;
}

export function useImportJob(onActivity?: () => void) {
  if (!started) {
    started = true;
    void events.importProgressEvent.listen((e) => {
      job.value = e.payload;
      paused.value = false;
      finishNotice.value = null;
    });
    void events.importPausedEvent.listen(() => (paused.value = true));
    void events.importResumedEvent.listen(() => (paused.value = false));
    void events.importFinishedEvent.listen((e) => {
      job.value = null;
      paused.value = false;
      failedBanner.value = e.payload.failed_count > 0;
      finishNotice.value =
        e.payload.failed_count > 0
          ? `导入完成，${e.payload.failed_count} 个文件无法读取`
          : "导入完成";
      window.setTimeout(() => (finishNotice.value = null), 6000);
      // 广播库变更（任何时刻注册的监听者都能收到，规避单例回调注册时序问题）
      window.dispatchEvent(new CustomEvent("mm-library-changed"));
      onActivity?.();
    });
  }
  return {
    job,
    paused,
    finishNotice,
    failedBanner,
    importError,
    async pickFolder() {
      const dir = await open({ directory: true, multiple: false });
      if (typeof dir !== "string") return false;
      return startFolder(dir);
    },
    async snapshot() {
      const snap = await commands.importSnapshot();
      if (snap.status === "ok" && snap.data.total > 0 && snap.data.done < snap.data.total) {
        job.value = snap.data;
      }
    },
    pause: () => void commands.pauseImport(),
    resume: () => void commands.resumeImport(),
    stop: () => void commands.stopImport(),
    dismissFailed: () => (failedBanner.value = false),
  };
}
