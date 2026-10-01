<script lang="ts">
  import { onMount, tick } from "svelte";
  import ComposerBar from "$lib/composer/ComposerBar.svelte";
  import { soundboard } from "$lib/soundboard/state";
  import type { PickerTab } from "$lib/composer/ExpressionPicker.svelte";
  import type { PendingMedia } from "$lib/utils/models";
  import type { MediaQuality } from "$lib/utils/wire";
  import type { SlashCommandId } from "$lib/utils/slash-commands";

  let draft = $state(""), input: HTMLTextAreaElement | undefined = $state();
  let account = $state("synthetic-composer-a"), chat = $state("synthetic-group@g.us"), generation = $state(1);
  let recording = $state(false), pickerTab = $state<PickerTab | null>(null);
  let pending = $state<PendingMedia[]>([]), defaultQuality = $state<MediaQuality>("hd");
  let sends = $state<string[]>([]), created = $state<string[]>([]), mentions = $state<{ jid: string; name: string }[]>([]);
  let checks = $state<string[]>([]), failed = $state(""), complete = $state(false);
  const noop = () => {};
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 15));
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 5000;
    while (!condition()) { if (performance.now() > end) throw new Error("synthetic UI timeout"); await wait(); }
    await tick();
  }
  async function write(text: string, caret = text.length) {
    input!.focus(); input!.value = text; input!.setSelectionRange(caret, caret);
    input!.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText" }));
    await tick();
  }
  function key(value: string, extra: KeyboardEventInit = {}) {
    const event = new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true, ...extra });
    input!.dispatchEvent(event); return event;
  }
  const menu = () => document.querySelector("#slash-command-menu");
  function onkey(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); sends.push(draft); }
  }
  function command(command: SlashCommandId) {
    if (command === "mention-all" && chat.endsWith("@g.us") && !mentions.some((mention) => mention.jid === "@all")) mentions.push({ jid: "@all", name: "all" });
  }
  Object.assign(window, { composerFixture: {
    write, snapshot: () => ({ draft, sends: [...sends], created: [...created], mentions: [...mentions], caret: input?.selectionStart }),
  } });

  onMount(() => { void (async () => {
    try {
      await until(() => input);
      await write("/pll"); await until(() => menu());
      assert(document.querySelectorAll("#slash-command-menu [role=option]").length === 1, "fuzzy poll result");
      assert(key("Enter").defaultPrevented, "command Enter intercepted");
      await until(() => created.length === 1 && !menu());
      assert(created[0] === "poll" && sends.length === 0 && draft === "", "poll command chose existing flow without sending");
      checks.push("typing fuzzy slash and Enter selects poll without sending");

      await write("/"); await until(() => menu());
      key("ArrowDown"); await tick();
      assert(document.querySelector('[aria-selected="true"]')?.textContent?.includes("/event"), "arrow changed command");
      key("Enter"); await until(() => created.length === 2);
      assert(created[1] === "event" && sends.length === 0, "arrow Enter chose event");
      checks.push("Arrow and Enter select the existing event callback");

      await write("/poll"); await until(() => menu()); key("Escape"); await until(() => !menu());
      assert(draft === "/poll", "Escape preserves literal draft"); key("Enter"); await tick();
      assert(sends.length === 1 && sends[0] === "/poll", "dismissed slash can send literally");
      checks.push("Escape keeps draft and subsequent Enter reaches literal send callback");

      await write("/poll"); await until(() => menu());
      const composed = key("Enter", { isComposing: true }), legacy = key("Enter", { keyCode: 229 }), newline = key("Enter", { shiftKey: true });
      assert(!composed.defaultPrevented && !legacy.defaultPrevented && !newline.defaultPrevented, "composition/newline retain native handling");
      assert(created.length === 2 && sends.length === 1 && draft === "/poll", "IME and ShiftEnter did not choose or send");
      checks.push("IME, legacy 229 and ShiftEnter do not choose or send");

      await write("before /poll after", 12); await until(() => menu()); key("Enter");
      await until(() => draft === "before  after" && input?.selectionStart === 7);
      assert(input?.selectionEnd === 7 && sends.length === 1, "remaining draft and caret preserved");
      checks.push("token replacement preserves surrounding draft and restores insertion caret");

      await write("/mention-all"); await until(() => menu()); key("Enter");
      await until(() => draft === "@all " && input?.selectionStart === 5);
      assert(mentions.length === 1 && mentions[0].jid === "@all" && mentions[0].name === "all", "group callback records native mention metadata");
      checks.push("group mention-all inserts @all and supplies the existing mention callback metadata");

      for (const scope of ["account", "chat", "generation"]) {
        await write("/poll"); await until(() => menu());
        const before = created.length;
        if (scope === "account") account = "synthetic-composer-b";
        else if (scope === "chat") chat = "synthetic-other@g.us";
        else generation++;
        draft = `${scope} draft`; await tick();
        assert(!menu() && created.length === before && draft === `${scope} draft`, `${scope} transition preserves new scope draft`);
      }
      checks.push("account, chat and generation transitions close stale palette and preserve the new draft");

      await soundboard.add(new File(["synthetic audio"], "fixture.wav", { type: "audio/wav" }), "Synthetic Bell", null);
      document.querySelector<HTMLButtonElement>('[aria-label="Soundboard"]')!.click();
      await until(() => document.querySelector('[role="dialog"][aria-label="Soundboard"]'));
      assert(document.querySelector('[aria-label="Send Synthetic Bell"]'), "named clip visible in toolbar library");
      assert(document.querySelector('.soundboard input[type="file"]') && document.querySelector('.soundboard input[maxlength="64"]'), "file and name controls present");
      document.querySelector<HTMLButtonElement>('[aria-label="Close soundboard"]')!.click();
      checks.push("Soundboard toolbar opens the actual named clip library and editor");

      const canvas = document.createElement("canvas"); canvas.width = 4; canvas.height = 4;
      const blob = await new Promise<Blob>((resolve) => canvas.toBlob((value) => resolve(value!), "image/png"));
      const file = new File([blob], "photo.png", { type: "image/png" });
      pending = [{ id: 1, file, url: canvas.toDataURL(), kind: "image", caption: "", once: false },
        { id: 2, file: new File([blob], "second.png", { type: "image/png" }), url: canvas.toDataURL(), kind: "image", caption: "", once: false },
        { id: 3, file: new File(["synthetic document"], "document.pdf"), url: "", kind: "other", caption: "", once: false }];
      await tick();
      const quality = document.querySelector<HTMLSelectElement>('[aria-label="Upload quality for photo.png"]')!;
      assert(quality.value === "hd", "per-file dropdown follows default HD");
      quality.value = "standard"; quality.dispatchEvent(new Event("change", { bubbles: true })); await tick();
      assert(pending[0].quality === "standard" && defaultQuality === "hd", "per-file Standard does not change default");
      quality.value = "hd"; quality.dispatchEvent(new Event("change", { bubbles: true })); await tick();
      defaultQuality = "standard"; await tick();
      assert(quality.value === "hd" && pending[0].quality === "hd", "explicit HD remains despite default change");
      assert(document.querySelector<HTMLSelectElement>('[aria-label="Upload quality for second.png"]')?.value === "standard", "unset second file follows default Standard");
      assert(document.querySelectorAll("select.quality").length === 2, "documents have no media quality dropdown");
      checks.push("Standard and HD update each file while unset media follow the default quality prop");
      complete = true;
    } catch (error) { failed = String(error); }
  })(); });
</script>

<h1>Synthetic ComposerBar</h1>
<ComposerBar bind:draft bind:composerInput={input} replyingTo={null} replyAuthor="" replySnippet="" editing={null} {pending}
  bind:recording mentionMatches={[]} mentionIndex={0} onselectmention={noop} emojiToken={null} emojiMatches={[]} emojiIndex={0}
  onselectemoji={noop} bind:pickerTab selectedChat={chat} {account} {generation} {defaultQuality}
  enqueue={(task) => task(new AbortController().signal)} takereply={() => ({})} onpickeremoji={noop} onpickersent={noop}
  onpickererror={noop} onstage={noop} oncreatekind={(kind) => created.push(kind)} oninput={(event) => { draft = (event.currentTarget as HTMLTextAreaElement).value; }}
  {onkey} onsend={() => sends.push(draft)} oncancelreply={noop} oncanceledit={noop} onremove={noop} ontoggleonce={noop}
  onsendvoice={noop} onvoiceerror={noop} onreceipts={noop} ontyping={noop} receiptsHidden={false} typingHidden={false} onslashcommand={command} />
<output data-complete={complete}>{checks.length} passed</output>
<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
{#if failed}<p data-failure role="alert">{failed}</p>{/if}

<style>
  :global(body) { margin: 24px; background: #111b21; color: #e9edef; font-family: system-ui; }
  :global(:root) { --surface: #202c33; --raised: #2a3942; --text: #e9edef; --muted: #8696a0; --line: #ffffff22; --accent: #00a884; --radius: 8px; --radius-sm: 6px; }
</style>
