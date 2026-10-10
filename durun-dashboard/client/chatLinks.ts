// 人間と Claude の発言に出たリンクを、セッションの資料として拾う。
import { Marked, type MarkedExtension, Tokenizer } from 'marked';
import type { ChatEntry } from '../shared/types.ts';
import { safeDecode } from './router.ts';

// 日本語の文では、URL の後ろに空白を置かずに文を続けることが多い。GFM は空白の手前までを URL とみなすので、閉じていない ) と、日本語の句読点や括弧から先を外す。
const URL_HEAD = /^(?:[^()、。，．「」『』（）【】]|\([^()]*\))+/;

/** 発言の表示と、発言から拾うリンクとで、同じところまでを URL とみなすための marked の拡張。 */
export const bareUrls: MarkedExtension = {
  tokenizer: {
    url(src) {
      const t = Tokenizer.prototype.url.call(this, src);
      const head = t && URL_HEAD.exec(t.raw)?.[0];
      if (!t || !head || head === t.raw) return t;
      return { ...t, raw: head, text: head, href: t.href.slice(0, head.length - t.raw.length), tokens: [{ type: 'text', raw: head, text: head }] };
    },
  },
};

// 発言の表示と同じく GFM で読むので、コードの中の URL は拾わない。
const md = new Marked(bareUrls);
const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1', '[::1]']);
const DOC_EXTENSION = /\.(html?|pdf|md)$/i;
const GITHUB_ITEM = /^\/([^/]+\/[^/]+)\/(pull|issues|actions\/runs)\/(\d+)/;
const GITHUB_COMMENT = /^#(discussion_r|issuecomment-|pullrequestreview-)/;

export type ChatLink = { url: string; title: string; host: string; at: number };

// インラインのコードに書いた Markdown ファイルのパス。行の指定は外して拾う。
const MD_FILE = /^([^\s:]+\.md)(?::\d+(?:-\d+)?)?$/i;

/** 発言に出た Markdown ファイル。ref はパスで、at は最後に出た時刻。 */
export type ChatFile = { ref: string; at: number };

/** 発言のインラインのコードに出た Markdown ファイルを、同じパスは 1 件にまとめ、新しく出た順に並べる。 */
export const chatMarkdownFiles = (entries: ChatEntry[]): ChatFile[] => {
  const files = new Map<string, number>();
  for (const e of entries) {
    if (e.kind === 'tool') continue;
    md.walkTokens(md.lexer(e.text), (t) => {
      const ref = t.type === 'codespan' && MD_FILE.exec(t.text)?.[1];
      if (ref) files.set(ref, e.at ?? 0);
    });
  }
  return [...files].map(([ref, at]) => ({ ref, at })).sort((a, b) => b.at - a.at);
};

// ダッシュボード自身の画面は、present の資料として別に並べるか、資料ではない画面なので拾わない。
const listed = (u: URL, ownPort: string): boolean =>
  (u.protocol === 'http:' || u.protocol === 'https:') && !(LOCAL_HOSTS.has(u.hostname) && u.port === ownPort);

/**
 * GitHub の PR と issue は、GitHub の書き方（owner/repo#番号）を題名にし、そのコメントへのリンクと Actions の実行は、それと分かる題名にする。
 * ほかの URL は、パスの最後の区切りを題名にする。index のページは、その上の区切りにする。
 */
export const titleOf = (u: URL): string => {
  const [, repo, kind, num] = (u.hostname === 'github.com' && GITHUB_ITEM.exec(u.pathname)) || [];
  if (kind === 'actions/runs') return `${repo} の Actions ${num}`;
  if (kind) return `${repo}#${num}${GITHUB_COMMENT.test(u.hash) ? ' のコメント' : ''}`;
  const parts = u.pathname.split('/').filter(Boolean).map((p) => safeDecode(p).replace(DOC_EXTENSION, '').replaceAll('+', ' '));
  const last = parts.at(-1);
  return (last === 'index' ? parts.at(-2) : last) || u.host;
};

/**
 * 同じ URL は 1 件にまとめ、[題名](URL) の書き方があれば、その題名を使う。新しく出た順に並べる。
 * ownPort はダッシュボードのポートで、手元のそのポートを指すリンクは除く。
 */
export const chatLinks = (entries: ChatEntry[], ownPort: string): ChatLink[] => {
  const links = new Map<string, ChatLink>();
  for (const e of entries) {
    if (e.kind === 'tool') continue;
    md.walkTokens(md.lexer(e.text), (t) => {
      if (t.type !== 'link' || !URL.canParse(t.href)) return;
      const u = new URL(t.href);
      if (!listed(u, ownPort)) return;
      const titled = t.raw.startsWith('[');
      const prev = links.get(u.href);
      links.set(u.href, { url: u.href, host: u.host, at: e.at ?? 0, title: titled ? t.text : (prev?.title ?? titleOf(u)) });
    });
  }
  return [...links.values()].sort((a, b) => b.at - a.at);
};

/** まとめたリンク。url は最後に出たもので、urls にはまとめたものをすべて持つ。 */
export type MergedLink = ChatLink & { urls: string[] };

/** 題名とサイトが同じリンクは、同じ資料が並んでいるように見えるので 1 件にまとめる。新しく出た順に並べる。 */
export const mergeLinks = (links: ChatLink[]): MergedLink[] => {
  const merged = new Map<string, MergedLink>();
  for (const l of links.toSorted((a, b) => b.at - a.at)) {
    const key = `${l.host} ${l.title}`;
    const prev = merged.get(key);
    merged.set(key, prev ? { ...prev, urls: [...prev.urls, l.url] } : { ...l, urls: [l.url] });
  }
  return [...merged.values()];
};
