// 過去のセッションを、すべての会話ログの題名、人間の依頼、Claude の発言から探す。
import path from 'node:path';
import { setImmediate } from 'node:timers/promises';
import type { SearchHit, SearchResponse, SearchResult } from '../shared/types.ts';
import { PROJECTS_DIR, listDir, readCustomTitle, summarizeLog } from './claude.ts';
import { repoName } from './snapshot.ts';

const MAX_RESULTS = 50;
const MAX_HITS = 3;
const SNIPPET_BEFORE = 40;
const SNIPPET_AFTER = 120;
const UNTITLED_CHARS = 80;

const transcripts = (): string[] =>
  listDir(PROJECTS_DIR).flatMap((dir) =>
    listDir(path.join(PROJECTS_DIR, dir))
      .filter((f) => f.endsWith('.jsonl'))
      .map((f) => path.join(PROJECTS_DIR, dir, f)),
  );

const matches = (text: string, terms: string[]): boolean => {
  const lower = text.toLowerCase();
  return terms.every((t) => lower.includes(t));
};

/** 最初の語が当たった位置の前後を、1 行に詰めて切り出す。 */
const snippet = (text: string, terms: string[]): string => {
  const at = Math.max(0, text.toLowerCase().indexOf(terms[0] ?? ''));
  const start = Math.max(0, at - SNIPPET_BEFORE);
  const end = Math.min(text.length, at + SNIPPET_AFTER);
  return `${start > 0 ? '…' : ''}${text.slice(start, end).replace(/\s+/g, ' ').trim()}${end < text.length ? '…' : ''}`;
};

const searchLog = (file: string, terms: string[]): SearchResult | undefined => {
  const log = summarizeLog(file);
  const title = readCustomTitle(file) ?? log.title;
  // 同じ依頼を送り直すと同じ文が並ぶので、1 つにまとめる。
  const seen = new Set<string>();
  const found: SearchHit[] = log.texts.flatMap((t) =>
    t.kind !== 'tool' && !seen.has(t.text) && seen.add(t.text) && matches(t.text, terms) ? [{ kind: t.kind, text: t.text, at: t.at }] : []);
  if (title && matches(title, terms)) found.unshift({ kind: 'title', text: title });
  if (!found.length) return undefined;
  const firstPrompt = log.texts.find((t) => t.kind === 'prompt')?.text.replace(/\s+/g, ' ').trim();
  return {
    id: path.basename(file, '.jsonl'),
    cwd: log.cwd ?? '',
    repo: log.cwd ? repoName(log.cwd) : path.basename(path.dirname(file)),
    title: title ?? firstPrompt?.slice(0, UNTITLED_CHARS) ?? '（無題）',
    updatedAt: log.lastActivityAt ?? 0,
    hits: found.slice(0, MAX_HITS).map((h) => ({ ...h, text: snippet(h.text, terms) })),
    hitCount: found.length,
  };
};

/** 空白で区切った語をすべて含む題名、依頼、発言を探し、当たったセッションを新しい順に返す。大文字と小文字は区別しない。 */
export const searchSessions = async (query: string): Promise<SearchResponse> => {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  const results: SearchResult[] = [];
  if (!terms.length) return { results, total: 0 };
  for (const file of transcripts()) {
    // 初回はすべての会話ログを読むので数秒かかる。その間もフックに応えられるよう、1 ファイルごとに処理を譲る。
    await setImmediate();
    const result = searchLog(file, terms);
    if (result) results.push(result);
  }
  results.sort((a, b) => b.updatedAt - a.updatedAt);
  return { results: results.slice(0, MAX_RESULTS), total: results.length };
};
