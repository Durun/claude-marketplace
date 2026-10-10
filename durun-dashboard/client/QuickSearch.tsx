// Cmd/Ctrl + K かメニューの「検索」で開く検索窓。セッション、要対応、資料、画面、会話ログから探し、選んだものの画面へ移る。
import type { Dispatch, StateUpdater } from 'preact/hooks';
import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { SearchResponse, Snapshot } from '../shared/types.ts';
import { api } from './api.ts';
import { useFetch } from './hooks.ts';
import { type SearchItem, localGroups, logGroup } from './searchItems.ts';
import { SEARCH_SHORTCUT, isShortcut } from './shortcut.ts';
import { StatusBadge } from './ui.tsx';

// 打っている間に会話ログを何度も読ませないよう、手が止まってから探す。
const LOG_SEARCH_DELAY_MS = 250;
const NO_RESULTS: SearchResponse = { results: [], total: 0 };

const useDebounced = (value: string, ms: number): string => {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(timer);
  }, [value]);
  return debounced;
};

/** 入力欄にフォーカスを置いたまま、上下で候補を選び、Enter で開く。 */
const Results = ({ snapshot, onClose }: { snapshot: Snapshot | null; onClose: () => void }) => {
  const [q, setQ] = useState('');
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const query = q.trim();
  const logQuery = useDebounced(query, LOG_SEARCH_DELAY_MS);
  const logs = useFetch(() => (logQuery ? api.search(logQuery) : Promise.resolve(NO_RESULTS)), [logQuery]);
  const note = logs.loading || logQuery !== query ? '探しています…' : logs.error && `探せませんでした（${logs.error}）`;
  const groups = [...localGroups(snapshot, query), ...(query ? [{ ...logGroup(snapshot, query, logs.data), note }] : [])];
  const items = groups.flatMap((g) => g.items);
  // 会話ログの結果が届くと候補の数が変わるので、選んでいる位置を候補の中に収める。
  const current = Math.min(active, items.length - 1);
  useLayoutEffect(() => list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }), [current]);
  const go = (item: SearchItem | undefined) => {
    if (!item) return;
    location.hash = item.href;
    onClose();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.isComposing || !items.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') setActive((current + (e.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length);
    else if (e.key === 'Enter') go(items[current]);
    else return;
    e.preventDefault();
  };
  return (
    <>
      <input
        class="input quick-input"
        // type="search" だと、文字があるときの Esc は文字を消すだけになり、検索窓を閉じない。
        type="text"
        role="combobox"
        aria-label="検索"
        aria-expanded="true"
        aria-controls="quick-list"
        aria-autocomplete="list"
        aria-activedescendant={items[current] && `quick-${current}`}
        placeholder="セッション、要対応、資料、会話ログを探す"
        autoFocus
        value={q}
        onInput={(e) => {
          setQ(e.currentTarget.value);
          setActive(0);
        }}
        onKeyDown={onKeyDown}
      />
      <div id="quick-list" class="quick-list" role="listbox" aria-label="検索の候補" ref={list}>
        {groups.map((g) => (
          <div key={g.title} role="group" aria-label={g.title}>
            <div class="quick-group" aria-hidden="true">
              {g.title}
              {g.note && <span>{g.note}</span>}
            </div>
            {g.items.map((it) => {
              const i = items.indexOf(it);
              return (
                // フォーカスは入力欄に残し、Tab で候補へ移らないようにする。
                <a key={it.key} id={`quick-${i}`} class="quick-item" role="option" aria-selected={i === current} tabIndex={-1} href={it.href} onMouseMove={() => setActive(i)} onClick={onClose}>
                  <span class="quick-label">{it.label}</span>
                  {it.status && <StatusBadge status={it.status} />}
                  {it.sub && <span class="quick-sub">{it.sub}</span>}
                </a>
              );
            })}
          </div>
        ))}
        {items.length === 0 && <p class="empty quick-empty">当たるものはありません。</p>}
      </div>
      <p class="quick-foot">空白で区切ると、すべての語を含むものに絞ります。↑↓ で選び、Enter で開きます。Esc で閉じます。</p>
    </>
  );
};

type Props = { snapshot: Snapshot | null; open: boolean; setOpen: Dispatch<StateUpdater<boolean>> };

/** 開くたびに空の入力欄から始める。開いている間は検索窓の外を操作させず、Esc か外側のクリックで閉じる。 */
export const QuickSearch = ({ snapshot, open, setOpen }: Props) => {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isShortcut(e, SEARCH_SHORTCUT)) return;
      e.preventDefault();
      setOpen((v) => !v);
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, []);
  // 描いた直後に開き、Cmd + K のすぐ後に打った文字も入力欄で受ける。
  useLayoutEffect(() => {
    const el = dialog.current;
    if (open && !el?.open) el?.showModal();
    if (!open && el?.open) el.close();
  }, [open]);
  return (
    <dialog ref={dialog} class="quick-search" aria-label="検索" closedby="any" onClose={() => setOpen(false)}>
      {open && <Results snapshot={snapshot} onClose={() => setOpen(false)} />}
    </dialog>
  );
};
