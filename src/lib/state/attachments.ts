import { invoke } from "$lib/ipc";
import { composer } from "./composer.svelte";
import { ui } from "./ui.svelte";

/** Media extensions that can be staged from a pasted file path. */
const PASTABLE = /\.(jpe?g|png|gif|webp|svg|mp4|mov|m4v|webm|mkv|ogg|opus|mp3|m4a|aac|wav)$/i;

function mimeForName(name: string) {
  const extension = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
  const table: Record<string, string> = {
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    png: "image/png",
    gif: "image/gif",
    webp: "image/webp",
    svg: "image/svg+xml",
    mp4: "video/mp4",
    mov: "video/mp4",
    m4v: "video/mp4",
    webm: "video/webm",
    mkv: "video/x-matroska",
    ogg: "audio/ogg",
    opus: "audio/ogg",
    mp3: "audio/mpeg",
    m4a: "audio/mp4",
    aac: "audio/mp4",
    wav: "audio/wav",
  };
  return table[extension] ?? "application/octet-stream";
}

/**
 * Reads a pasted file, either as clipboard bytes or from a copied file path.
 *
 * WebKitGTK does not put clipboard images in the paste event's
 * `clipboardData`; only the async clipboard API reaches them. Copying a file
 * in the file manager usually exposes just a `text/uri-list`, so that path is
 * read back through the shell (restricted to media extensions) instead.
 */
async function clipboardFile(): Promise<File | null> {
  try {
    const items = await navigator.clipboard?.read();
    for (const item of items ?? []) {
      const media = item.types.find(
        (t) => t.startsWith("image/") || t.startsWith("video/") || t.startsWith("audio/"),
      );
      if (media) {
        const blob = await item.getType(media);
        // send_media classifies by file extension, so a pasted item needs a
        // real one or a photo goes out as a document.
        const sub = media.split("/")[1]?.split(";")[0] || "bin";
        const extension = sub === "jpeg" ? "jpg" : sub;
        return new File([blob], `pasted.${extension}`, { type: media });
      }
      if (item.types.includes("text/uri-list")) {
        const text = await (await item.getType("text/uri-list")).text();
        const uri = text
          .split("\n")
          .map((line) => line.trim())
          .find((line) => line.length > 0);
        if (!uri?.startsWith("file://")) continue;
        // `file:///C:/x` has the pathname `/C:/x` on Windows.
        const path = decodeURIComponent(new URL(uri).pathname).replace(/^\/([A-Za-z]:)/, "$1");
        if (!PASTABLE.test(path)) continue;
        const data = await invoke<string>("read_file", { path });
        const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
        return new File([bytes], path.split("/").pop() ?? "pasted", { type: mimeForName(path) });
      }
    }
  } catch {
    // Nothing readable; the caller falls back to a hint.
  }
  return null;
}

/**
 * Stages pasted image bytes and blocks the default paste otherwise.
 *
 * Pasting a *file* (copying it in the file manager) puts a `text/uri-list` on
 * the clipboard rather than an image. Without `preventDefault` WebKit then
 * navigates the whole webview to that URI. That navigation is fatal: wry's
 * page-load handler does `webview.uri().unwrap()`, the URI is absent for such
 * a load, and the panic aborts the process. So anything file-like is
 * swallowed; only real image bytes are staged, and plain text stays native.
 */
export async function onPaste(event: ClipboardEvent) {
  const data = event.clipboardData;
  const item = data
    ? Array.from(data.items).find(
        // A file copied in Explorer arrives here as a file item of any type.
        (i) => i.type.startsWith("image/") || i.type.startsWith("video/") || i.kind === "file",
      )
    : undefined;
  const isUriList = data ? Array.from(data.types).includes("text/uri-list") : false;
  const isPlainText =
    !item && !isUriList && !!data && Array.from(data.types).includes("text/plain");
  if (isPlainText) return;
  if (item || isUriList || data) event.preventDefault();

  const file = item?.getAsFile() ?? (await clipboardFile());
  if (file) void composer.stageFile(file);
  else if (isUriList) ui.fail("Could not read that file. Try the 📎 button.");
}

/** Dropping files stages them; dropping anything else must not navigate. */
export function onDrop(event: DragEvent) {
  event.preventDefault();
  for (const file of Array.from(event.dataTransfer?.files ?? [])) void composer.stageFile(file);
}
