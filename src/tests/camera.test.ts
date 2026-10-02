import assert from "node:assert/strict";
import test from "node:test";
import { cameraFailure, cameraPhoto, MAX_CAMERA_BYTES, openCamera, stopCamera } from "../lib/utils/camera.ts";

function stream() {
  let stops = 0;
  return { value: { getTracks: () => [{ stop() { stops++; } }] } as unknown as MediaStream, stops: () => stops };
}
function canvas(blob: Blob | null) {
  const draws: unknown[][] = [];
  const value = { width: 0, height: 0, getContext: () => ({ drawImage: (...args: unknown[]) => draws.push(args) }),
    toBlob: (done: BlobCallback, type: string, quality: number) => { assert.equal(type, "image/jpeg"); assert.equal(quality, 0.85); done(blob); } };
  return { value: value as unknown as HTMLCanvasElement, draws };
}

test("camera requests video only and stops a permission grant that arrives after close", async () => {
  const fake = stream(); let live = true;
  let resolve!: (value: MediaStream) => void;
  const granted = new Promise<MediaStream>((done) => { resolve = done; });
  let request: MediaStreamConstraints | undefined;
  const devices = { getUserMedia: (constraints?: MediaStreamConstraints) => { request = constraints; return granted; } };
  const pending = openCamera(() => live, devices);
  assert.equal(request?.audio, false); assert.ok(request?.video);
  assert.equal(await openCamera(() => false, { getUserMedia: async () => { assert.fail("closed camera requested permission"); } }), null);
  live = false; resolve(fake.value);
  assert.equal(await pending, null); assert.equal(fake.stops(), 1);
  const active = stream(); assert.equal(await openCamera(() => true, { getUserMedia: async () => active.value }), active.value);
  stopCamera(active.value); assert.equal(active.stops(), 1);
});

test("camera permission and device failures have useful visible messages", async () => {
  await assert.rejects(openCamera(() => true, {} as Pick<MediaDevices, "getUserMedia">), { name: "NotSupportedError" });
  await assert.rejects(openCamera(() => true, { getUserMedia: async () => { throw new DOMException("synthetic", "NotAllowedError"); } }), { name: "NotAllowedError" });
  assert.match(cameraFailure(new DOMException("synthetic", "NotAllowedError")), /Camera access was denied/);
  assert.match(cameraFailure(new DOMException("synthetic", "NotFoundError")), /No camera was found/);
  assert.match(cameraFailure(new DOMException("synthetic", "NotReadableError")), /in use by another app/);
  assert.match(cameraFailure(new DOMException("synthetic", "NotSupportedError")), /unavailable on this device/);
});

test("photo capture bounds pixels and produces an ordinary JPEG File without sending", async () => {
  const video = { videoWidth: 8000, videoHeight: 4000, readyState: 2 } as HTMLVideoElement;
  const target = canvas(new Blob([new Uint8Array([255, 216, 255])], { type: "image/jpeg" }));
  const file = await cameraPhoto(video, target.value);
  assert.equal(target.value.width, 1920); assert.equal(target.value.height, 960);
  assert.deepEqual(target.draws, [[video, 0, 0, 1920, 960]]);
  assert.equal(file.type, "image/jpeg"); assert.match(file.name, /^camera-\d+\.jpg$/); assert.equal(file.size, 3);
});

test("unready frames, failed encoders and oversized photos cannot be staged", async () => {
  const video = { videoWidth: 0, videoHeight: 0, readyState: 0 } as HTMLVideoElement;
  await assert.rejects(cameraPhoto(video, canvas(null).value), /Wait for the camera preview/);
  const ready = { videoWidth: 1920, videoHeight: 1080, readyState: 2 } as HTMLVideoElement;
  await assert.rejects(cameraPhoto(ready, canvas(null).value), /could not be captured/);
  await assert.rejects(cameraPhoto(ready, canvas(new Blob(["wrong format"], { type: "image/png" })).value), /could not be captured/);
  await assert.rejects(cameraPhoto(ready, canvas(new Blob([new Uint8Array(MAX_CAMERA_BYTES + 1)], { type: "image/jpeg" })).value), /too large/);
});
