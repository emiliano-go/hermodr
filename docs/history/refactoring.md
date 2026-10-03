# Historical refactoring ledger

> Archived status record from 2026-09-27. Counts and open-boundary notes below
> describe that checkpoint, not the current checkout or current release status.

Initial base: `4754994` (`origin/master`, pulled 2026-09-27).
Work happens on `codex/refactor-backend` in an isolated worktree.

## Completed follow-up work

- Storage policies now distinguish RAM windows from explicit disk retention.
  Independent history opt-in, typed limit modes, retained empty-chat metadata,
  indexed cursor paging and a Storage Manager have regression coverage.
- SQLite operations cross an async worker boundary. Batch leases preserve
  ordering and cancellation cleanup without blocking the async executor.
- LID/phone mappings fold existing rows immediately and atomically. Media retry,
  account cancellation, page races, receipt ordering and encrypted-reaction
  handling have synthetic regression tests.
- Local backups contain application tables and referenced attachments, exclude
  sessions, and restore into a separate account. Account-list saves are atomic.
- Large attachments use bounded renderer chunks and file-backed encryption,
  upload and download. Playback/conversion paths have not all been converted.
- The independent postal-plugins crate provides read-only sidecars, explicit
  consent, acknowledged events, eager/lazy lifecycle, bounded queues and crash
  handling. Synthetic child processes and a browser fixture verify this surface.
- Send actions reserve the outbox before conversion or downloads. Attachment
  batches remain contiguous, and account changes cancel pending preparation.
  Message retrieval uses wire timestamps followed by a persisted first-seen
  sequence, rather than treating random message IDs as chronological. Paging,
  read boundaries, retention and previews share this order. Migration 10 seeds
  existing rows from their stored row order; unavailable wire precision is not
  reconstructed.

The counts below record earlier checkpoints, not the latest integrated suite.

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

## Issue work as of 2026-09-27

- Initial pairing changes advertised Android / `ANDROID_TABLET`, version
  `2.26.32.84`, in `54ab4c7`. Subsequent upstream work uses an External main
  companion plus an optional Android tablet companion for one-time media.
  Existing links require re-pairing to change their advertised device identity.
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
- Operator-approved RAM limits and explicit disk retention for #131/#132/#161
  are implemented, preserving existing disk settings during migration.

## Remaining boundaries at that checkpoint

- Android companion handshake metadata applies before registration and reconnect.
  Native Android transport and four-part handshake versions remain outside the
  library's support; operator chose supported metadata first. The main companion
  retains the External profile introduced by the dual-connection changes.
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
- Duplicate-chat fix: history resolves conversation IDs, newly learned mappings
  fold existing rows immediately, and repository access canonicalizes stale LIDs.
  Atomic merge tests cover collisions, replay and rollback. UI rows remain keyed
  by JID; merging by display name would incorrectly combine different contacts.

Account-generation guards, queued-send cancellation/order, stale history pages,
local-first history navigation, atomic account saves and file-backed transfers
now have focused regression coverage. Remaining high-risk protocol scenarios
still belong to #156; these tests do not establish live-device parity.

At that checkpoint, agent changes still needed to be merged before final
validation and push. That note is historical and does not describe the current
checkout.
