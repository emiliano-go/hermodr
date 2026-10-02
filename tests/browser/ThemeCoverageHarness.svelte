<script lang="ts">
  import Panel from "$lib/ui/Panel.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import BooleanProps from "$lib/ui/BooleanProps.svelte";
  import ContactEditor from "$lib/contacts/ContactEditor.svelte";
  import GroupInviteLinks from "$lib/chat/GroupInviteLinks.svelte";
  import TranscriptionSettings from "$lib/settings/TranscriptionSettings.svelte";
  import TypingIndicator from "$lib/media/TypingIndicator.svelte";
  import TypingDots from "$lib/media/TypingDots.svelte";
  import ChatPreview from "$lib/chat/ChatPreview.svelte";
  import { BUILT_IN, applyTheme, type Theme } from "$lib/utils/theme.svelte";
  import { fixture } from "./theme-coverage-state.svelte";

  const themes: Theme[] = [...BUILT_IN, { ...BUILT_IN.find((t) => t.id === "light")!, id: "custom", name: "Full custom",
    tokens: { ...BUILT_IN.find((t) => t.id === "light")!.tokens, font: '"Courier New", monospace',
      accent: "#fff0aa", "accent-hover": "#ffe066", "accent-ink": "#142033" } },
    { id: "partial", name: "Partial custom", tokens: { accent: "#fff0aa", "accent-ink": "#142033" } }];
  const nav = ["controls", "contact", "flags", "invite", "transcription", "typing", "preview"].map((id) => ({ id, label: id, group: "Fixture" }));
  let themeId = $state("dark"), section = $state("controls"), dialog = $state(false), connected = $state(true), sweep = $state(false), backplate = $state("#111b21");
  const selected = $derived(themes.find((t) => t.id === themeId)!);
  $effect(() => { applyTheme(selected); if (sweep) document.documentElement.style.setProperty("--motion-scale", "0"); });
  const typers = $derived((sweep ? Array.from({ length: 360 }, (_, hue) => hue) : [0, 50, 160, 280])
    .map((hue) => ({ sender: `synthetic-${hue}`, state: "composing", label: `Synthetic ${hue}`, hue })));
  const loadLink = async () => { if (fixture.mode === "error") throw new Error("Synthetic invite error"); return "https://example.invalid/synthetic-invite"; };
</script>

<svelte:head>{@html `<style>${selected.css ?? ""}</style>`}</svelte:head>
<div class="fixture-toolbar">
  <label>Theme<select bind:value={themeId}>{#each themes as t}<option value={t.id}>{t.name}</option>{/each}</select></label>
  <label>Fixture state<select bind:value={fixture.mode}><option>ready</option><option>empty</option><option>error</option></select></label>
  <label><input type="checkbox" bind:checked={sweep} />All 360 hues</label>
  <label>Glass backplate<select bind:value={backplate}><option value="#111b21">Dark</option><option value="#ffffff">White</option></select></label>
  <button onclick={() => dialog = true}>Show dialog</button>
</div>
<Panel label="Theme component fixture" {nav} bind:section onclose={() => {}}>
  {#snippet header()}<h2>Theme component fixture</h2>{/snippet}
  {#snippet children()}
    <section data-scene={section} data-theme={themeId} data-call-count={fixture.calls.length}>
      {#key `${section}:${fixture.mode}`}
        {#if section === "controls"}
          <h2>Real button and panel controls</h2>
          <div class="samples">
            <Button variant="primary">Primary</Button><Button variant="primary" disabled>Disabled primary</Button>
            <Button variant="ghost">Ghost</Button><Button variant="ghost" danger>Danger</Button>
            <Button variant="chip" selected>Selected chip</Button><Button variant="chip">Chip</Button>
            <Button variant="send" aria-label="Synthetic send" /><Button variant="icon" icon="settings" aria-label="Synthetic icon" />
          </div>
          <label class="setting">Unchecked switch<input class="switch" type="checkbox" /></label>
          <label class="setting">Checked switch<input class="switch" type="checkbox" checked /></label>
          <label class="setting">Disabled switch<input class="switch" type="checkbox" disabled /></label>
          <label class="setting">Panel text field<input class="field" value="Synthetic field" /></label>
        {:else if section === "contact"}
          <label><input type="checkbox" bind:checked={connected} /> Connected fixture</label>
          <ContactEditor account="theme-fixture" {connected} jid="10000000001@s.whatsapp.net" onsaved={() => {}} />
        {:else if section === "flags"}
          <BooleanProps />
        {:else if section === "invite"}
          <GroupInviteLinks chat="synthetic-group" canReset onload={loadLink} onreset={loadLink} />
        {:else if section === "transcription"}
          <TranscriptionSettings autoTranscribe={false} onAutoTranscribe={async () => {}} />
        {:else if section === "preview"}
          <ChatPreview chat="synthetic@g.us" account="theme-fixture" name="Synthetic" x={480} y={130} />
        {:else}
          {#each typers as typer}
            <div class="typing-sample" data-hue={typer.hue} style:background={selected.id === "glass" ? backplate : undefined}>
              <TypingIndicator typers={[typer]} isGroup avatarOf={() => null} />
              <TypingDots hue={typer.hue} label={`Standalone ${typer.hue}`} />
            </div>
          {/each}
        {/if}
      {/key}
    </section>
  {/snippet}
</Panel>
{#if dialog}
  <ConfirmDialog label="Synthetic confirmation" title="Synthetic confirmation" hint="No native action or account data." onclose={() => dialog = false}>
    {#snippet actions()}<Button variant="ghost" danger>Danger action</Button><Button variant="ghost" onclick={() => dialog = false}>Cancel</Button>{/snippet}
  </ConfirmDialog>
{/if}

<style>
  .fixture-toolbar { position: fixed; z-index: 500; top: 8px; right: 12px; display: flex; align-items: center; gap: 12px; padding: 8px; background: var(--surface); color: var(--text); border: 1px solid var(--line-strong); }
  .fixture-toolbar label { display: flex; align-items: center; gap: 5px; }
  .fixture-toolbar button { font: inherit; color: var(--text); background: var(--raised); border: 1px solid var(--line-strong); padding: 5px; }
  .samples { display: flex; flex-wrap: wrap; gap: 12px; margin: 20px 0; }
  .typing-sample { display: flex; gap: 20px; align-items: center; margin: 18px 0; }
</style>
