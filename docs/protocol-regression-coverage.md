# Protocol regression coverage

Source snapshot: 2026-10-03. This maps the risk areas tracked by [issue #156](https://github.com/emiliano-go/postal/issues/156) to Postal's automated regression fixtures and their limits. It is not evidence that a real WhatsApp account completed every workflow. Keep #156 open while pairing, reconnect and transport requirements remain unproved.

## Existing automated boundaries

| Risk area | Production-facing regression evidence | What remains outside this evidence |
| --- | --- | --- |
| QR/pairing | [`connection.rs`](../crates/postal-core/src/service/connection.rs): `pairing_uses_android_tablet_identity_in_both_history_modes`, `pairing_keeps_the_external_identity_when_not_android`. [`protocol_tests.rs`](../crates/postal-core/src/service/protocol_tests.rs): `pair_code_failures_classify_for_the_ui`. | Device properties and pairing-code error mapping are checked. A rendered QR being scanned, a completed pairing handshake, and phone/server acceptance are not proved. |
| Registration/reconnect | [`connection.rs`](../crates/postal-core/src/service/connection.rs): `registration_and_reconnect_use_android_tablet_handshake_metadata` calls the SDK's real device-payload builder for unregistered and registered identities. | Correct payload metadata does not prove socket reconnection, resume-watchdog recovery, or resumed delivery after sleep. |
| Disconnect/logout | [`inbound_lifecycle_tests.rs`](../crates/postal-core/src/service/inbound_lifecycle_tests.rs): `disconnect_clears_primary_and_companion_links_preserving_cached_rows`, `logout_clears_primary_and_companion_links_preserving_cached_rows` exercise actual `Inbound::handle` for primary and companion modes, including replay. | Core event dispatch, connected-state clearing and cached-row preservation are exercised. Native shell account routing and remote unlinking are not proved by this fixture. |
| Offline messages | [`chat_unarchive_tests.rs`](../crates/postal-core/src/service/chat_unarchive_tests.rs): `only_new_live_incoming_unarchives_with_nullable_override_and_global_default` distinguishes live, offline, history and offline-drain origins. | This proves local archive policy for synthetic events, not server backlog delivery, transport ordering or offline-sync completion end to end. |
| History admission/replay | [`history.rs`](../crates/postal-core/src/history.rs), [`history_floor_tests.rs`](../crates/postal-core/src/service/history_floor_tests.rs), and [`protocol_tests.rs`](../crates/postal-core/src/service/protocol_tests.rs): requested history floors, pairing/wrong-chat exclusions, early/duplicate completion and late-mapping replay. | Fixtures exercise Postal's handler/store boundary. They do not prove a real phone answered a history request or a server delivered every chunk. |
| LID/PN identity | [`store/tests.rs`](../crates/postal-core/src/store/tests.rs): `new_mappings_merge_immediately_and_survive_reopen`, `late_mapping_merges_local_state_and_rolls_back_on_failure`. [`protocol_tests.rs`](../crates/postal-core/src/service/protocol_tests.rs): `history_chunks_replay_under_one_chat_after_late_mapping`. | Covers local alias merge, persistence, replay and rollback. It does not establish that a remote identity mapping is authentic beyond the fixture's supplied event. |
| Delivery/read receipts | [`protocol_tests.rs`](../crates/postal-core/src/service/protocol_tests.rs): `a_delivery_receipt_arrives_as_a_status_hint`, `receipt_events_advance_delivery_without_regression`, `group_ticks_need_every_member`. [`store/tests.rs`](../crates/postal-core/src/store/tests.rs): `receipts_only_move_forward`. | Covers synthetic receipt processing, monotonic status and group aggregation. It does not prove receipt transmission or delivery to a real recipient device. |
| Media upload | [`media_files.rs`](../crates/postal-core/src/service/media_files.rs): `file_upload_matches_buffered_crypto_and_reopens_at_retry_offsets` uses actual SDK encryption, checks both hashes/sidecar and encrypted bytes at retry offsets. [`upload.test.ts`](../src/tests/upload.test.ts) and [`uploads.rs`](../src-tauri/src/uploads.rs) cover bounded staging, ownership, chunk order, cancellation and queue order. | Offline encryption and local staging are exercised. No CDN upload or remote message acceptance is proved by these fixtures. |
| Media download/reupload | [`media_download_tests.rs`](../crates/postal-core/src/service/media_download_tests.rs): `retries_replace_and_persist_locators_and_stop_after_one_reupload`, round-video retry and artwork-cache cases. [`media-retry.test.ts`](../src/tests/media-retry.test.ts) covers in-flight coalescing, retry caps and stale-account suppression. | Downloader/reupload callbacks and frontend IPC are synthetic. These tests do not contact the CDN or sender's phone. |
| Download integrity/publication | [`media_receive_hash_tests.rs`](../crates/postal-core/src/service/media_receive_hash_tests.rs) composes actual SDK encryption and stream verification with Postal's `receive_file`. [`media_receive_tests.rs`](../crates/postal-core/src/service/media_receive_tests.rs) also covers publication failure, partial-output cleanup and path traversal. | The new tests exercise real offline hash/MAC rejection and local publication. They do not exercise HTTP, automatic reupload after that rejection, or a real media message. |
| Send failures/uncertain outcomes | [`albums_tests.rs`](../crates/postal-core/src/service/albums_tests.rs): `partial_sequence_separates_before_send_failure_from_uncertain_item`, `account_fence_after_written_item_preserves_sent_prefix_without_retry`. [`event-actions.test.ts`](../src/tests/event-actions.test.ts) and [`poll-actions.test.ts`](../src/tests/poll-actions.test.ts) preserve queued scope, native failures and successful-send behavior after local refresh errors. | These are controlled transport outcomes/IPC fixtures. A local write or an SDK return value must not be relabelled as proof of recipient delivery. |
| Group metadata/protocol fixtures | [`group_settings_tests.rs`](../crates/postal-core/src/service/group_settings_tests.rs) checks current role/lock/token guards. [`service/tests.rs`](../crates/postal-core/src/service/tests.rs): `group_changes_invalidate_fetched_metadata_and_overviews`. [`protocol_tests.rs`](../crates/postal-core/src/service/protocol_tests.rs) exercises group notices, actors, raw values and replay identity. | These prove local handling of supplied SDK events/protobufs. They are not an independent wire decoder, real group mutation or proof of current server policy. |

## Measured core lifecycle fixture

Parent-run local verification on 2026-10-03: **2 lifecycle tests passed, 0 failed; 0.14 seconds of test execution**. These exercise four primary/companion mode-and-event cases plus replay through actual `Inbound::handle`, using a bare client with no account session. This is core handler evidence, not a remote logout or reconnect result.

## Actual SDK integrity fixture

The lockfile pins `whatsapp-rust`/`wacore` to `6f18f5d3c98a4e72a498eb4966c67a990da4a59d`. The fixture uses these public APIs through the existing dependency:

- `whatsapp_rust::wacore::upload::encrypt_media_with_key`
- `whatsapp_rust::wacore::download::DownloadUtils::decrypt_stream_to_writer_with_hashes`
- `whatsapp_rust::wacore::download::MediaDecryptionError::{EncryptedSha256Mismatch, PlaintextSha256Mismatch}`

The SDK's production download writer uses the same stream verifier. Postal's `receive_file` gives it an owned temporary file and publishes the destination only after verification returns successfully. The verifier can write plaintext before a final hash rejection, so rejecting the error alone is insufficient: the fixture also checks temporary-file cleanup and destination preservation.

Four test functions cover 13 scenarios:

| Fixture | Scenarios | Assertions |
| --- | --- | --- |
| `media_hash_valid_stream_publishes_only_after_verification` | Seven plaintext lengths: 0, 1, 16, 8191, 8192, 8193 and 32785 bytes. | Both declared hashes match; destination is absent during verification; published bytes equal the original plaintext; no temporary file remains. |
| `media_hash_encrypted_sha256_mismatch_discards_unverified_output` | Incorrect declared encrypted hash, with and without an existing destination. | Actual typed encrypted-hash rejection after streaming writes; existing verified bytes survive or destination stays absent; no temporary output remains. |
| `media_hash_plaintext_sha256_mismatch_discards_unverified_output` | Incorrect declared plaintext hash, with and without an existing destination. | Actual typed plaintext-hash rejection after streaming writes; same publication/cleanup assertions. |
| `media_hash_corrupt_ciphertext_discards_unauthenticated_output` | Tampered encrypted bytes, with and without an existing destination. | Actual SDK MAC rejection after streaming writes; same publication/cleanup assertions. MAC validation precedes encrypted-hash validation in the pinned SDK. |

Parent-run local verification on 2026-10-03: **4 tests passed, 0 failed; 0.03 seconds of test execution**, after module wiring. Compilation time is excluded. This result belongs to that local focused run; it is not a claim that every platform or CI job ran this new leaf.

The fixture uses synthetic bytes and a fixed test key. It creates no SDK client, HTTP request, protocol send, credential-store entry or real-account session. No new dependency is required.

## Focused reproduction

Run from the repository root with its configured Node/Rust toolchains. Windows needs the MSVC environment described in `AGENTS.md`. Linux/macOS preview/conversion tests need ffmpeg; CI installs it for the core job.

```sh
# Actual SDK verification plus Postal publication: four test functions.
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib media_hash_

# Payload metadata and pairing-code error mapping; no real pairing.
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib pairing_tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib pair_code_failures_classify_for_the_ui

# Synthetic handler/store scenarios.
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib disconnect_clears_primary_and_companion_links_preserving_cached_rows
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib logout_clears_primary_and_companion_links_preserving_cached_rows
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib protocol_tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib history_floor_tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib chat_unarchive_tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib late_mapping_merges_local_state_and_rolls_back_on_failure
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib receipts_only_move_forward
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib group_settings
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib group_changes_invalidate_fetched_metadata_and_overviews

# Offline upload parity, download orchestration and publication failures.
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib file_upload_matches_buffered_crypto_and_reopens_at_retry_offsets
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib media_download::tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib media_receive::tests
node scripts/cargo.mjs test --locked --manifest-path crates/postal-core/Cargo.toml --lib albums::tests

# Synthetic IPC/state/SSR fixtures, without a native browser or account.
node --experimental-strip-types --test src/tests/upload.test.ts src/tests/media-retry.test.ts src/tests/group-history.test.ts src/tests/poll-actions.test.ts src/tests/event-actions.test.ts
```

These commands are reproduction instructions, not additional recorded results. Apart from the parent-measured lifecycle and hash results above, this document does not assign a new pass/fail status to existing tests. Check each runner's selected-test count; zero matched tests is not evidence.

## CI and remaining proof

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) configures three jobs on each of Linux, Windows and macOS:

- Frontend: `pnpm check`, phone/format self-checks, `pnpm test`, function-length and Tauri ACL checks.
- Core: `node scripts/cargo.mjs test --locked --workspace`.
- Tauri shell: frontend/helper build followed by `node scripts/cargo.mjs test --locked --manifest-path src-tauri/Cargo.toml --features wire-types`.

The Node fixtures bind production state/functions/components to synthetic IPC. They do not execute the native command implementation or real WebView permissions. Postal's workspace tests also do not run every dependency's own test suite merely because that dependency is compiled.

Before closing #156, record direct evidence for the remaining requirements rather than treating payload/serialization tests as substitutes:

1. Native shell stale-account isolation on lifecycle events and any required remote logout semantics; the new core fixture does not exercise those boundaries.
2. Resume/reconnect lifecycle and cancellation behavior beyond registration metadata.
3. Any required completed pairing, remote unlinking, real receipt/media transfer or group behavior, with the evidence method and its limits stated explicitly.

Do not add another generic callback-error test to claim hash coverage. The new leaf already performs actual SDK hash/MAC verification and tests Postal's publication boundary. Likewise, adding a test that copies a watchdog predicate does not establish reconnect lifecycle behavior.
