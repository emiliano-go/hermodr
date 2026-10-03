# Group history sharing: limits and recovery

Postal keeps prepared group-history shares and upstream retry tokens in memory
for the lifetime of the account service. It allows at most 16 pending shares
per account. A retry token expires after 15 minutes. The share payload itself
is not persisted by Postal.

When a share result includes a retry action, use **Retry history** in the Add
members dialog while that dialog and account service remain open. Postal reuses
the same upstream retry token, so a retry does not add the participants again.
The service returns an explicit error when the retry ID has expired or belongs
to another group. A missing ID returns `error.history_retry_expired`. A known
ID that is older than 15 minutes or belongs to another group returns
`error.history_retry_group`, localized as unavailable for that group. The
current message does not distinguish expiration from a group mismatch. At the
16-share ceiling, the service returns `error.history_pending_limit` instead of
silently discarding another share.

## App or service restart

Postal does not persist pending share IDs, retry tokens, or the last share
result. Restarting the app or account service discards that recovery state. A
group-member change that already reached WhatsApp may remain, but Postal cannot
tell whether the history bundle or its group notice completed. Check the
current group membership and the original result before describing the share as
sent; the success message itself says server acceptance does not confirm
recipient delivery.

The current Add members dialog filters out existing group members. It therefore
cannot recreate a failed history share for recipients whose member-add step
succeeded. The app has no history-only action for existing members. Do not
remove and re-add people to work around this gap; that changes group membership
and does not guarantee a history resend.

For recipients who are still selectable, an admin can create a new
history-enabled Add members request. This is a new share, not a retry of the
previous bundle. There is no safe in-app recovery for already-added recipients
after restart until Postal persists retry state or adds a share-only flow.
Track that work under [issue #238](https://github.com/emiliano-go/postal/issues/238).
