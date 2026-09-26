// File helpers for staging attachments. Moved out of +page.svelte so the
// composer bar can reuse them without importing the page.
/**
 * Builds a small preview image data URL.
 *
 * The full-size bitmap is never handed to the layout. Decoding a large photo
 * into the render tree is what killed the webview: pasting a screenshot
 * crashed the renderer and took the chat with it. Drawing a downscaled copy
 * keeps the decoded surface small.
 */
export async function imagePreview(file: File): Promise<string> {
  const bitmap = await createImageBitmap(file);
  const maxSide = 480;
  const scale = Math.min(1, maxSide / Math.max(bitmap.width, bitmap.height));
  const width = Math.max(1, Math.round(bitmap.width * scale));
  const height = Math.max(1, Math.round(bitmap.height * scale));

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext("2d");
  if (!context) {
    bitmap.close();
    throw new Error("no 2d context for preview");
  }
  context.drawImage(bitmap, 0, 0, width, height);
  bitmap.close();
  return canvas.toDataURL("image/jpeg", 0.7);
}

/** Draws an SVG to a PNG whose longer side is Full HD, since WhatsApp cannot show SVGs. */
export async function rasterizeSvg(file: File): Promise<File> {
  const LONG_SIDE = 1920;
  const url = URL.createObjectURL(file);
  try {
    const image = new Image();
    image.src = url;
    await image.decode();
    // An SVG without width and height has no intrinsic size; treat it as square.
    const naturalWidth = image.naturalWidth || LONG_SIDE;
    const naturalHeight = image.naturalHeight || LONG_SIDE;
    const scale = LONG_SIDE / Math.max(naturalWidth, naturalHeight);
    const canvas = document.createElement("canvas");
    canvas.width = Math.max(1, Math.round(naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(naturalHeight * scale));
    const context = canvas.getContext("2d");
    if (!context) throw new Error("no 2d context to draw the SVG");
    context.imageSmoothingQuality = "high";
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
    if (!blob) throw new Error("the SVG could not be drawn");
    return new File([blob], `${file.name.replace(/\.svg$/i, "")}.png`, { type: "image/png" });
  } finally {
    URL.revokeObjectURL(url);
  }
}
