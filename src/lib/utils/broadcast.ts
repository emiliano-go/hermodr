export const BROADCAST_SEND_REASON = "Sending to broadcast lists is not supported.";

export function isBroadcastList(chat: string | null | undefined): boolean {
  return !!chat && chat.endsWith("@broadcast") && chat !== "status@broadcast";
}

export function broadcastSendReason(chat: string | null | undefined): string | null {
  return isBroadcastList(chat) ? BROADCAST_SEND_REASON : null;
}

export function guardBroadcastSend(chat: string | null | undefined): void {
  const reason = broadcastSendReason(chat);
  if (reason) throw new Error(reason);
}
