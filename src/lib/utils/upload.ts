import { invoke } from "$lib/utils/ipc";
import { base64Of } from "$lib/utils/files";
import { canChooseMediaQuality } from "$lib/utils/media-quality";
import type { PendingMedia } from "$lib/utils/models";

export const UPLOAD_CHUNK_BYTES = 256 * 1024;
export const INLINE_UPLOAD_BYTES = 1024 * 1024;

export function isAlbumMedia(item: PendingMedia): boolean {
  return !item.once && item.kind !== "other" && canChooseMediaQuality(item.file);
}

export function isAlbumSelection(items: readonly PendingMedia[]): boolean {
  return items.length >= 2 && items.length <= 8 && items.every(isAlbumMedia);
}

export async function cancelStagedAttachment(token: string, accountId?: string): Promise<void> {
  await invoke("cancel_upload", { token, accountId }).catch(() => {});
}

export async function stageAttachment(file: File, signal?: AbortSignal, accountId?: string): Promise<string> {
  signal?.throwIfAborted();
  const token = await invoke<string>("begin_upload", { name: file.name, size: file.size, accountId });
  try {
    for (let offset = 0; offset < file.size; offset += UPLOAD_CHUNK_BYTES) {
      signal?.throwIfAborted();
      const data = await base64Of(file.slice(offset, offset + UPLOAD_CHUNK_BYTES));
      signal?.throwIfAborted();
      await invoke("append_upload", { token, offset, data, accountId });
    }
    signal?.throwIfAborted();
    return token;
  } catch (error) {
    await cancelStagedAttachment(token, accountId);
    throw error;
  }
}

export async function sendAttachment(file: File, args: Record<string, unknown>, signal?: AbortSignal): Promise<string | null> {
  signal?.throwIfAborted();
  if (file.size <= INLINE_UPLOAD_BYTES) {
    const data = await base64Of(file);
    signal?.throwIfAborted();
    return invoke("send_media", { ...args, name: file.name, data });
  }
  const token = await stageAttachment(file, signal);
  try {
    signal?.throwIfAborted();
    return await invoke("send_media", { ...args, name: file.name, upload: token });
  } finally {
    await cancelStagedAttachment(token);
  }
}
