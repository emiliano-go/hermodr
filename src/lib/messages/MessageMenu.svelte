<script lang="ts" module>
  import type { MenuItem } from "$lib/messages/MessageMenuPanel.svelte";
  export type { MenuItem };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import MessageMenuPanel from "$lib/messages/MessageMenuPanel.svelte";
  import { motion } from "$lib/utils/theme.svelte";

  let {
    x,
    y,
    items,
    reactions,
    reactionReason = null,
    current,
    onreact,
    onmore,
    onclose,
  }: {
    x: number;
    y: number;
    items: MenuItem[];
    /** Quick reactions, shown as a row above the items. */
    reactions: string[];
    reactionReason?: string | null;
    /** Our current reaction, which a second click takes back. */
    current: string | null;
    onreact: (emoji: string) => void;
    /** Opens the full emoji picker for more reactions. */
    onmore: () => void;
    onclose: () => void;
  } = $props();

  let menu: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });
  let closing = $state(false);

  /**
   * Fades out, then tells the parent to drop the menu.
   *
   * The removal is deferred past the event that dismissed it (a right-click
   * here crashed WebKitGTK when the overlay vanished mid-dispatch), so the wait
   * is a frame plus the fade rather than a bare timeout.
   */
  function close() {
    if (closing) return;
    closing = true;
    const ms = Math.max(1, motion(120));
    requestAnimationFrame(() => setTimeout(onclose, ms));
  }

  // Position after paint, then focus. Doing either during the opening click
  // can race the engine's own focus/context-menu handling.
  onMount(() => {
    requestAnimationFrame(() => {
      const rect = menu?.getBoundingClientRect();
      if (rect) {
        pos = {
          left: Math.max(8, Math.min(x, window.innerWidth - rect.width - 8)),
          top: Math.max(8, Math.min(y, window.innerHeight - rect.height - 8)),
        };
      }
      menu?.querySelector("button")?.focus();
    });

    // Outside dismissal runs in the capture phase so the second right-click is
    // consumed before it reaches the message behind the menu.
    const outside = (target: EventTarget | null) =>
      menu ? !menu.contains(target as Node) : true;
    const onPointerDown = (event: PointerEvent) => {
      if (outside(event.target)) close();
    };
    const onContextMenu = (event: MouseEvent) => {
      if (menu?.contains(event.target as Node)) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        close();
      }
    };
    const onResize = () => close();

    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("contextmenu", onContextMenu, true);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("contextmenu", onContextMenu, true);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("resize", onResize);
    };
  });
</script>

<MessageMenuPanel bind:element={menu} left={pos.left} top={pos.top} {closing}
  {items} {reactions} {reactionReason} {current} {onreact} {onmore} onclose={close} />
