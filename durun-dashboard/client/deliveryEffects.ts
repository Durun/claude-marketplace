import { useEffect, useRef } from 'preact/hooks';
import type { Snapshot } from '../shared/types.ts';
import { failureKeys, newFailures } from './delivery.ts';
import { truncate } from './format.ts';
import { notificationPermission } from './notify.ts';
import { href } from './router.ts';
import { toast } from './toast.tsx';

const BODY_MAX = 140;

/**
 * 届けられなかったメッセージを、どの画面を開いていても知らせる。初回の読み込みでは知らせない。
 * 送信の API は届く前に返り、失敗は後から分かる。理由は送信したメッセージの一覧にも出るが、その一覧を開いていないと気づけない。
 */
export const useDeliveryFailureEffects = (snapshot: Snapshot | null): void => {
  const known = useRef<Set<string> | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const previous = known.current;
    known.current = failureKeys(snapshot.outbox);
    if (!previous) return;
    for (const m of newFailures(previous, snapshot.outbox)) {
      const sessionName = snapshot.sessions.find((s) => s.id === m.sessionId)?.name ?? m.sessionId;
      toast(`${sessionName} にメッセージを届けられませんでした: ${m.error}`, 'error');
      if (notificationPermission() !== 'granted') continue;
      try {
        const n = new Notification(`届けられませんでした: ${sessionName}`, { body: truncate(m.error ?? '', BODY_MAX), tag: `delivery-${m.id}` });
        n.onclick = () => {
          window.focus();
          location.hash = href.session(m.sessionId);
          n.close();
        };
      } catch {
        // 通知を出せない環境でも、トーストは出ている。
      }
    }
  }, [snapshot]);
};
