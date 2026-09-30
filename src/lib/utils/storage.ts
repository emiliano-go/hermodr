export type { StorageFile, ChatStorage, StorageReport, StorageCleanup } from "./wire";

export function storageSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const unit = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** unit).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
