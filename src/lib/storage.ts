export type StorageFile = {
  chat: string; id: string; kind: string; filename: string;
  timestamp: number; quoted: boolean; bytes: number; available: boolean;
};
export type ChatStorage = { chat: string; name: string | null; bytes: number; by_kind: Record<string, number> };
export type StorageReport = {
  database_bytes: number; attachment_bytes: number; cache_bytes: number; other_bytes: number;
  total_files: number; chats: ChatStorage[]; files: StorageFile[];
};
export type StorageCleanup = { kind: "attachment"; chat: string; id: string; quoted: boolean }
  | { kind: "chat_media"; chat: string } | { kind: "cache" };

export function storageSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const unit = Math.min(4, Math.floor(Math.log(bytes) / Math.log(1024)));
  return `${(bytes / 1024 ** unit).toLocaleString(undefined, { maximumFractionDigits: 1 })} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
