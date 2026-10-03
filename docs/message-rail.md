# Message rail contributor guide

The open chat has two separate bounds. SQLite holds the searchable local
archive. `MessageWindow` holds at most 150 messages by default, with a setting
range of 50–2,000. Changing this window or paging it never deletes SQLite rows.

## SQLite paging and ordering

`message_page` reads an indexed page for one chat. It fetches at most the
requested limit plus one row to report `has_more`, and clamps requests to
2,000. The cursor is `(timestamp, sort_order, id)`. `sort_order` preserves local
arrival order when wire timestamps tie; do not sort by random WhatsApp message
IDs alone.

The UI loads the latest page when a chat opens. Near the top of the scroll area,
it requests an older SQLite page. When *Load older messages when scrolling up*
is enabled and local history runs out, it asks the phone for older history.
Explicit day recall also pages local history before requesting the phone. Near
the bottom, browsing history loads a newer SQLite page. The Latest button
reloads the newest page. These pages merge into `MessageWindow`, which trims
the opposite edge and preserves the current reading direction.

This demand-driven paging keeps large archives off the renderer and avoids
reading history the user has not reached. Do not fetch the whole chat into the
page or couple disk retention to window eviction.

## Virtual rows and media

`MessageList.svelte` flattens day dividers, unread markers, message groups,
uploads, and typing state into one oldest-first `VList`. `bufferSize={0}` means
the virtualizer does not mount extra rows outside its visible range. This is
intentional: message rows can decode large images or video thumbnails, and
overscan mounts that media before it enters the viewport. More mounted media
raises DOM, decoded-image, and GPU memory use.

Keep rows keyed by their stable row key. Album child messages may share one
rendered group, but their invisible `data-id` anchors must remain attached to
that group so lookup, reveal, and read marking work. Do not add a second
scrolling element inside `VList` or eagerly mount the full `MessageWindow`.

## Prepending and scroll anchoring

When an older page arrives, `MessagesState` sets `prepending` for one animation
frame and merges the page at the older edge. `MessageList` passes this state to
`VList` through `shift`, which preserves the reader's visible place while rows
are inserted above. Do not force the list to the bottom during a prepend.
`anchorId()` reports the first visible message for state changes that need to
restore the reading position. `revealMessage()` scrolls a loaded message into
view; it returns `false` if the current window does not contain it.

## Read marking

The rail reports message IDs whose markers fall before the viewport's bottom
edge, including rows scrolled above the viewport. After a 200 ms scroll debounce,
the route finds the newest such message in chat order and calls `mark_read_until`
for that boundary. Messages below the fold stay unread. The route skips the
work when the window is unfocused and avoids repeating the last attempted
boundary. Preserve these rules when changing overscan, albums, or scroll
callbacks.

## Browser test hooks

`tests/browser/MessageWindow.svelte` exposes buttons for older/newer pages,
latest, jumping to a stored row, incoming rows, delayed requests, failures, and
empty-chat switching. It uses a synthetic 350-row archive and shows loaded RAM
count separately from disk count and fake phone requests.

`tests/browser/AlbumListHarness.svelte` checks the production `MessageList`
against synthetic rows. It verifies that the virtual list leaves offscreen raw
rows unmounted, that offscreen messages can still be found and revealed, and
that visible album anchors drive read marking without inventing IDs. The
standalone runner is `tests/browser/check-album-list.mjs`; its Vite fixture uses
`tests/browser/album-list.vite.config.ts` and a local server on `127.0.0.1:1464`.
These fixtures replace IPC and use synthetic data. They do not measure native
WebKit GPU or process memory.

`MessageRailHarness.svelte` checks a 400-message rail at 400 px and 320 px:
fewer than 30 mounted rows, no extra offscreen rows beyond boundary measurement,
stable scroll anchor after an append, unread-divider reveal, and live append
at the bottom. Run `node tests/browser/check-album-list.mjs --message-rail`
against the same fixture server. `src/tests/message-rail.test.ts` rejects a
source-level increase to `bufferSize` in the ordinary frontend CI suite.
The [native smoke job](rail-smoke-test.md) launches the Tauri shell with the
same synthetic rail and drives open/send actions through WebDriver.
