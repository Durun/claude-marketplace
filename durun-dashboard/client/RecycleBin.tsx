// アーカイブしたのに、作業ツリーに人間の手当てが要るものが残り、Claude Code の一覧から消せなかったセッションを並べる「ゴミ箱」。Windows XP のごみ箱に倣う。
// ゴミ箱から削除すると、作業ツリーを残っている変更ごと消してから、Claude Code の一覧から消す。
import { useState } from 'preact/hooks';
import type { Snapshot, TrashItem } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { AppIcon, DesktopIcon } from './apps.tsx';
import { EXPLORER_ICON } from './Explorer.tsx';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { XpConfirm } from './XpConfirm.tsx';
import { XpMenu } from './XpMenu.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。ウィンドウは 1 つだけ開くので、タブの id にもこのキーを入れる。 */
export const RECYCLE_BIN_KEY = 'recycle-bin';
export const RECYCLE_BIN_TITLE = 'ゴミ箱';
export const RECYCLE_BIN_ICON = '/recycle-bin.svg';
// XP と同じく、中身があれば紙くずの入ったアイコンにする。
const FULL_ICON = '/recycle-bin-full.svg';
const SIZE = { w: 760, h: 420 };

export const openRecycleBin = (): void => floatUp(RECYCLE_BIN_KEY, '', { id: RECYCLE_BIN_KEY }, SIZE);
const close = () => dock(RECYCLE_BIN_KEY, '', RECYCLE_BIN_KEY);

export const RecycleBinIcon = ({ full }: { full: boolean }) => (
  <DesktopIcon icon={full ? FULL_ICON : RECYCLE_BIN_ICON} title={RECYCLE_BIN_TITLE} hint="Claude Code の一覧から消せなかったセッションを並べます。ダブルクリックで開きます" onLaunch={openRecycleBin} />
);

// claude rm が作業ツリーを残した理由のうち、よく出るものを短く言い換える。ほかの理由は、そのまま出す。
const LEFT: [RegExp, string][] = [
  [/uncommitted changes/, '未コミットの変更'],
  [/unpushed commit/, '未 push のコミット'],
  [/weren't checked against a repository/, 'git で確かめられないファイル'],
];
const leftOf = (reason: string): string => LEFT.find(([re]) => re.test(reason))?.[1] ?? reason;

const discard = (items: TrashItem[]) =>
  attempt('ゴミ箱からの削除', async () => {
    for (const t of items) await api.discard(t.id);
  });

/** 削除してよいかを、XP の削除の確認に倣って尋ねる。 */
const confirmText = (items: TrashItem[]): string =>
  items.length === 1
    ? `「${items[0]?.name}」を完全に削除しますか？作業ツリー ${items[0]?.path} も、残っている変更ごと消えます。`
    : `これらの ${items.length} 個の項目を完全に削除しますか？作業ツリーも、残っている変更ごと消えます。`;

/** 左のタスク欄の「詳細」。選んだ項目の場所と、claude rm が作業ツリーを残した理由を出す。 */
const Details = ({ t, count }: { t?: TrashItem; count: number }) =>
  t ? (
    <>
      <b>{t.name}</b>
      <p>{t.path}</p>
      <p>{t.reason}</p>
      {t.error && <p class="recycle-error">削除できませんでした: {t.error}</p>}
    </>
  ) : (
    <>
      <b>{RECYCLE_BIN_TITLE}</b>
      <p>{count} 個の項目</p>
      <p>アーカイブしたのに、作業ツリーに変更が残って Claude Code の一覧から消せなかったセッションが入ります。削除すると、作業ツリーも変更ごと消えます。</p>
    </>
  );

/** 行を押すか Tab で移ると選び、Delete キーで削除を尋ねる。 */
export const RecycleBinFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const items = snapshot.trash ?? [];
  const [selectedId, setSelectedId] = useState<string>();
  const selected = items.find((t) => t.id === selectedId);
  // 尋ねている間にゴミ箱の中身が変わっても、尋ねた項目だけを削除する。
  const [confirming, setConfirming] = useState<TrashItem[]>();
  const empty = items.length ? () => setConfirming(items) : undefined;
  const remove = selected ? () => setConfirming([selected]) : undefined;
  return (
    <FloatFrame f={f} title={RECYCLE_BIN_TITLE} icon={items.length ? FULL_ICON : RECYCLE_BIN_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: close }]}>
      <div class="recycle">
        <nav class="recycle-menubar">
          <XpMenu
            id={`${RECYCLE_BIN_KEY}-file`}
            class="xp-menu"
            label="ファイル(F)"
            items={[{ label: 'ゴミ箱を空にする(B)', onClick: empty }, { label: '削除(D)', key: 'Delete', onClick: remove }, '-', { label: '閉じる(C)', onClick: close }]}
          />
        </nav>
        <div class="recycle-main">
          <aside class="recycle-tasks">
            <section class="xp-task">
              <h3 class="xp-task-title">ゴミ箱のタスク</h3>
              <div class="xp-task-body">
                <button class="xp-task-link" disabled={!empty} onClick={empty}><AppIcon icon={RECYCLE_BIN_ICON} />ゴミ箱を空にする</button>
                {remove && <button class="xp-task-link" onClick={remove}><AppIcon icon="❌" size={11} />この項目を削除する</button>}
              </div>
            </section>
            <section class="xp-task">
              <h3 class="xp-task-title">詳細</h3>
              <div class="xp-task-body"><Details t={selected} count={items.length} /></div>
            </section>
          </aside>
          <div class="recycle-list">
            <table class="recycle-table">
              <thead>
                <tr><th>名前</th><th>元の場所</th><th>残っているもの</th></tr>
              </thead>
              <tbody>
                {items.map((t) => (
                  <tr key={t.id} tabIndex={0} aria-selected={t.id === selectedId} title={t.reason} onFocus={() => setSelectedId(t.id)} onKeyDown={(e) => e.key === 'Delete' && setConfirming([t])}>
                    <td><AppIcon icon={EXPLORER_ICON} />{t.name}</td>
                    <td>{t.path}</td>
                    <td>{t.error ? '削除できませんでした' : leftOf(t.reason)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
      {confirming && (
        <XpConfirm
          title={confirming.length === 1 ? 'ファイルの削除の確認' : '複数ファイルの削除の確認'}
          text={confirmText(confirming)}
          onClose={(ok) => {
            setConfirming(undefined);
            if (ok) void discard(confirming);
          }}
        />
      )}
    </FloatFrame>
  );
};
