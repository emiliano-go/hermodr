<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import ThemePreview from "$lib/ThemePreview.svelte";
  import {
    TOKENS,
    activeTheme,
    allThemes,
    customization,
    duplicate,
    isBuiltIn,
    newId,
    type Theme,
  } from "$lib/theme.svelte";

  const groups = [...new Set(TOKENS.map((t) => t.group))];
  const ACCENTS = ["#00a884", "#53bdeb", "#7f66ff", "#e26ab6", "#f0b232", "#f15c6d"];
  const MOTION: [string, string][] = [
    ["0", "Off"],
    ["1", "Normal"],
    ["2", "Slow"],
  ];

  const SCENES = [
    ["chat", "Chat"],
    ["signin", "Sign-in"],
    ["dialog", "Dialogs & menus"],
  ] as const;
  let scene = $state<(typeof SCENES)[number][0]>("chat");
  /** Keeps the preview in view while the controls below scroll. */
  let pinned = $state(true);

  let importing = $state(false);
  let importText = $state("");
  let importError = $state<string | null>(null);
  let copied = $state(false);

  const theme = $derived(activeTheme());
  const builtIn = $derived(isBuiltIn(theme));
  const t = $derived(theme.tokens);

  /** Built-in themes stay pristine: the first edit forks a copy. */
  function editable(): Theme {
    return builtIn ? duplicate(theme) : theme;
  }

  function setTokens(values: Record<string, string>) {
    Object.assign(editable().tokens, values);
  }

  /** `[r, g, b, alpha]` of a hex or rgb()/rgba() value; null for anything else. */
  function rgba(value: string | undefined): [number, number, number, number] | null {
    const v = (value ?? "").trim();
    const h = /^#([0-9a-f]{6})$/i.exec(v);
    if (h) {
      const n = Number.parseInt(h[1], 16);
      return [n >> 16, (n >> 8) & 255, n & 255, 1];
    }
    const m = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/i.exec(v);
    return m ? [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]] : null;
  }

  function hex(value: string | undefined) {
    const c = rgba(value) ?? [0, 0, 0, 1];
    return "#" + c.slice(0, 3).map((v) => v.toString(16).padStart(2, "0")).join("");
  }

  /** A picked colour that keeps the old value's transparency. */
  function recolor(old: string, picked: string) {
    const alpha = rgba(old)?.[3] ?? 1;
    const [r, g, b] = rgba(picked)!;
    return alpha === 1 ? picked : `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  function tint(color: string, alpha: number) {
    const [r, g, b] = rgba(color)!;
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }

  /** Moves a colour towards white (`amount` > 0) or black (< 0). */
  function shade(color: string, amount: number) {
    const target = amount > 0 ? 255 : 0;
    const c = rgba(color)!.slice(0, 3).map((v) => Math.round(v + (target - v) * Math.abs(amount)));
    return hex(`rgba(${c.join(", ")})`);
  }

  /** Sets the accent and everything derived from it, so the palette stays coherent. */
  function setAccent(color: string) {
    const light = t.scheme === "light";
    setTokens({
      accent: color,
      "accent-hover": shade(color, light ? -0.2 : 0.2),
      "accent-text": light ? shade(color, -0.2) : color,
      "accent-soft": tint(color, 0.18),
      replying: color,
      "replying-soft": tint(color, 0.16),
      "jump-soft": tint(color, 0.3),
    });
  }

  function px(key: string, fallback: number) {
    const n = Number.parseFloat(t[key] ?? "");
    return Number.isFinite(n) ? n : fallback;
  }

  function setRoundness(r: number) {
    setTokens({ "radius-sm": `${Math.max(0, r - 0.5)}px`, radius: `${r}px`, "radius-lg": `${r + 2}px` });
  }

  function removeTheme(id: string) {
    customization.themes = customization.themes.filter((th) => th.id !== id);
    if (customization.theme === id) customization.theme = "dark";
  }

  async function exportTheme() {
    await navigator.clipboard.writeText(JSON.stringify({ name: theme.name, tokens: theme.tokens }, null, 2));
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function importTheme() {
    importError = null;
    try {
      const parsed = JSON.parse(importText);
      if (typeof parsed?.tokens !== "object" || parsed.tokens === null) throw new Error();
      const tokens: Record<string, string> = {};
      for (const [key, value] of Object.entries(parsed.tokens)) {
        if (typeof value === "string") tokens[key] = value;
      }
      const imported = { id: newId("theme"), name: String(parsed.name ?? "Imported"), tokens };
      customization.themes.push(imported);
      customization.theme = imported.id;
      importText = "";
      importing = false;
    } catch {
      importError = "That is not a theme. Paste the JSON that Export copies.";
    }
  }

  function addExtension() {
    customization.extensions.push({
      id: newId("ext"),
      name: `Extension ${customization.extensions.length + 1}`,
      css: "",
      enabled: true,
    });
  }
</script>

<section class="block">
  <h3>Theme</h3>
  <div class="gallery">
    {#each allThemes() as option (option.id)}
      {@const o = option.tokens}
      <button
        class="theme-card"
        class:active={option.id === theme.id}
        aria-pressed={option.id === theme.id}
        onclick={() => (customization.theme = option.id)}>
        <span class="preview" style:background={o["chat-bg"]}>
          <span class="p-side" style:background={o.bg} style:border-color={o.line}>
            {#each [0, 1, 2, 3] as i (i)}
              <span class="p-row">
                <span class="p-dot" style:background={o["raised-2"]}></span>
                <span class="p-line" style:background={o.faint}></span>
              </span>
            {/each}
          </span>
          <span class="p-chat">
            <span class="p-head" style:background={o.surface}></span>
            <span class="p-bubble" style:background={o.bubble}></span>
            <span class="p-bubble mine" style:background={o["bubble-mine"]}></span>
            <span class="p-bubble short" style:background={o.bubble}></span>
            <span class="p-input" style:background={o.surface}>
              <span class="p-send" style:background={o.accent}></span>
            </span>
          </span>
        </span>
        <span class="card-name">
          {option.name}
          {#if option.id === theme.id}<Icon name="check" size={14} />{/if}
        </span>
      </button>
    {/each}
  </div>

  <div class="toolbar">
    {#if builtIn}
      <span class="hint">{theme.name} is built in. Changing anything makes an editable copy.</span>
    {:else}
      <input
        class="input name"
        value={theme.name}
        aria-label="Theme name"
        oninput={(e) => (theme.name = e.currentTarget.value)} />
    {/if}
    <span class="spacer"></span>
    <button class="ghost" onclick={() => duplicate(theme)}><Icon name="copy" size={14} /> Duplicate</button>
    <button class="ghost" onclick={exportTheme}>{copied ? "Copied" : "Export"}</button>
    <button class="ghost" class:on={importing} onclick={() => (importing = !importing)}>Import</button>
    {#if !builtIn}
      <button class="ghost danger" onclick={() => removeTheme(theme.id)}>
        <Icon name="trash" size={14} /> Delete
      </button>
    {/if}
  </div>

  {#if importing}
    <div class="import">
      <textarea
        class="input code"
        rows="4"
        spellcheck="false"
        placeholder={'{ "name": "…", "tokens": { "accent": "#00a884" } }'}
        bind:value={importText}></textarea>
      {#if importError}<p class="error-text">{importError}</p>{/if}
      <div class="toolbar">
        <span class="hint">Paste a theme someone exported.</span>
        <span class="spacer"></span>
        <button class="ghost" onclick={() => (importing = false)}>Cancel</button>
        <button class="primary" disabled={!importText.trim()} onclick={importTheme}>Import theme</button>
      </div>
    </div>
  {/if}
</section>

<section class="block dock" class:pinned>
  <div class="dock-head">
    <h3>Preview</h3>
    <div class="segmented small" role="tablist" aria-label="Preview">
      {#each SCENES as [id, label] (id)}
        <button role="tab" aria-selected={scene === id} class:active={scene === id} onclick={() => (scene = id)}>
          {label}
        </button>
      {/each}
    </div>
    <span class="spacer"></span>
    <button
      class="icon-btn pin"
      class:on={pinned}
      title={pinned ? "Unpin: let the preview scroll away" : "Pin: keep the preview in view"}
      aria-pressed={pinned}
      onclick={() => (pinned = !pinned)}><Icon name="pin" size={15} /></button>
  </div>
  <ThemePreview {scene} />
</section>

<section class="block">
  <h3>Look and feel</h3>
  <div class="panel">
    <div class="quick">
      <div class="q-text">
        <span class="q-title">Accent colour</span>
        <span class="hint">Buttons, the send button, selection and replies.</span>
      </div>
      <div class="accents">
        {#each ACCENTS as color (color)}
          <button
            class="accent-dot"
            class:active={t.accent?.toLowerCase() === color}
            style:background={color}
            aria-label="Accent {color}"
            onclick={() => setAccent(color)}></button>
        {/each}
        <label
          class="accent-dot custom"
          class:active={!ACCENTS.includes(t.accent?.toLowerCase() ?? "")}
          title="Custom colour">
          <input
            type="color"
            value={hex(t.accent)}
            aria-label="Custom accent colour"
            oninput={(e) => setAccent(e.currentTarget.value)} />
        </label>
      </div>
    </div>

    <div class="quick">
      <div class="q-text">
        <span class="q-title">Text size</span>
        <span class="hint">Messages, chat list and menus.</span>
      </div>
      <div class="slider">
        <input
          type="range"
          min="12"
          max="18"
          step="0.2"
          value={px("font-size", 14.2)}
          aria-label="Text size"
          oninput={(e) => setTokens({ "font-size": `${e.currentTarget.value}px` })} />
        <span class="readout">{px("font-size", 14.2).toFixed(1)} px</span>
      </div>
    </div>

    <div class="quick">
      <div class="q-text">
        <span class="q-title">Corner roundness</span>
        <span class="hint">Bubbles, fields and dialogs.</span>
      </div>
      <div class="slider">
        <input
          type="range"
          min="0"
          max="18"
          step="1"
          value={px("radius", 8)}
          aria-label="Corner roundness"
          oninput={(e) => setRoundness(Number(e.currentTarget.value))} />
        <span class="readout">{px("radius", 8)} px</span>
      </div>
    </div>

    <div class="quick">
      <div class="q-text">
        <span class="q-title">Animations</span>
        <span class="hint">Transitions between chats, menus and dialogs.</span>
      </div>
      <div class="segmented" role="radiogroup" aria-label="Animations">
        {#each MOTION as [value, label] (value)}
          <button
            role="radio"
            aria-checked={Number(t["motion-scale"] ?? "1") === Number(value)}
            class:active={Number(t["motion-scale"] ?? "1") === Number(value)}
            onclick={() => setTokens({ "motion-scale": value })}>{label}</button>
        {/each}
      </div>
    </div>
  </div>
</section>

<section class="block">
  <h3>Fine-tune</h3>
  <p class="hint">Every colour and value the interface is drawn with. Click a swatch to pick a colour.</p>
  <div class="groups">
    {#each groups as group (group)}
      {@const tokens = TOKENS.filter((token) => token.group === group)}
      <details class="group">
        <summary>
          <span class="g-name">{group}</span>
          <span class="g-strip">
            {#each tokens.filter((token) => rgba(t[token.key])).slice(0, 8) as token (token.key)}
              <span class="chip" style:--c={t[token.key]}></span>
            {/each}
          </span>
          <span class="g-count">{tokens.length}</span>
          <span class="g-chevron"><Icon name="chevronDown" size={16} /></span>
        </summary>
        <div class="tokens">
          {#each tokens as token (token.key)}
            {@const value = t[token.key] ?? ""}
            <div class="token">
              {#if rgba(value)}
                <label class="chip swatch" style:--c={value} title="Pick a colour">
                  <input
                    type="color"
                    value={hex(value)}
                    aria-label="{token.label} colour"
                    oninput={(e) => setTokens({ [token.key]: recolor(value, e.currentTarget.value) })} />
                </label>
              {:else}
                <span class="chip swatch blank"></span>
              {/if}
              <span class="token-label">{token.label}</span>
              <input
                class="input value"
                {value}
                spellcheck="false"
                aria-label={token.label}
                title={value}
                onchange={(e) => setTokens({ [token.key]: e.currentTarget.value.trim() })} />
            </div>
          {/each}
        </div>
      </details>
    {/each}
  </div>
</section>

<section class="block">
  <div class="block-head">
    <div>
      <h3>CSS extensions</h3>
      <p class="hint">
        Custom CSS applied after the theme. Component styles are scoped, so use <code>!important</code> or a
        doubled class (<code>.bubble.bubble</code>) to override them.
      </p>
    </div>
    <button class="ghost" onclick={addExtension}><Icon name="plus" size={15} /> Add</button>
  </div>
  {#if customization.extensions.length === 0}
    <div class="empty">No extensions yet.</div>
  {/if}
  {#each customization.extensions as extension, i (extension.id)}
    <div class="extension" class:off={!extension.enabled}>
      <div class="ext-head">
        <input class="input name" bind:value={extension.name} aria-label="Extension name" />
        <label class="toggle" title={extension.enabled ? "Enabled" : "Disabled"}>
          <input type="checkbox" role="switch" bind:checked={extension.enabled} aria-label="Enabled" />
        </label>
        <button
          class="icon-btn"
          title="Delete"
          aria-label="Delete {extension.name}"
          onclick={() => customization.extensions.splice(i, 1)}><Icon name="trash" size={16} /></button>
      </div>
      <textarea
        class="input code"
        rows="6"
        spellcheck="false"
        placeholder=".bubble.bubble {'{'} border-radius: 18px; {'}'}"
        bind:value={extension.css}></textarea>
    </div>
  {/each}
</section>

<style>
  .block {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .block-head {
    display: flex;
    align-items: flex-start;
    gap: 16px;
  }
  .block-head > div {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h3 {
    margin: 0;
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.45;
  }
  code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }
  .spacer {
    flex: 1;
  }

  /* Theme gallery */
  .gallery {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 12px;
  }
  .theme-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 6px 6px 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    color: var(--text);
    font: inherit;
    cursor: pointer;
    text-align: left;
    transition:
      border-color calc(120ms * var(--motion-scale, 1)),
      transform calc(120ms * var(--motion-scale, 1)) var(--ease);
  }
  .theme-card:hover {
    border-color: var(--muted);
    transform: translateY(-1px);
  }
  .theme-card.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .card-name {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
    font-size: 13px;
    font-weight: 500;
  }
  .theme-card.active .card-name {
    color: var(--accent-text);
  }
  .preview {
    display: flex;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    border-radius: calc(var(--radius-lg) - 3px);
    pointer-events: none;
  }
  .p-side {
    width: 34%;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 6px;
    border-right: 1px solid;
  }
  .p-row {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .p-dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }
  .p-line {
    height: 3px;
    flex: 1;
    border-radius: 2px;
    opacity: 0.6;
  }
  .p-chat {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .p-head {
    height: 14%;
  }
  .p-bubble {
    width: 55%;
    height: 11%;
    margin: 0 6px;
    border-radius: 3px;
  }
  .p-bubble.mine {
    align-self: flex-end;
    width: 45%;
  }
  .p-bubble.short {
    width: 35%;
  }
  .p-input {
    margin-top: auto;
    height: 15%;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    padding: 0 5px;
  }
  .p-send {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  /* Toolbars and buttons */
  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .ghost,
  .primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border-radius: var(--radius);
    font: inherit;
    font-size: 13px;
    padding: 6px 12px;
    cursor: pointer;
  }
  .ghost {
    background: var(--raised);
    border: 1px solid var(--line-strong);
    color: inherit;
  }
  .ghost:hover:not(:disabled),
  .ghost.on {
    background: var(--raised-2);
  }
  .ghost.danger {
    color: var(--danger);
  }
  .primary {
    background: var(--accent);
    border: 1px solid var(--accent);
    color: var(--accent-ink);
    font-weight: 500;
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .ghost:disabled,
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: 0;
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .icon-btn:hover {
    background: var(--raised);
    color: var(--danger);
  }
  .icon-btn.pin:hover {
    color: var(--text);
  }
  .icon-btn.pin.on {
    color: var(--accent-text);
  }

  /* Preview dock */
  .dock.pinned {
    position: sticky;
    top: 0;
    z-index: 2;
    margin-top: -10px;
    padding: 10px 0 12px;
    background: var(--bg);
    box-shadow: 0 10px 12px -12px rgba(0, 0, 0, 0.6);
  }
  .dock-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .segmented.small button {
    padding: 3px 10px;
    font-size: 12.5px;
  }
  .input {
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    padding: 6px 9px;
    color: inherit;
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .input:focus {
    border-color: var(--accent);
  }
  .name {
    flex: 1;
    min-width: 140px;
    max-width: 280px;
  }
  .code {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    font: 12.5px/1.5 ui-monospace, Consolas, monospace;
  }
  .import {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .error-text {
    margin: 0;
    color: var(--danger);
    font-size: 12.5px;
  }

  /* Quick settings */
  .panel {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .quick {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 16px;
  }
  .quick + .quick {
    border-top: 1px solid var(--line);
  }
  .q-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .q-title {
    font-size: 14px;
  }
  .accents {
    display: flex;
    gap: 8px;
  }
  .accent-dot {
    position: relative;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 50%;
    background-clip: padding-box;
    box-shadow: 0 0 0 1px var(--line-strong);
    cursor: pointer;
  }
  .accent-dot.active {
    box-shadow: 0 0 0 2px var(--surface), 0 0 0 4px var(--text);
  }
  .accent-dot.custom {
    background: conic-gradient(#f15c6d, #f0b232, #00a884, #53bdeb, #7f66ff, #e26ab6, #f15c6d);
  }
  .accent-dot input,
  .swatch input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
  }
  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  input[type="range"] {
    width: 170px;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .readout {
    width: 52px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-size: 12.5px;
    color: var(--muted);
  }
  .segmented {
    display: flex;
    padding: 3px;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
  }
  .segmented button {
    padding: 5px 14px;
    background: transparent;
    border: 0;
    border-radius: calc(var(--radius) - 2px);
    color: var(--muted);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .segmented button.active {
    background: var(--raised-2);
    color: var(--text);
  }

  /* Fine-tune groups */
  .groups {
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .group + .group {
    border-top: 1px solid var(--line);
  }
  summary {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    cursor: pointer;
    list-style: none;
    user-select: none;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary:hover {
    background: var(--raised);
  }
  .g-name {
    font-size: 14px;
    min-width: 120px;
  }
  .g-strip {
    flex: 1;
    display: flex;
    gap: 4px;
  }
  .g-count {
    color: var(--faint);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .g-chevron {
    display: grid;
    color: var(--muted);
    transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease);
  }
  details[open] .g-chevron {
    transform: rotate(180deg);
  }
  .tokens {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 2px 20px;
    padding: 4px 16px 14px;
  }
  .token {
    display: grid;
    grid-template-columns: 26px 1fr 150px;
    align-items: center;
    gap: 10px;
    min-height: 38px;
    font-size: 13px;
  }
  .token-label {
    min-width: 0;
    line-height: 1.3;
  }
  .value {
    width: 100%;
    box-sizing: border-box;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
    text-overflow: ellipsis;
  }
  /* Colour over a checkerboard, so transparency shows. */
  .chip {
    position: relative;
    display: block;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    box-shadow: inset 0 0 0 1px rgba(128, 128, 128, 0.35);
    background:
      linear-gradient(var(--c), var(--c)),
      repeating-conic-gradient(#8a8a8a 0 25%, #d4d4d4 0 50%) 0 0 / 8px 8px;
  }
  .swatch {
    width: 26px;
    height: 26px;
    border-radius: 7px;
    cursor: pointer;
  }
  .swatch.blank {
    background: none;
    box-shadow: inset 0 0 0 1px var(--line-strong);
    cursor: default;
    opacity: 0.4;
  }

  /* Extensions */
  .empty {
    padding: 18px;
    text-align: center;
    color: var(--faint);
    font-size: 13px;
    border: 1px dashed var(--line-strong);
    border-radius: var(--radius-lg);
  }
  .extension {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
  }
  .extension.off .code {
    opacity: 0.55;
  }
  .ext-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ext-head .name {
    max-width: none;
  }
  .toggle input {
    appearance: none;
    position: relative;
    display: block;
    width: 34px;
    height: 20px;
    margin: 0;
    background: var(--raised-2);
    border-radius: 10px;
    cursor: pointer;
    transition: background calc(150ms * var(--motion-scale, 1));
  }
  .toggle input::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--text);
    transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease);
  }
  .toggle input:checked {
    background: var(--accent);
  }
  .toggle input:checked::after {
    transform: translateX(14px);
    background: var(--accent-ink);
  }
</style>
