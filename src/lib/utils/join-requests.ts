import { changeText } from "./group-actions.ts";
import type { ParticipantChange } from "./models.ts";

export function joinRequestResults(jids: string[], changes: ParticipantChange[], approve: boolean) {
  return jids.map((jid) => {
    const change = changes.find((result) => result.jid === jid);
    const completed = !!change?.ok && !change.pending;
    return { jid, completed, text: change
      ? changeText(change) ?? (approve ? "Approved." : "Denied.")
      : "WhatsApp did not return a result for this request." };
  });
}
