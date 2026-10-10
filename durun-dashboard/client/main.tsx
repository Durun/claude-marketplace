import { type ComponentType, render } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { JIRA_ISSUE_KEY, PR_URL, type Snapshot } from '../shared/types.ts';
import { api } from './api.ts';
import { showWidget } from './Dashboard.tsx';
import { DOC_KEY, DocFloat, DocViewer, DocsPage, FILE_KEY, FileFloat, openDocFloat, plainClick } from './Docs.tsx';
import { Dolphin } from './Dolphin.tsx';
import { Feedback } from './Feedback.tsx';
import { type Float, floatUp, useFloats } from './Float.tsx';
import { Header } from './Header.tsx';
import { HOME_KEY } from './dashboards.ts';
import { Home, HomeFloat, NEW_SESSION_KEY, NewSessionFloat } from './Home.tsx';
import { NowContext, useNow, useSnapshot } from './hooks.ts';
import { useDeliveryFailureEffects } from './deliveryEffects.ts';
import { useAttentionEffects } from './notify.ts';
import { QuickSearch } from './QuickSearch.tsx';
import { JIRA_KEY, JIRA_WIDGET, JiraFloat, pickJira } from './JiraView.tsx';
import { PR_KEY, PR_WIDGET, PrFloat } from './PrView.tsx';
import { type Route, href, useRoute } from './router.ts';
import { SearchPage } from './Search.tsx';
import { SESSION_KEY, SessionFloat, SessionPage } from './SessionPage.tsx';
import { EDIT_SHORTCUT, isShortcut } from './shortcut.ts';
import { Toasts } from './toast.tsx';

type PageProps = { route: Route; snapshot: Snapshot; editing: boolean; onEditDone: () => void };

const Page = ({ route, snapshot, editing, onEditDone }: PageProps) => {
  switch (route.page) {
    case 'home':
      return <Home snapshot={snapshot} editing={editing} onEditDone={onEditDone} />;
    case 'session':
      return <SessionPage key={route.id} id={route.id} tab={route.tab} file={route.file} snapshot={snapshot} editing={editing} onEditDone={onEditDone} />;
    case 'docs':
      return <DocsPage snapshot={snapshot} />;
    case 'doc':
      return <DocViewer key={route.id} id={route.id} snapshot={snapshot} />;
    case 'search':
      return <SearchPage key={route.q} q={route.q} snapshot={snapshot} />;
  }
};

// 浮かべたウィジェットを、元のダッシュボードの保存キーごとに描く部品。
const FLOATS: Record<string, ComponentType<{ f: Float; snapshot: Snapshot }>> = { [HOME_KEY]: HomeFloat, [SESSION_KEY]: SessionFloat, [FILE_KEY]: FileFloat, [DOC_KEY]: DocFloat, [NEW_SESSION_KEY]: NewSessionFloat, [PR_KEY]: PrFloat, [JIRA_KEY]: JiraFloat };

const floatId = (f: Float): string => `${f.key} ${f.scope} ${f.tab.id}`;

/**
 * ウィンドウに浮かべたウィジェットは、どの画面にも出す。
 * 手前に出したときに要素を並べ替えると中のスクロールの位置が戻るので、要素は決まった順に並べ、重なりの順は z-index で決める。
 */
const Floats = ({ snapshot }: { snapshot: Snapshot }) =>
  useFloats()
    .toSorted((a, b) => floatId(a).localeCompare(floatId(b)))
    .map((f) => {
      const Window = FLOATS[f.key];
      return Window && <Window key={floatId(f)} f={f} snapshot={snapshot} />;
    });

/**
 * PR と JIRA の課題へのリンクを押したら、外のサイトではなくダッシュボードで開く。
 * セッションのバッジと、開いているセッションの PR や課題は、そのセッションの「PR」「JIRA」ウィジェットで開き、ほかのものはウィンドウに浮かべる。
 * ダッシュボードの資料へのリンクは、ページを移らずにウィンドウに浮かべる。
 * 修飾キーつきのクリック、コメントなど PR の中の場所へのリンク、data-external を付けたリンクは、今までどおりブラウザで開く。
 */
const useLinks = (route: Route, snapshot: Snapshot | null): void => {
  const latest = useRef({ route, snapshot });
  latest.current = { route, snapshot };
  useEffect(() => {
    const click = (e: MouseEvent) => {
      const a = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>('a[href]') : null;
      if (!a || e.defaultPrevented || !plainClick(e) || a.hasAttribute('data-external') || !URL.canParse(a.href)) return;
      const u = new URL(a.href);
      // 発言の中では、ダッシュボードを localhost とも 127.0.0.1 とも書くので、ポートで見分ける。
      const doc = u.port === location.port ? /^#\/doc\/([^/]+)$/.exec(u.hash)?.[1] : undefined;
      if (doc) {
        e.preventDefault();
        return openDocFloat(decodeURIComponent(doc));
      }
      const url = `${u.origin}${u.pathname.replace(/\/$/, '')}`;
      const { route, snapshot } = latest.current;
      const base = snapshot?.jiraBase;
      const jira = base && url.startsWith(`${base}/browse/`) ? url.slice(base.length + '/browse/'.length) : undefined;
      const isPr = !u.hash && PR_URL.test(url);
      if (!isPr && !(jira && JIRA_ISSUE_KEY.test(jira))) return;
      e.preventDefault();
      const here = route.page === 'session' ? snapshot?.sessions.find((s) => s.id === route.id) : undefined;
      const mine = isPr ? here?.pr?.url === url : here?.jira.some((j) => j.key === jira);
      const id = a.dataset.session ?? (mine ? here?.id : undefined);
      if (!id) return isPr ? floatUp(PR_KEY, '', { id: url }) : floatUp(JIRA_KEY, '', { id: jira ?? '' });
      if (route.page !== 'session' || route.id !== id) location.hash = href.session(id);
      if (jira) pickJira(id, jira);
      showWidget(SESSION_KEY, id, isPr ? PR_WIDGET : JIRA_WIDGET);
    };
    document.addEventListener('click', click);
    return () => document.removeEventListener('click', click);
  }, []);
};

const App = () => {
  const route = useRoute();
  const now = useNow();
  const { snapshot, fresh, connected } = useSnapshot();
  const [searching, setSearching] = useState(false);
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  // ダッシュボードの編集は、ヘッダーのメニューかショートカットで始める。ショートカットをもう一度押すか、画面を移ったら編集を終える。
  const [editing, setEditing] = useState(false);
  useEffect(() => setEditing(false), [route]);
  const hasDashboard = route.page === 'home' || route.page === 'session';
  useEffect(() => {
    if (!hasDashboard) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isShortcut(e, EDIT_SHORTCUT)) return;
      e.preventDefault();
      setEditing((v) => !v);
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, [hasDashboard]);
  useAttentionEffects(fresh);
  useDeliveryFailureEffects(fresh);
  useLinks(route, snapshot);
  return (
    <NowContext.Provider value={now}>
      <Header
        snapshot={snapshot}
        assets={fresh?.assets}
        connected={connected}
        home={route.page === 'home'}
        onSearch={() => setSearching(true)}
        onEdit={hasDashboard ? () => setEditing(true) : undefined}
        onFeedback={() => setFeedbackOpen((v) => !v)}
      />
      <main class="app">
        {snapshot ? <Page route={route} snapshot={snapshot} editing={editing} onEditDone={() => setEditing(false)} /> : <p class="empty">読み込み中…</p>}
      </main>
      {snapshot && <Floats snapshot={snapshot} />}
      <Toasts />
      <Feedback snapshot={snapshot} open={feedbackOpen} setOpen={setFeedbackOpen} />
      <Dolphin />
      <QuickSearch snapshot={snapshot} open={searching} setOpen={setSearching} />
    </NowContext.Provider>
  );
};

// 画面で起きた想定外の例外をサーバーへ送り、改善要望にしてもらう。毎フレームの処理などで繰り返し起きても、同じものは 1 回だけ送る。
const reported = new Set<string>();
const reportError = (error: unknown): void => {
  const message = error instanceof Error ? error.message : String(error);
  if (reported.has(message)) return;
  reported.add(message);
  api.reportError(message, error instanceof Error ? error.stack : undefined).catch(() => {});
};
addEventListener('error', (e) => reportError(e.error ?? e.message));
addEventListener('unhandledrejection', (e) => reportError(e.reason));

const root = document.getElementById('app');
if (root) render(<App />, root);
