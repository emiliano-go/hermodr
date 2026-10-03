<script lang="ts">
  import { chats } from "$lib/state/chats.svelte";
  import { session } from "$lib/state/session.svelte";
  import { activeTheme, appPicture, applyTheme, chatPicture, customization, save as saveCustomization } from "$lib/utils/theme.svelte";
  import { accessibility, applyAccessibility } from "$lib/utils/accessibility.svelte";

  $effect(() => {
    applyTheme(activeTheme());
    // The accessibility layer owns motion-off, font scale and data attributes;
    // re-apply after the theme so its overrides win.
    void accessibility.enabled;
    void accessibility.reduceMotion;
    void accessibility.highContrast;
    void accessibility.reduceTransparency;
    void accessibility.targetSize;
    void accessibility.alwaysShowFocus;
    void accessibility.enhancedFocus;
    void accessibility.colorBlindPalette;
    void accessibility.textScale;
    void accessibility.textSpacing;
    void accessibility.fontChoice;
    applyAccessibility();
    saveCustomization();
  });

  /** The app's background picture, loaded from IndexedDB. */
  let appPictureUrl = $state<string | null>(null);
  $effect(() => {
    void customization.background?.v;
    if (!customization.background) {
      appPictureUrl = null;
      return;
    }
    appPicture()
      .then((url) => (appPictureUrl = url ?? null))
      .catch(() => {});
  });

  /** The user's background picture, else the theme's wallpaper, as a CSS background. */
  const wallpaper = $derived.by(() => {
    if (!appPictureUrl || !customization.background) return activeTheme().wallpaper ?? null;
    const dim = `rgba(0, 0, 0, ${customization.background.dim})`;
    return `linear-gradient(${dim}, ${dim}), url("${appPictureUrl}") center / cover no-repeat, #000`;
  });

  /** The theme's own layer, then CSS extensions, kept from closing their style element. */
  const extensionCss = $derived(
    [
      {
        id: `theme-${activeTheme().id}`,
        // The wallpaper is its own oversized layer behind everything, so a theme can move it
        // cheaply. A picture also shows through the chat, which is otherwise opaque.
        css:
          (wallpaper
            ? `html, body { background: transparent !important; }
               .stage { isolation: isolate; }
               body::before, .stage::before { content: ""; position: fixed; inset: -25%; z-index: -1;
                 pointer-events: none; background: ${wallpaper}; }
               .stage::before { position: absolute; }`
            : "") +
          // A picture stays still: moving it would re-filter every glass surface on each frame.
          (appPictureUrl
            ? `.conversation, .pairing { background: color-mix(in srgb, var(--chat-bg) 55%, transparent) !important; }
               body::before, .stage::before { animation: none !important; }`
            : "") +
          (activeTheme().css ?? ""),
      },
      ...customization.extensions.filter((e) => e.enabled),
    ]
      .filter((e) => e.css.trim())
      .map((e) => `<style data-extension="${e.id}">${e.css.replace(/<\/style/gi, "<\\/style")}</style>`)
      .join(""),
  );

  /** The open chat's own background picture, loaded from IndexedDB. */
  let chatPictureValue = $state<{ account: string; jid: string; url: string } | null>(null);
  $effect(() => {
    const jid = chats.selectedChat;
    const account = session.activeAccount;
    const meta = jid ? customization.chatBackgrounds?.[jid] : undefined;
    chatPictureValue = null;
    if (!jid || !account || !meta) return;
    let live = true;
    void meta.v;
    chatPicture(jid)
      .then((url) => { if (live && account === session.activeAccount && jid === chats.selectedChat) chatPictureValue = url ? { account, jid, url } : null; })
      .catch(() => {});
    return () => {
      live = false;
    };
  });
  const chatPictureCss = $derived.by(() => {
    if (!chatPictureValue || chatPictureValue.account !== session.activeAccount || chatPictureValue.jid !== chats.selectedChat) return "";
    const dim = `rgba(0, 0, 0, ${customization.chatBackgrounds?.[chatPictureValue.jid]?.dim ?? 0.25})`;
    return `<style data-chat-picture>.conversation { background: linear-gradient(${dim}, ${dim}), url("${chatPictureValue.url}") center / cover no-repeat !important; }</style>`;
  });
</script>

<svelte:head>
  {@html extensionCss}
  {@html chatPictureCss}
</svelte:head>
