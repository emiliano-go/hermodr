export type { StorageFile, ChatStorage, StorageReport, StorageCleanup } from "./wire";
import { formatNumber } from "$lib/i18n/localizer";

export function storageSize(bytes: number): string {
  if (bytes === 0) return `${formatNumber(0)} B`;
  const unit = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${formatNumber(bytes / 1024 ** unit, { maximumFractionDigits: 1 })} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
