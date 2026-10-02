import type { QuickReply } from "./wire";

export type QuickReplyScope = { account: string; chat: string; generation: number; requestKey: string | number };

export function quickReplyScopeMatches(scope: QuickReplyScope | null, account: string | null, chat: string, generation: number, requestKey: string | number): boolean {
  return !!account && !!chat && scope !== null && scope.account === account && scope.chat === chat
    && scope.generation === generation && scope.requestKey === requestKey;
}

export function filterQuickReplies(replies: readonly QuickReply[], query: string): QuickReply[] {
  const terms = query.trim().replace(/^\/+/, "").toLowerCase().split(/\s+/).filter(Boolean);
  return replies.filter((reply) => {
    const text = [reply.shortcut, reply.message, ...reply.keywords].join("\n").toLowerCase();
    return terms.every((term) => text.includes(term));
  });
}
