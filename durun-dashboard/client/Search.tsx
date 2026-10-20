import { useEffect, useRef } from 'preact/hooks';
import type { SearchHit, SearchResult, Session, Snapshot } from '../shared/types.ts';
import { api } from './api.ts';
import { dateTime, escapeRegExp } from './format.ts';
import { useFetch } from './hooks.ts';
import { href } from './router.ts';
import { CopyButton, Empty, Panel } from './ui.tsx';

const HIT_LABEL: Record<SearchHit['kind'], string> = { title: '題名', prompt: '依頼', message: '発言' };

/** 検索語を mark で囲む。サーバーと同じく、大文字と小文字は区別しない。 */
const Highlight = ({ text, terms }: { text: string; terms: string[] }) => {
  // 区切りをキャプチャすると、split の結果は当たらない部分と当たった部分が交互に並ぶ。
  const parts = text.split(new RegExp(`(${terms.map(escapeRegExp).join('|')})`, 'i'));
  return <>{parts.map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part))}</>;
};

const shellQuote = (s: string): string => `'${s.replaceAll("'", `'\\''`)}'`;

/** ホームの一覧にあるセッションは概要へ進み、ないセッションはターミナルで再開するコマンドを出す。 */
const ResultItem = ({ r, session, terms }: { r: SearchResult; session?: Session; terms: string[] }) => {
  const resume = `cd ${shellQuote(r.cwd)} && claude --resume ${r.id}`;
  return (
    <li class="search-item">
      <div class="search-head">
        {session ? <a class="search-title" href={href.session(r.id)}>{session.name}</a> : <span class="search-title">{r.title}</span>}
        <span class="mono muted nowrap">{r.repo}</span>
        <span class="muted nowrap">{r.hitCount} 件</span>
        <time class="muted nowrap" dateTime={new Date(r.updatedAt).toISOString()}>{dateTime(r.updatedAt)}</time>
      </div>
      <ul class="search-hits">
        {r.hits.map((h, i) => (
          <li key={i}>
            <span class="search-kind">{HIT_LABEL[h.kind]}</span>
            <Highlight text={h.text} terms={terms} />
          </li>
        ))}
      </ul>
      {!session && r.cwd && (
        <span class="attach">
          <code>{resume}</code>
          <CopyButton text={resume} label="コピー" title="ターミナルで再開するコマンドをコピー" />
        </span>
      )}
    </li>
  );
};

export const SearchPage = ({ q, snapshot }: { q: string; snapshot: Snapshot }) => {
  const input = useRef<HTMLInputElement>(null);
  const { data, error, loading } = useFetch(() => api.search(q), [q]);
  useEffect(() => input.current?.focus(), []);
  const terms = q.toLowerCase().split(/\s+/).filter(Boolean);
  const sessions = new Map(snapshot.sessions.map((s) => [s.id, s]));
  // 語を変えるたびに作り直す画面なので、入力欄は状態に持たず、送るときに値を読む。
  const submit = (e: Event) => {
    e.preventDefault();
    location.hash = href.search(input.current?.value.trim() ?? '');
  };
  return (
    <div class="stack">
      <form class="search-form" role="search" onSubmit={submit}>
        <input
          ref={input}
          class="input"
          type="search"
          aria-label="過去のセッションを探す"
          placeholder="題名、依頼、Claude の発言から探す（空白で区切ると、すべての語を含むものに絞る）"
          defaultValue={q}
        />
        <button class="btn btn-primary" type="submit">検索</button>
      </form>
      {terms.length > 0 && (
        <Panel title="検索結果" count={data?.total}>
          {loading && <p class="help">検索しています。初回はすべての会話ログを読むので、数秒かかります。</p>}
          {error && <p class="help error-text">検索できませんでした: {error}</p>}
          {data?.total === 0 && <Empty>当たるセッションはありません。</Empty>}
          {data && data.total > data.results.length && <p class="help">新しい順に {data.results.length} 件を表示しています。</p>}
          <ul class="search-list">
            {data?.results.map((r) => <ResultItem key={r.id} r={r} session={sessions.get(r.id)} terms={terms} />)}
          </ul>
        </Panel>
      )}
    </div>
  );
};
