# Synthetic browser checks

Run `pnpm exec vite --config tests/browser/vite.config.ts` and open
`http://127.0.0.1:1432` in an isolated browser tab.
This harness imports production components but replaces IPC with synthetic
responses. Unknown commands fail; no account, database or native API is opened.
The harness is separate from the production SvelteKit routes.

Timestamp layout checks: Check timestamp layouts must pass for Segoe UI, Georgia
and monospace. The 288 cases cover one/two-character text and photo captions,
12/24-hour and Arabic times, stars, edits, incoming/outgoing ticks and 100/200%
UI scale. Geometry rejects overlaps and an extra empty time line.

Chat menu checks: expand Retained chat metadata, focus Quiet contact and press
Shift+F10. Tab reaches menu actions; Escape restores row focus. Media auto-download
uses the loaded override or global default. Click it and reopen to see the new
state. Toggle chat settings failure: an alert appears and the media toggle stays
disabled. Delay a settings read, open the menu, switch the synthetic account and
release the read: the old menu must stay closed. All actions stay in fixture state.

Passive preview checks: tab to Quiet contact and wait for its tooltip. It shows a
saved sender, text and an image placeholder without selecting the chat. Escape,
blur or list scrolling dismisses it. Clear preview calls, repeat with Test send
receipts off/on, then Show preview calls: only `message_page` may appear. Delay a
preview read, switch the synthetic account and release it: no tooltip returns.
Toggle preview failure: the error stays visible without opening or marking read.

Message selection checks: expand Message selection fixture and reset it. Ctrl-click
or Command-click one message, then click two more. Evict the oldest visible row:
the count stays three. Star/Unstar and React must target all three IDs, including
the evicted row; Copy includes their text in arrival order. Forward and Delete
use the complete selection. Cancel selection and Escape exit; a simulated failure
shows an error and retains selection. Show selection operations exposes only
synthetic command payloads. Narrow selection layout: actions wrap without horizontal overflow.

Flag diagnostics checks:

1. Expand Server feature flags: true, false and Not received have separate values.
2. Filter by name or code; clear the filter to see all rows.
3. Enable Watch, apply the synthetic delta, then wait five seconds: enabled turns false.
4. Simulate failure and refresh: error is visible and previous values remain.
5. Toggle diagnostics off and Count reads. Wait over five seconds and count again:
   the count must stay unchanged.

Chat metadata check: expand Retained chat metadata. Quiet contact must remain
pinned with its last activity and "No stored messages"; selecting it must set
the displayed selection. No expired message preview is supplied to the component.

Retention checks: open per-chat retention. Default means inherit; Forever and
No limit save unlimited tags. A one-week age with Default count saves
limited(168) and inherit. Open global retention, clear both numeric fields and
save: both limits must be unlimited. Saved payloads appear below the launch buttons.

Storage Manager checks: expand the manager, filter Synthetic A and switch between
Largest first and Oldest first. Cancel a deletion and confirm cleanup-call count
stays zero; confirm a deletion and verify updated totals/list. Clean chat media
and clear the picture cache independently. Toggle cleanup failure and attempt
another deletion: error must stay visible and file/totals remain unchanged.

Bounded message window checks: open the synthetic archive (350 disk rows, 100
loaded), page older/newer, and jump to stored 0005 without phone requests. Older
at that boundary asks the fake phone; "Phone has no older rows" ends the request.
An incoming row while browsing must leave the current range intact. Back to
latest resumes the newest range. Delay a page, then either inject an incoming
row or switch to the empty chat before releasing it: refreshes must not cancel
paging, and stale responses must not overwrite the other chat. Page failures
leave existing rows visible. No database or real IPC is used.
