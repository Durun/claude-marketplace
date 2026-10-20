import type { ComponentChildren } from 'preact';
import { useContext, useRef, useState } from 'preact/hooks';
import type { IssueInfo, JiraIssue, PrInfo, RepoRef, SessionStatus, Usage } from '../shared/types.ts';
import { fmtInt, relativeTime, totalTokens } from './format.ts';
import { NowContext, useWaitCursor } from './hooks.ts';
import { toast } from './toast.tsx';

type Tone = 'ok' | 'bad' | 'warn' | 'info' | 'muted';

const STATUS_LABEL: Record<SessionStatus, string> = { busy: '作業中', idle: '入力待ち', done: '完了', interrupted: '中断', ended: '終了' };

export const Chip = ({ tone = 'muted', title, children }: { tone?: Tone; title?: string; children: ComponentChildren }) => (
  <span class={`chip chip-${tone}`} title={title}>{children}</span>
);

export const StatusBadge = ({ status }: { status: SessionStatus }) => (
  <span class={`badge badge-${status}`}>
    <i class="dot" aria-hidden="true" />
    {STATUS_LABEL[status]}
  </span>
);

export const Time = ({ at }: { at: number }) => {
  const now = useContext(NowContext);
  const date = new Date(at);
  return <time dateTime={date.toISOString()} title={date.toLocaleString('ja-JP')}>{relativeTime(at, now)}</time>;
};

const isHttpUrl = (url: string): boolean => /^https?:\/\//i.test(url);

/** Claude の出力から作られた URL で javascript: などを実行させないよう、http(s) だけをリンクにする。 */
/** attach は「なんでも添付」で URL の代わりに書き足す値。 */
export const Ext = ({ href, class: cls, title, attach, children }: { href: string; class?: string; title?: string; attach?: string; children: ComponentChildren }) =>
  isHttpUrl(href) ? (
    <a class={cls} href={href} title={title} data-attach={attach} target="_blank" rel="noopener noreferrer">{children}</a>
  ) : (
    <span class={cls} title={title} data-attach={attach}>{children}</span>
  );

export const Empty = ({ children }: { children: ComponentChildren }) => <p class="empty">{children}</p>;

export const PinIcon = () => (
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <g transform="rotate(45 8 8)">
      <path d="M5 1h6v1.5H9.8v4L12 9H4l2.2-2.5v-4H5z" fill="currentColor" />
      <path d="M8 9v6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
    </g>
  </svg>
);

// 帯の幅（%）。chat は、依頼（右）と応答（左）の吹き出しを交互に並べる。
const SKELETON = { text: [90, 75, 85, 60], chat: [45, 80, 35, 70] };

/** 中身を読み込んでいる間に、中身の代わりに灰色の帯を並べる。出ている間は、カーソルを砂時計にする。 */
export const Loading = ({ kind = 'text' }: { kind?: keyof typeof SKELETON }) => {
  useWaitCursor(true);
  return (
    <div class={`skeleton skeleton-${kind}`} role="status" aria-label="読み込み中">
      {SKELETON[kind].map((w, i) => <span key={i} style={`--w: ${w}%`} />)}
    </div>
  );
};

type ChoiceProps = {
  label: string;
  value?: string;
  options: readonly string[];
  onChange: (value: string) => void;
  /** 空の選択肢の見出し。false なら空を選べない。 */
  empty?: string | false;
  disabled?: boolean;
  title?: string;
};

/** options から 1 つを選ぶ。空の「既定」も選べる。 */
export const Choice = ({ label, value, options, onChange, empty = '既定', disabled, title }: ChoiceProps) => (
  <label class="inline-field" title={title}>
    {label}
    <select class="input input-sm input-auto" disabled={disabled} value={value ?? ''} onChange={(e) => onChange(e.currentTarget.value)}>
      {empty !== false && <option value="">{empty}</option>}
      {options.map((o) => <option key={o} value={o}>{o}</option>)}
    </select>
  </label>
);

export const Panel = ({ title, count, children }: { title: string; count?: number; children: ComponentChildren }) => (
  <section class="card panel">
    <h2 class="panel-title">
      {title}
      {count !== undefined && <span class="count-pill">{count}</span>}
    </h2>
    {children}
  </section>
);

/** Cmd/Ctrl + Enter で送信する。日本語入力を確定する Enter では送信しない。 */
export const onSubmitKey = (e: KeyboardEvent, submit: () => void): void => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && !e.isComposing) {
    e.preventDefault();
    submit();
  }
};

const REVIEW_CHIP: Record<string, { text: string; tone: Tone } | undefined> = {
  APPROVED: { text: '承認済み', tone: 'ok' },
  CHANGES_REQUESTED: { text: '変更要求', tone: 'bad' },
  REVIEW_REQUIRED: { text: 'レビュー未了', tone: 'muted' },
};

type PrState = 'draft' | 'open' | 'merged' | 'closed';

const PR_STATE_LABEL: Record<PrState, string> = { draft: 'ドラフト', open: 'オープン', merged: 'マージ済み', closed: 'クローズ' };

// 左の線と丸は共通。右側の形で、オープン（矢印）、ドラフト（点線）、マージ（合流）、クローズ（×）を描き分ける。
const PR_ICON_PATH: Record<PrState, string> = {
  open: 'M12 11V6.5A2.5 2.5 0 0 0 9.5 4H7M9 2 7 4l2 2',
  draft: 'M12 11V9M12 7V5.5',
  merged: 'M4 4.5c0 2.5 2.5 3.5 6.5 3.5',
  closed: 'M10 2.5l4 4M14 2.5l-4 4',
};

export const prState = (pr: PrInfo): PrState =>
  pr.state === 'MERGED' ? 'merged' : pr.state === 'CLOSED' ? 'closed' : pr.isDraft ? 'draft' : 'open';

export const PrIcon = ({ state, label = PR_STATE_LABEL[state] }: { state: PrState; label?: string }) => (
  <svg class={`pr-icon pr-${state}`} viewBox="0 0 16 16" width="12" height="12" role="img" aria-label={label}>
    <g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round">
      <circle cx="4" cy="3" r="1.5" />
      <circle cx="4" cy="13" r="1.5" />
      <path d="M4 4.5v7" />
      <circle cx={12} cy={state === 'merged' ? 8 : 13} r="1.5" />
      <path d={PR_ICON_PATH[state]} />
    </g>
  </svg>
);

const ciChip = (c: PrInfo['checks']): { text: string; tone: Tone } | undefined => {
  if (c.failed > 0) return { text: `× CI 失敗 ${c.failed}`, tone: 'bad' };
  if (c.pending > 0) return { text: `● CI 実行中 ${c.pending}`, tone: 'warn' };
  if (c.passed > 0) return { text: '✓ CI 成功', tone: 'ok' };
  return undefined;
};

/** JIRA のチップと同じく、PR 番号の右に題名の先頭を添え、区切って CI とレビューの状態を並べ、1 つのチップにまとめる。 */
/**
 * session を渡すと、押したときにそのセッションの「PR」ウィジェットでその PR を開く。開く処理は、PR へのリンクの横取りが data-session を読んで行う。
 * hideTitle は、すぐそばに題名を出している画面で、題名を重ねて出さないために渡す。
 */
export const PrChips = ({ pr, session, hideTitle }: { pr: PrInfo; session?: string; hideTitle?: boolean }) => {
  const state = prState(pr);
  const subs = [ciChip(pr.checks), REVIEW_CHIP[pr.review]].filter((c) => c !== undefined);
  return (
    <a class={`chip chip-link pr-${state}`} href={pr.url} title={`${PR_STATE_LABEL[state]}: ${pr.title}`} data-session={session} target="_blank" rel="noopener noreferrer">
      <PrIcon state={state} />
      PR #{pr.number}
      {!hideTitle && pr.title && <span class="chip-title">{pr.title}</span>}
      {subs.map((c) => <span key={c.text} class={`chip-sub chip-sub-${c.tone}`}>{c.text}</span>)}
    </a>
  );
};

const ISSUE_STATE_LABEL: Record<IssueInfo['state'], string> = { Open: 'オープン', Closed: '完了', Canceled: 'キャンセル' };

// 円は共通。中の形で、オープン（点）、完了（チェック）、キャンセル（斜線）を描き分ける。
const IssueIcon = ({ state, label = ISSUE_STATE_LABEL[state] }: { state: IssueInfo['state']; label?: string }) => (
  <svg class={`issue-icon issue-${state.toLowerCase()}`} viewBox="0 0 16 16" width="12" height="12" role="img" aria-label={label}>
    <g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
      <circle cx="8" cy="8" r="6.3" />
      {state === 'Open' ? <circle cx="8" cy="8" r="1.2" fill="currentColor" /> : <path d={state === 'Closed' ? 'M5.3 8.2l1.8 1.8 3.6-3.6' : 'M4 12 12 4'} />}
    </g>
  </svg>
);

/** PR のチップと同じく、Issue の番号の右に題名の先頭を添える。ダッシュボードに Issue の画面は無いので、押すと GitHub で開く。 */
export const IssueChip = ({ issue }: { issue: IssueInfo }) => (
  <a class={`chip chip-link issue-${issue.state.toLowerCase()}`} href={issue.url} title={`${ISSUE_STATE_LABEL[issue.state]}: ${issue.title}`} target="_blank" rel="noopener noreferrer">
    <IssueIcon state={issue.state} />
    Issue #{issue.number}
    {issue.title && <span class="chip-title">{issue.title}</span>}
  </a>
);

/** 補完の候補のように種別を文字で出さないところで、PR と Issue をバッジと同じアイコンで描く。読み上げには種別も含める。 */
export const RefIcon = ({ kind, state }: Pick<RepoRef, 'kind' | 'state'>) => {
  // RepoRef の state は、PR なら小文字にすると PrState に、Issue なら IssueInfo['state'] に収まる。
  if (kind === 'pr') {
    const pr = state.toLowerCase() as PrState;
    return <PrIcon state={pr} label={`PR ${PR_STATE_LABEL[pr]}`} />;
  }
  const issue = state as IssueInfo['state'];
  return <IssueIcon state={issue} label={`Issue ${ISSUE_STATE_LABEL[issue]}`} />;
};

/** session を渡すと、押したときにそのセッションの「JIRA」ウィジェットでその課題を開く。PrChips と同じく、課題へのリンクの横取りが data-session を読んで開く。 */
export const JiraChips = ({ issues, session }: { issues: JiraIssue[]; session?: string }) => (
  <>
    {issues.map((j) =>
      isHttpUrl(j.url) ? (
        <a key={j.key} class="chip chip-link" href={j.url} title={j.summary} data-attach={j.key} data-session={session} target="_blank" rel="noopener noreferrer">
          {j.key}
          {j.summary && <span class="chip-title">{j.summary}</span>}
          {j.status && <span class="chip-sub">{j.status}</span>}
        </a>
      ) : (
        <span key={j.key} class="chip chip-link" title={j.summary} data-attach={j.key}>
          {j.key}
          {j.summary && <span class="chip-title">{j.summary}</span>}
          {j.status && <span class="chip-sub">{j.status}</span>}
        </span>
      ),
    )}
  </>
);

type Step = [min: number, color: string];

// コンテキストは小さいうちから色を変えて肥大化に気づかせる。レート制限は使い切りに近い 70% と 90% で変える。
// 使用ペースは、リセットでちょうど使い切る 100% と、その 1 割増しの 110% で変える。

const CONTEXT_STEPS: Step[] = [[40, 'var(--critical)'], [30, 'var(--serious)'], [20, 'var(--warning)']];
const RATE_STEPS: Step[] = [[90, 'var(--critical)'], [70, 'var(--warning)']];
const PACE_STEPS: Step[] = [[110, 'var(--critical)'], [100, 'var(--warning)'], [0, 'var(--good)']];

const stepColor = (steps: Step[], percent: number): string =>
  steps.find(([min]) => Math.round(percent) >= min)?.[1] ?? 'var(--accent)';

export const contextColor = (percent: number): string => stepColor(CONTEXT_STEPS, percent);
export const rateColor = (percent: number): string => stepColor(RATE_STEPS, percent);
export const paceColor = (percent: number): string => stepColor(PACE_STEPS, percent);

export const Meter = ({ percent, color, label }: { percent: number; color: string; label: string }) => (
  <div class="meter" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(percent)} style={`--c: ${color}`}>
    <div class="meter-fill" style={`width: ${Math.min(100, Math.max(0, percent))}%`} />
  </div>
);

export const ContextBar = ({ usage }: { usage: Usage }) => (
  <div class="ctx" title={`${fmtInt(usage.contextTokens)} / ${fmtInt(usage.contextWindow)} トークン`}>
    <span class="ctx-label">コンテキスト</span>
    <Meter percent={usage.contextPercent} color={contextColor(usage.contextPercent)} label="コンテキスト使用率" />
    <span class="ctx-value">{Math.round(usage.contextPercent)}%</span>
    {!usage.exact && <span class="ctx-est">推定</span>}
  </div>
);

/**
 * 送信ボタンの横に置く、コンテキストの使用率のドーナツグラフ。12 時から時計回りに使用率の分だけ弧を伸ばし、中央に使用率の数字を出す。
 * マウスを載せるかフォーカスすると内訳を開く。ウィジェットの中身は枠で切られるので、内訳は popover にして最前面に出し、source でアイコンに寄せる。
 */
export const ContextPie = ({ usage }: { usage: Usage }) => {
  const percent = Math.round(usage.contextPercent);
  const button = useRef<HTMLButtonElement>(null);
  const flyout = useRef<HTMLDivElement>(null);
  const show = () => flyout.current?.showPopover({ source: button.current ?? undefined });
  const hide = () => flyout.current?.hidePopover();
  return (
    <>
      <button ref={button} type="button" class="ctx-pie" aria-label={`コンテキスト ${percent}%${usage.exact ? '' : '（推定）'}`} onPointerEnter={show} onPointerLeave={hide} onFocus={show} onBlur={hide}>
        <svg viewBox="0 0 28 28" aria-hidden="true">
          <circle class="ctx-pie-track" cx="14" cy="14" r="12" />
          <circle
            cx="14" cy="14" r="12" pathLength={100} transform="rotate(-90 14 14)"
            style={`stroke: ${contextColor(usage.contextPercent)}`} stroke-dasharray={`${Math.min(100, Math.max(0, usage.contextPercent))} 100`}
          />
          <text class="ctx-pie-value" x="14" y="14">{percent}%</text>
        </svg>
      </button>
      <div ref={flyout} class="ctx-flyout" popover="manual">
        <b class="ctx-flyout-head">コンテキスト</b>
        <ContextBar usage={usage} />
        <span>{fmtInt(usage.contextTokens)} / {fmtInt(usage.contextWindow)} トークン</span>
        {usage.model && <span class="muted">モデル {usage.model}</span>}
        <span class="muted">
          合計 {fmtInt(totalTokens(usage.totals))} トークン{usage.costUsd !== undefined && ` · 概算 $${usage.costUsd.toFixed(2)}`}
        </span>
      </div>
    </>
  );
};

export const copyText = (text: string): Promise<boolean> =>
  (navigator.clipboard?.writeText(text) ?? Promise.reject(new Error('clipboard unavailable'))).then(
    () => true,
    () => false,
  );

const COPIED_MS = 1500;

export const CopyButton = ({ text, label, title }: { text: string; label: string; title?: string }) => {
  const [done, setDone] = useState(false);
  const click = async () => {
    if (!(await copyText(text))) {
      toast('クリップボードにコピーできませんでした', 'error');
      return;
    }
    setDone(true);
    setTimeout(() => setDone(false), COPIED_MS);
  };
  return <button class="btn btn-sm" title={title} onClick={click}>{done ? 'コピーしました' : label}</button>;
};

/** 文字のまま置き、押すと text をコピーする。見えている文字と違う値（ディレクトリ名に対する絶対パスなど）もコピーできる。 */
export const CopyText = ({ text, title, children }: { text: string; title: string; children: ComponentChildren }) => {
  const click = async () => {
    if (await copyText(text)) toast(`コピーしました: ${text}`);
    else toast('クリップボードにコピーできませんでした', 'error');
  };
  return <button class="copy-text" title={title} onClick={click}>{children}</button>;
};
