<script lang="ts">
  import { onMount, tick } from "svelte";
  import ChatWallpaper from "$lib/chat/ChatWallpaper.svelte";
  import ThemeLayers from "$lib/settings/ThemeLayers.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { session } from "$lib/state/session.svelte";
  import { chatPicture, customization, save, setChatPicture } from "$lib/utils/theme.svelte";

  let account = $state("synthetic-wallpaper-account");
  let chat = $state("synthetic-wallpaper-chat");
  let mounted = $state(true);
  let checks = $state<string[]>([]);
  let failed = $state("");
  let complete = $state(false);
  $effect(() => { session.activeAccount = account; chats.selectedChat = chat; });
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 15));
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  async function until(condition: () => unknown, label = "synthetic UI timeout") {
    const end = performance.now() + 5000;
    while (!condition()) { if (performance.now() > end) throw new Error(label); await wait(); }
    await tick();
  }
  const error = () => document.querySelector(".wallpaper [role=alert]")?.textContent ?? "";
  const picture = () => document.querySelector<HTMLImageElement>(".wallpaper img")?.src;
  const buttons = () => Array.from(document.querySelectorAll<HTMLButtonElement>(".wallpaper button"));
  function upload(file: File) {
    const transfer = new DataTransfer(); transfer.items.add(file);
    const input = document.querySelector<HTMLInputElement>(".wallpaper input[type=file]")!;
    input.files = transfer.files; input.dispatchEvent(new Event("change", { bubbles: true }));
  }
  async function raster(color: string) {
    const canvas = document.createElement("canvas"); canvas.width = 8; canvas.height = 4;
    const context = canvas.getContext("2d")!; context.fillStyle = color; context.fillRect(0, 0, 8, 4);
    const blob = await new Promise<Blob>((resolve) => canvas.toBlob((value) => resolve(value!), "image/png"));
    return { file: new File([blob], "synthetic.png", { type: "image/png" }), image: canvas.toDataURL("image/jpeg", 0.78) };
  }
  async function delayed(file: File, change: () => void) {
    const decode = window.createImageBitmap.bind(window);
    let release: (() => void) | undefined, finished = false, decodeError = "";
    window.createImageBitmap = ((...args: Parameters<typeof createImageBitmap>) => new Promise<ImageBitmap>((resolve, reject) => {
      release = () => { void decode(...args).then((bitmap) => { resolve(bitmap); finished = true; }, (error) => { decodeError = String(error); finished = true; reject(error); }); };
    })) as typeof createImageBitmap;
    try {
      upload(file); await until(() => release, "pending decoder was not reached");
      change(); await tick(); release!(); await until(() => finished, "released decoder did not finish"); await wait(); await wait();
      assert(!decodeError, `delayed picture failed decoding: ${decodeError}`);
    } finally { window.createImageBitmap = decode; }
  }

  onMount(() => { void (async () => {
    try {
      if (new URL(location.href).searchParams.has("reload")) {
        await until(() => picture());
        assert(picture() === await chatPicture(chat), "restart reads persisted IDB wallpaper");
        assert(customization.chatBackgrounds?.[chat]?.dim === 0.45, "restart reads persisted metadata");
        checks.push("restart loads unchanged local wallpaper and darkening"); complete = true; return;
      }
      const old = await raster("#204060"), next = await raster("#a0b040");
      await setChatPicture(chat, old.image);
      await setChatPicture("synthetic-wallpaper-other", next.image);
      customization.chatBackgrounds![chat].dim = 0.45;
      await until(() => picture() === old.image);
      assert(document.querySelector<HTMLInputElement>(".wallpaper input[type=range]")?.value === "0.45", "legacy dim kept");
      checks.push("legacy JID image and darkening remain available");

      upload(new File(["<svg xmlns='http://www.w3.org/2000/svg'></svg>"], "fake.png", { type: "image/png" }));
      await until(() => error().includes("PNG, JPEG, WebP or GIF"));
      assert(await chatPicture(chat) === old.image && picture() === old.image, "invalid image retains saved wallpaper");
      checks.push("disguised SVG rejected visibly and saved wallpaper retained");

      upload(new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "broken.png", { type: "image/png" }));
      await until(() => error().includes("could not be decoded"));
      assert(await chatPicture(chat) === old.image, "decode error retains saved wallpaper");
      checks.push("real browser decode failure remains visible");

      const put = IDBObjectStore.prototype.put;
      IDBObjectStore.prototype.put = function(value: unknown, key?: IDBValidKey) { return this.add(value, key); };
      try { upload(next.file); await until(() => error().includes("ConstraintError") && buttons().every((button) => !button.disabled)); }
      finally { IDBObjectStore.prototype.put = put; }
      assert(await chatPicture(chat) === old.image && picture() === old.image, "failed IDB request retains saved wallpaper");
      checks.push("IDB write rejection is visible and does not replace old image");

      const transaction = IDBDatabase.prototype.transaction;
      IDBDatabase.prototype.transaction = function() { throw new DOMException("synthetic transaction failure", "InvalidStateError"); };
      try { upload(next.file); await until(() => error().includes("InvalidStateError") && buttons().every((button) => !button.disabled)); }
      finally { IDBDatabase.prototype.transaction = transaction; }
      assert(await chatPicture(chat) === old.image && picture() === old.image, "transaction throw retains saved wallpaper");
      checks.push("synchronous transaction failure rejects and leaves controls usable");

      IDBObjectStore.prototype.put = function() { throw new DOMException("synthetic quota failure", "QuotaExceededError"); };
      try { upload(next.file); await until(() => error().includes("QuotaExceededError") && buttons().every((button) => !button.disabled)); }
      finally { IDBObjectStore.prototype.put = put; }
      assert(await chatPicture(chat) === old.image && picture() === old.image, "synchronous request failure retains saved wallpaper");
      checks.push("synchronous quota failure rejects visibly without replacing the image");

      const metadata = JSON.stringify(customization.chatBackgrounds);
      IDBObjectStore.prototype.put = function(value: unknown, key?: IDBValidKey) {
        const request = put.call(this, value, key);
        request.addEventListener("success", () => this.transaction.abort());
        return request;
      };
      try { upload(next.file); await until(() => error().includes("transaction aborted") && buttons().every((button) => !button.disabled)); }
      finally { IDBObjectStore.prototype.put = put; }
      assert(await chatPicture(chat) === old.image && JSON.stringify(customization.chatBackgrounds) === metadata, "aborted commit changed neither image nor metadata");
      checks.push("abort after request success fails the save and preserves image and metadata");

      const get = IDBObjectStore.prototype.get;
      IDBObjectStore.prototype.get = function(key: IDBValidKey | IDBKeyRange) {
        const request = get.call(this, key);
        request.addEventListener("success", () => this.transaction.abort());
        return request;
      };
      try {
        account = "synthetic-wallpaper-layer-account"; await tick();
        assert(!document.querySelector("style[data-chat-picture]"), "old account wallpaper CSS cleared before read finishes");
        await until(() => error().includes("Could not load the saved wallpaper"));
      } finally { IDBObjectStore.prototype.get = get; }
      account = "synthetic-wallpaper-account";
      await until(() => document.querySelector("style[data-chat-picture]")?.textContent?.includes(old.image));
      checks.push("account transition immediately clears old wallpaper CSS while IDB read fails");

      await delayed(next.file, () => { account = "synthetic-wallpaper-other-account"; });
      assert(await chatPicture(chat) === old.image, "stale account decode did not save");
      checks.push("account switch discards pending image conversion");
      await delayed(next.file, () => { chat = "synthetic-wallpaper-other"; });
      assert(await chatPicture("synthetic-wallpaper-chat") === old.image, "stale chat decode did not save");
      await until(() => picture() === next.image);
      await until(() => document.querySelector("style[data-chat-picture]")?.textContent?.includes(next.image));
      assert(!document.querySelector("style[data-chat-picture]")?.textContent?.includes(old.image), "theme layer uses selected chat only");
      checks.push("chat switch discards conversion and loads selected local image");

      chat = "synthetic-wallpaper-chat"; await until(() => picture() === old.image);
      upload(next.file); await until(() => picture() === next.image);
      assert(await chatPicture(chat) === next.image && customization.chatBackgrounds?.[chat]?.dim === 0.45, "real PNG persisted as JPEG with existing dim");
      checks.push("real PNG converts to bounded JPEG and persists through existing IDB");
      mounted = false; await tick(); mounted = true; await until(() => picture() === next.image);
      checks.push("remounted controls load saved wallpaper without uploading again");

      const remove = IDBObjectStore.prototype.delete;
      IDBObjectStore.prototype.delete = function() { throw new DOMException("synthetic remove failure", "InvalidStateError"); };
      try {
        buttons().find((button) => button.textContent?.trim() === "Remove")!.click();
        await until(() => error().includes("Could not remove wallpaper") && buttons().every((button) => !button.disabled));
      } finally { IDBObjectStore.prototype.delete = remove; }
      assert(await chatPicture(chat) === next.image && customization.chatBackgrounds?.[chat], "failed remove retained image and metadata");
      checks.push("failed Remove is visible and preserves the saved wallpaper");

      buttons().find((button) => button.textContent?.trim() === "Remove")!.click();
      await until(() => !customization.chatBackgrounds?.[chat] && !picture());
      assert(await chatPicture(chat) === undefined && await chatPicture("synthetic-wallpaper-other") === next.image, "remove targets only selected chat");
      checks.push("Remove clears selected wallpaper and preserves another chat");
      upload(next.file); await until(() => picture() === next.image);
      const dim = document.querySelector<HTMLInputElement>(".wallpaper input[type=range]")!;
      dim.value = "0.45"; dim.dispatchEvent(new Event("input", { bubbles: true }));
      save(); complete = true;
    } catch (error) { failed = String(error); }
  })(); });
</script>

<h1>Synthetic chat wallpaper</h1>
<ThemeLayers />
{#if mounted}<ChatWallpaper {account} {chat} />{/if}
<output data-complete={complete}>{checks.length} passed</output>
<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
{#if failed}<p data-failure role="alert">{failed}</p>{/if}

<style>
  :global(body) { font-family: system-ui; color: #e9edef; background: #111b21; margin: 24px; }
  :global(:root) { --muted: #8696a0; --danger: #f15c6d; --accent: #00a884; --radius-sm: 8px; }
</style>
