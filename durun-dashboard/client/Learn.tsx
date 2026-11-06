// 人間が会話で教えたことから、Claude の指示への差分を提案させる「学習」アプリ。
// 提案と承認は、学習のセッションのチャットで行う。知見のファイルは利用者の側にあり、このアプリは設定と記録だけを扱う。
import type { LearnState, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { AppIcon, DesktopIcon } from './apps.tsx';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { shortDateTime } from './format.ts';
import { href } from './router.ts';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。ウィンドウは 1 つだけ開くので、タブの id にもこのキーを入れる。 */
export const LEARN_KEY = 'learn';
export const LEARN_TITLE = '学習';
export const LEARN_ICON = '/learn.svg';
const SIZE = { w: 640, h: 400 };
const OFF: LearnState = { scheduled: false, running: false, runs: [] };

export const openLearn = (): void => floatUp(LEARN_KEY, '', { id: LEARN_KEY }, SIZE);
const close = () => dock(LEARN_KEY, '', LEARN_KEY);

export const LearnIcon = () => <DesktopIcon icon={LEARN_ICON} title={LEARN_TITLE} hint="人間が会話で教えたことから、Claude の指示への改善を提案させます。ダブルクリックで開きます" onLaunch={openLearn} />;

const learnNow = () => void attempt('学習の開始', () => api.learnNow());
const schedule = (on: boolean) => void attempt('学習の設定の変更', () => api.learn(on));

export const LearnFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const learn = snapshot.learn ?? OFF;
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
              </div>
            </section>
            <section class="xp-task">
              <h3 class="xp-task-title">詳細</h3>
              <div class="xp-task-body">
                {learn.running && <p>提案を作っています…</p>}
                {learn.error && <p class="recycle-error">{learn.error}</p>}
                <p>前回より後の人間の発言から、人間が教えたことを拾い、CLAUDE.md・スキル・記憶・フックへの差分を提案させます。</p>
                <p>定期的に学習するときは、作業が止まって 90 分たったら、1 日 1 回まで動きます。</p>
                <p>提案はセッションのチャットに届きます。承認した提案だけが書き込まれます。</p>
              </div>
            </section>
          </aside>
          <div class="recycle-list">
            <table class="recycle-table">
              <thead>
                <tr><th>学習した日時</th><th>提案</th></tr>
              </thead>
              <tbody>
                {learn.runs.map((r) => (
                  <tr key={r.sessionId}>
                    <td><AppIcon icon={LEARN_ICON} />{shortDateTime(r.at, snapshot.now)}</td>
                    <td><a href={href.session(r.sessionId)}>セッションで見る</a></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </FloatFrame>
  );
};
