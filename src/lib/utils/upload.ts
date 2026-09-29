import { invoke } from "$lib/utils/ipc";
import { base64Of } from "$lib/utils/files";

export const UPLOAD_CHUNK_BYTES = 256 * 1024;
export const INLINE_UPLOAD_BYTES = 1024 * 1024;

export async function sendAttachment(file: File, args: Record<string, unknown>, signal?: AbortSignal): Promise<string | null> {
  signal?.throwIfAborted();
  if (file.size <= INLINE_UPLOAD_BYTES) {
    const data = await base64Of(file);
    signal?.throwIfAborted();
    return invoke("send_media", { ...args, name: file.name, data });
  }
  const token = await invoke<string>("begin_upload", { name: file.name, size: file.size });
  try {
    for (let offset = 0; offset < file.size; offset += UPLOAD_CHUNK_BYTES) {
      signal?.throwIfAborted();
      const data = await base64Of(file.slice(offset, offset + UPLOAD_CHUNK_BYTES));
      signal?.throwIfAborted();
      await invoke("append_upload", { token, offset, data });
    }
    signal?.throwIfAborted();
    return await invoke("send_media", { ...args, name: file.name, upload: token });
  } finally {
    await invoke("cancel_upload", { token }).catch(() => {});
  }
}
