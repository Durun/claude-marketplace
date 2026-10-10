import type { OutboxMessage } from '../shared/types.ts';

/** 届けられなかった記録を見分けるキー。同じメッセージが別の理由でまた失敗したら、別の失敗として扱う。 */
const failureKey = (m: OutboxMessage): string => `${m.id}:${m.error}`;

export const failureKeys = (outbox: OutboxMessage[]): Set<string> => new Set(outbox.filter((m) => m.error).map(failureKey));

/** 前に見たときには無かった、届けられなかったメッセージ。 */
export const newFailures = (known: Set<string>, outbox: OutboxMessage[]): OutboxMessage[] => outbox.filter((m) => m.error && !known.has(failureKey(m)));
