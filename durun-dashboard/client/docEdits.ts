import type { Session } from '../shared/types.ts';

/** 資料の行の出し方を決める、セッションの状態。 */
export type DocState = Pick<Session, 'hiddenDocs' | 'pinnedDocs' | 'docTitles'>;

/** 資料のウィジェットで押した操作。at はサーバーが済ませた時刻で、応答が届くまでは無い。 */
export type DocEdit = ({ kind: 'hide'; keys: string[] } | { kind: 'pin'; keys: string[]; pinned: boolean } | { kind: 'rename'; key: string; title: string }) & { at?: number };

/** サーバーの hideDocs、pinDocs、renameDoc と同じように変える。 */
const applyDocEdit = (s: DocState, e: DocEdit): DocState => {
  if (e.kind === 'hide') return { ...s, hiddenDocs: [...s.hiddenDocs, ...e.keys] };
  if (e.kind === 'pin') return { ...s, pinnedDocs: e.pinned ? [...s.pinnedDocs, ...e.keys] : s.pinnedDocs.filter((k) => !e.keys.includes(k)) };
  const title = e.title.trim();
  const docTitles = { ...s.docTitles, [e.key]: title };
  if (!title) delete docTitles[e.key];
  return { ...s, docTitles };
};

/** snapshotAt に作ったスナップショットに、まだ載っていない操作。 */
export const pendingEdits = (edits: DocEdit[], snapshotAt: number): DocEdit[] => edits.filter((e) => e.at === undefined || e.at >= snapshotAt);

/** snapshotAt に作ったスナップショットの状態に、まだ載っていない操作を、押した順に重ねる。 */
export const withDocEdits = (s: DocState, edits: DocEdit[], snapshotAt: number): DocState => pendingEdits(edits, snapshotAt).reduce(applyDocEdit, s);
