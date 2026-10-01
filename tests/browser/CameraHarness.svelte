<script lang="ts">
  import { onMount, tick } from "svelte";
  import CameraCapture from "$lib/composer/CameraCapture.svelte";
  import type { CameraScope } from "$lib/utils/camera";

  let opened = $state(false), account = $state("synthetic-camera-a"), chat = $state("synthetic-chat"), generation = $state(1);
  let checks = $state<string[]>([]), failed = $state(""), complete = $state(false);
  let mode: "ready" | "denied" | "pending" = "ready";
  let grant: ((stream: MediaStream) => void) | undefined;
  let encoder: (() => void) | undefined;
  const staged: { file: File; scope: CameraScope }[] = [];
  const streams: MediaStream[] = [], requests: MediaStreamConstraints[] = [];
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 15));
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 5000;
    while (!condition()) { if (performance.now() > end) throw new Error("synthetic camera timeout"); await wait(); }
    await tick();
  }
  function syntheticStream() {
    const canvas = document.createElement("canvas"); canvas.width = 2048; canvas.height = 1024;
    const context = canvas.getContext("2d")!; context.fillStyle = "#285f83"; context.fillRect(0, 0, canvas.width, canvas.height);
    const stream = canvas.captureStream(10); streams.push(stream); return stream;
  }
  const stopped = (stream: MediaStream) => stream.getTracks().every((track) => track.readyState === "ended");
  const dialog = () => document.querySelector("dialog");
  const capture = () => Array.from(document.querySelectorAll<HTMLButtonElement>("dialog button")).find((button) => button.textContent?.includes("Take photo"));
  function click(label: string) {
    const button = Array.from(document.querySelectorAll<HTMLButtonElement>("dialog button")).find((item) => item.textContent?.trim() === label);
    assert(button, `button ${label}`); button!.click();
  }
  async function open(next: typeof mode = "ready") {
    mode = next; document.querySelector<HTMLButtonElement>("#camera-open")!.focus(); opened = true;
    await until(() => dialog()?.open);
  }
  async function ready() { await until(() => capture() && !capture()!.disabled); }
  async function dismissed() { await until(() => !dialog()); }

  onMount(() => {
    const devices = navigator.mediaDevices, original = devices.getUserMedia;
    const toBlob = HTMLCanvasElement.prototype.toBlob;
    devices.getUserMedia = (constraints: MediaStreamConstraints) => {
      requests.push(constraints);
      if (mode === "denied") return Promise.reject(new DOMException("synthetic denial", "NotAllowedError"));
      if (mode === "pending") return new Promise<MediaStream>((resolve) => { grant = resolve; });
      return Promise.resolve(syntheticStream());
    };
    function delayEncoding() {
      HTMLCanvasElement.prototype.toBlob = function(callback, type, quality) {
        const canvas = this;
        encoder = () => { toBlob.call(canvas, callback, type, quality); };
      };
    }
    void (async () => {
      try {
        await open("denied"); await until(() => document.querySelector('[role="alert"]'));
        assert(dialog()?.getAttribute("aria-labelledby") === "camera-heading", "named accessible dialog");
        assert(document.querySelector('[role="alert"]')?.textContent?.includes("Camera access was denied"), "visible permission failure");
        assert(capture()?.disabled && staged.length === 0, "denial neither captures nor stages");
        checks.push("permission rejection stays visible in an accessible dialog");

        mode = "ready"; click("Try again"); await ready();
        assert(staged.length === 0 && requests.every((request) => request.audio === false), "preview only, never microphone");
        checks.push("Retry opens a synthetic video preview without staging or audio");
        capture()!.click(); await dismissed();
        const saved = staged[0];
        assert(staged.length === 1 && saved.file.type === "image/jpeg" && saved.file.size > 0 && saved.file.size <= 4 * 1024 * 1024, "one bounded JPEG attachment");
        const pixels = await createImageBitmap(saved.file);
        assert(pixels.width === 1920 && pixels.height === 960, "real JPEG resize bounds"); pixels.close();
        assert(/^camera-\d+\.jpg$/.test(saved.file.name) && saved.scope.account === account && saved.scope.chat === chat && saved.scope.generation === generation, "normal file and captured scope");
        assert(stopped(streams[0]) && document.activeElement?.id === "camera-open", "capture stops tracks and restores focus");
        checks.push("Take photo stages one bounded JPEG with scope, stops tracks and restores focus");

        await open(); await ready(); const failedCapture = streams.at(-1)!;
        HTMLCanvasElement.prototype.toBlob = function(callback) { callback(null); };
        capture()!.click(); await until(() => document.querySelector('[role="alert"]'));
        assert(document.querySelector('[role="alert"]')?.textContent?.includes("could not be captured") && staged.length === 1, "visible encoder failure without staging");
        HTMLCanvasElement.prototype.toBlob = toBlob; click("Try again"); await ready();
        assert(stopped(failedCapture), "retry releases previous stream"); click("Cancel"); await dismissed();
        checks.push("encoder failure stays visible and Retry releases the previous stream");

        await open(); await ready(); const cancelled = streams.at(-1)!;
        dialog()!.dispatchEvent(new Event("cancel", { cancelable: true })); await dismissed();
        assert(stopped(cancelled) && staged.length === 1, "cancel stops tracks without staging");
        checks.push("Escape cancellation stops active tracks without staging");

        await open(); await ready(); const removed = streams.at(-1)!; opened = false; await dismissed();
        assert(stopped(removed) && staged.length === 1, "unmount stops active tracks");
        checks.push("parent unmount stops active tracks without staging");

        await open("pending"); await until(() => grant); click("Cancel"); await dismissed();
        const late = syntheticStream(); grant!(late); grant = undefined; await until(() => stopped(late));
        assert(staged.length === 1, "late permission grant cannot stage");
        checks.push("permission grant resolving after close is immediately stopped");

        for (const change of [() => { account = "synthetic-camera-b"; }, () => { chat = "synthetic-other-chat"; }, () => { generation++; }]) {
          await open("pending"); await until(() => grant); change(); await dismissed();
          const old = syntheticStream(); grant!(old); grant = undefined; await until(() => stopped(old));
        }
        assert(staged.length === 1, "old account/chat/generation cannot stage");
        checks.push("account, chat and generation changes reject delayed camera grants");

        await open(); await ready(); delayEncoding(); capture()!.click(); await until(() => encoder);
        click("Cancel"); await dismissed(); encoder!(); encoder = undefined;
        HTMLCanvasElement.prototype.toBlob = toBlob; await wait(); await wait();
        assert(staged.length === 1 && streams.every(stopped), "late encoding after close cannot stage");
        checks.push("JPEG encoding resolving after close cannot stage or leak tracks");

        for (const change of [() => { account = "synthetic-camera-c"; }, () => { chat = "synthetic-final-chat"; }, () => { generation++; }]) {
          await open(); await ready(); delayEncoding(); capture()!.click(); await until(() => encoder);
          change(); await dismissed(); encoder!(); encoder = undefined;
          HTMLCanvasElement.prototype.toBlob = toBlob; await wait(); await wait();
        }
        assert(staged.length === 1 && streams.every(stopped), "late encoding after scope change cannot stage");
        checks.push("account, chat and generation changes reject delayed JPEG encoding");
        complete = true;
      } catch (error) { failed = error instanceof Error ? error.stack ?? error.message : String(error); }
      finally { opened = false; devices.getUserMedia = original; HTMLCanvasElement.prototype.toBlob = toBlob; streams.forEach((stream) => stream.getTracks().forEach((track) => track.stop())); }
    })();
    return () => { devices.getUserMedia = original; HTMLCanvasElement.prototype.toBlob = toBlob; streams.forEach((stream) => stream.getTracks().forEach((track) => track.stop())); };
  });
</script>

<button id="camera-open" onclick={() => { opened = true; }}>Camera</button>
{#if opened}<CameraCapture {account} {chat} {generation} onstage={(file, scope) => { staged.push({ file, scope }); }} onclose={() => { opened = false; }} />{/if}
<ol>{#each checks as check}<li>{check}</li>{/each}</ol>
{#if failed}<pre id="fixture-error">{failed}</pre>{/if}
{#if complete}<p id="fixture-complete">PASS {checks.length}</p>{/if}

<style>
  :global(body) { margin: 20px; font-family: system-ui; background: #eef1f4; color: #18222e; --surface: #fff; --text: #18222e; --muted: #57616b; --line: #ccd4db; --shadow: #0002; --scrim: #0005; --danger: #af2525; --radius-lg: 10px; --radius-sm: 5px; }
  pre { white-space: pre-wrap; color: #af2525; }
</style>
