import type { ChatEntry, OutboxMessage } from '../shared/types.ts';

/** 届けられなかった記録を見分けるキー。同じメッセージが別の理由でまた失敗したら、別の失敗として扱う。 */
const failureKey = (m: OutboxMessage): string => `${m.id}:${m.error}`;

export const failureKeys = (outbox: OutboxMessage[]): Set<string> => new Set(outbox.filter((m) => m.error).map(failureKey));

/** 前に見たときには無かった、届けられなかったメッセージ。 */
export const newFailures = (known: Set<string>, outbox: OutboxMessage[]): OutboxMessage[] => outbox.filter((m) => m.error && !known.has(failureKey(m)));

/** 送ったメッセージが会話に現れた位置。送った後の依頼のうち、文面を含む最初のもの。 */
export const chatIndexOf = (chat: ChatEntry[], m: OutboxMessage): number =>
  chat.findIndex((e) => e.kind === 'prompt' && (e.at ?? m.createdAt) >= m.createdAt && e.text.includes(m.text.trim()));

/**
 * セッションへ送ったメッセージのうち、Claude がまだ読んでいないもの。会話に現れた最後のメッセージより後に送ったものを、送った順に返す。
 * Claude は送った順に読むので、それより前に送って会話に無いものは、巻き戻して捨てた分岐に宛てたもの。
 */
export const unreadMessages = (outbox: OutboxMessage[], sessionId: string, chat: ChatEntry[]): OutboxMessage[] => {
  const sent = outbox.filter((m) => m.sessionId === sessionId);
  return sent.slice(sent.findLastIndex((m) => chatIndexOf(chat, m) >= 0) + 1);
};
