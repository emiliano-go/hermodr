<script lang="ts">
  import { sendAttachment } from "$lib/utils/upload";
  import { uploadFixture } from "./ipc";
  let result = $state("");
  let error = $state("");
  let busy = $state(false);
  let fail = $state(false);
  let abort = $state(false);
  async function send(large: boolean) {
    busy = true; error = result = "";
    Object.assign(uploadFixture, { calls: [], chunks: [], maxChunk: 0, failChunk: fail, failSend: false });
    const controller = new AbortController();
    uploadFixture.afterChunk = abort ? () => controller.abort() : null;
    try {
      const file = new File([new Uint8Array(large ? 2 * 1024 * 1024 + 17 : 16)], "synthetic.bin");
      await sendAttachment(file, { chat: "synthetic@s" }, controller.signal);
      result = "Transfer completed";
    } catch (failure) { error = String(failure); }
    finally {
      result += `; calls: ${uploadFixture.calls.join(", ")}; maximum chunk: ${uploadFixture.maxChunk} bytes`;
      uploadFixture.afterChunk = null;
      busy = false;
    }
  }
</script>

<details>
  <summary>Bounded upload fixture</summary>
  <label><input type="checkbox" bind:checked={fail} /> Fail staging</label>
  <label><input type="checkbox" bind:checked={abort} /> Abort after first chunk</label>
  <button disabled={busy} onclick={() => send(true)}>Send synthetic 2 MiB attachment</button>
  <button disabled={busy} onclick={() => send(false)}>Send synthetic small attachment</button>
  <p role="status">{result}</p>
  {#if error}<p role="alert">{error}</p>{/if}
</details>
