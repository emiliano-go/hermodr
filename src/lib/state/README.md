# State ownership

Single source of truth for where application state lives. The rule:

- **Shared domain state lives in `src/lib/state/`, one class per domain.**
  `session` (connection, gate, accounts, settings, zoom, presence config),
  `ui` (notices, dialogs, overlays, view flags), `chats` (list, selection,
  search, pins, avatars, group panel), `messages` (scrollback, marks, recall,
  downloads, autoplay, mention queue), `members` (roster, learned names,
  sender resolution, typing/presence, open group), `composer` (drafts,
  tray, autocomplete, send pipeline, send privacy), `player` (the single
  voice-note audio engine, waveform cache and heard marks).
- **The backend event stream maps onto domains in `state/events.ts`.**
  Each `ServiceEvent` kind updates domain state there; view-only callbacks
  (scrolling, reconnecting) arrive via the `EventHost` the route registers.
- **Cross-domain workflows use plain functions.** `accounts.ts` coordinates
  session changes; `finder.ts` owns search and starred lists;
  `attachments.ts` stages clipboard/drop input; `message-actions.ts` owns
  menu actions and event edits. These modules may use domain state, but
  domain classes must not import them.
- **The route (`src/routes/+page.svelte`) composes views.**
  It owns chat navigation, DOM effects/wiring, DOM refs, and
  ephemeral view-local state (scroll position, jump highlight). No new
  `$state` in the route except view-local state and element refs.
- **Dependencies flow one way, never in a cycle:**
  `ui` stands alone; `session` and `messages` use only `ui`;
  `members` uses `session` plus messages state (first-seen push names);
  `chats` uses `ui`, `members` and `session`; `composer` uses all of those;
  `events`, workflows and the route may use everything. Keep it that way.
- **Views live in domain folders mirroring state.**
  `chat/`, `messages/` (+ `messages/cards/`), `composer/`, `media/`,
  `contacts/`, `settings/`; dumb primitives in `ui/` (must not import
  siblings); pure helpers in `utils/`. Tests live in `src/tests/`, mirroring `src/lib/`.
- **Never pass a domain method bare** (e.g. `onclick={chats.togglePin}`):
  `this` is lost. Wrap in an arrow, or export a plain function.
  Element access a domain cannot own (textarea, scroller) arrives via an
  explicitly registered host object, as `composer.host` does.
- **Views keep stable prop contracts.** New needs go through domain fields;
  do not thread route-local state into components.
