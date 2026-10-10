import type { Session, SessionStatus } from '../shared/types.ts';

export const ARROW_STEP: Record<string, number> = { ArrowUp: -1, ArrowDown: 1 };

/** key を target の位置へ動かす。 */
export const moveTo = (keys: string[], key: string, target: string): string[] => {
  const rest = keys.filter((k) => k !== key);
  const i = keys.indexOf(target);
  return [...rest.slice(0, i), key, ...rest.slice(i)];
};

const STATUS_RANK: Record<SessionStatus, number> = { busy: 0, idle: 1, done: 2, interrupted: 2, ended: 3 };

const bySessionOrder = (a: Session, b: Session): number =>
  STATUS_RANK[a.status] - STATUS_RANK[b.status] || b.updatedAt - a.updatedAt;

/** 人間が並べ替えた順に従う。並べ替えた後に現れたセッションは、並べ替えたものより前に、状態と更新の順で置く。 */
export const arrangeSessions = (sessions: Session[], order: string[]): Session[] => {
  const rank = new Map(order.map((id, i) => [id, i]));
  return sessions.toSorted((a, b) => (rank.get(a.id) ?? -1) - (rank.get(b.id) ?? -1) || bySessionOrder(a, b));
};
