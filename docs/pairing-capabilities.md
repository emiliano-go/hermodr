# Pairing capabilities

Postal preserves the pinned SDK's WhatsApp Web history capability mirrors.
These are compatibility claims sent during registration, not a list of
fully implemented Postal features. No source-only test establishes the
smallest flag set that still delivers history from a real phone.
[Issue #154](https://github.com/emiliano-go/postal/issues/154) remains open
for that server-behavior question.

## Identities and history requests

The desktop's main External link uses `os = "Postal"` and
`PlatformType::UWP`. UWP is the established pairing protocol identity on
every host; it does not describe the computer's OS or make Postal a UWP app.
The optional companion uses `Android`, `ANDROID_TABLET`, and the Android
app version already selected by Postal. Its handshake profile uses Android
metadata while transport remains the Web companion socket.

These branches are separate: `config_for` sets `android_pair = false`;
`once_config_for` sets it to `true`, disables full-history requests, and
rejects history chunks through the companion's admission policy. Pairing
metadata changes do not update an existing link's identity.

Recent-only pairing leaves `require_full_sync` and the history config
override unset. The SDK then supplies `require_full_sync = false` and its
default capability mirrors. Full-history pairing additionally requests:

| Field | Value |
| --- | --- |
| `require_full_sync` | `true` |
| `full_sync_days_limit` | `10_000` |
| `on_demand_ready` | `true` |
| `complete_on_demand_ready` | `true` |

This is a request. The phone may supply less history. Postal's
`HistoryPolicy` separately decides which chunks to accept; disk retention
is another policy. Full-history opt-in does not grant a different set of
capability booleans.

## Mirrored flags

All twelve fields below remain `Some(true)` in both history modes. Postal
uses `default_history_sync_config()` for the full-history override rather
than maintaining a second copy. At the current pin, this preserves the
registration bytes.

| Mirrored field | Evidence boundary |
| --- | --- |
| `inline_initial_payload_in_e2_ee_msg` | Pinned SDK compatibility claim |
| `support_bot_user_agent_chat_history` | Pinned SDK compatibility claim |
| `support_cag_reactions_and_polls` | Pinned SDK compatibility claim |
| `support_recent_sync_chunk_message_count_tuning` | Pinned SDK compatibility claim |
| `support_hosted_group_msg` | Pinned SDK compatibility claim |
| `support_biz_hosted_msg` | Pinned SDK compatibility claim |
| `support_fbid_bot_chat_history` | Pinned SDK compatibility claim |
| `support_message_association` | Pinned SDK compatibility claim |
| `support_call_log_history` | Pinned SDK compatibility claim |
| `support_group_history` | Pinned SDK compatibility claim |
| `support_manus_history` | Pinned SDK compatibility claim |
| `support_hatch_history` | Pinned SDK compatibility claim |

The SDK documents these defaults as WhatsApp Web's static claims and says
it handles variants or treats them as opaque. That statement does not prove
Postal provides each feature, or that every claim is necessary for server
history delivery. Postal decodes supported messages and can store unsupported
nonempty payloads as unknown message rows. Such fallback handling is not full
support for bot history, hosted messages, call logs, Manus, or Hatch.

No flag has been removed. Deleting a field from Postal's override would
also not disable it in recent-only mode, which inherits the SDK default.
Disabling a claim would need an explicit config override and controlled
new-link history evidence. That check has not been performed.

## Source and regression evidence

- [Postal pairing producer and synthetic byte regression](../crates/postal-core/src/service/connection.rs):
  `pairing_props` and `pairing_capability_mirrors_preserve_registration_bytes`.
- [Desktop and optional companion configuration](../src-tauri/src/account_store.rs):
  `config_for` and `once_config_for`.
- [History admission](../crates/postal-core/src/history.rs): `HistoryPolicy`.
- [Message projection](../crates/postal-core/src/service/message_decode.rs):
  supported cards and the unknown-message fallback.
- [Pinned SDK defaults and registration encoding](https://github.com/oxidezap/whatsapp-rust/blob/6f18f5d3c98a4e72a498eb4966c67a990da4a59d/wacore/src/store/device.rs):
  `default_history_sync_config`, `DEVICE_PROPS`, `set_device_props`, and
  `get_registration_payload`.

The regression checks encoded registration `DeviceProps` against an
independent twelve-flag fixture for External/Android and recent/full modes.
It catches flag additions, removals, presence changes, and altered request
limits when the SDK pin changes. It proves payload construction when run;
it does not pair a device or prove server delivery.
