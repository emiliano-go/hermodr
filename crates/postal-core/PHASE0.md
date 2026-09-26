# Phase 0 results

Verified against a real account (paired by QR, live traffic).

## Outcome

| Metric | v1 (WhatsApp Web in WebKit) | v2 (whatsapp-rust) |
|---|---|---|
| Idle RSS | ~2.2 GB | **35 MB** |
| RSS after connect | 2.2 GB+ | **40 MB** |
| RSS after 60s live traffic | 4.2 GB | **53 MB** |
| History downloaded | entire account (~20 GB) | none |
| Message history stored | 604k+ records, 774 MB | **0 rows** |

## What was proven

1. `whatsapp-rust` at rev `9eb43b9b` builds on **stable Rust 1.98**. `simd` is
   the only nightly-only default feature and is removed at that revision.
2. The repository's `.cargo/config.toml` injects nightly-only rustflags
   (`-Zshare-generics`, `-Zunstable-options`). These do **not** leak into a
   path dependency at build time, so consuming the local checkout on stable
   works.
3. Pairing by QR succeeds, connection establishes, and live text messages are
   received and decoded.
4. `HistorySyncAdmission` refuses the deep `FULL` history while keeping the
   recent window, contacts metadata, and on-demand fetches.
5. The session database stores **protocol state only**. Tables present:
   `device`, `sessions`, `identities`, `prekeys`, `sender_keys`, `msg_secrets`,
   `app_state_*`, `lid_pn_mapping`, `tc_tokens`. `sent_messages` is a retry
   buffer and was empty; `pending_inbound_messages` was empty. There is no
   message-history table.

## Observation

Some inbound events print as `<non-text>`. These are protocol/system messages
(key distribution, receipts, reactions) rather than failures; real text bodies
decode correctly, as shown by the received message content.

RSS climbed slowly under load (35 MB → 53 MB). This is gradual growth while
processing live events, not the unbounded growth seen in v1.

## Reproduce

```console
$ rm -f spike.db*
$ cargo run -p postal-core --bin spike
```

`SPIKE_HISTORY=accept` accepts the deep history sync, for comparison.

---

# Phase 1 verification

Ran `service-check` against the same paired account, with retention set to
24 hours and 20 messages per chat so the policy is observable within a run.

| Check | Result |
|---|---|
| Connects using a stored session | yes |
| Group messages stored with decoded senders | yes (43 chats) |
| Retention prunes on write | yes (logged each removal) |
| Per-chat cap enforced | yes (max 20 per chat, 0 chats over) |
| RSS while receiving live traffic | 124 MB |

Message bodies, chat ids, and LID senders all round-trip through the store.

---

# Session database growth

`msg_secrets` holds decryption keys so edits, reactions and poll votes can be
applied to their parent message. The library default keeps them for 30 days
(text) and 90 days (polls), which on a large account grows the session database
into the hundreds of megabytes.

| Profile state | Rows | File size |
|---|---|---|
| Library default horizon (30d) | 399,620 | 97 MB |
| Retention-matched horizon (24h) | 17,062 | 5.8 MB |

The cache configuration now caps the secret horizon at the message retention
window, with a one-hour floor so an edit arriving just after its parent is
never lost. Existing profiles are reclaimed on startup: secrets whose parent is
already outside the window are deleted, remaining deadlines are shortened, and
the file is compacted when a significant freelist has built up. SQLite reuses
freed pages rather than releasing them, so without the compaction the file
stays large even after the rows are gone.
