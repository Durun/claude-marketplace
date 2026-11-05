// セッションの状態の変わり目のうち、トーストで知らせるもの。
import type { Attention, AttentionKind, Session, SessionStatus, Snapshot } from '../shared/types.ts';

/** 人間の答えを待つ要対応と、待っているものの呼び方。入力待ちの発言（reply）は、作業の完了として知らせる。 */
const WAITING: Partial<Record<AttentionKind, string>> = { ask: '回答', question: '回答', command: 'コマンドの実行', permission: '権限の確認', review: 'レビュー' };

/** done は作業を終えた、ask は人間の答えを待ち始めた、auto はデンジャラスモードでダッシュボードが代わりに許可や実行をした。ask は、待っている要対応を持つ。 */
export type SessionEvent = { kind: 'done' | 'ask' | 'auto'; session: Session; text: string; attention?: Attention };

/** 前に見たときの、セッションごとの状態と、答えを待っていた要対応の id と、デンジャラスモードで代わりにした回数。 */
export type SessionsSeen = { status: Map<string, SessionStatus>; asks: Set<string>; autoActions: Map<string, number> };

const waiting = (snapshot: Snapshot): Attention[] => snapshot.attention.filter((a) => WAITING[a.kind]);

export const sessionsSeen = (snapshot: Snapshot): SessionsSeen => ({
  status: new Map(snapshot.sessions.map((s) => [s.id, s.status])),
  asks: new Set(waiting(snapshot).map((a) => a.id)),
  autoActions: new Map(snapshot.sessions.map((s) => [s.id, s.autoActions?.count ?? 0])),
});

/** 前に見たときから、デンジャラスモードで代わりにしたこと。続けて何件もしたら、最後のものに件数を添える。 */
const autoEvents = (seen: SessionsSeen, sessions: Session[]): SessionEvent[] =>
  sessions.flatMap((session): SessionEvent[] => {
    const auto = session.autoActions;
    const added = (auto?.count ?? 0) - (seen.autoActions.get(session.id) ?? 0);
    if (!auto || added <= 0) return [];
    const more = added > 1 ? `（ほか ${added - 1} 件）` : '';
    return [{ kind: 'auto', session, text: `${session.name}: デンジャラスモードで${auto.last}${more}` }];
  });

/**
 * 前に見たときから、答えを待ち始めたセッションと、作業中から止まったセッション。アーカイブしたセッションは知らせない。
 * 答えを待って止まったセッションは、待っていることだけを知らせる。
 */
export const sessionEvents = (seen: SessionsSeen, snapshot: Snapshot): SessionEvent[] => {
  const live = new Map(snapshot.sessions.filter((s) => !s.archived).map((s) => [s.id, s]));
  const asks = waiting(snapshot).flatMap((a): SessionEvent[] => {
    const session = live.get(a.sessionId);
    if (!session || seen.asks.has(a.id)) return [];
    return [{ kind: 'ask', session, attention: a, text: `${session.name} が${WAITING[a.kind]}を待っています: ${a.title}` }];
  });
  const asking = new Set(asks.map((e) => e.session.id));
  const done = [...live.values()]
    .filter((s) => seen.status.get(s.id) === 'busy' && (s.status === 'idle' || s.status === 'done') && !asking.has(s.id))
    .map((session): SessionEvent => ({ kind: 'done', session, text: `${session.name} が完了しました` }));
  return [...asks, ...autoEvents(seen, [...live.values()]), ...done];
};
