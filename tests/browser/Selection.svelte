<script lang="ts">
  import MessageBubble from "$lib/messages/MessageBubble.svelte";
  import SelectionBar from "$lib/messages/SelectionBar.svelte";
  import ExpressionPicker from "$lib/composer/ExpressionPicker.svelte";
  import { messages as samples, bubbleView, bubbleApi } from "$lib/utils/theme-preview";
  import { copyMessages, deleteSelected, forwardMessages, pickedInOrder, reactMessages, starMessages } from "$lib/state/message-actions";
  import { composer } from "$lib/state/composer.svelte";
  import { chats } from "$lib/state/chats.svelte";
  import { ui } from "$lib/state/ui.svelte";
  import type { StoredMessage } from "$lib/utils/models";
  import { selectionFixture } from "./ipc";

  const rows = samples.slice(0, 3).map((message, index) => ({ ...message, chat: "selection@s.whatsapp.net", id: `pick-${index}`, timestamp: index,
    text: `Selection message ${index}`, from_me: true, sender: "synthetic@s.whatsapp.net" }));
  let visible = $state(rows);
  let starred = $state<string[]>([]);
  let output = $state("");
  let narrow = $state(false);
  const batch = $derived(pickedInOrder(ui.picking, visible));
  const allStarred = $derived(batch.length > 0 && batch.every((message) => starred.includes(message.id)));
  const emojiChat = $derived(ui.emojiFor?.messages[0]?.chat ?? "");
  const anchor = $derived(ui.emojiFor ? { x: ui.emojiFor.x, y: ui.emojiFor.y } : null);

  function pick(message: StoredMessage) {
    const next = { ...(ui.picking ?? {}) };
    if (next[message.id]) delete next[message.id]; else next[message.id] = message;
    ui.picking = next;
  }
  function clear() { ui.picking = null; ui.emojiFor = null; }
  async function star() {
    const value = !allStarred;
    ui.error = null;
    await starMessages(batch, value);
    if (!ui.error) starred = value ? batch.map((message) => message.id) : [];
  }
  async function reaction(emoji: string) {
    const selected = ui.emojiFor?.messages ?? [];
    ui.emojiFor = null;
    await reactMessages(selected, emoji);
  }
</script>

<details>
  <summary>Message selection fixture</summary>
  <button onclick={() => { clear(); visible = rows; starred = []; selectionFixture.calls = []; selectionFixture.failure = false; ui.error = null; output = ""; chats.selectedChat = rows[0].chat; }}>Reset selection fixture</button>
  <button onclick={() => { visible = visible.slice(1); }}>Evict oldest visible row</button>
  <button onclick={() => { selectionFixture.failure = !selectionFixture.failure; }}>Toggle selection failure</button>
  <button onclick={() => { output = JSON.stringify(selectionFixture.calls); }}>Show selection operations</button>
  <output aria-label="Selection operations">{output}</output>
  <label><input type="checkbox" bind:checked={narrow} />Narrow selection layout</label>
  {#if ui.error}<p role="alert">{ui.error}</p>{/if}
  <div style:max-width={narrow ? "260px" : undefined}>
  {#each visible as message, index (message.id)}
    <MessageBubble {message} vm={{ ...bubbleView(message, index, "selection"), picking: !!ui.picking, picked: !!ui.picking?.[message.id] }} api={{ ...bubbleApi, onpick: pick }} />
  {/each}
  {#if ui.picking}
    <SelectionBar count={Object.keys(ui.picking).length} {allStarred}
      onforward={() => forwardMessages(batch, ["target@s.whatsapp.net"])}
      ondelete={() => deleteSelected(false)} oncopy={() => copyMessages(batch)} onstar={star}
      onreact={(event) => { const box = (event.currentTarget as HTMLElement).getBoundingClientRect(); ui.emojiFor = { messages: batch, x: box.left, y: box.top }; }}
      oncancel={clear} />
  {/if}
  </div>
  {#if ui.emojiFor}
    <ExpressionPicker chat={emojiChat} tab="emoji" {anchor} emojiOnly
      enqueue={(task) => composer.enqueue(task)} takereply={() => ({})} onemoji={reaction}
      onsent={() => {}} onerror={(error) => { ui.error = error; }} onclose={() => { ui.emojiFor = null; }} />
  {/if}
</details>
