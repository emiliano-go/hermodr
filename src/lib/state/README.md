# State ownership

Single source of truth for where application state lives. The rule:

- **Shared domain state lives in `src/lib/state/`, one class per domain.**
  `session` (connection, gate, accounts, settings, zoom, presence config),
  `ui` (notices, dialogs, overlays, view flags), `chats` (list, selection,
  search, pins, avatars, group panel), `messages` (scrollback, marks, recall,
  downloads, autoplay, mention queue), `members` (roster, learned names,
  sender resolution, typing/presence, open group), `composer` (drafts,
  tray, autocomplete, send pipeline, send privacy).
- **The backend event stream maps onto domains in `state/events.ts`.**
  Each `ServiceEvent` kind updates domain state there; view-only callbacks
  (scrolling, reconnecting) arrive via the `EventHost` the route registers.
- **The route (`src/routes/+page.svelte`) orchestrates, it does not own.**
  It composes views, runs cross-domain flows (open/send/switch chats,
  navigation, account switching), owns effects/wiring, DOM refs, and
  ephemeral view-local state (scroll position, jump highlight). No new
  `$state` in the route except view-local state and element refs.
- **Dependencies flow one way, never in a cycle:**
  `ui` stands alone; `session` and `messages` use only `ui`;
  `members` uses `session` plus messages state (first-seen push names);
  `chats` uses `ui`, `members` and `session`; `composer` uses all of those;
  `events` and the route may use everything. Keep it that way.
- **Never pass a domain method bare** (e.g. `onclick={chats.togglePin}`):
  `this` is lost. Wrap in an arrow, or export a plain function.
  Element access a domain cannot own (textarea, scroller) arrives via an
  explicitly registered host object, as `composer.host` does.
- **Views keep stable prop contracts.** New needs go through domain fields;
  do not thread route-local state into components.
