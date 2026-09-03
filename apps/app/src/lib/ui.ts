// 文案与工具：错误码 → 中文说明（状态矩阵见 docs/spec/0001、0002）
import { convertFileSrc } from "@tauri-apps/api/core";

const ERROR_COPY: Record<string, string> = {
  decode_failed: "照片文件损坏，无法读取",
  read_failed: "文件不存在或已被移动",
  write_failed: "磁盘空间不足，写入失败",
  store_failed: "数据库读写失败",
  import_cancelled: "导入已停止",
  import_busy: "已有导入任务进行中",
  model_missing: "搜索模型尚未下载",
  model_download_failed: "模型下载失败，请检查网络",
  search_unavailable: "语义搜索暂不可用",
  unknown: "发生未知错误",
};

export function errorCopy(code: string | null | undefined): string {
  if (!code) return ERROR_COPY.unknown;
  return ERROR_COPY[code] ?? `发生错误（${code}）`;
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null) return "估算中…";
  if (seconds < 5) return "马上就好";
  if (seconds < 60) return `预计还需 ${Math.round(seconds)} 秒`;
  const m = Math.floor(seconds / 60);
  return `预计还需 ${m} 分钟`;
}

export function formatDate(unixSeconds: number | null | undefined): string {
  if (unixSeconds == null) return "";
  const d = new Date(unixSeconds * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString("zh-CN", { year: "numeric", month: "long", day: "numeric" });
}

export function assetSrc(absPath: string): string {
  return convertFileSrc(absPath);
}
