# Assigned issue review — 2026-09-27

Reviewed all 55 open issues assigned to SecondNewtonLaw. A library API alone
does not complete an application feature. “Resolved” below means acceptance
was checked against current source and test evidence; GitHub comments link
the relevant commits. Remaining rows stay open.

Closed #52, #151, #152, #153, #184 and #185 using `gh`; 49 assigned issues remain
open. Concurrent master commit `a1c144b` added dual pairing modes and retained
media/read-sync behavior after this review. Its changes were preserved; the
missing `paired_mode` build/capability declarations were fixed and a command
registration parity test was added.

## Resolved

| Issues | Evidence |
| --- | --- |
| #151, #152, #153 | `6e1f945`: restrictive CSP assertions, CSS trust/CSP documentation, public HTTP(S) preview filtering, reserved/translated-address coverage, redirects and proxy handling. Six synthetic network tests and shell CSP test pass. |
| #184, #185 | Postal pin commit `4754994` selects upstream `6f18f5d`. Its successful Build & Test run `36272675489` has eight passing, non-skipped IQ/keepalive tests in the JUnit artifact, including real loopback TCP/Noise exchanges, wedged writes and normal traffic. |
| #52 | One encryption header outside the message loop; encryption stubs produce no duplicate line. Identity/device events are subscribed, persisted and rendered. Notice formatter tests and view-once decode/quote tests cover the relevant boundaries. |

## Still open

| Issues | Remaining requirement |
| --- | --- |
| #207 | Updater commands, release workflow, signed artifacts and update UX absent. Production signing identity/secrets also require operator provisioning. |
| #201, #150 | Password-locked chats, encrypted databases, keychain storage and plaintext migration absent. Transport encryption does not encrypt local SQLite. |
| #190 | Persisted pack/favorite/recent synchronization, outgoing mutations and resync UI absent. Existing local sticker handling does not satisfy this. |
| #186 | Upstream A/B props API is available; Postal has no watched-prop/diagnostic surface. |
| #182, #163, #164 | Plugin process host, AI/STT integration, permissions, verified model installation and provider workflows absent. |
| #180 | Spaces tree, membership persistence and export absent. |
| #179 | No paired still/motion storage, offset handling or motion-photo UI. |
| #178 | No per-chat/global usage dashboard or individual attachment cleanup surface. |
| #176 | Existing admin reports/member labels are partial; membership, sharing and governance controls remain. |
| #175, #67, #66, #63, #33 | Channel reading/subscription, administration, publishing, live reactions/views and threaded comments remain unwired. |
| #172 | Upstream PR #1550 has merged; the issue's upstream-open prerequisite is stale. Postal still lacks the add-member/history-sharing flow. |
| #171 | Basic RSVP counts/actions exist; full attendees, reminders, plus-one, invitation and pin state remain. |
| #170 | Startup resync exists; persisted collection health, repair reports and per-collection controls do not. |
| #168 | Interactive controls, response sends, native flows and carousel rendering remain. |
| #161, #132, #131 | Operator chose separate RAM limits and explicit disk retention. Typed limits, bounded message window, SQLite paging and independent chat metadata still need implementation. Existing disk settings must survive migration. |
| #156 | Coverage improved; minimum complete media upload/download/failure and receipt-delivery harnesses still missing locally. Upstream hash tests pass but do not replace Postal integration coverage. |
| #155 | Logged store writes are partial; ignored account-file failures and swallowed reads remain. |
| #154 | Android/tablet pairing and Android handshake metadata are implemented. Minimal history capability set still requires controlled live pairing/history evidence. |
| #145 | Pruning and address-merge indexing improved in `464df36`; synchronous SQLite still runs in async service paths behind one mutex. |
| #130 | Original default-true/two-day premise is stale: defaults are false and opt-in requests full history. Original rename/limited-window acceptance must be reconciled with the selected storage model. |
| #127 | Chat-pin events are handled; acceptance also specifies pinned-message initial/resync consistency, which lacks equivalent proof. |
| #123 | Uploads still cross IPC as base64/full buffers. |
| #122 | New image actions are scoped, but existing `read_file` and APPDATA/APPCACHE asset scope remain broad. |
| #110, #109, #108, #107 | Call links/scheduling, screen share and group/1:1 calling product flows remain absent. |
| #103 | Scheduled-send queue, restart catch-up and management UI absent. |
| #98 | Conversation export and complete archive restore absent. |
| #83 | Required app-state mutations, including sticker favorites/recents, do not all reach matching UI. |
| #82 | Undecryptable stubs exist; general per-message phone re-request/retry action is incomplete. |
| #79 | Attachments send individually; album grouping, association and grid rendering absent. |
| #70 | Download fallback and capped retries exist; pinned upstream hash tests pass. Upload-side deletion named in the title remains unwired; full local retry integration proof is still needed. |
| #65 | Status screen, posting/privacy and interaction flows absent; status messages are excluded from ordinary chats. |
| #64 | No encrypted community-reaction receive/send path in Postal. |
| #62 | Event edit support is partial; encrypted poll edits/options and atomic inbound secret-edit handling remain. |
| #60 | No bot directory or streamed rich AI-response handling; a whatsmeow API is not a whatsapp-rust integration. |
| #39 | Linked-device listing/logout UI and commands absent. |
| #32 | Hierarchy/announcement presentation exists; community creation/linking remains. |
| #30 | Group info/invites/reporting exist; creation and full participant/settings administration remain. |

## Current incident follow-ups

- Sync indicator placement/spinner: `e1c4fd3`, checked in browser with single-
  and multiline composer and decorative motion disabled.
- Android handshake metadata: `977a6c3`; registration/login payload fixtures
  pass. Web companion transport and its three-part protocol version remain;
  operator explicitly selected supported metadata before native transport work.
- Retention: `464df36` reduces a 150,000-row, 1,000-chat prune from 10.229 s to
  0.516 s with identical deletions. This does not change disk-retention policy.
- Startup: quote-chat index removes repeated full-table scans during address
  reconciliation. New phase timings are needed to attribute the reported
  22-second startup gap; it is not claimed fully resolved.
- Duplicate chat rows: raw history conversation IDs can coexist with canonical
  live-message IDs until reconciliation. Live-store inspection after restart
  found no mapped LID/PN pairs remaining. Equal display names can also belong
  to different phone-number IDs; never merge by name alone.
