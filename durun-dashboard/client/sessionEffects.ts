import { useEffect, useRef } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';
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
