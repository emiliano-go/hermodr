// UI-domain state: transient notices, open dialogs/overlays and view flags.
// These are shared between the route (which orchestrates flows) and the views
// (which render them), so they live here instead of in +page.svelte.
import type { FoundItem } from "$lib/MessageFinder.svelte";
import type { ChatEvent, StoredMessage } from "$lib/models";
import type { Section } from "$lib/Settings.svelte";
import type { StarredItem } from "$lib/StarredList.svelte";

/** Mentions of us (everywhere or in one chat), or a search inside one chat. */
export type Finder = {
  mode: "pings" | "search";
  chat: string | null;
  items: FoundItem[] | null;
  /** Search only: the query shown, how far back the chat is loaded, and whether the phone may have older days. */
  query?: string;
  reach?: number | null;
  more?: boolean;
};

export class UiState {
  /** A fatal-ish banner; null hides it. */
  error = $state<string | null>(null);
  /** A transient notice, such as a video sent without a preview. */
  notice = $state<string | null>(null);

  showSettings = $state(false);
  settingsSection = $state<Section>("accounts");
  accountMenu = $state(false);
  chatSettingsOpen = $state(false);

  /** Message context menu anchor. */
  menu = $state<{ x: number; y: number; message: StoredMessage } | null>(null);
  forwarding = $state<StoredMessage | null>(null);
  deleting = $state<StoredMessage | null>(null);
  reporting = $state<StoredMessage | null>(null);
  /** Our message whose delivery and reads are shown. */
  infoFor = $state<StoredMessage | null>(null);
  /** Bumped so the open info reloads on new receipts. */
  infoVersion = $state(0);

  starredItems = $state<StarredItem[] | null>(null);
  showStarred = $state(false);
  finder = $state<Finder | null>(null);

  viewerIndex = $state<number | null>(null);
  /** The view-once message being shown; closing it spends it. */
  onceOpen = $state<StoredMessage | null>(null);
  onceIndex = $state(0);

  creating = $state<"poll" | "event" | null>(null);
  /** Our own event being edited in the create dialog. */
  editingEvent = $state<{ chat: string; event: ChatEvent } | null>(null);
  /** The profile card open beside a mention, name or picture. */
  profileCard = $state<{ jid: string; name: string; x: number; y: number; self: boolean } | null>(
    null,
  );
  /** A quote whose target is not loaded yet, offered as a load action. */
  pendingJump = $state<{ chat: string; id: string } | null>(null);
  /** Walks the chat's past back until the pending jump's message lands. */
  seeking = $state(false);

  /** True while a newly opened chat's messages load, so the old ones fade out. */
  switching = $state(false);
  /** True while the user is reading older messages with new ones below. */
  scrolledUp = $state(false);
  /** Message briefly outlined after a jump, so it is easy to spot. */
  highlightedId = $state<string | null>(null);

  /** Surface a failure instead of dropping it. */
  fail(e: unknown) {
    this.error = String(e);
  }

  notify(message: string) {
    this.notice = message;
  }

  /** Mirrors resetUi: only the account menu is UI-owned there. */
  resetAccount() {
    this.accountMenu = false;
  }
}

export const ui = new UiState();
