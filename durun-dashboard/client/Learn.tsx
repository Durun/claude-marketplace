// 人間が会話で教えたことから、Claude の指示への差分を提案させる「学習」アプリ。
// 提案と承認は、学習のセッションのチャットで行う。「知見の一覧」では、Claude Code が読む指示を、いつ効くかとともに並べ、メモ帳で開いて直せる。
import { useState } from 'preact/hooks';
import type { HarnessItem, HarnessToggle, LearnState, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { AppIcon, DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { shortDateTime } from './format.ts';
import { useFetch } from './hooks.ts';
import { NOTEPAD_ICON, openNotepadFile } from './Notepad.tsx';
import { href } from './router.ts';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。ウィンドウは 1 つだけ開くので、タブの id にもこのキーを入れる。 */
export const LEARN_KEY = 'learn';
export const LEARN_TITLE = '学習';
export const LEARN_ICON = '/learn.svg';
const SIZE = { w: 860, h: 480 };
const OFF: LearnState = { scheduled: false, running: false, runs: [] };
const KINDS: Record<HarnessItem['kind'], string> = { instructions: 'CLAUDE.md', skill: 'スキル', memory: '記憶', hook: 'フック' };

type View = 'runs' | HarnessItem['kind'];

export const openLearn = (): void => floatUp(LEARN_KEY, '', { id: LEARN_KEY }, SIZE);
const close = () => dock(LEARN_KEY, '', LEARN_KEY);

export const LearnIcon = () => <DesktopIcon icon={LEARN_ICON} title={LEARN_TITLE} hint="人間が会話で教えたことから、Claude の指示への改善を提案させます。ダブルクリックで開きます" onLaunch={openLearn} />;

const learnNow = () => void attempt('学習の開始', () => api.learnNow());
const schedule = (on: boolean) => void attempt('学習の設定の変更', () => api.learn(on));

const Runs = ({ learn, now }: { learn: LearnState; now: number }) => (
  <table class="recycle-table">
    <thead>
      <tr><th>学習した日時</th><th>提案</th></tr>
    </thead>
    <tbody>
      {learn.runs.map((r) => (
        <tr key={r.sessionId}>
          <td><AppIcon icon={LEARN_ICON} />{shortDateTime(r.at, now)}</td>
          <td><a href={href.session(r.sessionId)}>セッションで見る</a></td>
        </tr>
      ))}
    </tbody>
  </table>
);

type Toggle = (t: HarnessToggle, on: boolean) => void;

/** settings.json に書くスイッチ。行のクリック（枝の開閉）に伝えない。 */
const Switch = ({ t, label, onToggle }: { t: HarnessToggle; label: string; onToggle: Toggle }) => (
  <input type="checkbox" role="switch" class="learn-switch" checked={t.enabled} aria-label={`${label}を使う`} title={t.enabled ? '使う（クリックで止める）' : '止めている（クリックで使う）'} onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()} onChange={(e) => onToggle(t, e.currentTarget.checked)} />
);

/** 置き場所ごとの枝に分けて並べる。枝の行をクリックすると開閉する。行をダブルクリックするか Enter で、その指示のファイルをメモ帳で開く。 */
const Harness = ({ items, closed, flip, onToggle }: { items: HarnessItem[]; closed: Set<string>; flip: (group: string) => void; onToggle: Toggle }) => {
  const groups = Map.groupBy(items, (h) => h.group);
  return (
    <table class="recycle-table learn-harness">
      <thead>
        <tr><th>名前</th><th>いつ効くか</th><th>置き場所</th><th>文字数</th></tr>
      </thead>
      <tbody>
        {[...groups].map(([group, list]) => {
          const open = !closed.has(group);
          const toggle = list[0]?.groupToggle;
          return [
            <tr key={`group\n${group}`} class="learn-group" tabIndex={0} aria-expanded={open} onClick={() => flip(group)} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && flip(group)}>
              <td colSpan={4}>
                <span class="learn-twisty">{open ? '−' : '+'}</span>
                {toggle && <Switch t={toggle} label={group} onToggle={onToggle} />}
                {group}（{list.length}）
              </td>
            </tr>,
            ...(open
              ? list.map((h) => (
                  <tr key={`${h.path}\n${h.trigger}\n${h.name}`} class={h.toggle?.enabled === false || toggle?.enabled === false ? 'is-off' : undefined} tabIndex={0} title={h.path} onDblClick={() => openNotepadFile(h.path)} onKeyDown={(e) => e.key === 'Enter' && openNotepadFile(h.path)}>
                    <td class="learn-leaf">{h.toggle ? <Switch t={h.toggle} label={h.name} onToggle={onToggle} /> : <AppIcon icon={NOTEPAD_ICON} />}{h.name}</td>
                    <td title={h.trigger}>{h.trigger}</td>
                    <td>{h.source}</td>
                    <td>{h.chars.toLocaleString()}</td>
                  </tr>
                ))
              : []),
          ];
        })}
      </tbody>
    </table>
  );
};

export const LearnFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const learn = snapshot.learn ?? OFF;
  const [view, setView] = useState<View>('runs');
  const [closed, setClosed] = useState(new Set<string>());
  const harness = useFetch(() => api.harness(), []);
  const all = harness.data?.items ?? [];
  const items = all.filter((h) => h.kind === view);
  const count = (kind: HarnessItem['kind']) => all.filter((h) => h.kind === kind).length;
  const flip = (group: string) => setClosed((c) => (c.delete(group) ? new Set(c) : new Set(c).add(group)));
  const onToggle: Toggle = (t, on) => void attempt('有効・無効の切り替え', () => api.harnessToggle(t, on)).then(harness.reload);
  const kindToggle = view === 'runs' ? undefined : harness.data?.kindToggles[view];
  return (
    <FloatFrame f={f} title={LEARN_TITLE} icon={LEARN_ICON} controls={[{ kind: 'close', label: '閉じる', onClick: close }]}>
      <div class="recycle">
        <div class="recycle-main">
          <aside class="recycle-tasks">
            <section class="xp-task">
              <h3 class="xp-task-title">学習のタスク</h3>
              <div class="xp-task-body">
                <button class="xp-task-link" disabled={learn.running} onClick={learnNow}><AppIcon icon={LEARN_ICON} />今すぐ学習する</button>
                <label class="check">
                  <input type="checkbox" role="switch" checked={learn.scheduled} onChange={(e) => schedule(e.currentTarget.checked)} />
                  定期的に学習する
                </label>
                <button class="xp-task-link" aria-pressed={view === 'runs'} onClick={() => setView('runs')}>学習の履歴</button>
              </div>
            </section>
            <section class="xp-task">
              <h3 class="xp-task-title">知見の一覧</h3>
              <div class="xp-task-body">
                {(Object.keys(KINDS) as HarnessItem['kind'][]).map((k) => (
                  <button key={k} class="xp-task-link" aria-pressed={view === k} onClick={() => setView(k)}>{KINDS[k]}（{count(k)}）</button>
                ))}
                <button class="xp-task-link" onClick={harness.reload}>読み直す</button>
                {harness.error && <p class="recycle-error">{harness.error}</p>}
              </div>
            </section>
            <section class="xp-task">
              <h3 class="xp-task-title">詳細</h3>
              <div class="xp-task-body">
                {learn.running && <p>提案を作っています…</p>}
                {learn.error && <p class="recycle-error">{learn.error}</p>}
                {view === 'runs' ? (
                  <>
                    <p>前回より後の人間の発言から、人間が教えたことを拾い、CLAUDE.md・スキル・記憶・フックへの差分を提案させます。</p>
                    <p>定期的に学習するときは、作業が止まって 90 分たったら、1 日 1 回まで動きます。</p>
                    <p>提案はセッションのチャットに届きます。承認した提案だけが書き込まれます。</p>
                  </>
                ) : (
                  <>
                    {kindToggle && (
                      <label class="check">
                        <input type="checkbox" role="switch" checked={kindToggle.enabled} onChange={(e) => onToggle(kindToggle, e.currentTarget.checked)} />
                        {KINDS[view as HarnessItem['kind']]}を使う
                      </label>
                    )}
                    <p>Claude Code が読む指示です。行をダブルクリックすると、メモ帳で開いて直せます。スキルの「いつ効くか」は SKILL.md の description です。</p>
                    <p>スイッチは ~/.claude/settings.json に書き、次に始めるセッションから効きます。{view === 'hook' && 'フックは 1 つずつは止められず、まとめて止めます。'}</p>
                  </>
                )}
              </div>
            </section>
          </aside>
          <div class="recycle-list">{view === 'runs' ? <Runs learn={learn} now={snapshot.now} /> : <Harness items={items} closed={closed} flip={flip} onToggle={onToggle} />}</div>
        </div>
      </div>
    </FloatFrame>
  );
};
