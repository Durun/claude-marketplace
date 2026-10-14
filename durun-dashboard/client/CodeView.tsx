import type { ComponentChildren } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { type BranchView, type CommitInfo, type DiffFile, type DiffRange, type DiffResult, type PrInfo, type ReviewComment, type Session, WORKTREE } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { DiffStat, FileDiff, STATUS_LETTER } from './DiffTable.tsx';
import { type Focus, findFocus } from './fileRef.ts';
import { useCached, useFetch } from './hooks.ts';
import { ReviewBar } from './ReviewBar.tsx';
import { type ReviewDraft, useReviewDraft } from './review.ts';
import { Empty, Loading } from './ui.tsx';

// レビューバーの高さ。ファイル一覧とスクロール位置の余白をバーの下に合わせる。
const BAR_HEIGHT = 76;
const BANNER_HEIGHT = 41;

type Entry = { file: DiffFile; index: number };

const anchorId = (index: number): string => `diff-file-${index}`;
const WHOLE_ANCHOR = 'diff-file-whole';

const splitPath = (path: string): { dir: string; name: string } => {
  const i = path.lastIndexOf('/');
  return { dir: i < 0 ? '' : path.slice(0, i), name: path.slice(i + 1) };
};

const commentCounts = (comments: ReviewComment[]): Map<string, number> => {
  const counts = new Map<string, number>();
  for (const c of comments) counts.set(c.path, (counts.get(c.path) ?? 0) + 1);
  return counts;
};

const FileList = ({ entries, counts }: { entries: Entry[]; counts: Map<string, number> }) => (
  <ul class="file-list">
    {entries.map(({ file, index }) => {
      const { dir, name } = splitPath(file.path);
      const n = counts.get(file.path) ?? 0;
      return (
        <li key={index}>
          <button class="file-item" onClick={() => document.getElementById(anchorId(index))?.scrollIntoView({ block: 'start' })}>
            <span class="file-line">
              <span class={`fstatus fstatus-${file.status}`}>{STATUS_LETTER[file.status]}</span>
              <span class="file-name">{name}</span>
              <DiffStat file={file} />
            </span>
            <span class="file-line file-sub">
              <span class="file-dir" title={dir}>{dir || '/'}</span>
              {file.editedBySession && <span class="tag tag-info">編集</span>}
              {n > 0 && <span class="tag tag-warn">コメント {n}</span>}
            </span>
          </button>
        </li>
      );
    })}
  </ul>
);

type RangeProps = { commits: CommitInfo[]; range: DiffRange; worktree: boolean; onChange: (range: DiffRange) => void };

/**
 * 範囲の端の候補は新しい順に並べ、未コミットの変更を最新のコミットの 1 つ先に置く。ブランチの表示には、未コミットの変更が無いので置かない。
 * 始まりが終わりより新しくなる選び方をしたら、もう一方の端も合わせる。
 */
const RangePicker = ({ commits, range, worktree, onChange }: RangeProps) => {
  const ends = [WORKTREE, ...commits.map((c) => c.sha)];
  const options = () => [
    worktree && <option key={WORKTREE} value={WORKTREE}>未コミットの変更</option>,
    ...commits.map((c) => <option key={c.sha} value={c.sha}>{c.sha.slice(0, 7)} {c.subject}</option>),
  ];
  const setFrom = (from: string) => onChange({ from, to: ends.indexOf(from) < ends.indexOf(range.to) ? from : range.to });
  const setTo = (to: string) => onChange({ from: ends.indexOf(to) > ends.indexOf(range.from) ? to : range.from, to });
  return (
    <span class="range-picker">
      範囲
      <select class="input input-sm" aria-label="範囲の始まり（含める最も古いコミット）" value={range.from} onChange={(e) => setFrom(e.currentTarget.value)}>
        {options()}
      </select>
      〜
      <select class="input input-sm" aria-label="範囲の終わり（含める最も新しいコミット）" value={range.to} onChange={(e) => setTo(e.currentTarget.value)}>
        {options()}
      </select>
    </span>
  );
};

type ViewProps = { views: BranchView[]; index: number; onPick: (index: number) => void };

/** Claude が並べたブランチと、このセッションの変更を切り替える。 */
const ViewPicker = ({ views, index, onPick }: ViewProps) => (
  <label class="inline-field">
    対象
    <select class="input input-sm" value={index} onChange={(e) => onPick(Number(e.currentTarget.value))}>
      <option value={-1}>このセッションの変更</option>
      {views.map((v, i) => <option key={i} value={i}>{v.label ?? v.branch}</option>)}
    </select>
  </label>
);

type ToolbarProps = {
  diff?: DiffResult;
  picker: ComponentChildren;
  worktree: boolean;
  full: boolean;
  loading: boolean;
  onRange: (range: DiffRange) => void;
  onFull: (v: boolean) => void;
  onReload: () => void;
};

const CodeToolbar = ({ diff, picker, worktree, full, loading, onRange, onFull, onReload }: ToolbarProps) => (
  <div class="code-toolbar">
    {picker}
    {diff ? <RangePicker commits={diff.commits} range={diff.range} worktree={worktree} onChange={onRange} /> : <span class="muted">範囲 …</span>}
    <label class="check">
      <input type="checkbox" checked={full} onChange={(e) => onFull(e.currentTarget.checked)} />
      ファイル全体を表示
    </label>
    <button class="btn btn-sm" disabled={loading} onClick={onReload}>再読み込み</button>
  </div>
);

type DiffFilesProps = { entries: Entry[]; full: boolean; review: ReviewDraft; focus?: Focus; sync?: PrSync };

const DiffFiles = ({ entries, full, review, focus, sync }: DiffFilesProps) => (
  <div class="diff-files">
    {entries.length === 0 && <Empty>表示できる差分はありません。</Empty>}
    {entries.map(({ file, index }) => (
      <FileDiff
        key={`${file.path}:${full}`}
        id={anchorId(index)}
        file={file}
        comments={review.comments.filter((c) => c.path === file.path)}
        threads={sync?.threads.filter((t) => t.path === file.path)}
        viewed={sync?.viewed.has(file.path)}
        onViewed={sync && ((v) => sync.setViewed(file.path, v))}
        onSave={review.upsert}
        onDelete={review.remove}
        focusLine={focus?.index === index ? (focus.line ?? null) : undefined}
      />
    ))}
  </div>
);

type PrSync = ReturnType<typeof usePrSync>;

/**
 * PR を表示しているとき、GitHub の PR のスレッドと「Viewed」を読む。Viewed を切り替えたら、画面を先に変えてから GitHub に書く。
 * スレッドは、Claude が応答を始めたときと終えたときに読み直す。PR に返信や修正が加わっていることがあるため。
 */
const usePrSync = (s: Session, pr: PrInfo | undefined) => {
  const threads = useCached((fresh) => (pr ? api.prThreads(s.id, undefined, fresh) : Promise.resolve([])), [pr?.url, s.status]);
  // Viewed は、サーバーが裏で読み直しても取り直さない。切り替えた直後に取り直すと、書き込む前の状態で画面を戻してしまうため。
  const remote = useFetch((fresh) => (pr ? api.prViewed(s.id, fresh) : Promise.resolve([])), [pr?.url]);
  const [viewed, setLocal] = useState<Set<string>>(new Set());
  useEffect(() => setLocal(new Set(remote.data)), [remote.data]);
  if (!pr) return undefined;
  const setViewed = async (path: string, v: boolean) => {
    const next = new Set(viewed);
    if (v) next.add(path);
    else next.delete(path);
    setLocal(next);
    // 書けなかったら、GitHub の状態に戻す。
    if (!(await attempt('Viewed の切り替え', () => api.setPrViewed(s.id, path, v)))) remote.reload();
  };
  return { pr, threads: threads.data ?? [], viewed, setViewed, reloadThreads: threads.reload };
};

type BodyProps = { s: Session; focus?: string; view?: BranchView; pr?: PrInfo; picker: ComponentChildren };

/** view は、Claude が並べたブランチか PR のうち、表示しているもの。無ければ、このセッションの変更を出す。pr は、PR を表示しているとき。 */
const CodeBody = ({ s, focus: ref, view, pr, picker }: BodyProps) => {
  const [full, setFull] = useState(false);
  const [onlyEdited, setOnlyEdited] = useState(false);
  // 人間が選んだ範囲。Claude がレビューを頼んだら、その範囲にする。どちらも無ければ、サーバーの既定（分岐点から先端まで）を出す。
  // レビューを頼まれるのはこのセッションの変更なので、ブランチの表示には範囲を当てない。
  const [range, setRange] = useState(view ? undefined : s.reviewRange);
  useEffect(() => {
    if (s.reviewRange && !view) setRange(s.reviewRange);
  }, [s.reviewRange?.from, s.reviewRange?.to]);
  const diff = useFetch(() => api.diff(s.id, full, range, view), [s.id, full, range?.from, range?.to]);
  const focus = diff.data && ref ? findFocus(diff.data.files, ref) : undefined;
  // 差分に無いファイルへのリンクで開いたときは、ファイル全体を読んで出す。
  const missing = !!ref && !!diff.data && !focus;
  const whole = useFetch(() => (missing && ref ? api.file(s.id, ref, view?.branch) : Promise.resolve(undefined)), [s.id, ref, missing]);
  const wholeFile = missing && !whole.loading && !whole.error ? whole.data : undefined;
  const wholeLine = wholeFile && ref ? findFocus([wholeFile], ref)?.line : undefined;
  // 開くファイルが届いたら一度だけ、そのファイル（行があればその行）までスクロールする。
  useEffect(() => {
    const id = focus ? anchorId(focus.index) : wholeFile ? WHOLE_ANCHOR : undefined;
    const section = id ? document.getElementById(id) : null;
    (section?.querySelector('.dl.is-focused') ?? section)?.scrollIntoView({ block: (focus?.line ?? wholeLine) ? 'center' : 'start' });
  }, [ref, focus?.index, focus?.line, wholeFile]);
  const review = useReviewDraft(s.id, view?.branch);
  const sync = usePrSync(s, pr);
  const entries: Entry[] = (diff.data?.files ?? [])
    .map((file, index) => ({ file, index }))
    .filter(({ file }) => !onlyEdited || file.editedBySession);
  const barHeight = BAR_HEIGHT + (s.awaitingReview ? BANNER_HEIGHT : 0);
  return (
    <div class="code-view" style={`--bar-h: ${barHeight}px`}>
      <ReviewBar s={s} review={review} branch={view?.branch} pr={sync && { info: sync.pr, commit: diff.data?.range.to, onPosted: sync.reloadThreads }} />
      <CodeToolbar diff={diff.data} picker={picker} worktree={!view} full={full} loading={diff.loading} onRange={setRange} onFull={setFull} onReload={diff.reload} />
      {diff.error && <p class="card panel error-note">差分を取得できませんでした（{diff.error}）</p>}
      {missing && whole.error && <p class="card panel warn-note">「{ref}」は、差分にも{view ? ` ${view.branch} ` : 'リポジトリ'}にもありません。</p>}
      {wholeFile && (
        <>
          <p class="card panel warn-note">「{ref}」は差分に無いため、ファイル全体を表示しています。</p>
          <FileDiff
            key={wholeFile.path}
            id={WHOLE_ANCHOR}
            file={wholeFile}
            comments={review.comments.filter((c) => c.path === wholeFile.path)}
            onSave={review.upsert}
            onDelete={review.remove}
            focusLine={wholeLine ?? null}
          />
        </>
      )}
      {diff.data?.truncated && <p class="card panel warn-note">差分が大きいため、一部を省略しています。</p>}
      {diff.data && (
        <div class={`code-layout ${diff.loading ? 'is-loading' : ''}`}>
          <aside class="code-side">
            {!view && (
              <label class="check">
                <input type="checkbox" checked={onlyEdited} onChange={(e) => setOnlyEdited(e.currentTarget.checked)} />
                このセッションが編集したファイルのみ
              </label>
            )}
            <FileList entries={entries} counts={commentCounts(review.comments)} />
          </aside>
          <DiffFiles entries={entries} full={full} review={review} focus={focus} sync={sync} />
        </div>
      )}
      {!diff.data && !diff.error && <Loading />}
    </div>
  );
};

/** 開いている PR の差分。GitHub の行番号と合うよう、手元のブランチではなく、push 済みの origin のブランチを出す。 */
const prViewOf = (s: Session): BranchView | undefined =>
  s.pr?.state === 'OPEN' && s.branch ? { branch: `origin/${s.branch}`, base: s.pr.baseRef && `origin/${s.pr.baseRef}`, label: `PR #${s.pr.number}` } : undefined;

/**
 * Claude が show_branches でブランチを並べたら、先頭のものを開く。並べていなくて、開いている PR があれば、PR を開く。
 * 人間は、並べたものと PR と、このセッションの変更を切り替えられる。
 * Claude が並べ直すか、このセッションの変更のレビューを頼んだら、人間の選択を捨てて、Claude の選んだものに戻す。
 */
export const CodeView = ({ s, focus }: { s: Session; focus?: string }) => {
  const prView = prViewOf(s);
  const views = [...(s.branchViews ?? []), ...(prView ? [prView] : [])];
  const preset = s.awaitingReview ? -1 : 0;
  const choice = `${JSON.stringify(views)}:${preset}`;
  const [picked, setPicked] = useState<{ choice: string; index: number }>();
  const index = picked?.choice === choice ? picked.index : preset;
  const view = views[index];
  const picker = views.length > 0 && <ViewPicker views={views} index={view ? index : -1} onPick={(i) => setPicked({ choice, index: i })} />;
  // 範囲と下書きは表示するものごとに違うので、表示を切り替えたら作り直す。
  return <CodeBody key={view ? `${view.branch}\0${view.base ?? ''}` : WORKTREE} s={s} focus={focus} view={view} pr={prView && view === prView ? s.pr : undefined} picker={picker} />;
};
