# Refactoring ledger

Base: `4754994` (`origin/master`, pulled 2026-09-27).
Work happens on `codex/refactor-backend` in an isolated worktree.

## Scope

Move responsibilities out of the route, shell entry point, core message/media
modules, and store initialization. Preserve IPC names, payloads, send order,
history admission, retention timing, saved-name precedence, and quote secrets.
Keep structural changes separate from issue fixes.

| Area | Work | Status |
| --- | --- | --- |
| Frontend | Extract account, finder, attachment and message-action workflows; give theme layers their own component; move shared data types out of views | Implemented; Svelte check passes |
| Shell | Separate account persistence, connection, settings, platform/logging, and command domains | Implemented; 6 tests pass |
| Core | Separate wire-message decoding, media codecs/transfers, and schema setup | Implemented; 78 unit and 10 integration tests pass |
| Preview | Render real app components with inert fixture data; share menu presentation | Implemented; all four scenes checked in browser |
| Backend issues | Complete #129, #143, #146, #148, #149 with deterministic regression coverage | Implemented |
| Verification | Core and shell tests, frontend checks/build, IPC registration parity, diff review | Windows: 86 core unit, 10 integration and 6 shell tests pass; frontend gates pass; 90 commands match handlers/build declarations/capabilities |

## Baseline

- Local `pnpm check` initially found an unused `@ts-expect-error` in
  `vite.config.js`. Clean GitHub CI exposed missing Node declarations;
  `@types/node` is now explicit instead of relying on ancestor dependencies
  or a suppression.
- Phone and formatting self-checks pass; production build passes.
- Initial Rust builds exhausted C:; builds rerun with explicit target directories
  on F:. Baseline and extracted code both pass 78 core unit tests,
  10 quote-recovery integration tests and 6 shell tests.
- Refactored frontend: Svelte check 0 errors / 0 warnings; production build,
  phone/format self-checks and base64 binary/chunk-boundary checks pass.
- Browser checks cover preview rendering; live WhatsApp/Tauri IPC was not exercised.

## Backend acceptance coverage

- #129: one adapter maps upstream enum constants to semantic history kinds;
  table tests cover all known kinds, missing values and unknown values under
  both admission policies.
- #143: schema versions 1 and 2 run in immediate transactions. Tests cover
  fresh, legacy and unversioned current databases, secret preservation,
  rollback/retry, unsupported versions and no repeated cleanup. Address-form
  reconciliation remains startup maintenance; a disk reopen test covers a
  mapping learned after schema migration.
- #146: `WhatsAppService` replaces `Service` in the core, shell and CLI;
  no compatibility alias. All crates compile and existing tests pass.
- #148: group updates invalidate metadata and overview caches and notify the
  UI to refresh. Tests feed subject, description and membership events through
  the real inbound handler with populated caches. Empty group lists are now
  cached separately from an invalidated list.
- #149: every user-info caller shares 100-JID batches and a two-request
  semaphore. Tests cover three overlapping 1,001-JID requests, empty input,
  failure cleanup, complete result collection and learned LID/phone mappings.

## Current issue work (2026-09-27)

- Pairing: Android / `ANDROID_TABLET`, version `2.26.32.84`, shipped in
  `54ab4c7`. Existing links require re-pairing. #154 remains open until a live
  pairing/history check establishes which compatibility flags can be removed.
- Incoming media captions: shared decoding preserves caption text and wire
  mention tokens. Schema migration 3 restores lost placeholders from retained
  protocol references, without overwriting edits, revocations or view-once rows.
  Tests cover rollback/retry, malformed references and multiple batches.
- Image menu: Copy Image, Save Image and Open Image use original attachments;
  other attachments support Save/Open. Native commands resolve message IDs and
  restrict files to the configured media folder. Deleted/view-once media is
  excluded in both layers. Frontend routing and native pixel/path tests pass;
  live clipboard, save-dialog and new-pairing checks remain unverified.
- Validation: 90 core unit tests, 10 integration tests, 7 shell tests, frontend
  self-checks, Svelte check (0 errors/warnings) and production build pass.
- Assigned backlog: 55 open issues were found. Operator approved separate RAM
  limits and explicit disk retention for #131/#132/#161; preserve existing disk
  settings during migration. Implementation remains pending. Do not treat the
  earlier extraction or these media fixes as completion of that storage work.

## Remaining boundaries

- Handshake profile now selects Android/Tablet/Android 13 before the bot starts,
  for registration and reconnect payloads. WebInfo is omitted. Native Android
  transport and four-part handshake versions remain outside the library's
  current support; operator chose supported metadata first.
- Retention benchmark: 150,000 messages across 1,000 chats, identical 149,000
  deletions, 10.229 s before and 0.516 s after replacing composite NOT IN with
  indexed newest-message lookups. Disk-retention behavior is unchanged.
- Schema migration 4 indexes reply-to chat IDs: address reconciliation no longer
  scans every message for each merged chat. Startup now reports separate schema
  and reconciliation times; the reported 22-second startup gap is not yet
  attributed by runtime phase measurements.
- #151/#152/#153: restrictive CSP assertions and trust-boundary documentation;
  preview IP filtering covers reserved ranges and translated IPv4, redirects
  use the HTTP library with the same resolver, and configured proxies skip
  preview fetching. Six synthetic network tests cover policy/redirect behavior.
- Duplicate-chat investigation: history stores raw conversation IDs, while live
  messages canonicalize them. Learned mappings do not immediately reconcile
  existing rows; restart does. UI replaces chat snapshots and keys rows by JID,
  so merging by display name would incorrectly combine different contacts.

Account-generation guards, queued-send lifetime/order, stale chat responses,
frontend resource teardown, history-navigation convergence, atomic account-file
saves and media streaming need behavior-specific regression
proof. Extraction alone does not resolve those audit findings.

Large presentation files are not split merely to reduce line counts. Their
CSS and markup stay with their components unless there is a real shared view.
Changes from other agents must be merged before final validation and push.
