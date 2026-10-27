import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { BgTask, Session, SessionSummary, Snapshot, TaskItem } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { fmtInt, fmtTokens } from './format.ts';
import { useCollapsed } from './hooks.ts';
import { ARROW_STEP, arrangeSessions, gatherAfter, moveTo } from './order.ts';
import { href } from './router.ts';
import { Chip, ContextBar, Empty, IssueChip, JiraChips, PinIcon, PrChips, StatusBadge, StatusCounts, Time } from './ui.tsx';

const UNCLASSIFIED = '未分類';
// テーマの名前は空にならない（空のテーマは未分類にまとめる）ので、空の名前をピン留めのまとまりに使う。
const PINNED = '';
const PINNED_TITLE = 'ピン留め';

type Group = { theme: string; sessions: Session[] };

/** セッションが入るまとまり。ピン留めしたセッションは、テーマから外して一覧の先頭にまとめる。 */
const groupOf = (s: Session): string => (s.pinned ? PINNED : s.theme.trim() || UNCLASSIFIED);

/** テーマの並びは themeOrder に従う。ピン留めは最初、未分類は最後に置く。 */
const groupSessions = (sessions: Session[], themeOrder: string[], order: string[], placeOf: (s: Session) => string): Group[] => {
  const groups = new Map<string, Session[]>();
  for (const s of sessions) groups.set(placeOf(s), [...(groups.get(placeOf(s)) ?? []), s]);
  const rank = (theme: string): number => {
    if (theme === PINNED) return -1;
    if (theme === UNCLASSIFIED) return Number.MAX_SAFE_INTEGER;
    const index = themeOrder.indexOf(theme);
    return index >= 0 ? index : themeOrder.length;
  };
  return [...groups.entries()]
    .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b, 'ja'))
    .map(([theme, list]) => ({ theme, sessions: arrangeSessions(list, order) }));
};

const TaskLine = ({ tasks }: { tasks: TaskItem[] }) => {
  if (tasks.length === 0) return null;
  const done = tasks.filter((t) => t.status === 'completed').length;
  const active = tasks.find((t) => t.status === 'in_progress');
  return (
    <div class="task-line">
      <span class="muted">タスク</span> <b>{done}/{tasks.length}</b>
      {active && <span class="active-form">{active.activeForm ?? active.subject}</span>}
    </div>
  );
};

const runningTools = (s: Session, kind: BgTask['kind']): number => s.background.filter((t) => t.kind === kind && t.status === 'running').length;

const Activity = ({ s }: { s: Session }) => {
  const running = s.subagents.filter((a) => a.running).length;
  const shells = runningTools(s, 'shell');
  const monitors = runningTools(s, 'monitor');
  return (
    <div class="activity">
      <TaskLine tasks={s.tasks} />
      {running > 0 && <Chip tone="info">サブエージェント {running} 実行中</Chip>}
      {shells > 0 && <Chip tone="info">shell {shells} 実行中</Chip>}
      {monitors > 0 && <Chip tone="info">Monitor {monitors} 実行中</Chip>}
      {s.pendingMessages > 0 && <Chip tone="warn" title="Claude にまだ届いていないメッセージ">未配信 {s.pendingMessages}</Chip>}
      {s.awaitingReview && <Chip tone="info" title="Claude がレビューを待っています">レビュー待ち</Chip>}
      {s.archiveWhenIdle && <Chip title="作業が終わったらアーカイブします">アーカイブ待ち</Chip>}
    </div>
  );
};

/** リポジトリとバッジの右の空いたところに出す概要。1 行しか入らなければ、「いま」を出す。 */
const CardSummary = ({ summary }: { summary: SessionSummary }) => (
  <div class="card-summary">
    <p><span class="card-summary-label">いま</span>{summary.now}</p>
    <p><span class="card-summary-label">やっていること</span>{summary.doing}</p>
  </div>
);

// 合計はキャッシュ読み取りが大半を占め、作業量より大きく見える。カードには出力を出し、内訳はツールチップに回す。
const totalsTitle = (s: Session): string => {
  const t = s.usage.totals;
  return `入力 ${fmtInt(t.input)} / 出力 ${fmtInt(t.output)} / キャッシュ読取 ${fmtInt(t.cacheRead)} / キャッシュ作成 ${fmtInt(t.cacheCreation)}`;
};

export type Selection = { selected: Set<string>; toggle: (ids: string[], on: boolean) => void };

/** カードでは状態の左、テーマではテーマ名の左に置く。テーマでは、その配下のセッションをまとめて選ぶ。 */
const SelectBox = ({ ids, sel, label }: { ids: string[]; sel: Selection; label: string }) => {
  const count = ids.filter((id) => sel.selected.has(id)).length;
  return (
    <label class="select-box">
      <input
        type="checkbox"
        aria-label={label}
        checked={count > 0 && count === ids.length}
        indeterminate={count > 0 && count < ids.length}
        onChange={(e) => sel.toggle(ids, e.currentTarget.checked)}
      />
    </label>
  );
};

/** 押すたびに、表示している状態の逆を送る。届く前に続けて押しても、同じ状態を送るだけで済む。 */
const PinButton = ({ s }: { s: Session }) => (
  <button
    class="pin"
    aria-pressed={s.pinned}
    aria-label={`${s.name} をピン留め`}
    title={s.pinned ? 'ピン留めを外す' : 'ピン留めして一覧の先頭に出す'}
    onClick={() => attempt('ピン留めの切り替え', () => api.pin(s.id, !s.pinned))}
  >
    <PinIcon />
  </button>
);

/** 選んでいる間は、カードを押しても画面を移らず、選ぶかどうかを切り替える。続けて選んでいる途中で、セッションの画面へ飛ばないようにするため。 */
const toggleWhileSelecting = (e: MouseEvent, s: Session, sel: Selection): void => {
  if (!sel.selected.size) return;
  e.preventDefault();
  sel.toggle([s.id], !sel.selected.has(s.id));
};

const SessionCard = ({ s, sel, dragging, drag }: { s: Session; sel: Selection; dragging: boolean; drag: DragHandlers }) => (
  <article class={`card session-card is-${s.status}${dragging ? ' is-dragging' : ''}`} draggable {...drag}>
    <header class="card-head">
      <SelectBox ids={[s.id]} sel={sel} label={`${s.name} を選ぶ`} />
      <StatusBadge status={s.status} waitingFor={s.waitingFor} />
      <h3 class="card-title"><a class="stretched" href={href.session(s.id)} onClick={(e) => toggleWhileSelecting(e, s, sel)}>{s.name}</a></h3>
      <span class="muted nowrap"><Time at={s.updatedAt} /></span>
      <PinButton s={s} />
    </header>
    <div class="card-mid">
      <div class="card-mid-main">
        <p class="card-sub mono">{s.repo}{s.branch && <> · {s.branch}</>}</p>
        {/* 前に開いていたときのデータには、PR の一覧が無いことがある。 */}
        {(s.prs?.length || s.jira.length > 0 || !!s.issues?.length) && (
          <div class="chips">
            {/* 一覧のまま PR や課題を見られるよう、session を渡さず、セッションの画面へ移らずにウィンドウに浮かべて開く。 */}
            {s.prs?.map((pr) => <PrChips key={pr.url} pr={pr} />)}
            <JiraChips issues={s.jira} />
            {s.issues?.map((i) => <IssueChip key={i.url} issue={i} />)}
          </div>
        )}
      </div>
      {s.summary && <CardSummary summary={s.summary} />}
    </div>
    <div class="ctx-row">
      <ContextBar usage={s.usage} />
      <span class="muted nowrap" title={totalsTitle(s)}>出力 {fmtTokens(s.usage.totals.output)}</span>
    </div>
    <Activity s={s} />
  </article>
);

/** カードに付ける、並べ替えの操作。 */
type DragHandlers = {
  onDragStart: (e: DragEvent) => void;
  onDragOver: (e: DragEvent) => void;
  onDrop: (e: DragEvent) => void;
  onDragEnd: (e: DragEvent) => void;
  onKeyDown: (e: KeyboardEvent) => void;
};

/**
 * テーマ名は、件数の右の鉛筆のボタンで入力欄に変わる。Enter かフォーカスを外したときに確定し、Esc で取り消す。
 * 未分類は、テーマが空のセッションに画面が付けた名前なので、変えられない。ピン留めもテーマではないので、変えられない。
 */
const ThemeTitle = ({ theme, count }: { theme: string; count: number }) => {
  const [editing, setEditing] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  // 押した直後のキー入力も入力欄に届くよう、描画を待たずにフォーカスを移す。
  useLayoutEffect(() => input.current?.select(), [editing]);
  const pill = <span class="count-pill">{count}</span>;
  if (!editing) {
    return (
      <>
        {theme || PINNED_TITLE}{pill}
        {theme !== UNCLASSIFIED && theme !== PINNED && (
          <button class="theme-rename" title="名前を変更" aria-label={`${theme} の名前を変更`} onClick={() => setEditing(true)}>✎</button>
        )}
      </>
    );
  }
  // 名前を変えたら、新しい名前で描き直されるまで、入力欄に新しい名前を出しておく。
  const commit = async (el: HTMLInputElement) => {
    const to = el.value.trim();
    if (!to || to === theme || !(await attempt('テーマの名前の変更', () => api.renameTheme(theme, to)))) setEditing(false);
  };
  const onKeyDown = (e: KeyboardEvent) => {
    const el = e.currentTarget as HTMLInputElement;
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      // フォーカスを外すと確定するので、先に入力欄の値を戻しておく。
      el.value = theme;
      el.blur();
    } else if (e.key === 'Enter') {
      el.blur();
    }
  };
  return (
    <>
      <input
        ref={input}
        class="name-input"
        defaultValue={theme}
        aria-label="テーマの名前"
        onKeyDown={onKeyDown}
        // Chrome は、summary の中の入力欄で Space を離したときにも details を開閉するので止める。
        onKeyUp={(e) => e.preventDefault()}
        onBlur={(e) => commit(e.currentTarget)}
      />
      {pill}
    </>
  );
};

// 折りたたんだテーマは、ホームを開き直しても閉じたままにするため、ブラウザに保存する。
const COLLAPSED_KEY = 'dashboard-collapsed-themes';

/** ピン留めは付けるだけにして、テーマを残す。ピン留めから出すときは、外してから落とした先のテーマにする。 */
const moveToGroup = (s: Session, to: string): Promise<unknown> =>
  attempt('セッションの移動', async () => {
    if (to === PINNED) await api.pin(s.id, true);
    else {
      if (s.pinned) await api.pin(s.id, false);
      if (s.theme.trim() !== to) await api.theme(s.id, to);
    }
    return true;
  });

/**
 * セッションの並べ替えと、まとまりの間の移動。並べた順と移した先は、サーバーから届くまでの間も手元の状態で出す。ドラッグの間は、離すまで保存しない。
 * 未分類はテーマが空のセッションに画面が付けた名前で、空のテーマを送るとリポジトリ名に戻るので、ほかのまとまりからは移せない。
 * 選んだカードをほかのまとまりへ移すと、選んだほかのセッションも一緒に移す。ドラッグの間に動かして見せるのは、ドラッグしたカードだけにする。
 */
const useSessionOrder = (snapshot: Snapshot, visible: Session[], selected: Set<string>, themes: string[]) => {
  const [order, setOrder] = useState(snapshot.sessionOrder);
  useEffect(() => setOrder(snapshot.sessionOrder), [snapshot.sessionOrder.join('\n')]);
  const [dragging, setDragging] = useState<Session>();
  const [moved, setMoved] = useState<{ ids: string[]; group: string }>();
  // 移した先がサーバーから届いたら、手元の移動を捨てる。
  useEffect(() => {
    const arrived = moved?.ids.every((id) => {
      const s = visible.find((v) => v.id === id);
      return !s || groupOf(s) === moved.group;
    });
    if (!dragging && arrived) setMoved(undefined);
  }, [snapshot]);
  const placeOf = (s: Session): string => (moved?.ids.includes(s.id) ? moved.group : groupOf(s));
  const carried = !dragging ? [] : selected.has(dragging.id) ? visible.filter((v) => selected.has(v.id)) : [dragging];
  const groups = groupSessions(visible, themes, order, placeOf);
  const ids = groups.flatMap((g) => g.sessions.map((s) => s.id));
  // キーボードで動かしたカードは、並べ直すときに DOM から外れてフォーカスを失うので、動かした先でフォーカスし直す。
  const refocus = useRef<string>();
  useLayoutEffect(() => {
    if (refocus.current === undefined) return;
    document.querySelector<HTMLElement>(`.session-card a[href="${href.session(refocus.current)}"]`)?.focus();
    refocus.current = undefined;
  }, [order]);
  // 一覧に出していないアーカイブのセッションは、並べた順の後ろに残す。
  const save = (next: string[]) => {
    const all = [...next, ...order.filter((id) => !next.includes(id))];
    setOrder(all);
    void attempt('セッションの並べ替え', () => api.orderSessions(all));
  };
  const canDrop = (group: string): boolean => !!dragging && (group !== UNCLASSIFIED || groupOf(dragging) === UNCLASSIFIED);
  /** dragging を group へ移し、そのまとまりの before の前に入れる。before が無ければ最後に入れる。 */
  const insert = (group: string, before?: string) => {
    if (!dragging) return;
    const rest = ids.filter((id) => id !== dragging.id);
    const last = groups.find((g) => g.theme === group)?.sessions.at(-1)?.id;
    const i = before ? rest.indexOf(before) : rest.indexOf(last ?? '') + 1;
    setOrder([...rest.slice(0, i), dragging.id, ...rest.slice(i)]);
    setMoved({ ids: [dragging.id], group });
  };
  const cancel = () => {
    setDragging(undefined);
    setOrder(snapshot.sessionOrder);
    setMoved(undefined);
  };
  // ほかのまとまりへ移して見せたカードは DOM から外れるので、Chrome は dragend を送らない。
  // ドラッグの間はマウスの移動が届かないので、離さずに終わったドラッグは、次に届いた移動で元に戻す。
  // 離した直後の移動で戻さないよう、離して描き直したときに、描画を待たずに見張りを外す。
  useLayoutEffect(() => {
    if (!dragging) return;
    window.addEventListener('mousemove', cancel, { once: true });
    return () => window.removeEventListener('mousemove', cancel);
  }, [dragging]);
  // 閉じたまとまりや、カードの無いところに来たら、そのまとまりの最後へ移して見せる。
  // 離したときは、カードの上でもまとまりで確定する。ドラッグを始めたカードは移した先で描き直されて dragend が届かないため。
  // Chrome は、ポインタの下の要素が変わった回には dragover を送らず、dragenter で落とせるかを決める。移して見せるとポインタの下が変わるので、dragenter でも受け付ける。
  const groupDrag = (group: string) => ({
    onDragEnter: (e: DragEvent) => {
      if (dragging && canDrop(group)) e.preventDefault();
    },
    onDragOver: (e: DragEvent) => {
      if (!dragging || !canDrop(group)) return;
      e.preventDefault();
      if (placeOf(dragging) !== group) insert(group);
    },
    onDrop: (e: DragEvent) => {
      if (!dragging) return;
      e.preventDefault();
      setDragging(undefined);
      const to = placeOf(dragging);
      if (to === groupOf(dragging)) return save(ids);
      // 一緒に移すセッションは、ドラッグしたカードの後ろに並べる。
      const moving = carried.filter((s) => s.id === dragging.id || groupOf(s) !== to);
      const others = moving.filter((s) => s.id !== dragging.id).map((s) => s.id);
      save(gatherAfter(ids, dragging.id, others));
      setMoved({ ids: moving.map((s) => s.id), group: to });
      void Promise.all(moving.map((s) => moveToGroup(s, to))).then((oks) => oks.every(Boolean) || setMoved(undefined));
    },
  });
  const dragOf = (s: Session, group: string): DragHandlers => ({
    onDragStart: (e) => {
      setDragging(s);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
    },
    // ほかのカードの上に来たら、その位置へ動かして見せる。ほかのまとまりから来たら、そのカードの前に入れる。
    onDragOver: (e) => {
      if (!dragging || !canDrop(group) || s.id === dragging.id) return;
      e.preventDefault();
      // まとまりの最後へ移す動きと重ならないよう、まとまりに届けない。
      e.stopPropagation();
      if (placeOf(dragging) === group) setOrder(moveTo(ids, dragging.id, s.id));
      else insert(group, s.id);
    },
    // リンクを落とした先として開かないよう、既定の動きを止める。確定は、まとまりに届けて行う。
    onDrop: (e) => e.preventDefault(),
    // Esc で止めたときと、並べ替えの外で離したときは、元に戻す。
    onDragEnd: (e) => {
      if (e.dataTransfer?.dropEffect === 'none') cancel();
    },
    // Alt と上下の矢印で、まとまりの中で 1 つずつ動かす。
    onKeyDown: (e) => {
      const list = groups.find((g) => g.theme === group)?.sessions ?? [];
      const step = e.altKey ? ARROW_STEP[e.key] : undefined;
      const target = step && list[list.findIndex((v) => v.id === s.id) + step];
      if (!target) return;
      e.preventDefault();
      refocus.current = s.id;
      save(moveTo(ids, s.id, target.id));
    },
  });
  return { groups, carried: new Set(carried.map((s) => s.id)), dragOf, groupDrag };
};

/**
 * テーマの並べ替え。見出しをほかのテーマの見出しの上へ運ぶとその位置へ動かして見せ、一覧のどこで離しても見せた順で保存する。
 * 並べた順は、サーバーから届くまでの間も手元の状態で出す。ピン留めと未分類はテーマではないので、動かさない。
 */
const useThemeOrder = (themes: string[]) => {
  const [order, setOrder] = useState(themes);
  useEffect(() => setOrder(themes), [themes.join('\n')]);
  const [dragging, setDragging] = useState<string>();
  const headDrag = (theme: string) => ({
    draggable: true,
    // 名前を変えている間は、入力欄の中でマウスを引いて文字を選べるよう、ドラッグを始めない。
    onDragStart: (e: DragEvent) => {
      if ((e.currentTarget as Element).querySelector('.name-input')) return e.preventDefault();
      setDragging(theme);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
    },
    // まとまりの高さはまちまちなので、まとまりの上で動かすと、動かした先でまた入れ替わる。見出しの上に来たときだけ動かす。
    onDragOver: () => {
      if (dragging && dragging !== theme) setOrder(moveTo(order, dragging, theme));
    },
    // 見出しは並べ直しても DOM から外れないので、カードと違って dragend が届く。Esc で止めたときと、一覧の外で離したときは、元に戻す。
    onDragEnd: (e: DragEvent) => {
      setDragging(undefined);
      if (e.dataTransfer?.dropEffect === 'none') setOrder(themes);
    },
  });
  // Chrome は、ポインタの下の要素が変わった回には dragover を送らず、dragenter で落とせるかを決めるので、どちらでも受け付ける。
  const accept = (e: DragEvent) => {
    if (dragging) e.preventDefault();
  };
  const listDrop = {
    onDragEnter: accept,
    onDragOver: accept,
    onDrop: (e: DragEvent) => {
      if (!dragging) return;
      e.preventDefault();
      setDragging(undefined);
      void attempt('テーマの並べ替え', () => api.orderThemes(order));
    },
  };
  return { themes: order, dragging, headDrag, listDrop };
};

/** アーカイブしたセッションは並べない。 */
export const ThemeGroups = ({ snapshot, sel }: { snapshot: Snapshot; sel: Selection }) => {
  const { collapsed, toggle } = useCollapsed(COLLAPSED_KEY);
  const visible = snapshot.sessions.filter((s) => !s.archived);
  const { themes, dragging: draggedTheme, headDrag, listDrop } = useThemeOrder(snapshot.themes);
  const { groups, carried, dragOf, groupDrag } = useSessionOrder(snapshot, visible, sel.selected, themes);
  if (visible.length === 0) return <Empty>表示できるセッションがありません。</Empty>;
  return (
    <div class={`theme-list${draggedTheme ? ' is-sorting' : ''}`} {...listDrop}>
      {groups.map((g) => (
        <details key={g.theme} class={`theme-group${g.theme === draggedTheme ? ' is-dragging' : ''}`} open={!collapsed.has(g.theme)} onToggle={(e) => toggle(g.theme, e.currentTarget.open)} {...groupDrag(g.theme)}>
          <summary {...(g.theme !== PINNED && g.theme !== UNCLASSIFIED ? headDrag(g.theme) : {})}>
            <h3 class="theme-head">
              <SelectBox ids={g.sessions.map((s) => s.id)} sel={sel} label={`${g.theme || PINNED_TITLE} のセッションをすべて選ぶ`} />
              <ThemeTitle theme={g.theme} count={g.sessions.length} />
              <StatusCounts sessions={g.sessions} />
              <span class="chevron" aria-hidden="true">{collapsed.has(g.theme) ? '▸' : '▾'}</span>
            </h3>
          </summary>
          <div class="cards">
            {g.sessions.map((s) => <SessionCard key={s.id} s={s} sel={sel} dragging={carried.has(s.id)} drag={dragOf(s, g.theme)} />)}
          </div>
        </details>
      ))}
    </div>
  );
};
