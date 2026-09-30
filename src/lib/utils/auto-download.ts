import type { MediaAutoDownload, MediaAutoDownloadOverrides } from "./wire";

export const MEDIA_TYPES: [keyof MediaAutoDownload, string][] = [
  ["image", "Photos and images"],
  ["video", "Videos and video notes"],
  ["audio", "Audio and voice notes"],
  ["document", "Documents"],
  ["sticker", "Stickers"],
  ["gif", "GIFs"],
];

export function emptyMediaOverrides(): MediaAutoDownloadOverrides {
  return { image: null, video: null, audio: null, document: null, sticker: null, gif: null };
}
