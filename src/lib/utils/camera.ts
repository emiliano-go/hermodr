export type CameraScope = { account: string; chat: string; generation: number };
export const MAX_CAMERA_EDGE = 1920;
export const MAX_CAMERA_BYTES = 4 * 1024 * 1024;

export function stopCamera(stream: Pick<MediaStream, "getTracks"> | null) {
  stream?.getTracks().forEach((track) => track.stop());
}

export async function openCamera(current: () => boolean,
  devices: Pick<MediaDevices, "getUserMedia"> | undefined = globalThis.navigator?.mediaDevices): Promise<MediaStream | null> {
  if (!current()) return null;
  if (!devices?.getUserMedia) throw new DOMException("Camera unavailable", "NotSupportedError");
  const stream = await devices.getUserMedia({ audio: false, video: { facingMode: { ideal: "environment" }, width: { ideal: 1280 }, height: { ideal: 720 } } });
  if (!current()) { stopCamera(stream); return null; }
  return stream;
}

export function cameraFailure(error: unknown): string {
  const name = error && typeof error === "object" && "name" in error ? error.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") return "Camera access was denied. Allow camera access in system settings and try again.";
  if (name === "NotFoundError") return "No camera was found. Connect a camera and try again.";
  if (name === "NotReadableError" || name === "AbortError") return "The camera is unavailable or in use by another app. Try again.";
  if (name === "NotSupportedError") return "Camera capture is unavailable on this device.";
  return "The camera could not be opened. Try again.";
}

export async function cameraPhoto(video: HTMLVideoElement, canvas: HTMLCanvasElement = document.createElement("canvas")): Promise<File> {
  const width = video.videoWidth, height = video.videoHeight;
  if (!Number.isInteger(width) || !Number.isInteger(height) || width <= 0 || height <= 0 || video.readyState < 2) throw new Error("Wait for the camera preview before taking a photo.");
  const scale = Math.min(1, MAX_CAMERA_EDGE / Math.max(width, height));
  canvas.width = Math.max(1, Math.round(width * scale)); canvas.height = Math.max(1, Math.round(height * scale));
  const context = canvas.getContext("2d");
  if (!context) throw new Error("Photo capture is unavailable on this device.");
  try { context.drawImage(video, 0, 0, canvas.width, canvas.height); }
  catch { throw new Error("The camera preview is not ready. Try again."); }
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.85));
  if (!blob || blob.type !== "image/jpeg" || !blob.size) throw new Error("The photo could not be captured. Try again.");
  if (blob.size > MAX_CAMERA_BYTES) throw new Error("The photo is too large. Try again.");
  return new File([blob], `camera-${Date.now()}.jpg`, { type: "image/jpeg" });
}
