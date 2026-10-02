# Postal

![Postal](assets/banner.jpg)

A native WhatsApp desktop client. It speaks the WhatsApp multi-device protocol
directly instead of embedding WhatsApp Web in a browser engine.

## Why

The obvious way to build a WhatsApp desktop client is to wrap WhatsApp Web in a
webview, which is what Altus (and most alternatives) do. That approach has a
cost: WhatsApp Web treats a desktop client like another browser tab, so it syncs
the account's **entire message history** into local storage and then keeps it in
memory. On a large account that means tens of gigabytes downloaded, multi-
gigabyte memory use, and a client that gets slower the more history you have.

Postal takes the other path. Because it implements the protocol itself:

- **History sync is a decision this program makes.** Pairing brings the recent
  window only unless *Settings → Storage & history → Request full history when
  pairing* is on (off by default). This requests up to 10,000 days; the phone
  may supply less. Older messages load on demand as you scroll.
  Sync requests never change disk retention. Legacy full-history settings
  migrate to explicit unlimited global disk retention, preserving their effect.
- **Disk history and RAM have separate limits.** SQLite is a durable archive,
  unlimited on new installations unless explicit disk retention is configured.
  Existing disk policies survive upgrades. The open conversation keeps a bounded
  `MessageWindow` (150 messages by default, adjustable from 50 to 2,000), drawn
  through a virtualized rail: only the rows in and around the viewport are
  mounted, so DOM and decoded media stay bounded by the screen rather than by
  the conversation. Scrolling up pulls older pages from SQLite and asks the
  phone only once local history runs out; scrolling down pulls newer pages back
  in. Eviction from RAM never deletes disk rows, and the floating Latest button
  returns to live messages.
  History can also be cleared, or kept in memory only.
  The archive lives in the app data folder by default; *Settings → Storage &
  history → History folder* points it at another drive (cold storage) and moves
  the existing archive there on the next start. "Delete on this computer" never
  removes a message: it is flagged locally only and shown greyed out, so only
  disk retention or clearing history actually remove rows.
  Disk retention deletes expired messages strictly, including a quiet chat's
  last message. Chat identity, last activity, names and pins survive separately;
  an empty chat has no retained message preview. Delete chat removes it from the
  list; clearing history keeps chat metadata.

- **No browser engine for WhatsApp.** The app speaks the protocol itself: no
  WhatsApp Web page to keep alive, no per-tab network processes, no account
  history synced into a browser store. Webviews render this app's own UI.

Retention limits use explicit `limited`, `unlimited` and per-chat `inherit`
states. Existing numeric settings and per-chat overrides migrate without changing
their disk policy. A limited count of zero means retain no messages; unlimited
is a separate state. Global settings cannot inherit.

## Measured impact

Measurements comparing the old webview approach with this one. The Altus figures
are from the original comparison; the Postal ones reflect current `master`.
These are not measurements of unlimited archive mode:

| Metric | Altus (WhatsApp Web in a webview) | Postal |
| --- | --- | --- |
| CPU, idle | ~200% of one core, sustained | **~0.3%** |
| Memory, whole app | ~2.2 GB, climbing to ~23 GB | **~0.5–0.9 GB**, depending on cached media |
| GPU memory, open chat | grows with the whole synced page | **~0.2–0.3 GB**, viewport-bounded |
| History downloaded at pairing | entire account (~20 GB) | none |
| Message history stored | 604k+ rows, 774 MB | bounded by retention |

Memory is the whole app. The protocol core is around 35 MB; the rest includes
the WebKit UI in the measured main window. Browser engines render only the UI.
The open conversation is virtualized (`virtua`): only the visible window is
mounted, and decoded images leave the renderer as rows scroll away. Before the
virtualized rail, a media-heavy chat pushed the client's GPU memory past 3 GB
because every photo in the message window was decoded at once; the same chat now
settles around 0.2 GB. An unlimited SQLite archive can grow on disk; these
measurements do not establish its maximum size or query latency.

CPU was sampled with `pidstat` in 30-second windows. Altus held 130-220% of one
core the entire time and its RSS kept climbing toward the full 23 GB history, so
it never reaches a true idle. Postal sits under 1% when idle.

This is not a knock on Altus. It is a good project, and a fairly optimized one;
the numbers above are a property of the approach, not of its authors. Any client
that drives WhatsApp Web inherits the web app's behaviour: it has to pull the
account's history in, and it has to keep the page holding that history alive.

The session database also needed bounding: decryption secrets for edits and
reactions default to a 30-day horizon, which grew one profile to 97 MB across
399,620 rows. The horizon now tracks the message retention window, and existing
profiles are reclaimed on startup (**97 MB → 5.8 MB**).

## The name

**Postal** is the service that carries a letter from one person to another:
it takes what you wrote, routes it, and hands it over intact. A chat client
does the same with messages, one delivery at a time. More in
[docs/name.md](docs/name.md).

## Architecture

```
crates/postal-core/    protocol client, storage, retention
  history.rs            which history-sync chunks to accept
  store/                SQLite repository and explicit DiskRetentionManager
  service/              connection lifecycle, protocol handlers, typed events
crates/postal-plugins/   sidecar lifecycle, consent, bounded event delivery
src-tauri/              Tauri shell: commands and event forwarding
src/lib/state/          Svelte 5 state and IPC coordination
src/lib/messages/       virtualized message rail (virtua) and bubbles
src/lib/                chat, composer, media, settings, UI and utilities
src/routes/+page.svelte application layout and component wiring
src/routes/float/       account-bound floating chat windows
```

`postal-core` is built on [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust),
a pure-Rust implementation of the WhatsApp multi-device protocol.

Service database calls use `StoreWorker`/`AliasWorker`: an async gate queues callers
before `spawn_blocking` runs SQLite. Reads and writes share one connection per
service. Live/history batches hold an exclusive lease through their savepoint;
background downloads and other callers wait without occupying Tokio workers.
Cancellation lets an already-started write finish before releasing the savepoint
and lease. Network requests remain async and do not run in the blocking pool.

One connection preserves batch ordering and avoids cross-connection cache and
transaction rules. A large query can delay another database request, but cannot
stall unrelated async tasks. Add separate read connections only if measured read
latency warrants them. The optional Android companion owns its own connection;
SQLite coordinates those connections through WAL and its busy timeout. Synchronous
`MessageStore` remains available for startup migrations, CLI tools and tests;
async callers must use the worker boundary.

Settings → Storage & history → Export and local backup writes a new folder under
the chosen destination. Conversation exports contain JSON pages of up to 500
messages with their marks and downloaded attachments. Full backups contain a
snapshot of the message database, local aliases, referenced attachment files and
a versioned manifest. Shared files are copied once; missing downloads are counted
and retain their media locators for later recovery. Session credentials and
unreferenced cache files are excluded. Backups contain private content and are
not encrypted.

Restore creates a separate account and media folder; it never replaces an active
account or imports a linked session. Enable history storage, then select “Restored
backup” in Accounts and link the same WhatsApp account again. Current disk
retention applies once that account receives messages. Version 1 backups migrate
older supported schemas; newer schemas, unsupported formats, corrupt databases and unsafe
attachment paths fail explicitly. Incomplete imports are removed before the
account is registered. Backups can be moved between folders on the same machine.

Attachments larger than 1 MiB cross IPC in 256 KiB chunks and are staged under
the app cache. Upload tokens belong to an account, expire after an idle hour,
and are removed after success, failure or cancellation. At most eight uploads
can wait in staging. Ciphertext is generated into a second temporary file and
streamed through the pinned library's Ureq transport, including resumed uploads.
Small attachments retain their single-call path. GIF library sends and ordinary
media forwarding use cached files directly. Prepared stickers are staged before
encryption; conversion accepts at most 64 MiB compressed input and bounds decoded
images to 8192 pixels per side and 128 MiB. Staging and encryption can temporarily
require roughly twice the attachment's size on disk.

Downloads, including automatic live downloads and quoted copies, stream to
temporary files and publish a cache path only after verification succeeds. Failed
automatic downloads retain their locator for a later attempt. Retry attempts truncate partial output.
Video previews read files directly; image previews cap decoding at 8192 pixels
per side and 128 MiB. These bounds cover transfer buffers, not total application
memory: voice/sticker conversion and Linux's playback fallback can still buffer
media. Switching accounts cancels queued sends and unfinished attachment staging.

Right-click a chat or group and choose **Float chat** to open a separate window.
Up to eight chats can float at once; reopening the same chat focuses its window.
Floats show local history and accept text replies, with a background transparency
slider that keeps text opaque. Drafts are saved on this device separately from the
main composer. Account switching or logout closes floats; they never follow another
account. Media and unsupported message types use readable summaries. Native macOS
transparency uses Tauri's private API feature, which prevents App Store acceptance.

## Installing

Releases are on hold until they cover every platform Postal supports, so the
installer builds from the tip of `master` on this machine:

```console
curl -fsSL https://raw.githubusercontent.com/emiliano-go/postal/master/scripts/install.sh | sh
```

It fetches the source, builds it with the local toolchain, and puts the binary
in `~/.local/bin` along with a desktop entry. Set `POSTAL_REF=v0.1.0` to pin a
tag or commit, and pass `--debug` for a quicker, unoptimized build:

```console
curl -fsSL https://raw.githubusercontent.com/emiliano-go/postal/master/scripts/install.sh | sh -s -- --debug
```

There is no Windows shell installer yet; build from a clone as below.

## Building

To build from a clone:

```console
scripts/install-dev.sh --release   # optimized build and install (Linux, macOS)
scripts/install-dev.sh             # quicker unoptimized build
scripts/install-dev.sh --dev       # start the dev server
```

On Windows, run `pnpm install` then `pnpm tauri build` in the clone; the
installers land in `src-tauri/target/release/bundle/`.

The release bundles, AppImage included, are built with `scripts/build-release.sh`,
which sets the two environment variables the AppImage tooling needs on current
distros, and leaves the files to upload (AppImage, icon, `SHA256SUMS`) in
`dist/`. `install-dev.sh` never bundles, so it does not run into them.

Dependencies are declared in `Cargo.toml`: Tauri comes from crates.io, and
`whatsapp-rust` is pinned to a git revision because per-chunk history control
(`HistorySyncAdmission`) and phone-number pairing are newer than its last
release. Cargo fetches both, so there is nothing to clone by hand. `.cargo/config.toml` has Cargo use the system
`git`, so a global HTTPS-to-SSH rewrite still works.

Requirements: Rust 1.94+ (stable), Node with pnpm, and the usual Tauri Linux
dependencies (WebKitGTK 4.1, GTK 3). The dev script does not install system
packages; it prints what the build needs. On Linux, `ffmpeg` on `PATH` also
lets videos play when the system's GStreamer cannot decode their AAC audio:
the file is remuxed to H.264 + Opus under the media cache on first play.

Builds are tuned for iteration: dev binaries carry line tables only and
dependencies none, the scripts skip the frontend build when it is current,
and installs from HEAD build at `opt-level = 3` without LTO unless
`--release` asks for the thin-LTO profile. `sccache` on `PATH` is picked up
automatically and shares compiled dependencies across builds; `mold` can
replace `lld` for linking (`RUSTFLAGS="-C link-arg=-fuse-ld=mold"`).

On Wayland, WebKitGTK's DMA-BUF renderer can fail with `Gdk Error 71`. Under
NVIDIA the failure comes from explicit sync, so Postal keeps the accelerated
renderer and sets `__NV_DISABLE_EXPLICIT_SYNC=1` instead; other Wayland drivers
fall back to `WEBKIT_DISABLE_DMABUF_RENDERER=1`. X11 needs neither. Environment
variables set by hand always win.

## Diagnostics

Settings → About → Server feature flags shows watched boolean A/B properties,
their registry defaults and their last received server values. Refresh reads the
local cache; Watch checks it every five seconds while the panel is open.
"Not received" is distinct from false. Postal currently exposes these flags for
diagnostics without using them to gate product features.

New links pair as an ordinary External companion, by QR or by phone-number code.
For the code flow, enter the number with its country and Postal asks WhatsApp
for an eight-character code; confirm it on the phone under *Linked devices →
Link with phone number*. The code refreshes until it is used or expires, with a
manual refresh beside it.

Settings → Device can add an Android companion: a second link that pairs as an
`ANDROID_TABLET` running WhatsApp Android `2.26.32.84`, published on the
[official download page](https://www.whatsapp.com/download) when checked on
2026-09-27, to receive the one-time photos, videos and voice notes the External
link never gets. It pairs the same two ways — QR or phone-number code — in its
own short step, and can only be enabled once linked. It shares the message
store, wakes when a one-time message arrives, fetches it, and goes dormant
again.

The companion's handshake sets Android metadata (`ANDROID`, device `Tablet`,
Android `13`) and omits browser `WebInfo`. This is the library's supported
profile: transport remains the Web companion socket, with its separate
three-part protocol version. Full native Android transport and a four-part
handshake version require upstream support.

Two binaries exercise the core without the UI:

```console
cargo run -p postal-core --bin spike          # pair by QR, print a terminal QR
cargo run -p postal-core --bin service-check  # store messages, show retention
```

`spike` renders its pairing code as Unicode blocks, so it needs no image viewer.
Set `SPIKE_HISTORY=accept` to compare behaviour when the full history is allowed.

Settings → Advanced turns the library's verbose logging (keepalive pings,
transport frames, link probes) on or off; it is on by default, and applies the
next time Postal starts. A watchdog also samples the transport and probes a
link that has moved no frames for minutes, so a half-open socket reconnects
instead of leaving the app silently offline.

The app itself logs to `postal.log` in its data directory (*Settings → About →
Open log*): connection changes, sync progress, per-batch timings, failed store
writes, failed UI commands and crashes with a backtrace. `RUST_LOG` overrides
the levels; past 5 MB the file moves to `postal.log.old`.

Tolerated database and media-cache failures use the `postal_core::storage`
error target with the calling source location. Missing message rows are normal
and stay quiet. Malformed protocol messages still skip without stopping the
event loop. Broadcast sends without subscribers are expected during shutdown;
Tauri event-emission failures and lagged consumers are logged separately.

## Media

Right-click an image for **Copy Image**, **Save Image…**, or **Open Image**.
Save uses the native file picker and preserves the original file. Copy places
decoded image pixels on the clipboard. Other attachments offer Save and Open.
Missing files download on demand; revoked and unkept one-time messages cannot be
exported. Messages deleted on this computer stay in the archive greyed out, so
their media can still be saved. One-time media kept by the Android companion
follows ordinary attachment actions.

Downloaded media is written to the folder set in Settings, which defaults to the
app data directory. The asset protocol is scoped to whatever folder is
configured, so a custom location is served to the UI as well.

Small attachments use one base64 IPC call. Files larger than 1 MiB use bounded
256 KiB chunks, account-owned staging files and file-backed uploads. Downloads
stream to verified temporary files before publication. This bounds transfer
buffers; browser preview decoding, sticker conversion and playback fallbacks
can still buffer media. Inline media in the open chat is mounted only for the
visible window of the virtualized rail, so decoded images leave the renderer as
they scroll away.

## Security

- Message and protocol-session databases, downloaded media and backups are
  plaintext on disk. WhatsApp transport encryption does not encrypt these
  local files. OS database encryption and password-locked chats remain open work.
- The UI runs under a content security policy: scripts only from the app, no
  remote fonts, images or connections. Media uses explicit asset, blob and data
  sources. Inline styles remain allowed for Svelte and user themes; inline
  scripts and eval are not allowed. Development additionally allows the local
  Vite server and its reload websocket.
- Link previews for messages you send are fetched by the app, from public
  HTTP(S) addresses only (checked after DNS resolution at connect time, including
  redirects and preview images). Remote sites see your device's request and
  public IP. Previews are skipped when an HTTP proxy is configured because its
  target resolution cannot be verified locally; Postal does not bypass it.
- CSS extensions are trusted local customization: they can restyle or hide
  anything. Sanitization only prevents closing the surrounding `</style>` tag;
  it does not make CSS safe or prevent resource loads. The CSP blocks remote
  loads, while resources allowed by that policy remain accessible to CSS.
- Voice-note transcription is opt-in. The local Whisper plugin keeps audio on
  the machine; choosing a cloud provider sends it the notes you transcribe,
  automatically if auto-transcribe is on.

## Testing

```console
cargo test --locked --workspace
pnpm check
pnpm test
node --experimental-strip-types src/lib/utils/format.ts
node --experimental-strip-types src/lib/utils/phone.ts
pnpm check:tauri-acl
pnpm check:fn-length
pnpm check:wire
pnpm build
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

CI runs frontend, core/plugin, and Tauri shell tests on Linux, Windows, and
macOS. Linux and macOS core tests require `ffmpeg` on `PATH`; Windows uses
Media Foundation. Video tests require both portrait and landscape decoding,
including extensionless files and paths with spaces and Unicode. Filesystem
and plugin tests use synthetic data and subprocesses, without opening Postal
or reading application accounts. Native dialogs, desktop portals, keychains,
notifications, and playback still require interactive platform verification.

## Status

This describes current `master`. Tagged release notes describe their release;
the latest download can lack changes listed here.

Working:

- Pairing by QR or phone-number code, and re-signing in with the stored
  session; several accounts, switchable from the account menu
- Text with WhatsApp formatting, mentions, replies and quotes, edits, deletes,
  forwards, reactions, stars and pinned messages and chats
- Images, video, GIFs, stickers, documents and voice notes: received inline,
  sent from the composer, played in place; view-once media kept through the
  optional Android companion
- Polls and events, including voting and RSVPs
- Link previews as Discord-style embeds; locations and contacts as cards
- Live locations update in place with their map snapshot, accuracy, speed and heading,
  show how fresh the last position is, and keep it as ended once the share stops
- Delivery and read receipts, message info, typing and presence
- Group creation, participant add/remove/promote/demote, subject/photo/description
  editing, admin-only settings, member tags and admin reports; profiles and privacy
  settings
- Stored system notices for group membership, permissions, disappearing messages,
  security changes and missed calls
- A mentions inbox, starred messages and search within a chat; a quick switcher
  across chats, contacts and messages; and a notification history that jumps
  back to its message
- Slash commands in the composer: polls, events, stickers, GIFs, locations,
  @all mentions and keep-in-chat
- Labels with colors for chats and messages, created and managed from the chat
  list
- Voice-note transcription through plugins (a local Whisper binary, or the
  OpenAI/Deepgram APIs), automatic for downloaded notes, with per-chat overrides
- A Standard or HD (original) quality choice for outgoing media, and a local
  soundboard of per-chat clips with keyboard shortcuts
- Scheduled messages, with an outbox to review, retry or cancel a send before
  it goes
- A chat preview on hover that shows recent messages and can page through and
  play their media, toggleable in Settings
- Retention with per-chat overrides, clearing history, in-memory-only history,
  and a virtualized, scroll-driven window that pages history in from disk
- Themes (System, Dark, Light, Midnight, Liquid Glass, Material 3), a theme editor with
  live previews, CSS extensions, background pictures per app or per chat,
  density, text size and animation settings
- Desktop notifications for direct messages and groups, with per-chat mutes
  and a global toggle under settings/notifications
- Multi-message selection, ordered forwarding to several chats, bulk local deletion
  and eligible delete-for-everyone actions
- JSON conversation export and local backup/restore into a separate account

Not yet: calls, the tray icon, status updates,
communities and channel management, broadcast lists and albums. The issue
tracker has the details.

## License

MIT, see [LICENSE](LICENSE). Third party dependencies keep their own
licenses, listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
