const MAX_FILE = 8 * 1024 * 1024;
const MAX_IMAGE = 1024 * 1024;

export async function wallpaperDataUrl(file: File): Promise<string> {
  if (!file.size || file.size > MAX_FILE) throw new Error("Choose a picture smaller than 8 MB.");
  const bytes = new Uint8Array(await file.slice(0, 12).arrayBuffer());
  const text = String.fromCharCode(...bytes);
  const raster = bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff
    || text.startsWith("\x89PNG\r\n\x1a\n") || /^GIF8[79]a/.test(text)
    || text.startsWith("RIFF") && text.slice(8, 12) === "WEBP";
  if (!raster) throw new Error("Choose a PNG, JPEG, WebP or GIF picture.");
  let bitmap: ImageBitmap;
  try { bitmap = await createImageBitmap(file); } catch { throw new Error("That picture could not be decoded."); }
  try {
    if (!bitmap.width || !bitmap.height || bitmap.width * bitmap.height > 16_000_000) throw new Error("Choose a picture with at most 16 megapixels.");
    const scale = Math.min(1, 1280 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext("2d");
    if (!context) throw new Error("Pictures are unavailable in this browser.");
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const image = canvas.toDataURL("image/jpeg", 0.78);
    if (image.length > MAX_IMAGE || !/^data:image\/jpeg;base64,\/9j\/[A-Za-z0-9+/]*={0,2}$/.test(image)) throw new Error("Choose a smaller or less detailed picture.");
    return image;
  } finally { bitmap.close(); }
}
