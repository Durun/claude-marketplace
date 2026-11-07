// ムービーメーカーのウィンドウに出す「次にすること」と、ログの文。サーバーが送った生の値から決めるので、テストで確かめる。
import type { MmChange, MmLogEntry, MovieMakerView } from '../shared/types.ts';

export type MmAction = 'install' | 'launch';
export type MmStep = { key: string; tone: 'warn' | 'info'; title: string; text?: string; action?: { kind: MmAction; label: string; disabled?: string } };

const installStep = (v: MovieMakerView): MmStep | undefined => {
  const i = v.install;
  if (i.busy) return { key: 'install', tone: 'info', title: 'movie-maker を入れています', text: '数分から数十分かかります。進み具合は下に出ます。' };
  if (i.error) return { key: 'install', tone: 'warn', title: '入れられませんでした', text: i.error, action: i.mode === 'build' ? { kind: 'install', label: 'やり直す' } : undefined };
  if (!i.path) {
    return i.mode === 'build'
      ? { key: 'install', tone: 'warn', title: 'movie-maker が入っていません', text: 'ソースからビルドして入れます（UI と Rust のビルド）。', action: { kind: 'install', label: 'ビルドして入れる' } }
      : { key: 'install', tone: 'warn', title: 'movie-maker が入っていません', action: { kind: 'install', label: '入れる' } };
  }
  if (i.mode === 'build' && i.commit && i.sourceCommit && i.commit !== i.sourceCommit) {
    return { key: 'rebuild', tone: 'info', title: 'ソースが新しくなっています', text: `入れたもの ${i.commit}、ソース ${i.sourceCommit}。`, action: { kind: 'install', label: '作り直す' } };
  }
  return undefined;
};

/** 次にすること。上から順に片づける。済んでいれば空。 */
export const mmSteps = (v: MovieMakerView): MmStep[] => {
  const steps: MmStep[] = [];
  const install = installStep(v);
  if (install) steps.push(install);
  if (!v.project) {
    steps.push({ key: 'project', tone: 'warn', title: 'プロジェクトのフォルダーを選んでください', text: 'project.mmdb のあるフォルダーか、新しく作るフォルダー。' });
  } else if (v.link.status === 'waiting') {
    const disabled = v.install.path ? undefined : '先に movie-maker を入れてください';
    steps.push({ key: 'launch', tone: 'info', title: 'movie-maker でこのプロジェクトを開いてください', text: v.link.message, action: { kind: 'launch', label: v.hasStore ? '起動する' : 'プロジェクトを作って起動する', disabled } });
  }
  return steps;
};

/** 変更の通知を文にする。取り消しとやり直しは、戻した元のトランザクションと、それをした人の種類を添える。 */
export const describeMmChange = (c: MmChange): string => {
  const touched = c.touched.length ? `（${c.touched.length} か所）` : '';
  if (c.kind === 'apply') return `${c.name}${touched}`;
  const verb = c.kind === 'undo' ? '取り消し' : 'やり直し';
  return c.of ? `${verb}: ${c.of.name}（${c.of.actor === 'gui' ? '人間' : 'エージェント'}の操作）` : verb;
};

export const describeMmEntry = (e: MmLogEntry): string => (e.who === 'you' ? describeMmChange(e.change) : `${e.tool}${e.ok ? '' : '（失敗）'}`);
