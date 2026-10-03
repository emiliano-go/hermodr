import { uiError } from "../state/localized.ts";

export const BROADCAST_SEND_REASON = "Sending to broadcast lists is not supported.";

export function isBroadcastList(chat: string | null | undefined): boolean {
  return !!chat && chat.endsWith("@broadcast") && chat !== "status@broadcast";
}

export function broadcastSendReason(chat: string | null | undefined): string | null {
  return broadcastSendError(chat)?.message ?? null;
}

export function broadcastSendError(chat: string | null | undefined) {
  return isBroadcastList(chat) ? uiError("error.state.broadcast_send") : null;
}

export function guardBroadcastSend(chat: string | null | undefined): void {
  const failure = broadcastSendError(chat);
  if (failure) throw failure;
}
