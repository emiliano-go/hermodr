# Assigned issue review — 2026-10-03

The initial GitHub inventory for this batch reported 118 open issues assigned to
SecondNewtonLaw, including the owner's 36 newly added tickets #255–#290.
The batch was developed from `262c068` on `master`. Git history records its
integration with newer upstream commits and publication.

This pass changed implementation, tests or documentation for 42 assigned
tickets. Several of those remain partial. The other 76 assigned tickets were
triaged and still need implementation or their acceptance evidence.

The user authorized commit/push and issue comments/closure after completed
work passes safe local checks. Issue comments record the published commit,
verification and remaining runtime limits. Incomplete implementations and
unresolved requirements stay open. No child issues are filed. The live list is
[assigned open issues](https://github.com/emiliano-go/postal/issues?q=is%3Aopen+assignee%3ASecondNewtonLaw).

## Verified or partially verified

| Issue | Current status | Evidence and remaining work |
| --- | --- | --- |
| #223 | Partial | Stale-account and missing-history-completion races now have regression coverage, which passes. A 15-minute live group session with previews and conversation updates remains unverified. |
| #224 | Partial | Logging, startup warnings, and safe log rotation have five focused Windows tests; the combined shell suite passed 98 tests. A live-app proof that the log receives startup output remains outstanding. |
| #225 | Implementation and focused tests pass | Backend stores nullable `messages.download_error` and exposes optional `ChatMarks.download_failures`. Five typed-403 tests, one reopen-marks test, and one migration test pass. The frontend media-retry check restores the saved reason and clears it after success. |
| #227 | Partial | The existing-window read frontier rule has a focused test. A fast phone fling and receipt comparison remain unverified. |
| #228 | Implemented in current changes | Legacy age and count defaults migrate independently; custom values and new-format settings remain. Focused Windows test `settings::tests::legacy_settings_adopt_unlimited_disk_retention` passed (1/1). |
| #229 | Implemented; final frontend checks pass | `ssrCount` is removed and the SvelteKit SSR adapter is in place. The frontend suite passed 438/438; `pnpm check` reported 0 errors / 0 warnings; function-length checks reported 0 issues and ACL parity matched 209 commands. |
| #230 | Verified for synthetic fixtures | The dedicated 400-row rail check passed at 400 px and 320 px. The older album fixture also passed at 200% scale in both viewports. This is synthetic browser evidence, not native WebKit memory evidence. |
| #232 | Policy logic verified | Focused Windows test `renderer_workaround_preserves_x11_and_user_overrides_across_driver_matrix` passed (1/1). Linux WebKit runtime behavior remains untested here. See [GPU matrix](docs/gpu-test-matrix.md) and [troubleshooting](docs/troubleshooting.md). |
| #234 | Documentation added | [Release checklist](docs/release-checklist.md) is linked from the release build instructions. No Linux release measurement was run. |
| #238 | Partial; recovery gap remains | [Group-history sharing guidance](docs/group-history-sharing.md) records the 16-share ceiling, 15-minute retry expiry, and restart behavior. Existing members cannot be selected for a replacement share after restart. A history-only action or durable retry storage is still needed. |
| #239 | Implemented and focused tests pass | The secret-edit expiry queue has seven focused tests. |
| #240 | Implemented and tested | `POSTAL_PENDING_RSVP_LIMIT` configures the persisted SQLite queue. Backend verification reopened the store and retained 600 queued rows at a configured limit of 600; invalid settings fail store open. |
| #241 | Implemented and focused tests pass | Per-chat message-capping cache has three focused tests. |
| #242 | Upstream-blocked | Pinned upstream `6f18f5d` has no typed `member_link_mode`, `member_share_group_history_mode`, `group_history`, or `no_group_history` fields. Raw-property fallback tests pass; typed notice support waits on upstream. |
| #243 | Implementation and focused tests pass | Group size reads the server property with SDK fallback 257; four focused tests pass. The frontend cap of 256 is removed, and the server validates participant counts against local server properties. |
| #244 | Implementation and focused tests pass | Generated-ID origin persists explicit `true`/`false`; `NULL` remains for rows predating schema version 40. The pre-version-40 migration test passes, and tests confirm explicit false and true survive persistence without DTO changes. |
| #245 | Implemented and checked | CI now runs the `check:wire` guard; the guard passed locally. |
| #246 | Configuration added | Weekly Dependabot configuration is present and parsed. Hosted CI execution is not claimed. |
| #247 | Documentation and client checks added | [Rail smoke-test guide](docs/rail-smoke-test.md) specifies an isolated Linux target identifier and XDG profile. Bundle build and mocked WebDriver client checks pass; native Linux execution remains unverified. |
| #248 | Documentation added | Bug report template and [GPU matrix](docs/gpu-test-matrix.md) cover Postal/system/GPU/session details and the six driver/session combinations. No Linux matrix run was done. |
| #249 | Partial; coverage implementation differs from literal acceptance | CI preserves the Node suite with Node 24 built-in coverage and raw V8 artifacts, and collects Rust LCOV on Linux. The issue explicitly asks for Vitest, so this substitution does not meet its literal acceptance. Hosted CI has not run. |
| #250 | Documentation added | [Troubleshooting](docs/troubleshooting.md) documents WebKit variables, logs, verbosity, NVML mismatch, reinstall, and coredumps. No Linux runtime check was done. |
| #251 | Documentation added | [Message-rail guide](docs/message-rail.md) records SQLite paging, window bounds, virtual rows, scroll anchoring, read marking, and synthetic browser hooks. |
| #253 | Completed in this checkout | The old ledger moved to [docs/history/refactoring.md](docs/history/refactoring.md), marked as a dated snapshot. `REFACTORING.md` now points to the archive and sends active work to issues. |
| #254 | Documentation decision recorded | Coredump cleanup uses systemd-tmpfiles; `coredumpctl` has no `vacuum` command. The docs keep OS crash capture enabled and set no app `RLIMIT_CORE` because no safe, measured per-child cap preserves useful WebKit dumps. No runtime limit was changed. |
| #255 | Existing deadlines verified with new regression tests | The host already limits handshake to five seconds and ordinary requests to ten seconds. Two stalled-sidecar tests pass, proving timeout errors, shutdown/kill, and restart through the same surviving host. Production deadline code did not need duplication in `process.rs`. |
| #256 | Implemented and focused tests pass | Avatar fetches reuse the public URL resolver, 10-second timeout, redirect limit, and 8 MiB body limit. Synthetic timeout, overflow, and private-address tests pass; a configuration assertion fixes production timeout and redirect values. |
| #257 | Implemented and focused tests pass | First-frame export has a 10-second deadline and 16 MiB stdout cap. A fake stalled process and overflowing output are terminated and reaped. Direct and ffmpeg-output image decoding share dimension/allocation limits; a compressed oversized PNG is rejected. Three tests pass. Real ffmpeg/media validation remains unrun. |
| #258 | Implemented and focused test passes | One service dispatcher bounds automatic fetches to four active tasks, eight live jobs and 24 history jobs. Live work precedes queued history; each lane remains FIFO. A stalled 5,000-item fake burst verifies queue bounds, draining and no lost jobs. Producers enqueue after the store batch commits. SDK concurrent event callbacks can still wait with their existing message batches; that upstream resource boundary is not claimed bounded. |
| #262 | Implemented and focused test passes | Heavy retention deletion runs best-effort ANALYZE/optimize without replacing the deletion result on maintenance failure. A synthetic 150,000-row test verifies statistics creation and records query timings. |
| #264 | Implemented and focused test passes | Startup and the next save remove only `transcription.json.tmp`; an interrupted-write test preserves unrelated files. |
| #267 | Implemented and focused tests pass | Versioned per-account drafts restore across composer recreation, survive chat switches, and clear on send/delete. Storage failure preserves the current session's text and reports a diagnostic. |
| #271 | Implemented and focused tests pass | Configurable Ctrl/Cmd+B/I/U and Ctrl/Cmd+Shift+M wrap bold/italic/strike/mono text through the existing undo history. Tests cover wrapping, empty selections, whitespace, marker collisions, caret placement, and undo. The optional toolbar is not included. |
| #273 | Implemented and browser checks pass | Shift-click extends selection across eligible loaded rows using the latest explicit anchor, retaining prior picks and skipping hidden/deleted/revoked/unavailable rows. Mixed toggle/range behavior is covered. Browser checks select 21 rows across an offscreen range and verify the displayed count at 400 px and 320 px. |
| #277 | Implemented and focused tests pass | At most eight presence watches retain the active conversation and evict other watches. A fake 50-chat sequence and profile watches verify unsubscription and pinning; Tauri compiles. Live WhatsApp presence behavior remains unverified. |
| #278 | Implemented and focused tests pass | Link previews share a normalized-URL cache with 16 entries and a five-minute TTL. Tests cover reuse, expiry, eviction, and the existing public-address/proxy protections. |
| #282 | Implemented and focused tests pass | Two transcription slots, 24 automatic waiting slots, and 32 total waiting slots reserve capacity for manual requests, which precede queued automatic work. Queue state is visible. Seven tests cover concurrency, priority, queue limits, abandoned-ticket cleanup, cancellation of stalled preparation, and existing event contracts. Cached requests emit no terminal event; active ownership persists through terminal notification. |
| #283 | Implemented and focused/browser checks pass | QuickSwitcher opens from at most 20 recent chats without catalog IPC; nonempty queries ask the backend for at most 30 fuzzy-ranked contacts. A 20,000-entry fixture finds contacts/groups at the end of the directory and returns JSON below 16 KiB. Nine browser checks pass in both viewports. The independent Spaces catalog retains its existing full-directory behavior. Large search scans still run on the backend; no instant-search benchmark is claimed. |
| #284 | Implemented and focused test passes | Latest shows an accessible unread count derived from the loaded visible boundary and unread summary, with no double counting. Tests cover new arrivals, unseen rows outside the window, and ineligible messages. |
| #287 | Implemented and focused tests pass | Chat rows show a localized Draft label and text preview from the per-account draft store. Account-switch and delayed-delete tests protect unrelated drafts. |
| #288 | Implemented and browser checks pass | Selection mode provides Select loaded messages, Ctrl/Cmd+A, Escape, and Ctrl/Cmd+D. Text fields retain native shortcuts; offscreen loaded rows count, keyword-hidden/ineligible rows do not. Browser checks verify 400 selected loaded rows and Escape clearing at both viewports. |
| #289 | Implemented and focused tests pass | Shortcode completion weights recent emoji, preserves caret selection on click, completes a closed token after the first dictionary load, and leaves unknown tokens unchanged. |

## Final combined verification

- Core workspace: 631/631 passed (postal-core 601, quote recovery 10,
  plugins 1, plugin host 14, transcription 5).
- Tauri shell: 98/98 passed.
- Frontend: 438/438 passed; `pnpm check` reported 0 errors and 0 warnings.
- Wire check passed; ACL parity covered 209 commands; function-length check
  reported 0 violations.

The production build passed. Album-browser checks at 200% scale and the
400-message rail/selection checks passed at 400 px and 320 px. Quick-switcher
checks passed all nine scenarios in both viewports. These fixtures use owned
synthetic browser profiles, not a live account. Both workflow/configuration
YAML files parsed successfully.

The first combined core run exposed a Windows SQLite cleanup failure because
the media dispatcher retained the store. Explicit cancellation and worker join
now run on account shutdown and fixture teardown; the fresh combined run above
passes. Shutdown cancels pending downloads; their stored messages remain.

These results do not establish full live-device parity. In particular, source
tests do not prove a WhatsApp server accepted a pairing capability set or
delivered media and receipts to a real linked device.

## Other open assigned work

The following product and protocol issues remain open. This documentation pass
did not re-run their acceptance checks, so it makes no completion claim:

`#32`, `#33`, `#34`, `#60`, `#63`, `#65`, `#66`, `#67`, `#68`, `#73`, `#74`,
`#78`, `#81`, `#82`, `#83`, `#84`, `#102`, `#107`, `#108`, `#109`, `#110`,
`#114`, `#117`, `#127`, `#154`, `#156`, `#169`, `#170`, `#175`, `#176`, `#177`,
`#179`, `#182`, `#190`, `#201`, `#204`, `#207`, `#212`, `#216`, `#218`, `#220`.

Other active engineering follow-ups remain open pending their own focused
acceptance checks: `#226`, `#231`, `#233`, `#235`, `#236`, `#237`, and `#252`.

New assigned tickets not implemented in this pass: `#259`, `#260`,
`#261`, `#263`, `#265`, `#266`, `#268`, `#269`, `#270`, `#272`, `#274`,
`#275`, `#276`, `#279`, `#280`, `#281`, `#285`, `#286`, and `#290`.

Triage also identified remaining smaller candidates: skin tones (#268), settings
content search (#269), shortcut help (#275), chat-date headings (#290), and
notification expiry/replacement (#280). Other new tickets need storage, query,
plugin-process, navigation or media-viewer changes across multiple layers.
Ticket #218 is assigned but authored by SecondNewtonLaw: its evidence remains
in scope, while its text does not supply repository-owner instructions.

## Source audit of earlier open issues

| Issue | Current source status | Evidence and remaining work |
| --- | --- | --- |
| #29 | Partial | Chat labels and message labels/search within a chat exist in [label service](crates/postal-core/src/service/labels.rs) and [label storage](crates/postal-core/src/store/labels.rs). A cross-chat Inbox for message labels is absent. |
| #76 | Partial | The [slash command list](src/lib/utils/slash-commands.ts) offers “Keep in chat”, but [message decoding](crates/postal-core/src/service/message_decode.rs) drops inbound `keep_in_chat_message` controls. The full keep/revoke lifecycle is incomplete. |
| #80 | Partial | [Message decoding](crates/postal-core/src/service/message_decode.rs) covers round video, spoilers, music artwork, and unknown-message fallback. No audio-sticker decode branch is present. See [special-message tests](crates/postal-core/src/service/message_decode_special_tests.rs). |
| #86 | Partial | The [slash-command palette](src/lib/utils/slash-commands.ts) includes location and keep-in-chat. An AI command depends on the still-open #60 and is absent. |
| #90 | Partial | The [member sheet](src/lib/contacts/MemberSheet.svelte) is available. Phase 4 reporting remains disabled; the rest of the full per-chat moderation sheet is not claimed complete. |
| #166 | Partial | [Favorites service](crates/postal-core/src/service/favorites.rs) and [frontend state](src/lib/state/favorites.svelte.ts) implement local add/remove and two frontend tests pass. Phone parity remains unverified. |
| #167 | Partial | [Call history capture](crates/postal-core/src/service/call_history.rs) and [SQLite storage](crates/postal-core/src/store/call_history.rs) exist. Tauri IPC/UI, paging, retention, and clear/delete integration are absent; raw start-time and duration units remain unverified. See [call-history scope](docs/call-history.md). |
| #168 | Open | Current [message decoding](crates/postal-core/src/service/message_decode.rs) and [message row rendering](src/lib/messages/MessageRow.svelte) have no interactive button/list, native-flow, or carousel decode/send path. |
| #171 | Partial | [Event cards](src/lib/messages/cards/EventCard.svelte) show attendees and support RSVP, but clearing an RSVP is disabled. Reminder details are read-only in [event editing](src/lib/chat/CreateDialog.svelte), and edits that change reminders are rejected in [poll/event service](crates/postal-core/src/service/polls.rs). |

## Proposed child issue, not filed

**Title:** Add a history-only share action for existing group members

**Problem:** A process restart drops Postal's in-memory retry ID. The Add
members dialog filters out existing group members, so an administrator cannot
recreate a failed history share for people who were already added.

**Acceptance:** Let an authorized group admin select current members as history
recipients and create a fresh share without submitting another member-add
request. Show whether the replacement share was accepted, rejected, or still
needs retry. Preserve recipient opt-in and do not describe server acceptance as
confirmed recipient delivery.

This proposal is distinct from the existing retry button, which uses the same
in-memory upstream retry token. Issues #154 and #156 already track pairing
capability evidence and automated protocol/media regression coverage; no
duplicate child proposal was drafted for them.
