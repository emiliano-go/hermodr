// Members domain: group rosters, learned names, sender resolution, typing and
// presence labels, and the open group's info. Moved out of +page.svelte.
// Reads messages state (first-seen push names) and session (connection, self);
// never the reverse, so the domain graph stays acyclic.
import { invoke } from "$lib/ipc";
import { plain } from "$lib/format";
import { bare } from "$lib/message";
import type { GroupInfo, Member, StoredMessage } from "$lib/models";
import { displayName as phoneName, isPlaceholder } from "$lib/phone";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";

export class MembersState {
  participants = $state<Member[]>([]);
  /** Last member list per group, shown while a switch reloads it so the header does not flash. */
  memberCache: Record<string, Member[]> = {};
  /** The open group's info: members, and whether we may send (announcement mode, communities). */
  chatGroup = $state<GroupInfo | null>(null);

  /**
   * Names the core found under either address form (a LID sender, a phone
   * quote author), fetched in batches for JIDs rendered without one.
   */
  learnedNames = $state<Record<string, string>>({});
  requestedNames = new Set<string>();
  queuedNames: string[] = [];
  nameTimer: ReturnType<typeof setTimeout> | undefined = undefined;

  /** Who is typing in each chat, until they pause or ten seconds pass. */
  typing = $state<Record<string, { sender: string; state: string }[]>>({});
  typingTimers = new Map<string, ReturnType<typeof setTimeout>>();

  /** Online state of contacts we watch, as far as their privacy lets us see it. */
  presence = $state<Record<string, { online: boolean; last_seen: number | null }>>({});

  /** When each unnamed group's subject was last asked for; the core backs off failed ones. */
  askedSubjects = new Map<string, number>();

  /** What kind of group the open chat is, shown before its members in the header. */
  groupContext = $derived.by(() => {
    const chatGroup = this.chatGroup;
    if (!chatGroup) return null;
    if (chatGroup.community) return "Community";
    const parent = chatGroup.parent_name ?? (chatGroup.parent ? "a community" : null);
    if (chatGroup.announcements) return parent ? `Announcements · ${parent}` : "Announcements";
    return parent ? `In ${parent}` : null;
  });

  /** Every address form for a member, so lookups do not scan the roster per render. */
  memberByAddress = $derived.by(() => {
    const m = new Map<string, Member>();
    for (const p of this.participants) {
      const b = bare(p.jid);
      m.set(b, p);
      m.set(p.jid, p);
      m.set(b.split("@")[0], p);
      if (p.number) m.set(p.number, p);
    }
    return m;
  });

  /** First real push name per sender address, collected once instead of a scan per render. */
  senderNames = $derived.by(() => {
    const m = new Map<string, string>();
    for (const msg of messages.messages) {
      if (msg.from_me || !msg.sender_name || isPlaceholder(msg.sender_name)) continue;
      const b = bare(msg.sender);
      if (!m.has(b)) m.set(b, msg.sender_name);
    }
    return m;
  });

  /** Push names keyed by member, so a LID sender still names its member. */
  memberNames = $derived.by(() => {
    const m = new Map<string, string>();
    for (const [sender, name] of this.senderNames) {
      const member = this.memberByAddress.get(sender);
      if (member && !m.has(member.jid)) m.set(member.jid, name);
    }
    return m;
  });

  /** The group member a sender is, matched by LID or by phone number. */
  memberOf(jid: string) {
    return this.memberByAddress.get(bare(jid));
  }

  senderLabel(message: StoredMessage) {
    // The message row joins names on one address form only; the member list
    // resolves both, so it rescues senders whose name is keyed by the other.
    const own = message.sender_name;
    const known = own && !isPlaceholder(own) ? own : this.memberOf(message.sender)?.name;
    return this.displayName(known && !isPlaceholder(known) ? known : own, message.sender);
  }

  /** Resolves a JID to a known name, falling back to the bare address. */
  senderName(jid: string) {
    const b = bare(jid);
    const known = this.senderNames.get(b);
    const member = this.memberOf(jid)?.name;
    return this.displayName(known && !isPlaceholder(known) ? known : (member ?? known), jid);
  }

  /** Author shown on a quote; our own messages read "You". */
  quoteAuthor(jid: string | null) {
    if (!jid) return "Message";
    return jid === "@me" ? "You" : this.senderName(jid);
  }

  /** A name for a JID, asking the core when the given one is missing or a bare number. */
  displayName(name: string | null | undefined, jid: string) {
    if (!name || isPlaceholder(name)) {
      const key = bare(jid);
      const learned = this.learnedNames[key];
      if (learned) return phoneName(learned, key);
      this.requestName(key);
    }
    return phoneName(name, jid);
  }

  /** Who an `@<user>` token names: us, a group member, or whichever address form the core knows. */
  mentionTarget(user: string): { jid: string; name: string; self: boolean } {
    const own = session.me ? bare(session.me) : null;
    const member = this.participants.find(
      (p) => p.jid.split("@")[0] === user || p.number === user,
    );
    if (own && (own.split("@")[0] === user || member?.number === own.split("@")[0])) {
      // Our own contact card may be saved under a nickname; show our push name.
      return { jid: own, name: this.displayName(null, own), self: true };
    }
    if (member) {
      // A push name seen on any of their messages here beats the member list's bare number.
      const spoken = messages.messages.find(
        (m) => m.sender_name && !isPlaceholder(m.sender_name) && this.memberOf(m.sender) === member,
      )?.sender_name;
      const named = spoken ?? (isPlaceholder(member.name) ? null : member.name);
      return { jid: member.jid, name: this.displayName(named, member.jid), self: false };
    }
    // Cached names only: one message can mention hundreds of numbers, and a
    // lookup for each would flood the core and lag the whole app.
    const lid = `${user}@lid`;
    const pn = `${user}@s.whatsapp.net`;
    const lidName = this.learnedNames[lid];
    const lidKnown = !!lidName && !/^\d+$/.test(lidName);
    return lidKnown
      ? { jid: lid, name: phoneName(lidName, lid), self: false }
      : { jid: pn, name: phoneName(null, pn), self: false };
  }

  mentionName(user: string) {
    return this.mentionTarget(user).name;
  }

  /**
   * `@Name` typed for a member, as older captions were sent, rewritten to the
   * wire's `@<number>` so it draws as a mention tag too. Longest names first,
   * so "Ana María" wins over "Ana".
   */
  asWireMentions(text: string) {
    if (!text.includes("@") || this.participants.length === 0) return text;
    const named = this.participants
      .filter((p) => p.name.length > 1 && !isPlaceholder(p.name))
      .sort((a, b) => b.name.length - a.name.length);
    let out = text;
    for (const p of named) {
      const token = `@${p.name}`;
      if (out.includes(token)) out = out.split(token).join(`@${p.jid.split("@")[0]}`);
    }
    return out;
  }

  /** Label for a media message with no caption, used in reply previews. */
  replyPreviewText(message: StoredMessage) {
    // Media stores a "[image]" style placeholder when it has no caption.
    if (message.text && !message.text.startsWith("[")) {
      return plain(message.text, (user) => this.mentionName(user));
    }
    switch (message.media_kind) {
      case "image":
        return "Photo";
      case "video":
        return "Video";
      case "audio":
        return "Voice message";
      case "document":
        return "Document";
      default:
        return "";
    }
  }

  requestName(jid: string) {
    if (!session.connected || this.requestedNames.has(jid) || !jid.includes("@")) return;
    this.requestedNames.add(jid);
    this.queuedNames.push(jid);
    clearTimeout(this.nameTimer);
    this.nameTimer = setTimeout(async () => {
      const jids = this.queuedNames;
      this.queuedNames = [];
      try {
        Object.assign(this.learnedNames, await invoke<Record<string, string>>("names", { jids }));
      } catch {
        for (const jid of jids) this.requestedNames.delete(jid);
      }
    }, 30);
  }

  /** Asks again for every JID the core had no name for, once it may have learned some. */
  forgetUnresolvedNames() {
    const kept: Record<string, string> = {};
    for (const [jid, name] of Object.entries(this.learnedNames)) {
      if (isPlaceholder(name)) this.requestedNames.delete(jid);
      else kept[jid] = name;
    }
    for (const jid of this.requestedNames) if (!(jid in kept)) this.requestedNames.delete(jid);
    this.learnedNames = kept;
  }

  setTyping(chat: string, sender: string, state: string) {
    const key = `${chat} ${sender}`;
    clearTimeout(this.typingTimers.get(key));
    const others = (this.typing[chat] ?? []).filter((t) => t.sender !== sender);
    this.typing[chat] = state === "paused" ? others : [...others, { sender, state }];
    if (state !== "paused") {
      this.typingTimers.set(
        key,
        setTimeout(() => this.setTyping(chat, sender, "paused"), 10000),
      );
    }
  }

  presenceLabel(chat: string) {
    const seen = this.presence[chat];
    if (!seen) return null;
    if (seen.online) return "online";
    if (!seen.last_seen) return null;
    const date = new Date(seen.last_seen * 1000);
    const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    const days = Math.floor((Date.now() - date.getTime()) / 86_400_000);
    const day =
      new Date().toDateString() === date.toDateString()
        ? "today"
        : days < 2
          ? "yesterday"
          : date.toLocaleDateString([], { day: "numeric", month: "short" });
    return `last seen ${day} at ${time}`;
  }

  typingLabel(chat: string) {
    const who = this.typing[chat];
    if (!who?.length) return null;
    const verb = who.some((t) => t.state === "recording") ? "recording audio" : "typing";
    if (!chat.endsWith("@g.us")) return `${verb}…`;
    if (who.length > 1) return `${who.length} people are ${verb}…`;
    const person = this.memberOf(who[0].sender);
    const name = person && !isPlaceholder(person.name) ? person.name : null;
    return `${name ?? this.senderName(who[0].sender)} is ${verb}…`;
  }

  async loadChatGroup(chat: string, isCurrent: () => boolean) {
    if (!chat.endsWith("@g.us")) {
      this.chatGroup = null;
      this.participants = [];
      return;
    }
    try {
      const info = await invoke<GroupInfo>("group_info", { chat });
      this.memberCache[chat] = info.participants;
      if (!isCurrent()) return;
      this.chatGroup = info;
      this.participants = info.participants;
      // Loading members stores their group display names, so numbers looked up
      // before now may have a name; ask again.
      this.forgetUnresolvedNames();
    } catch {
      if (!isCurrent()) return;
      this.chatGroup = null;
      this.participants = this.memberCache[chat] ?? [];
    }
  }

  /**
   * Group subjects need a network query. Runs after the list is already
   * rendered so a slow query cannot delay showing new messages.
   * Returns how many resolved, so the caller can refresh the list.
   */
  async resolveNames(): Promise<number> {
    try {
      return await invoke<number>("resolve_names");
    } catch {
      // Names are cosmetic.
      return 0;
    }
  }

  /** Mirrors resetUi: the roster is dropped, caches survive for the next switch. */
  resetAccount() {
    this.participants = [];
  }
}

export const members = new MembersState();
