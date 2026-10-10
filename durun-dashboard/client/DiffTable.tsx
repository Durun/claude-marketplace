import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import type { DiffFile, DiffHunk, DiffLine, PrThread, ReviewComment } from '../shared/types.ts';
import { highlightLine, languageOf } from './highlight.ts';
import { Markdown } from './Markdown.tsx';
import { commentKey, lineTarget } from './review.ts';
import { Ext, Time, onSubmitKey } from './ui.tsx';

const SIGN: Record<DiffLine['kind'], string> = { ctx: ' ', add: '+', del: '-' };

export const STATUS_LETTER: Record<DiffFile['status'], string> = { added: 'A', modified: 'M', deleted: 'D', renamed: 'R', unchanged: '=' };
const STATUS_LABEL: Record<DiffFile['status'], string> = { added: '追加', modified: '変更', deleted: '削除', renamed: '名前変更', unchanged: '変更なし' };

export const DiffStat = ({ file }: { file: DiffFile }) => (
  <span class="stat">
    <span class="stat-add">+{file.additions}</span>
    <span class="stat-del">−{file.deletions}</span>
  </span>
);

type FileCtx = {
  path: string;
  /** コードへのリンクで開いた行（新しい側の行番号） */
  focusLine?: number | null;
  lang?: string;
  comments: Map<string, ReviewComment>;
  threads: Map<string, PrThread[]>;
  openKey: string | null;
  setOpenKey: (key: string | null) => void;
  onSave: (c: ReviewComment) => void;
  onDelete: (key: string) => void;
};

const CommentEditor = ({ initial, onSave, onCancel }: { initial: string; onSave: (body: string) => void; onCancel: () => void }) => {
  const [body, setBody] = useState(initial);
  const ref = useRef<HTMLTextAreaElement>(null);
  useEffect(() => ref.current?.focus(), []);
  const save = () => {
    if (body.trim()) onSave(body.trim());
  };
  return (
    <div class="comment-box is-editing">
      <textarea
        ref={ref}
        class="input"
        rows={3}
        placeholder="この行へのコメント（Cmd/Ctrl + Enter で保存、Esc で閉じる）"
        value={body}
        onInput={(e) => setBody(e.currentTarget.value)}
        onKeyDown={(e) => {
          onSubmitKey(e, save);
          if (e.key === 'Escape') onCancel();
        }}
      />
      <div class="actions">
        <button class="btn btn-sm btn-primary" disabled={!body.trim()} onClick={save}>保存</button>
        <button class="btn btn-sm" onClick={onCancel}>キャンセル</button>
      </div>
    </div>
  );
};

const SavedComment = ({ comment, onEdit, onDelete }: { comment: ReviewComment; onEdit: () => void; onDelete: () => void }) => (
  <div class="comment-box">
    <p class="plain">{comment.body}</p>
    <div class="actions">
      <button class="btn btn-sm" onClick={onEdit}>編集</button>
      <button class="btn btn-sm" onClick={onDelete}>削除</button>
    </div>
  </div>
);

/** PR に付いたスレッド。GitHub で返信や解決をするので、ここでは読むだけにする。 */
const PrThreadBox = ({ thread }: { thread: PrThread }) => (
  <div class="comment-box is-pr">
    {thread.comments.map((c) => (
      <div key={c.id} class="pr-comment">
        <span class="muted">{c.author} · <Time at={c.at} /></span>
        <Markdown text={c.body} />
      </div>
    ))}
    <div class="actions">
      <Ext href={thread.comments[0]?.url ?? ''}>GitHub で返信する</Ext>
    </div>
  </div>
);

const LineNo = ({ no, focusable, label, onClick }: { no?: number; focusable: boolean; label: string; onClick: () => void }) => (
  <td class="ln" onClick={onClick}>
    {focusable && no != null ? <button class="ln-btn" aria-label={label}>{no}</button> : no}
  </td>
);

// 差分の表は、更新が届くたびや操作のたびに描き直される。色付けは重いので、取得した差分の行ごとに 1 回だけにする。
const highlighted = new WeakMap<DiffLine, string>();

const highlightOnce = (line: DiffLine, lang: string): string => {
  const cached = highlighted.get(line);
  if (cached !== undefined) return cached;
  const html = highlightLine(line.text, lang);
  highlighted.set(line, html);
  return html;
};

const LineRows = ({ line, ctx }: { line: DiffLine; ctx: FileCtx }) => {
  const target = lineTarget(line);
  const key = commentKey({ path: ctx.path, ...target });
  const comment = ctx.comments.get(key);
  const threads = ctx.threads.get(key) ?? [];
  const open = ctx.openKey === key;
  const toggle = () => ctx.setOpenKey(open ? null : key);
  const label = `${target.line} 行目にコメントする`;
  return (
    <>
      <tr class={`dl dl-${line.kind}${comment || threads.length ? ' has-comment' : ''}${line.newNo != null && line.newNo === ctx.focusLine ? ' is-focused' : ''}`}>
        <LineNo no={line.oldNo} focusable={line.newNo == null} label={label} onClick={toggle} />
        <LineNo no={line.newNo} focusable={line.newNo != null} label={label} onClick={toggle} />
        <td class="sign">{SIGN[line.kind]}</td>
        {ctx.lang ? <td class="code" dangerouslySetInnerHTML={{ __html: highlightOnce(line, ctx.lang) }} /> : <td class="code">{line.text}</td>}
      </tr>
      {threads.length > 0 && (
        <tr class="comment-row">
          <td colSpan={4}>
            {threads.map((t) => <PrThreadBox key={t.id} thread={t} />)}
          </td>
        </tr>
      )}
      {(comment || open) && (
        <tr class="comment-row">
          <td colSpan={4}>
            {open ? (
              <CommentEditor
                initial={comment?.body ?? ''}
                onCancel={() => ctx.setOpenKey(null)}
                onSave={(body) => {
                  ctx.onSave({ path: ctx.path, ...target, lineText: line.text, body });
                  ctx.setOpenKey(null);
                }}
              />
            ) : comment && (
              <SavedComment comment={comment} onEdit={() => ctx.setOpenKey(key)} onDelete={() => ctx.onDelete(key)} />
            )}
          </td>
        </tr>
      )}
    </>
  );
};

const HunkRows = ({ hunk, ctx }: { hunk: DiffHunk; ctx: FileCtx }) => (
  <tbody>
    <tr class="hunk">
      <td colSpan={4}>{hunk.header}</td>
    </tr>
    {hunk.lines.map((line, i) => <LineRows key={i} line={line} ctx={ctx} />)}
  </tbody>
);

const FileBody = ({ file, ctx }: { file: DiffFile; ctx: FileCtx }) => {
  if (file.binary) return <p class="diff-note">バイナリファイルの差分は表示できません。</p>;
  if (file.hunks.length === 0) return <p class="diff-note">内容の変更はありません。</p>;
  return (
    <div class="diff-scroll">
      <table class="diff">
        {file.hunks.map((h, i) => <HunkRows key={i} hunk={h} ctx={ctx} />)}
      </table>
    </div>
  );
};

const NO_THREADS: PrThread[] = [];

const byLine = (threads: PrThread[]): Map<string, PrThread[]> => {
  const map = new Map<string, PrThread[]>();
  for (const t of threads) {
    if (t.line === undefined) continue;
    const key = commentKey({ path: t.path, side: t.side, line: t.line });
    map.set(key, [...(map.get(key) ?? []), t]);
  }
  return map;
};

type FileDiffProps = {
  file: DiffFile;
  id: string;
  comments: ReviewComment[];
  /** PR を表示しているときの、このファイルに付いた PR のスレッド。 */
  threads?: PrThread[];
  /** PR を表示しているときの、GitHub の「Viewed」。切り替えると onViewed を呼ぶ。 */
  viewed?: boolean;
  onViewed?: (viewed: boolean) => void;
  onSave: (c: ReviewComment) => void;
  onDelete: (key: string) => void;
  /** コードへのリンクで開いたファイルなら、行番号か、行の指定が無ければ null。 */
  focusLine?: number | null;
};

export const FileDiff = ({ file, id, comments, threads = NO_THREADS, viewed, onViewed, onSave, onDelete, focusLine }: FileDiffProps) => {
  const [collapsed, setCollapsed] = useState(false);
  const [openKey, setOpenKey] = useState<string | null>(null);
  const byKey = useMemo(() => new Map(comments.map((c) => [commentKey(c), c])), [comments]);
  const threadsByKey = useMemo(() => byLine(threads), [threads]);
  // GitHub と同じく、見終えたファイルは畳む。
  useEffect(() => {
    if (viewed !== undefined) setCollapsed(viewed);
  }, [viewed]);
  const ctx: FileCtx = { path: file.path, lang: languageOf(file.path), focusLine, comments: byKey, threads: threadsByKey, openKey, setOpenKey, onSave, onDelete };
  const head = (
    <button class="diff-file-head" aria-expanded={!collapsed} onClick={() => setCollapsed(!collapsed)}>
      <span class={`fstatus fstatus-${file.status}`} title={STATUS_LABEL[file.status]}>{STATUS_LETTER[file.status]}</span>
      <span class="path mono">{file.oldPath && file.oldPath !== file.path ? `${file.oldPath} → ${file.path}` : file.path}</span>
      {file.status !== 'unchanged' && <DiffStat file={file} />}
      {file.editedBySession && <span class="tag tag-info">このセッションが編集</span>}
      {comments.length > 0 && <span class="tag tag-warn">コメント {comments.length}</span>}
      <span class="chevron" aria-hidden="true">{collapsed ? '▸' : '▾'}</span>
    </button>
  );
  return (
    <section class={`diff-file${focusLine !== undefined ? ' is-focused' : ''}`} id={id}>
      {onViewed ? (
        <div class="diff-file-head-row">
          {head}
          <label class="check viewed-check">
            <input type="checkbox" checked={viewed ?? false} onChange={(e) => onViewed(e.currentTarget.checked)} />
            Viewed
          </label>
        </div>
      ) : head}
      {!collapsed && <FileBody file={file} ctx={ctx} />}
    </section>
  );
};
