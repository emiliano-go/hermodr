<script lang="ts">
  import LabelDialog from "../../src/lib/labels/LabelDialog.svelte";
  import type { Label } from "../../src/lib/utils/wire";

  let account = $state("alpha");
  let request = $state(0);
  let mode = $state<"manage" | "apply">("apply");
  let open = $state(false);
  let defer = $state(false);
  let fail = $state(false);
  let waiting = $state(0);
  let calls = $state<string[]>([]);
  let labels = $state<Label[]>([{ id: "work", name: "Work", color: 0 }, { id: "personal", name: "Personal", color: 1 }]);
  let selected = $state<string[]>(["personal"]);
  let mixed = $state<string[]>(["work"]);
  const releases: (() => void)[] = [];

  async function operation(kind: string, args: unknown[], done: () => void) {
    const owner = account, scope = request, refused = fail;
    calls = [...calls, `${owner} ${kind} ${JSON.stringify(args)}`];
    if (defer) await new Promise<void>((resolve) => { releases.push(resolve); waiting = releases.length; });
    if (refused) throw new Error(`Synthetic label refusal for ${owner}`);
    if (owner === account && scope === request) done();
  }
  const apply = (id: string, labeled: boolean) => operation("apply", [id, labeled], () => {
    selected = labeled ? [...new Set([...selected, id])] : selected.filter((value) => value !== id);
    mixed = mixed.filter((value) => value !== id);
  });
  const save = (id: string, name: string, color: number) => operation("save", [id, name, color], () => {
    labels = id ? labels.map((label) => label.id === id ? { id, name, color } : label) : [...labels, { id: `new-${labels.length}`, name, color }];
  });
  const remove = (id: string) => operation("delete", [id], () => { labels = labels.filter((label) => label.id !== id); });
  function release() { const next = releases.shift(); waiting = releases.length; next?.(); }
  function switchAccount() { account = account === "alpha" ? "beta" : "alpha"; selected = ["personal"]; mixed = ["work"]; }
  function scopeKey(event: KeyboardEvent) {
    if (!event.ctrlKey || !event.altKey) return;
    const key = event.key.toLowerCase();
    if (!["a", "q", "r"].includes(key)) return;
    event.preventDefault();
    if (key === "a") switchAccount(); else if (key === "q") request++; else release();
  }
</script>

<svelte:window onkeydown={scopeKey} />
<h1>Synthetic labels</h1>
<p>Scope shortcuts: Ctrl+Alt+A switches account; Ctrl+Alt+Q changes request; Ctrl+Alt+R releases pending action.</p>
<p>Current account: {account}, request: {request}</p>
<button onclick={() => { mode = "manage"; open = true; }}>Manage labels</button>
<button onclick={() => { mode = "apply"; open = true; }}>Apply labels</button>
<label><input type="checkbox" bind:checked={defer} /> Defer label actions</label>
<label><input type="checkbox" bind:checked={fail} /> Fail label actions</label>
<button onclick={release} disabled={!waiting}>Release actions {waiting}</button>
<button onclick={switchAccount}>Switch account</button>
<button onclick={() => request++}>Bump request</button>
<button onclick={() => (labels = [])}>Empty local catalog</button>
<pre aria-label="Synthetic label operations">{calls.join("\n")}</pre>
{#if open}<LabelDialog {account} requestKey={request} {labels} {selected} {mixed} {mode} complete={false}
  onapply={apply} onsave={save} ondelete={remove} onclose={() => (open = false)} />{/if}

<style>
  :global(body) { font-family: sans-serif; max-width: 860px; margin: 20px auto; --bg: #fff; --surface: #f3f4f5; --raised-2: #e7ebee; --text: #222; --muted: #555;
    --line: #ddd; --line-strong: #aaa; --radius-sm: 5px; --radius-lg: 12px; --accent: #067a68; --scrim: #0008; --danger: #a00; --shadow: 0 5px 25px #0003; }
  button, label { margin: 5px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
