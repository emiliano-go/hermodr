// The words a group member change's server answer turns into.
import type { GroupHistoryOffer, GroupHistoryResult, ParticipantChange } from "$lib/utils/models";

/** Why the server refused one person, or that the add needs approval. `null`
 * when it simply worked. */
export function changeText(change: ParticipantChange): string | null {
  if (change.pending) return "Sent for approval.";
  if (change.ok) return null;
  const code = change.code ?? "";
  if (code === "403") return "Not allowed (admin rights or privacy).";
  if (code === "409") return "They are no longer in the group.";
  if (code === "404" || code === "406") return "That number is not on WhatsApp.";
  if (code === "408") return "Could not be reached right now.";
  return change.error ? `WhatsApp refused it (${code || "error"}).` : "WhatsApp refused it.";
}

export function historyReceivers(jids: string[], requested: boolean, offer: GroupHistoryOffer | null): string[] {
  if (!requested) return [];
  if (!offer?.enabled) throw new Error(offer?.reason ?? "History sharing is unavailable.");
  return [...jids];
}

export function historyResultText(result: GroupHistoryResult): string {
  return result.state === "shared"
    ? "WhatsApp accepted the history bundle and group notice. Delivery to each person has not been confirmed."
    : result.message;
}
