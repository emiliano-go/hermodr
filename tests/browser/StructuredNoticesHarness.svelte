<script lang="ts">
  import StructuredNotice from "../../src/lib/messages/StructuredNotice.svelte";
  import { fixture } from "../../src/lib/utils/wire.fixture";
  import type { Poll, StoredMessage } from "../../src/lib/utils/wire";

  let fail = $state(false);
  let unknown = $state(false);
  let votes = $state<string[]>([]);
  let poll = $state<Poll>({ id: "poll", name: "Dinner @ 6?", options: ["Pizza", "Soup"], multi: false, votes: [] });
  const base: StoredMessage = { ...fixture.message, chat: "synthetic@g.us", sender: "100@lid", from_me: false, media_kind: null, preview_url: null, reply_to_id: null };
  const pollMessage = { ...base, id: "poll", media_kind: "poll", text: "Dinner @ 6?" };
  const rows = [
    { ...base, id: "event-update", system_kind: "EVENT_UPDATED", text: "Weekly sync", system_params: ["event"] },
    { ...base, id: "event-cancel", from_me: true, system_kind: "EVENT_CANCELED", text: "Weekly sync", system_params: ["event"] },
    { ...base, id: "call-create", system_kind: "SCHEDULED_CALL_CREATED", system_params: ["Project call", "2000000000", "video"] },
    { ...base, id: "call-cancel", system_kind: "SCHEDULED_CALL_CANCEL", text: "Project call", system_params: ["call"] },
  ];
  const namer = (jid: string) => jid === "100@lid" ? "Alice" : jid;
  async function vote(options: string[]) {
    if (fail) throw new Error("Synthetic vote failure");
    votes = [...votes, options.join(",")];
    poll = { ...poll, votes: [{ voter: "@me", options }] };
  }
</script>

<h1>Synthetic structured notices</h1>
<label><input type="checkbox" bind:checked={fail} /> Fail vote</label>
<label><input type="checkbox" bind:checked={unknown} /> Missing poll details</label>
<pre aria-label="Synthetic votes">{JSON.stringify(votes)}</pre>
<section>
  <StructuredNotice message={pollMessage} poll={unknown ? undefined : poll} {namer} picture={() => null} onvote={vote} />
  {#each rows as message}<StructuredNotice {message} {namer} picture={() => null} onvote={vote} />{/each}
  <StructuredNotice message={{ ...pollMessage, id: "spoiler", spoiler: true, text: "PRIVATE SPOILER" }} {poll} {namer} picture={() => null} onvote={vote} />
  <StructuredNotice message={{ ...pollMessage, id: "deleted", deleted: true, text: "PRIVATE DELETED" }} {poll} {namer} picture={() => null} onvote={vote} />
</section>

<style>
  :global(body) { font-family: sans-serif; max-width: 800px; margin: 2rem auto; --surface: #f5f5f5; --text: #222; --muted: #555; --link: #0864b7; --accent: #067a68; --line-strong: #bbb; --radius-sm: 5px; --radius-lg: 12px; --raised-2: #ddd; --bubble: #f5f5f5; --scrim: #0008; --danger: #a00; --shadow: 0 5px 25px #0003; }
  section { display: flex; flex-direction: column; margin-top: 20px; }
  label { margin-right: 1rem; }
</style>
