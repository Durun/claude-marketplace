import { useEffect, useRef } from 'preact/hooks';
import type { Attention, AttentionKind, Snapshot } from '../shared/types.ts';
import { truncate } from './format.ts';
import { attentionHref } from './router.ts';

const APP_NAME = 'durun-dashboard';
const BODY_MAX = 140;

export const KIND_LABEL: Record<AttentionKind, string> = {
  ask: '質問',
  question: '質問',
  review: 'レビュー依頼',
  permission: '権限確認',
  pr: 'PR',
  reply: '返信待ち',
  doc: '資料',
};

export const notificationPermission = (): NotificationPermission | 'unsupported' =>
  typeof Notification === 'undefined' ? 'unsupported' : Notification.permission;

const show = (a: Attention, sessionName: string) => {
  try {
    const body = truncate([a.title, a.body].filter(Boolean).join('\n'), BODY_MAX);
    const n = new Notification(`${KIND_LABEL[a.kind]}: ${sessionName}`, { body, tag: a.id });
    n.onclick = () => {
      window.focus();
      location.hash = attentionHref(a);
      n.close();
    };
  } catch {
    // 通知を出せない環境でも、画面の更新は続ける。
  }
};

/** 未対応件数をタブ名に出し、前回なかった要対応を通知する。初回の読み込みでは通知しない。 */
export const useAttentionEffects = (snapshot: Snapshot | null): void => {
  const known = useRef<Set<string> | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const count = snapshot.attention.length;
    document.title = count > 0 ? `(${count}) ${APP_NAME}` : APP_NAME;
    const previous = known.current;
    known.current = new Set(snapshot.attention.map((a) => a.id));
    if (!previous || notificationPermission() !== 'granted') return;
    for (const a of snapshot.attention) {
      if (previous.has(a.id)) continue;
      show(a, snapshot.sessions.find((s) => s.id === a.sessionId)?.name ?? a.sessionId);
    }
  }, [snapshot]);
};
