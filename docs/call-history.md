# Call history: P1 capture

P1 consumes the pinned SDK's public `CallLogSync` event into the account's
existing message database. Main sessions capture calls; the one-time companion
ignores them. A changed row emits `CallHistoryChanged`; replay does not emit a
second change. Capture creates no chat, message, or missed-call notice.

The mutation index supplies call ID and creator. Direction uses SDK `from_me`,
not the record's unreliable `is_incoming`. A valid group address supplies the
chat target. Incoming direct calls use the creator; outgoing direct calls need
one distinct external peer after own-PN/LID filtering and local alias
canonicalization. Missing or ambiguous targets stay unavailable. Reads apply
current aliases, including mappings learned after capture, without network
identity or profile lookups.

Records upsert by call ID. Stale or equal mutation revisions cannot replace an
existing row. Conflicting incoming creator or direction is rejected. Newer
partial records preserve known optional clock, outcome, type, and video fields.
Raw start time and duration remain signed integers in SQLite and exact decimal
strings in the DTO. Missing fields stay null; unknown outcomes retain their raw
enum value. Mutation milliseconds order capture revisions only, never call dates.
The SDK can substitute receipt time for a missing mutation timestamp, so this
watermark does not prove causal ordering in that case.

IDs and addresses are limited to 256 bytes, participant input to 1,024 entries,
and reads to 1–200 rows. Capture has no total-storage cap or eviction policy.
Storage follows the existing account database's memory/disk and encryption
lifecycle. Schema registration also heals a version-stamped database missing the
capture table.

## Remaining scope

[Issue #167](https://github.com/emiliano-go/postal/issues/167) remains open.

- Source proof of `CallLogRecord.start_time` and `duration` units is missing.
  No normalized call dates, duration conversion, day grouping, or completed
  Calls UI is claimed.
- Call retention and local history/chat clear/delete integration are unfinished.
  Calls must use explicit retention policy without changing existing message
  counts or guessing age from raw clock values.
- Exact-ID reconciliation with existing `call-{call_id}` missed notices is
  unfinished. Capture alone does not resolve contradictory or duplicated
  signalling/history outcomes.
- The pinned SDK exposes `Set` call-log events without a public delete marker.
  Remote deletion propagation is unresolved; snapshot absence and `INVALID`
  outcome are not deletion instructions.
- UI navigation, paging, lifecycle/alias updates, and full runtime verification
  remain part of the issue. Synthetic capture tests do not prove live phone
  history behavior.
