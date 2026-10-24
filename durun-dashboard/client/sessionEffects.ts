import { useEffect, useRef } from 'preact/hooks';
import type { Session, Snapshot } from '../shared/types.ts';
import { truncate } from './format.ts';
import { attentionHref, href } from './router.ts';
import { type SessionsSeen, sessionEvents, sessionsSeen } from './sessionEvents.ts';
import { toast } from './toast.tsx';

const TEXT_MAX = 120;

/**
 * セッションが作業を終えたときと、人間の答えを待ち始めたときに、どの画面を開いていてもトーストで知らせる。初回の読み込みでは知らせない。
 * トーストを押すと、そのセッションへ移る。レビューの依頼は、コードの画面へ移る。
 */
export const useSessionToasts = (snapshot: Snapshot | null): void => {
  const seen = useRef<SessionsSeen | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const previous = seen.current;
    seen.current = sessionsSeen(snapshot);
    if (!previous) return;
    for (const e of sessionEvents(previous, snapshot)) {
      toast(truncate(e.text, TEXT_MAX), 'ok', e.attention?.kind === 'review' ? attentionHref(e.attention) : href.session(e.session.id));
    }
  }, [snapshot]);
};

const removeKey = (s: Session): string => `${s.id}:${s.removeError}`;

/**
 * アーカイブしたセッションを Claude Code の一覧から消せなかったときに、どの画面を開いていても理由を知らせる。初回の読み込みでは知らせない。
 * アーカイブの API は消し終わる前に返るので、消せなかったことは後から分かる。
 * サーバーは消せなかったものを後で消し直すので、同じ理由で消せないままなら知らせ直さない。
 */
export const useRemoveFailureToasts = (snapshot: Snapshot | null): void => {
  const seen = useRef<Set<string> | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const previous = seen.current;
    const failed = snapshot.sessions.filter((s) => s.removeError);
    seen.current = new Set(failed.map(removeKey));
    if (!previous) return;
    for (const s of failed) {
      if (!previous.has(removeKey(s))) toast(`${s.name} を Claude Code の一覧から消せませんでした（${s.removeError}）。後で消し直します`, 'error');
    }
  }, [snapshot]);
};
