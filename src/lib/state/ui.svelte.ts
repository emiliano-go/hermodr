// UI-domain state: transient notices, open dialogs/overlays and view flags.
// These are shared between the route (which orchestrates flows) and the views
// (which render them), so they live here instead of in +page.svelte.
import type { ChatEvent, FoundItem, StarredItem, StoredMessage } from "$lib/utils/models";
import type { Section } from "$lib/settings/Settings.svelte";

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
  newGroup = $state(false);
  showInbox = $state(false);
  labelTargets = $state<{ chat: string; id?: string }[] | null>(null);
  manageLabels = $state(false);
  sharingContacts = $state(false);

  /** Message context menu anchor. */
  menu = $state<{ x: number; y: number; message: StoredMessage } | null>(null);
  /** Messages waiting for a target chat, one or many from a selection. */
  forwarding = $state<StoredMessage[] | null>(null);
  deleting = $state<StoredMessage | null>(null);
  /** A group member whose removal is waiting for confirmation. */
  removeMember = $state<{ chat: string; jid: string; name: string } | null>(null);
  /** Messages picked for a bulk action, in the open chat; null when not picking. */
  picking = $state<Record<string, StoredMessage> | null>(null);
  /** The picked message ids a bulk delete is confirming. */
  bulkDelete = $state<string[] | null>(null);
  reporting = $state<StoredMessage | null>(null);
  /** Our message whose delivery and reads are shown. */
  infoFor = $state<StoredMessage | null>(null);
  /** The message whose reactors are listed, opened from its menu or pill. */
  reactionsFor = $state<StoredMessage | null>(null);
  /** The messages receiving a full-emoji reaction, with the picker anchor. */
  emojiFor = $state<{ messages: StoredMessage[]; x: number; y: number } | null>(null);
  /** Bumped so the open info reloads on new receipts. */
  infoVersion = $state(0);

  /** Replaced wholesale when loaded or filtered, so raw like other long lists. */
  starredItems = $state.raw<StarredItem[] | null>(null);
  showStarred = $state(false);
  finder = $state.raw<Finder | null>(null);

  viewerId = $state<string | null>(null);
  /** The view-once message being shown; closing it spends it. */
  onceOpen = $state<StoredMessage | null>(null);
  onceIndex = $state(0);

  creating = $state<"poll" | "event" | null>(null);
  /** Our own event being edited in the create dialog. */
  editingEvent = $state<{ account: string; generation: number; chat: string; event: ChatEvent } | null>(null);
  /** The profile card open beside a mention, name or picture. */
  profileCard = $state<{ jid: string; name: string; x: number; y: number; self: boolean } | null>(
    null,
  );
  /** A quote whose target is not loaded yet, offered as a load action. */
  pendingJump = $state<{ chat: string; id: string } | null>(null);
  /** Walks the chat's past back until the pending jump's message lands. */
  seeking = $state(false);
  /** Per-chat clear/delete confirmation: local-only, never touches the other side. */
  chatConfirm = $state<{ kind: "clear" | "delete"; chat: string } | null>(null);

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

  resetAccount() {
    this.creating = null;
    this.editingEvent = null;
    this.accountMenu = false;
    this.newGroup = false;
    this.showInbox = false;
    this.labelTargets = null;
    this.manageLabels = false;
    this.sharingContacts = false;
    this.viewerId = null;
    this.finder = null;
    this.pendingJump = null;
    this.seeking = false;
  }
}

export const ui = new UiState();
