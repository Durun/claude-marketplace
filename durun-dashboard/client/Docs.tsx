import type { ComponentChildren } from 'preact';
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'preact/hooks';
import { type ChatEntry, type Doc, type DocFormat, type DocMeta, type FileTarget, type FileWhere, type SavedFile, type Session, type Snapshot, notionPageId } from '../shared/types.ts';
import { DELIVERY_TOAST, api, attempt } from './api.ts';
import { type ChatFile, type MergedLink, chatLinks, chatMarkdownFiles, mergeLinks, titleOf, withTitles } from './chatLinks.ts';
import { buildSrcdoc } from './docFrame.ts';
import { useSkin } from './skin.ts';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';
import { baseName } from './explorerFavorites.ts';
import { FILE_DRAG_TYPE, mediaTypeOf } from './fileRef.ts';
import { dateTime } from './format.ts';
import { useFetch } from './hooks.ts';
import { ARROW_STEP, moveTo } from './order.ts';
import { href } from './router.ts';
import { toast } from './toast.tsx';
import { Chip, Empty, Ext, Panel, onSubmitKey } from './ui.tsx';

const FORMAT_LABEL: Record<DocFormat, string> = {
  markdown: 'Markdown',
  html: 'HTML',
  mermaid: 'Mermaid',
  svg: 'SVG',
  text: 'テキスト',
};

/** present の資料を「なんでも添付」で書き足す値。ダッシュボードの URL は Claude が読めないので、ファイルか題名で指す。 */
const docAttach = (d: DocMeta): string => d.path ?? `資料「${d.title}」`;

const DocRow = ({ d, session }: { d: DocMeta; session?: Session }) => (
  <li>
    <a class={`doc-row ${d.seen ? '' : 'is-unread'}`} href={href.doc(d.id)} data-attach={docAttach(d)}>
      <span class="doc-title">{d.title}</span>
      {!d.seen && <Chip tone="info">未読</Chip>}
      <Chip>{FORMAT_LABEL[d.format]}</Chip>
      <span class="doc-session">{session?.name}</span>
      <time class="muted nowrap" dateTime={new Date(d.createdAt).toISOString()}>{dateTime(d.createdAt)}</time>
    </a>
  </li>
);

const DocList = ({ docs, sessions }: { docs: DocMeta[]; sessions: Session[] }) => {
  if (docs.length === 0) return <Empty>資料はありません。</Empty>;
  const byId = new Map(sessions.map((s) => [s.id, s]));
  const sorted = [...docs].sort((a, b) => b.createdAt - a.createdAt);
  return (
    <ul class="doc-list">
      {sorted.map((d) => <DocRow key={d.id} d={d} session={byId.get(d.sessionId)} />)}
    </ul>
  );
};

export const DocsPage = ({ snapshot }: { snapshot: Snapshot }) => (
  <Panel title="すべての資料" count={snapshot.docs.length}>
    <DocList docs={snapshot.docs} sessions={snapshot.sessions} />
  </Panel>
);

/**
 * セッション概要の資料の 1 行。keys は並べ替えと外すときに使う、present の資料の id か、1 行にまとめたリンクの URL。key はそのうち行を表すもの。
 * sub は形式かリンク先のホスト、icon はタイトルの先頭に添える favicon。
 */
type SideDoc = { key: string; keys: string[]; title: string; href: string; sub: string; icon?: string; at: number; unread?: boolean; external?: boolean; attach?: string; onClick?: (e: MouseEvent) => void };

// present で出した資料はダッシュボードの中で開くので、ダッシュボードの favicon を添える。
const OWN_ICON = document.querySelector<HTMLLinkElement>('link[rel="icon"]')?.href;
const faviconSrc = (url: string): string => `/api/favicon?${new URLSearchParams({ site: new URL(url).origin })}`;

/** 行に付ける、並べ替えの操作。 */
type DragHandlers = {
  onDragStart: (e: DragEvent) => void;
  onDragOver: (e: DragEvent) => void;
  onDrop: (e: DragEvent) => void;
  onDragEnd: (e: DragEvent) => void;
  onKeyDown: (e: KeyboardEvent) => void;
};

/** 資料の題名を付け直す入力欄。Enter とフォーカスを外したときに保存し、Esc でやめる。空にすると元の題名に戻す。 */
const RenameDoc = ({ title, onDone }: { title: string; onDone: (title?: string) => void }) => (
  <input
    class="input side-doc-rename"
    autoFocus
    aria-label="資料の題名"
    defaultValue={title}
    onFocus={(e) => e.currentTarget.select()}
    onBlur={(e) => onDone(e.currentTarget.value)}
    onKeyDown={(e) => {
      if (e.key === 'Enter') e.currentTarget.blur();
      if (e.key === 'Escape') onDone();
    }}
  />
);

type RowProps = { d: SideDoc; dragging: boolean; drag: DragHandlers; onRemove: () => void; onRename: (title: string) => void };

const SideDocRow = ({ d, dragging, drag, onRemove, onRename }: RowProps) => {
  const [renaming, setRenaming] = useState(false);
  const done = (title?: string) => {
    setRenaming(false);
    if (title !== undefined && title.trim() !== d.title) onRename(title);
  };
  const body = (
    <>
      <span class="side-doc-title" title={d.title}>
        {/* 読めない favicon は隠し、タイトルの位置はほかの行とそろえたままにする。 */}
        <img class="favicon" src={d.icon} alt="" width={16} height={16} onError={(e) => (e.currentTarget.style.visibility = 'hidden')} />
        {d.title}
      </span>
      <span class="side-doc-meta">
        {d.unread && <Chip tone="info">未読</Chip>}
        <span class="side-doc-sub">{d.sub}</span>
        {d.at > 0 && <time dateTime={new Date(d.at).toISOString()}>{dateTime(d.at)}</time>}
      </span>
    </>
  );
  return (
    <li class={`side-doc-item${dragging ? ' is-dragging' : ''}`} draggable={!renaming} {...drag}>
      {renaming ? (
        <RenameDoc title={d.title} onDone={done} />
      ) : d.external ? (
        <Ext class="side-doc" href={d.href}>{body}</Ext>
      ) : (
        <a class={`side-doc${d.unread ? ' is-unread' : ''}`} href={d.href} data-attach={d.attach} onClick={d.onClick}>{body}</a>
      )}
      <button class="side-doc-action" title="名前を変える" aria-label={`${d.title}の名前を変える`} onClick={() => setRenaming(true)}>
        ✎
      </button>
      <button class="side-doc-action" title="資料から外す" aria-label={`${d.title}を資料から外す`} onClick={onRemove}>
        ×
      </button>
    </li>
  );
};

/** 資料に足すリンクの入力欄。題名を空にすると、URL から題名を付ける。 */
const AddLink = ({ s, onClose }: { s: Session; onClose: () => void }) => {
  const [url, setUrl] = useState('');
  const [title, setTitle] = useState('');
  const save = async (e: Event) => {
    e.preventDefault();
    if (await attempt('資料の追加', () => api.addLink(s.id, url.trim(), title.trim()))) onClose();
  };
  return (
    <form class="card panel stack add-link" onSubmit={save}>
      <input class="input" type="url" required autoFocus aria-label="URL" placeholder="https://…" value={url} onInput={(e) => setUrl(e.currentTarget.value)} />
      <input class="input" aria-label="題名" placeholder="題名（空なら URL や Notion のページから付けます）" value={title} onInput={(e) => setTitle(e.currentTarget.value)} />
      <div class="actions">
        <span class="spacer" />
        <button type="button" class="btn btn-sm" onClick={onClose}>キャンセル</button>
        <button class="btn btn-sm btn-primary">追加</button>
      </div>
    </form>
  );
};

const linkDoc = (l: MergedLink): SideDoc => ({ key: l.url, keys: l.urls, title: l.title, href: l.url, sub: l.host, icon: faviconSrc(l.url), at: l.at, external: true });

/**
 * 並べ替えた後に出た資料は、並べ替えたものより前に新しい順で置く。
 * 1 行にまとめたリンクは、まとめたもののうち並べ替えた順でいちばん前の位置に置き、新しいリンクがまとまっても行を動かさない。
 */
const arrange = (docs: SideDoc[], order: string[]): SideDoc[] => {
  const rank = new Map(order.map((k, i) => [k, i]));
  const rankOf = (d: SideDoc): number => {
    const ranks = d.keys.flatMap((k) => rank.get(k) ?? []);
    return ranks.length ? Math.min(...ranks) : -1;
  };
  return docs.toSorted((a, b) => rankOf(a) - rankOf(b) || b.at - a.at);
};

/** 資料の並べ替え。並べ替えた順は、サーバーから届くまでの間も手元の順で出す。ドラッグの間は、離すまで保存しない。 */
const useDocOrder = (s: Session, docs: SideDoc[]) => {
  const [order, setOrder] = useState(s.docOrder);
  useEffect(() => setOrder(s.docOrder), [s.docOrder.join('\n')]);
  const [dragging, setDragging] = useState<string>();
  const list = useRef<HTMLUListElement>(null);
  // キーボードで動かした行は、並べ直すときに DOM から外れてフォーカスを失うので、動かした先でフォーカスし直す。
  const refocus = useRef<string>();
  const items = arrange(docs, order);
  const keys = items.map((d) => d.key);
  // 保存する順には、1 行にまとめたリンクをすべて入れる。
  const expand = (next: string[]): string[] => next.flatMap((k) => items.find((d) => d.key === k)?.keys ?? []);
  useLayoutEffect(() => {
    if (refocus.current === undefined) return;
    list.current?.querySelectorAll<HTMLElement>('.side-doc')[keys.indexOf(refocus.current)]?.focus();
    refocus.current = undefined;
  }, [order]);
  const save = (next: string[]) => {
    setOrder(next);
    void attempt('資料の並べ替え', () => api.orderDocs(s.id, next));
  };
  const dragOf = (key: string): DragHandlers => ({
    onDragStart: (e) => {
      setDragging(key);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
    },
    // ほかの行の上に来たら、その行の位置へ動かして見せる。
    onDragOver: (e) => {
      if (!dragging) return;
      e.preventDefault();
      if (key !== dragging) setOrder(expand(moveTo(keys, dragging, key)));
    },
    // リンクを落とした先として開かないよう、既定の動きを止める。
    onDrop: (e) => e.preventDefault(),
    // Esc で止めたときと、並べ替えの外で離したときは、元の順に戻す。
    onDragEnd: (e) => {
      setDragging(undefined);
      if (e.dataTransfer?.dropEffect === 'none') setOrder(s.docOrder);
      else save(expand(keys));
    },
    onKeyDown: (e) => {
      const step = e.altKey ? ARROW_STEP[e.key] : undefined;
      const target = step && keys[keys.indexOf(key) + step];
      if (!target) return;
      e.preventDefault();
      refocus.current = key;
      save(expand(moveTo(keys, key, target)));
    },
  });
  return { items, dragging, dragOf, list };
};

/** heading は、ファイルの最初の見出し。 */
type OpenableFile = ChatFile & { heading?: string };

/** 発言に出た Markdown ファイルのうち、ウィンドウで開けるもの。 */
const useOpenableFiles = (id: string, files: ChatFile[]): OpenableFile[] => {
  const [openable, setOpenable] = useState<Map<string, FileTarget>>(new Map());
  useEffect(() => {
    let alive = true;
    void Promise.all(files.map((f) => targetOf(id, f.ref))).then((targets) => {
      if (!alive) return;
      setOpenable(new Map(files.flatMap((f, i): [string, FileTarget][] => {
        const t = targets[i];
        return t?.where === 'local' ? [[f.ref, t]] : [];
      })));
    });
    return () => {
      alive = false;
    };
  }, [id, files]);
  return files.flatMap((f) => (openable.has(f.ref) ? [{ ...f, heading: openable.get(f.ref)?.heading }] : []));
};

// ponytail: ページの題名はページを開いている間覚えておく。読めなかったページは、読み込み直すまで尋ね直さない。
const notionTitles = new Map<string, Promise<string | undefined>>();

const notionTitleOf = (pageId: string): Promise<string | undefined> => {
  const cached = notionTitles.get(pageId);
  if (cached) return cached;
  const p = api.notionTitle(pageId).then((r) => r.title, () => undefined);
  notionTitles.set(pageId, p);
  return p;
};

/** Notion のページへのリンクの、URL ごとの題名。enabled は Notion のトークンを設定してあるかで、無ければ尋ねない。読めないページは入れない。 */
const useNotionTitles = (urls: string[], enabled: boolean): Map<string, string> => {
  const [titles, setTitles] = useState<Map<string, string>>(new Map());
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    const pages = urls.flatMap((url): [string, string][] => {
      const id = notionPageId(new URL(url));
      return id ? [[url, id]] : [];
    });
    void Promise.all(pages.map(([, id]) => notionTitleOf(id))).then((found) => {
      if (alive) setTitles(new Map(pages.flatMap(([url], i): [string, string][] => (found[i] ? [[url, found[i]]] : []))));
    });
    return () => {
      alive = false;
    };
  }, [urls.join('\n'), enabled]);
  return titles;
};

/** 修飾キー付きのクリックは、ブラウザの既定の動きに任せる。 */
export const plainClick = (e: MouseEvent): boolean => e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;

/** エクスプローラーから足したファイル。押すと、チャットのファイルパスを押したときと同じ開き方で開く。 */
const savedFileDoc = (id: string, f: SavedFile): SideDoc => ({
  key: f.path,
  keys: [f.path],
  title: baseName(f.path),
  href: href.file(id, f.path),
  sub: 'ファイル',
  icon: OWN_ICON,
  at: f.at,
  attach: f.path,
  onClick: (e) => {
    if (!plainClick(e)) return;
    e.preventDefault();
    void openFile(id, f.path);
  },
});

/** 題名は、ファイル名の後に最初の見出しを付ける。同じ名前のファイルが並んでも、中身で見分けられるようにするため。 */
const fileDoc = (id: string, f: OpenableFile): SideDoc => ({
  key: f.ref,
  keys: [f.ref],
  title: [f.ref.split('/').at(-1) ?? f.ref, f.heading].filter(Boolean).join(' — '),
  href: href.file(id, f.ref),
  sub: FORMAT_LABEL.markdown,
  icon: OWN_ICON,
  at: f.at,
  attach: f.ref,
  onClick: (e) => {
    if (!plainClick(e)) return;
    e.preventDefault();
    openFileFloat(id, f.ref);
  },
});

/**
 * セッション概要の資料のウィジェット。present で出したものと、人間と Claude の発言に出たリンクと Markdown ファイルと、人間が「追加」で足したリンクを並べる。
 * 足したリンクと同じ URL が発言にあれば、足したほうを残す。題名とサイトが同じリンクは 1 件にまとめ、最後に出たリンクを開く。外した資料は並べない。
 * 人間が付け直した題名は、まとめたリンクのどれかに付いていればその行に出す。
 * Notion のページへのリンクは、題名を付けずに足したものと、発言に URL だけを書いたものに、ページの題名を付ける。
 * エクスプローラーからファイルをドラッグして落とすと、そのファイルを足す。
 */
export const SessionDocs = ({ s, docs, entries, notion }: { s: Session; docs: DocMeta[]; entries: ChatEntry[]; notion: boolean }) => {
  const [adding, setAdding] = useState(false);
  // エクスプローラーのファイルをドラッグして上に来ている間、落とせることを枠で示す。
  const [dropping, setDropping] = useState(false);
  // 前に開いていたときのデータには、足したファイルが無いことがある。
  const savedFiles = s.files ?? [];
  const savedPaths = new Set(savedFiles.map((f) => f.path));
  const links = useMemo(() => chatLinks(entries, location.port), [entries]);
  const files = useOpenableFiles(s.id, useMemo(() => chatMarkdownFiles(entries), [entries]));
  const saved = new Set(s.links.map((l) => l.url));
  const hidden = new Set(s.hiddenDocs);
  const added = s.links.map((l) => ({ url: l.url, title: l.title || titleOf(new URL(l.url)), host: new URL(l.url).host, at: l.at }));
  const renamed = (d: SideDoc): SideDoc => {
    const title = d.keys.map((k) => s.docTitles[k]).find(Boolean);
    return title ? { ...d, title } : d;
  };
  const shown = [...links.filter((l) => !saved.has(l.url)), ...added].filter((l) => !hidden.has(l.url));
  const notionTitles = useNotionTitles(shown.map((l) => l.url), notion);
  const { items, dragging, dragOf, list } = useDocOrder(s, [
    ...docs
      .filter((d) => !hidden.has(d.id))
      .map((d) => ({ key: d.id, keys: [d.id], title: d.title, href: href.doc(d.id), sub: FORMAT_LABEL[d.format], icon: OWN_ICON, at: d.createdAt, unread: !d.seen, attach: docAttach(d) })),
    ...mergeLinks(withTitles(shown, notionTitles)).map(linkDoc),
    ...files.filter((f) => !hidden.has(f.ref) && !savedPaths.has(f.ref)).map((f) => fileDoc(s.id, f)),
    ...savedFiles.map((f) => savedFileDoc(s.id, f)),
  ].map(renamed));
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">
          資料<span class="count-pill">{items.length}</span>
        </h2>
        <button class="btn btn-sm" data-icon="add" aria-expanded={adding} onClick={() => setAdding((v) => !v)}>追加</button>
      </div>
      <div
        class={`widget-body${dropping ? ' is-drop' : ''}`}
        onDragOver={(e) => {
          if (!e.dataTransfer?.types.includes(FILE_DRAG_TYPE)) return;
          e.preventDefault();
          e.dataTransfer.dropEffect = 'copy';
          setDropping(true);
        }}
        onDragLeave={() => setDropping(false)}
        onDrop={(e) => {
          setDropping(false);
          const file = e.dataTransfer?.getData(FILE_DRAG_TYPE);
          if (!file) return;
          e.preventDefault();
          void attempt('資料の追加', () => api.addFile(s.id, file));
        }}
      >
        {adding && <AddLink s={s} onClose={() => setAdding(false)} />}
        {items.length === 0 ? (
          <Empty>資料はありません。</Empty>
        ) : (
          <ul class="side-docs" ref={list}>
            {items.map((d) => (
              <SideDocRow key={d.key} d={d} dragging={d.key === dragging} drag={dragOf(d.key)} onRemove={() => attempt('資料を外す', () => api.hideDocs(s.id, d.keys))} onRename={(title) => attempt('資料の名前の変更', () => api.renameDoc(s.id, d.key, title))} />
            ))}
          </ul>
        )}
      </div>
    </>
  );
};

const FeedbackForm = ({ doc }: { doc: Doc }) => {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const send = async () => {
    if (busy || !text.trim()) return;
    setBusy(true);
    const message = `資料「${doc.title}」へのフィードバック:\n${text.trim()}`;
    const res = await attempt('フィードバックの送信', () => api.message(doc.sessionId, message));
    setBusy(false);
    if (!res) return;
    setText('');
    toast(DELIVERY_TOAST[res.delivery]);
  };
  return (
    <Panel title="この資料へのフィードバック">
      <textarea
        class="input"
        rows={3}
        placeholder="Claude へ伝えたいこと（Cmd/Ctrl + Enter で送信）"
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, send)}
      />
      <div class="actions">
        <button class="btn btn-primary" disabled={busy || !text.trim()} onClick={send}>送信</button>
      </div>
    </Panel>
  );
};

/** 開いた資料が未読なら既読にする。失敗しても閲覧は続けられるため通知しない。 */
const useMarkSeen = (doc: Doc | undefined, listed: DocMeta | undefined): void => {
  const sent = useRef(false);
  useEffect(() => {
    if (!doc || sent.current || (doc.seen && listed?.seen !== false)) return;
    sent.current = true;
    api.dismiss(`doc:${doc.id}`).catch(() => undefined);
  }, [doc]);
};

const save = (blob: Blob, name: string): void => {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
};

/** HTML と SVG の資料だけを書き出す。ほかの形式は画面のスクリプトで描くので、ファイルにすると同じ見た目にならない。 */
const Downloads = ({ doc }: { doc: Doc }) => {
  const [busy, setBusy] = useState<'pdf' | 'png'>();
  if (doc.format !== 'html' && doc.format !== 'svg') return null;
  const name = doc.title.replace(/[\\/:*?"<>|]/g, '_');
  const download = async (format: 'pdf' | 'png') => {
    setBusy(format);
    const blob = await attempt(`${format.toUpperCase()} の書き出し`, () => api.exportDoc(doc.id, format));
    setBusy(undefined);
    if (blob) save(blob, `${name}.${format}`);
  };
  return (
    <div class="actions doc-downloads">
      <button class="btn btn-sm" disabled={!!busy} onClick={() => download('pdf')}>{busy === 'pdf' ? '書き出しています…' : 'PDF でダウンロード'}</button>
      <button class="btn btn-sm" onClick={() => save(new Blob([doc.content], { type: doc.format === 'svg' ? 'image/svg+xml' : 'text/html' }), `${name}.${doc.format}`)}>
        {doc.format.toUpperCase()} でダウンロード
      </button>
      <button class="btn btn-sm" disabled={!!busy} onClick={() => download('png')}>{busy === 'png' ? '書き出しています…' : 'PNG でダウンロード'}</button>
    </div>
  );
};

const DocHead = ({ doc, session, onReload, loading }: { doc: Doc; session?: Session; onReload: () => void; loading: boolean }) => (
  <header class="card doc-head">
    <div class="head-row">
      <h1 class="session-name">{doc.title}</h1>
      <Chip>{FORMAT_LABEL[doc.format]}</Chip>
      <span class="spacer" />
      <Downloads doc={doc} />
      {doc.path && <button class="btn btn-sm" disabled={loading} onClick={onReload}>再読み込み</button>}
    </div>
    <div class="head-row muted">
      {session && <a href={href.session(session.id)}>{session.name}</a>}
      <span>{dateTime(doc.createdAt)} 作成</span>
      {doc.path && <span class="mono" title="ファイルを指す資料です。開くたびに最新の内容を読みます。">{doc.path}</span>}
    </div>
  </header>
);

export const DocViewer = ({ id, snapshot }: { id: string; snapshot: Snapshot }) => {
  const fetched = useFetch(() => api.doc(id), [id]);
  const skin = useSkin();
  const doc = fetched.data;
  useMarkSeen(doc, snapshot.docs.find((d) => d.id === id));
  if (fetched.error && !doc) {
    return <div class="card panel error-note">資料を取得できませんでした（{fetched.error}）</div>;
  }
  if (!doc) return <Empty>読み込み中…</Empty>;
  const raw = doc.format === 'html' || doc.format === 'svg';
  return (
    <div class="doc-page">
      <DocHead
        doc={doc}
        session={snapshot.sessions.find((s) => s.id === doc.sessionId)}
        onReload={fetched.reload}
        loading={fetched.loading}
      />
      {fetched.error && <p class="card panel error-note">再読み込みできませんでした（{fetched.error}）</p>}
      <iframe
        class={`doc-frame ${raw ? 'is-raw' : ''}`}
        title={doc.title}
        sandbox="allow-scripts"
        allow="clipboard-write"
        srcdoc={buildSrcdoc(doc, skin)}
      />
      <FeedbackForm doc={doc} />
    </div>
  );
};

/** ウィンドウに浮かべた資料の保存キー。資料の id はどのセッションでも重ならないので、scope は使わない。 */
export const DOC_KEY = 'doc';

/** 資料をウィンドウに浮かべる。同じ資料は 1 つのウィンドウにまとめる。icon を渡すと、タイトルバーにそのアイコンを出す。 */
export const openDocFloat = (id: string, icon?: string): void => floatUp(DOC_KEY, '', { id, settings: icon ? { icon } : undefined });

/** ウィンドウには資料の中身だけを出す。題名はウィンドウのタイトルバーに出す。 */
export const DocFloat = ({ f, snapshot }: { f: Float; snapshot: Snapshot }) => {
  const fetched = useFetch(() => api.doc(f.tab.id), [f.tab.id]);
  const skin = useSkin();
  const doc = fetched.data;
  const listed = snapshot.docs.find((d) => d.id === f.tab.id);
  useMarkSeen(doc, listed);
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={listed?.title ?? doc?.title ?? '資料'} icon={f.tab.settings?.icon ?? '📄'} controls={[close]}>
      {fetched.error && <p class="card panel error-note">資料を取得できませんでした（{fetched.error}）</p>}
      {doc && <Downloads doc={doc} />}
      {doc && (
        <iframe class={`file-frame ${doc.format === 'html' || doc.format === 'svg' ? 'is-raw' : ''}`} title={doc.title} sandbox="allow-scripts" allow="clipboard-write" srcdoc={buildSrcdoc(doc, skin)} />
      )}
    </FloatFrame>
  );
};

/** ウィンドウに浮かべたファイルの保存キー。scope はセッションの id で、タブの id にファイルのパスを入れる。 */
export const FILE_KEY = 'file';

// ponytail: 開き先はページを開いている間覚えておく。後から作られたファイルは、読み込み直すまで開けるものとみなさない。
const resolved = new Map<string, Promise<FileTarget | undefined>>();

/** チャットのファイルパスの開き先と、Markdown の最初の見出し。開けないものは undefined にする。 */
const targetOf = (id: string, ref: string): Promise<FileTarget | undefined> => {
  const key = `${id}\n${ref}`;
  const cached = resolved.get(key);
  if (cached) return cached;
  const p = api.resolveFile(id, ref).then((r) => r, () => undefined);
  resolved.set(key, p);
  return p;
};

/** チャットのファイルパスの開き先。開けないものは undefined にする。 */
export const whereOf = (id: string, ref: string): Promise<FileWhere | undefined> => targetOf(id, ref).then((t) => t?.where);

/** ファイルをウィンドウに浮かべる。行の指定は外し、同じファイルは 1 つのウィンドウにまとめる。sessionId が空なら、セッションの外のファイルとして開く。 */
export const openFileFloat = (sessionId: string, ref: string): void => floatUp(FILE_KEY, sessionId, { id: ref.replace(/:[\d-]+$/, '') });

/**
 * ファイルを、チャットのファイルパスを押したときと同じ開き方で開く。リポジトリにあるファイルはコードの画面で開き、ほかはウィンドウに浮かべる。
 * sessionId が空なら、どれもウィンドウに浮かべる。大きすぎるなど、開けないファイルはトーストで知らせる。
 */
export const openFile = async (sessionId: string, ref: string): Promise<void> => {
  const where = await whereOf(sessionId, ref);
  if (where === 'repo') location.hash = href.file(sessionId, ref);
  else if (where === 'local') openFileFloat(sessionId, ref);
  else toast(`「${baseName(ref)}」は開けません`, 'error');
};

const formatOf = (path: string): DocFormat =>
  /\.md$/i.test(path) ? 'markdown' : /\.html?$/i.test(path) ? 'html' : /\.svg$/i.test(path) ? 'svg' : 'text';

/** リポジトリの外のファイルを、present の資料と同じ見た目で読む。ファイルが書き換わったら読み込み直す。 */
const TextFile = ({ f }: { f: Float }) => {
  const file = useFetch(() => api.localFile(f.scope, f.tab.id), [f.scope, f.tab.id]);
  const skin = useSkin();
  useEffect(() => {
    const source = new EventSource(api.localFileEventsUrl(f.scope, f.tab.id));
    source.onmessage = file.reload;
    return () => source.close();
  }, [f.scope, f.tab.id]);
  const format = formatOf(f.tab.id);
  return (
    <>
      {file.error && <p class="card panel error-note">ファイルを読めませんでした（{file.error}）</p>}
      {file.data && (
        <iframe class={`file-frame ${format === 'html' || format === 'svg' ? 'is-raw' : ''}`} title={file.data.path} sandbox="allow-scripts" allow="clipboard-write" srcdoc={buildSrcdoc({ format, content: file.data.content }, skin)} />
      )}
    </>
  );
};

const mmss = (sec: number): string => {
  const t = Number.isFinite(sec) ? Math.floor(sec) : 0;
  return `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}`;
};

const Icon = ({ children }: { children: ComponentChildren }) => <svg viewBox="0 0 16 16" aria-hidden="true">{children}</svg>;
const SPEAKER = <path d="M1 6h3l4-3v10l-4-3H1z" />;
// 前後のボタンで飛ばす秒数。1 本の動画を見るので、曲の送りではなく早戻しと早送りにする。
const SKIP_SEC = 10;

/**
 * 動画と、Windows Media Player 12 の再生中の画面に似せた操作バー。操作バーは XP スキンのときだけ CSS で見せ、ブラウザ標準の操作バーと入れ替える。
 * 全画面は動画だけでなく枠ごと広げ、全画面でも操作バーを使えるようにする。
 */
const VideoPlayer = ({ src, name }: { src: string; name: string }) => {
  const box = useRef<HTMLDivElement>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [state, setState] = useState({ paused: true, time: 0, duration: 0, volume: 1, muted: false });
  const sync = () => {
    const v = video.current;
    if (v) setState({ paused: v.paused, time: v.currentTime, duration: v.duration || 0, volume: v.volume, muted: v.muted });
  };
  const v = () => video.current as HTMLVideoElement;
  return (
    <div ref={box} class="video-player">
      <p class="wmp-now">'{name}' を再生中</p>
      <video
        ref={video}
        class="file-media"
        src={src}
        controls
        onLoadedMetadata={sync}
        onTimeUpdate={sync}
        onPlay={sync}
        onPause={sync}
        onVolumeChange={sync}
      />
      <div class="wmp-bar">
        <input class="wmp-seek" type="range" aria-label="再生位置" min={0} max={state.duration} step="any" value={state.time} style={`--p: ${state.duration ? (state.time / state.duration) * 100 : 0}%`} onInput={(e) => (v().currentTime = e.currentTarget.valueAsNumber)} />
        <span class="wmp-time">{mmss(state.time)}</span>
        <div class="wmp-pill">
          <button class="wmp-btn" aria-label="停止" onClick={() => { v().pause(); v().currentTime = 0; }}><Icon><rect x="3.5" y="3.5" width="9" height="9" /></Icon></button>
          <button class="wmp-btn is-seg is-prev" aria-label={`${SKIP_SEC} 秒戻る`} onClick={() => (v().currentTime -= SKIP_SEC)}><Icon><path d="M2 3h2v10H2zM9 3 4 8l5 5zM14 3 9 8l5 5z" /></Icon></button>
          <button class="wmp-play" aria-label={state.paused ? '再生' : '一時停止'} onClick={() => (state.paused ? v().play() : v().pause())}>
            <Icon>{state.paused ? <path d="M5 2.5 14 8l-9 5.5z" /> : <path d="M4 3h3v10H4zM9 3h3v10H9z" />}</Icon>
          </button>
          <button class="wmp-btn is-seg is-next" aria-label={`${SKIP_SEC} 秒進む`} onClick={() => (v().currentTime += SKIP_SEC)}><Icon><path d="M2 3l5 5-5 5zM7 3l5 5-5 5zM12 3h2v10h-2z" /></Icon></button>
          <div class="wmp-vol">
            <button class="wmp-btn" aria-label={state.muted ? 'ミュート解除' : 'ミュート'} onClick={() => (v().muted = !state.muted)}>
              <Icon>{SPEAKER}{state.muted ? <path class="is-stroke" d="m10 5.5 5 5m0-5-5 5" /> : <path class="is-stroke" d="M10 5.5a3 3 0 0 1 0 5M12 3.5a6 6 0 0 1 0 9" />}</Icon>
            </button>
            <input class="wmp-volume" type="range" aria-label="音量" min={0} max={1} step="any" value={state.muted ? 0 : state.volume} onInput={(e) => { v().volume = e.currentTarget.valueAsNumber; v().muted = false; }} />
          </div>
        </div>
        <button class="wmp-btn wmp-full" aria-label="全画面" onClick={() => (document.fullscreenElement ? document.exitFullscreen() : box.current?.requestFullscreen())}>
          <Icon><path class="is-stroke" d="M9 2h5v5M14 2 8 8M7 3H2v11h11V9" /></Icon>
        </button>
      </div>
    </div>
  );
};

/** コードの画面で開けない、リポジトリの外のファイルと画像や動画を、ウィンドウに浮かべて見せる。 */
export const FileFloat = ({ f }: { f: Float }) => {
  const type = mediaTypeOf(f.tab.id);
  const src = api.mediaUrl(f.scope, f.tab.id);
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  const title = f.tab.id.split('/').at(-1) ?? f.tab.id;
  return (
    <FloatFrame f={f} title={title} icon={type?.startsWith('video/') ? '🎬' : type ? '🖼️' : '📄'} controls={[close]}>
      {type?.startsWith('video/') ? <VideoPlayer src={src} name={title} /> : type ? <img class="file-media" src={src} alt={f.tab.id} /> : <TextFile f={f} />}
    </FloatFrame>
  );
};
