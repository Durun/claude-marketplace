// セッションのコードの変更を、GitHub の PR の「Files changed」と同じ形でレビューするウィジェット。
// セッションの PR と、Claude が show_branches で足したブランチと、request_review で頼んだ変更をタブに並べる。
import type { ComponentChildren } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { type BranchView, type DiffFile, type DiffRange, type PrInfo, type ReviewComment, type Session, WORKTREE } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DiffStat, FileDiff, NO_THREADS, STATUS_LETTER } from './DiffTable.tsx';
import { type Focus, diffBatchEnd, findFocus } from './fileRef.ts';
import { type FileTreeNode, buildFileTree } from './fileTree.ts';
import { pickInWidget, useCached, useFetch, usePicked, useTabOrder } from './hooks.ts';
import { PickTabs } from './PrView.tsx';
import { ReviewBar } from './ReviewBar.tsx';
import { type ReviewDraft, useReviewDraft } from './review.ts';
import { Empty, Loading, PrIcon, prState } from './ui.tsx';

/** タブの並びと選んだタブを覚える、ウィジェットの種類の名前。 */
const CODE_WIDGET = 'code';
const REVIEW_TAB = 'review';

// レビューバーの高さ。ファイル一覧とスクロール位置の余白をバーの下に合わせる。
const BAR_HEIGHT = 76;
const BANNER_HEIGHT = 41;

type CodeTab = { key: string; label: ComponentChildren; title: string; pr?: PrInfo; view?: BranchView; at?: number };

const viewKey = (v: BranchView): string => `branch:${v.branch}\0${v.base ?? ''}`;

const codeTabs = (s: Session): CodeTab[] => [
  ...(s.awaitingReview ? [{ key: REVIEW_TAB, label: 'レビュー依頼', title: 'Claude がレビューを頼んだ、このセッションの変更' }] : []),
  ...(s.prs ?? []).map((pr) => ({ key: pr.url, label: <><PrIcon state={prState(pr)} />#{pr.number}</>, title: pr.title, pr })),
  ...(s.branchViews ?? []).map((v) => ({ key: viewKey(v), label: v.label ?? v.branch, title: `${v.branch}${v.base ? ` ← ${v.base}` : ''}`, view: v, at: v.at })),
];

/**
 * 人間が閉じたタブと、閉じた時刻。セッションごとにブラウザに保存する。
 * Claude がブランチを足し直すと、閉じた後に足したタブとして開き直す。
 */
const useClosedTabs = (session: string) => {
  const key = `code-closed:${session}`;
  const [closed, setClosed] = useState<Record<string, number>>(() => {
    try {
      return JSON.parse(localStorage.getItem(key) ?? '{}') as Record<string, number>;
    } catch {
      return {};
    }
  });
  const save = (next: Record<string, number>) => {
    setClosed(next);
    localStorage.setItem(key, JSON.stringify(next));
  };
  const isClosed = (t: CodeTab): boolean => closed[t.key] !== undefined && (t.at ?? 0) <= (closed[t.key] ?? 0);
  return {
    isClosed,
    close: (k: string) => save({ ...closed, [k]: Date.now() }),
    reopen: (k: string) => {
      const { [k]: _, ...rest } = closed;
      save(rest);
    },
  };
};

type Entry = { file: DiffFile; index: number };

const anchorId = (index: number): string => `diff-file-${index}`;
const WHOLE_ANCHOR = 'diff-file-whole';

const commentCounts = (comments: ReviewComment[]): Map<string, number> => {
  const counts = new Map<string, number>();
  for (const c of comments) counts.set(c.path, (counts.get(c.path) ?? 0) + 1);
  return counts;
};

type FileListProps = { entries: Entry[]; counts: Map<string, number>; viewed?: Set<string> };

/** GitHub のファイルツリーと同じく、ディレクトリを入れ子にし、押すと畳む。 */
const FileList = ({ entries, counts, viewed }: FileListProps) => {
  const [folded, setFolded] = useState<Set<string>>(new Set());
  const toggle = (path: string) => {
    const next = new Set(folded);
    if (!next.delete(path)) next.add(path);
    setFolded(next);
  };
  const render = (nodes: FileTreeNode<Entry>[], depth: number): ComponentChildren =>
    nodes.map((node) => {
      const indent = { paddingLeft: `${6 + depth * 14}px` };
      if (node.kind === 'dir') {
        const open = !folded.has(node.path);
        return (
          <li key={`d:${node.path}`}>
            <button class="file-dir" style={indent} title={node.path} aria-expanded={open} onClick={() => toggle(node.path)}>
              <span class="chevron">{open ? '▾' : '▸'}</span>
              {node.name}
            </button>
            {open && <ul>{render(node.children, depth + 1)}</ul>}
          </li>
        );
      }
      const { file, index } = node.item;
      const n = counts.get(file.path) ?? 0;
      return (
        <li key={index}>
          <button class={`file-item${viewed?.has(file.path) ? ' is-viewed' : ''}`} style={indent} title={file.path} onClick={() => document.getElementById(anchorId(index))?.scrollIntoView({ block: 'start' })}>
            <span class={`fstatus fstatus-${file.status}`}>{STATUS_LETTER[file.status]}</span>
            <span class="file-name">{node.name}</span>
            {n > 0 && <span class="tag tag-warn">{n}</span>}
            <DiffStat file={file} />
          </button>
        </li>
      );
    });
  return <ul class="file-list">{render(buildFileTree(entries, (e) => e.file.path), 0)}</ul>;
};

// 1 回に描く差分の表の行の数の目安。数万行の差分を一度に描くと画面が数秒固まるので、先頭のファイルからこの行数ずつ描き足す。
const ROWS_PER_FRAME = 1000;

/** 差分のファイルのうち、行まで描くものの数。届いた差分ごとに先頭から数え直す。 */
const useDrawnFiles = (files: DiffFile[]): number => {
  const [drawn, setDrawn] = useState({ files, count: 0 });
  const count = drawn.files === files ? drawn.count : diffBatchEnd(files, 0, ROWS_PER_FRAME);
  // useEffect は描いたものを画面に出してから動くので、描き足すたびに画面へ出し、その間の操作も受け付ける。
  useEffect(() => {
    if (count < files.length) setDrawn({ files, count: diffBatchEnd(files, count, ROWS_PER_FRAME) });
  }, [files, count]);
  return count;
};

type DiffFilesProps = { files: DiffFile[]; entries: Entry[]; review: ReviewDraft; focus?: Focus; sync?: PrSync };

const DiffFiles = ({ files, entries, review, focus, sync }: DiffFilesProps) => {
  const drawn = useDrawnFiles(files);
  return (
    <div class="diff-files">
      {entries.length === 0 && <Empty>表示できる差分はありません。</Empty>}
      {entries.map(({ file, index }) => (
        <FileDiff
          key={file.path}
          id={anchorId(index)}
          file={file}
          comments={review.comments}
          threads={sync?.threads}
          viewed={sync?.viewed.has(file.path)}
          onViewed={sync && ((v) => sync.setViewed(file.path, v))}
          onSave={review.upsert}
          onDelete={review.remove}
          focusLine={focus?.index === index ? (focus.line ?? null) : undefined}
          pending={index >= drawn && index !== focus?.index}
        />
      ))}
    </div>
  );
};

type PrSync = ReturnType<typeof usePrSync>;

/**
 * PR のタブでは、GitHub の PR のスレッドと「Viewed」を読む。Viewed を切り替えたら、画面を先に変えてから GitHub に書く。
 * スレッドは、Claude が応答を始めたときと終えたときに読み直す。PR に返信や修正が加わっていることがあるため。
 */
const usePrSync = (s: Session, pr: PrInfo | undefined) => {
  const threads = useCached((fresh) => (pr ? api.prThreads(s.id, pr.url, fresh) : Promise.resolve([])), [pr?.url, s.status]);
  // Viewed は、サーバーが裏で読み直しても取り直さない。切り替えた直後に取り直すと、書き込む前の状態で画面を戻してしまうため。
  const remote = useFetch((fresh) => (pr ? api.prViewed(s.id, pr.url, fresh) : Promise.resolve([])), [pr?.url]);
  const [viewed, setLocal] = useState<Set<string>>(new Set());
  useEffect(() => setLocal(new Set(remote.data)), [remote.data]);
  if (!pr) return undefined;
  const setViewed = async (path: string, v: boolean) => {
    const next = new Set(viewed);
    if (v) next.add(path);
    else next.delete(path);
    setLocal(next);
    // 書けなかったら、GitHub の状態に戻す。
    if (!(await attempt('Viewed の切り替え', () => api.setPrViewed(s.id, pr.url, path, v)))) remote.reload();
  };
  return { pr, threads: threads.data ?? NO_THREADS, viewed, setViewed, reloadThreads: threads.reload };
};

/** pr は開いている PR のタブ、view は Claude が足したブランチのタブ。どちらも無ければ、レビューを頼まれたこのセッションの変更を出す。 */
const CodeBody = ({ s, focus: ref, view, pr, awaiting }: { s: Session; focus?: string; view?: BranchView; pr?: PrInfo; awaiting: boolean }) => {
  const [filter, setFilter] = useState('');
  const range: DiffRange | undefined = view || pr ? undefined : s.reviewRange;
  const diff = useFetch(() => api.diff(s.id, range, view, pr?.url), [s.id, range?.from, range?.to]);
  // ブランチと PR では、差分に無いファイルも差分の先端のコミットから読む。
  const tip = diff.data && diff.data.range.to !== WORKTREE ? diff.data.range.to : undefined;
  const focus = diff.data && ref ? findFocus(diff.data.files, ref) : undefined;
  // 差分に無いファイルへのリンクで開いたときは、ファイル全体を読んで出す。
  const missing = !!ref && !!diff.data && !focus;
  const whole = useFetch(() => (missing && ref ? api.file(s.id, ref, tip) : Promise.resolve(undefined)), [s.id, ref, missing]);
  const wholeFile = missing && !whole.loading && !whole.error ? whole.data : undefined;
  const wholeLine = wholeFile && ref ? findFocus([wholeFile], ref)?.line : undefined;
  // 開くファイルが届いたら一度だけ、そのファイル（行があればその行）までスクロールする。
  useEffect(() => {
    const id = focus ? anchorId(focus.index) : wholeFile ? WHOLE_ANCHOR : undefined;
    const section = id ? document.getElementById(id) : null;
    (section?.querySelector('.dl.is-focused') ?? section)?.scrollIntoView({ block: (focus?.line ?? wholeLine) ? 'center' : 'start' });
  }, [ref, focus?.index, focus?.line, wholeFile]);
  const review = useReviewDraft(s.id, pr?.url ?? view?.branch);
  const sync = usePrSync(s, pr);
  const files = diff.data?.files ?? [];
  const q = filter.trim().toLowerCase();
  const entries: Entry[] = files.map((file, index) => ({ file, index })).filter(({ file }) => !q || file.path.toLowerCase().includes(q));
  const seen = sync ? files.filter((f) => sync.viewed.has(f.path)).length : 0;
  // Claude に送る指摘には、どのコミットの行かを添える。
  const where = pr ? `PR #${pr.number}${tip ? `（${tip.slice(0, 7)}）` : ''}` : view?.branch;
  return (
    <div class="code-view" style={`--bar-h: ${BAR_HEIGHT + (awaiting ? BANNER_HEIGHT : 0)}px`}>
      <ReviewBar s={s} review={review} awaiting={awaiting} branch={where} pr={sync && { info: sync.pr, commit: tip, onPosted: sync.reloadThreads }} />
      {diff.error && <p class="card panel error-note">差分を取得できませんでした（{diff.error}）</p>}
      {missing && whole.error && <p class="card panel warn-note">「{ref}」は、差分にも{tip ? ` ${tip.slice(0, 7)} ` : 'リポジトリ'}にもありません。</p>}
      {wholeFile && (
        <>
          <p class="card panel warn-note">「{ref}」は差分に無いため、ファイル全体を表示しています。</p>
          <FileDiff key={wholeFile.path} id={WHOLE_ANCHOR} file={wholeFile} comments={review.comments} onSave={review.upsert} onDelete={review.remove} focusLine={wholeLine ?? null} />
        </>
      )}
      {diff.data?.truncated && <p class="card panel warn-note">差分が大きいため、一部を省略しています。</p>}
      {diff.data && (
        <div class={`code-layout ${diff.loading ? 'is-loading' : ''}`}>
          <aside class="code-side">
            <div class="code-side-head">
              <input class="input input-sm" type="search" placeholder="ファイルを絞り込む" aria-label="ファイルを絞り込む" value={filter} onInput={(e) => setFilter(e.currentTarget.value)} />
              <button class="btn btn-sm" data-icon="refresh" disabled={diff.loading} title="差分を読み直します" onClick={diff.reload}>更新</button>
            </div>
            <span class="muted code-side-sum">
              {files.length} ファイル{sync && ` · ${seen} / ${files.length} Viewed`}
            </span>
            <FileList entries={entries} counts={commentCounts(review.comments)} viewed={sync?.viewed} />
          </aside>
          <DiffFiles files={files} entries={entries} review={review} focus={focus} sync={sync} />
        </div>
      )}
      {!diff.data && !diff.error && <Loading />}
    </div>
  );
};

const TabBody = ({ s, tab, focus }: { s: Session; tab: CodeTab; focus?: string }) => {
  if (tab.pr && tab.pr.state !== 'OPEN') return <Empty>PR #{tab.pr.number} は{tab.pr.state === 'MERGED' ? 'マージ済みです' : '閉じています'}。</Empty>;
  return <CodeBody s={s} focus={focus} view={tab.view} pr={tab.pr} awaiting={tab.key === REVIEW_TAB} />;
};

/**
 * タブはドラッグか Alt + ←→ で並べ替え、× で閉じる。閉じたタブは「閉じたタブ」から開き直す。
 * Claude がレビューを頼むかブランチを足したら、そのタブを開いて選ぶ。
 */
export const CodeView = ({ s, focus }: { s: Session; focus?: string }) => {
  const all = codeTabs(s);
  const closed = useClosedTabs(s.id);
  const tabs = all.filter((t) => !closed.isClosed(t));
  const hidden = all.filter((t) => closed.isClosed(t));
  const { ordered } = useTabOrder(CODE_WIDGET, s.id, tabs.map((t) => t.key));
  const pick = usePicked(CODE_WIDGET, s.id);
  const active = tabs.find((t) => t.key === pick) ?? tabs.find((t) => t.key === ordered[0]);
  const show = (key: string) => {
    closed.reopen(key);
    pickInWidget(CODE_WIDGET, s.id, key);
  };
  useEffect(() => {
    if (s.awaitingReview) show(REVIEW_TAB);
  }, [s.awaitingReview]);
  // 開いた時点で並んでいたブランチは選び直さず、その後に Claude が足したものだけを選ぶ。
  const latest = all.filter((t) => t.at).toSorted((a, b) => (b.at ?? 0) - (a.at ?? 0))[0];
  const mounted = useRef(false);
  useEffect(() => {
    if (mounted.current && latest) pickInWidget(CODE_WIDGET, s.id, latest.key);
    mounted.current = true;
  }, [latest?.at]);
  return (
    <>
      <div class="code-tabs">
        <PickTabs kind={CODE_WIDGET} session={s.id} label="レビューする変更" picked={active?.key} tabs={tabs} onClose={closed.close} />
        {hidden.length > 0 && (
          <select class="input input-sm" aria-label="閉じたタブを開く" value="" onChange={(e) => show(e.currentTarget.value)}>
            <option value="">閉じたタブ（{hidden.length}）</option>
            {hidden.map((t) => <option key={t.key} value={t.key}>{t.pr ? `#${t.pr.number} ${t.pr.title}` : t.label}</option>)}
          </select>
        )}
      </div>
      {active ? (
        <TabBody key={active.key} s={s} tab={active} focus={focus} />
      ) : s.prsPending ? (
        <Loading />
      ) : (
        <Empty>レビューする変更はありません。PR を作るか、Claude が request_review か show_branches で頼むと、タブが並びます。</Empty>
      )}
    </>
  );
};
