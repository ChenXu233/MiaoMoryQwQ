// 展示格式化（从 ui.ts 拆出：ui.ts 只留错误文案与 assetSrc）
export function formatBytes(n: number | null | undefined): string {
  if (!n) return "0 B";
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDate(unixSeconds: number | null | undefined): string {
  if (unixSeconds == null) return "";
  const d = new Date(unixSeconds * 1000);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleDateString("zh-CN", {
    year: "numeric",
    month: "long",
    day: "numeric",
  });
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds == null) return "估算中…";
  if (seconds < 5) return "马上就好";
  if (seconds < 60) return `预计还需 ${Math.round(seconds)} 秒`;
  const m = Math.floor(seconds / 60);
  return `预计还需 ${m} 分钟`;
}

/** 文件名（去扩展名）；无 storage_key 时回退占位 */
export function fileStem(path: string | null | undefined): string {
  if (!path) return "";
  const base = path.split(/[\\/]/).pop() ?? "";
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}
