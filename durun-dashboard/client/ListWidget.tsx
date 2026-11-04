// JIRA Issues と GitHub Inbox のウィジェットの共通の形。設定した検索の条件で探した項目を、選んだ属性の順に折りたためる組にまとめ、組の中は結果の順のまま並べる。
import type { JSX } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { WidgetSettings } from './layout.ts';
import { type Grouping, type ItemGroup, groupItems } from './listGroups.ts';
import { ARROW_STEP, moveTo } from './order.ts';
import { matcher } from './searchItems.ts';
import { Empty, Loading, onSubmitKey } from './ui.tsx';

type Field = { key: string; label: string };
/** 探した結果。more は、表示する件数を超える項目があるとき true。 */
export type Found<T> = { items?: T[]; more: boolean; error?: string };

/**
 * ウィジェットごとに違うところ。query は設定に検索の条件を持つキーで、fieldsOf と groupKeysOf は設定から選んだ属性を読む。groupKeysOf は外側の組の属性から順に並べる。
 * textOf は絞り込みで探す文字、hide はグループ化の属性に応じて行に出さない項目。
 */
export type ListSpec<T> = {
  title: string;
  noun: string;
  query: { key: string; label: string; fallback: string };
  maxLimit: number;
  defaultMax: number;
  fields: readonly Field[];
  fieldsOf: (settings: WidgetSettings) => Set<string>;
  groupings: Grouping<T>[];
  groupKeysOf: (settings: WidgetSettings) => Set<string>;
  keyOf: (item: T) => string;
  textOf: (item: T) => (string | undefined)[];
  hide?: (item: T, groupKeys: Set<string>) => boolean;
  Row: (p: { item: T; fields: Set<string> }) => JSX.Element;
  GroupName?: (p: { by: Grouping<T>; group: ItemGroup<T> }) => JSX.Element;
};

/** 設定に、選んだ key をカンマで区切って持つ値を読む。設定が無ければ fallback を読む。 */
export const keysOf = (value: string | undefined, fallback = ''): Set<string> => new Set((value ?? fallback).split(',').filter(Boolean));

const toggled = (all: readonly Field[], prev: Set<string>, key: string, on: boolean): Set<string> =>
  new Set(all.map((f) => f.key).filter((k) => (k === key ? on : prev.has(k))));

export const Checks = ({ legend, all, on, onToggle }: { legend: string; all: readonly Field[]; on: Set<string>; onToggle: (key: string, on: boolean) => void }) => (
  <fieldset class="jira-fields">
    <legend>{legend}</legend>
    {all.map((f) => (
      <label key={f.key} class="check">
        <input type="checkbox" checked={on.has(f.key)} onChange={(e) => onToggle(f.key, e.currentTarget.checked)} />
        {f.label}
      </label>
    ))}
  </fieldset>
);

type SettingsProps<T> = { spec: ListSpec<T>; settings: WidgetSettings; onSave: (next: WidgetSettings) => void; onClose: () => void };

/** 件数の範囲はフォームの検証で守る。Cmd/Ctrl + Enter でも検証を通すため、requestSubmit で送る。 */
const ListSettings = <T,>({ spec, settings, onSave, onClose }: SettingsProps<T>) => {
  const [title, setTitle] = useState(settings.title ?? '');
  const [query, setQuery] = useState(settings[spec.query.key] || spec.query.fallback);
  const [max, setMax] = useState(settings.max || String(spec.defaultMax));
  const [fields, setFields] = useState(() => spec.fieldsOf(settings));
  const [groups, setGroups] = useState(() => [...spec.groupKeysOf(settings)].filter((key) => spec.groupings.some((g) => g.key === key)));
  const [dragging, setDragging] = useState<string>();
  const labelOf = (key: string): string => spec.groupings.find((g) => g.key === key)?.label ?? key;
  const save = (e: SubmitEvent) => {
    e.preventDefault();
    onSave({ ...settings, title: title.trim(), [spec.query.key]: query.trim(), max, shown: [...fields].join(','), groups: groups.join(',') });
    onClose();
  };
  return (
    <form class="card panel stack jira-settings" onSubmit={save}>
      <label class="inline-field">
        見出し
        <input class="input input-sm" value={title} placeholder={spec.title} onInput={(e) => setTitle(e.currentTarget.value)} />
      </label>
      <label class="jira-jql">
        {spec.query.label}
        <textarea
          class="input mono"
          rows={3}
          required
          value={query}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => onSubmitKey(e, () => e.currentTarget.form?.requestSubmit())}
        />
      </label>
      <label class="inline-field">
        表示する件数
        <input class="input input-sm jira-max" type="number" min={1} max={spec.maxLimit} required value={max} onInput={(e) => setMax(e.currentTarget.value)} />
      </label>
      <Checks legend="表示する属性" all={spec.fields} on={fields} onToggle={(key, on) => setFields((prev) => toggled(spec.fields, prev, key, on))} />
      {/* 上の属性ほど外側の組にする。ドラッグか Alt + ↑↓ で並べ替える。 */}
      <fieldset class="jira-groups">
        <legend>グループ化</legend>
        <ol class="jira-group-list">
          {groups.map((key) => (
            <li
              key={key}
              class={`jira-group-item${dragging === key ? ' is-dragging' : ''}`}
              draggable
              tabIndex={0}
              aria-label={`${labelOf(key)}。Alt + ↑↓ で並べ替える`}
              onDragStart={() => setDragging(key)}
              onDragOver={(e) => {
                if (!dragging) return;
                e.preventDefault();
                if (key !== dragging) setGroups(moveTo(groups, dragging, key));
              }}
              onDragEnd={() => setDragging(undefined)}
              onKeyDown={(e) => {
                const step = e.altKey ? ARROW_STEP[e.key] : undefined;
                const target = step && groups[groups.indexOf(key) + step];
                if (!target) return;
                e.preventDefault();
                setGroups(moveTo(groups, key, target));
              }}
            >
              <span class="jira-group-grip" aria-hidden="true">⠿</span>
              {labelOf(key)}
              <button type="button" class="jira-group-remove" aria-label={`${labelOf(key)}で分けるのをやめる`} onClick={() => setGroups(groups.filter((k) => k !== key))}>×</button>
            </li>
          ))}
        </ol>
        {groups.length < spec.groupings.length && (
          <select class="input input-sm jira-group-add" value="" aria-label="グループ化の属性を足す" onChange={(e) => setGroups([...groups, e.currentTarget.value])}>
            <option value="">＋ 追加</option>
            {spec.groupings.filter((g) => !groups.includes(g.key)).map((g) => <option key={g.key} value={g.key}>{g.label}</option>)}
          </select>
        )}
      </fieldset>
      <div class="actions">
        <span class="spacer" />
        <button type="button" class="btn btn-sm" onClick={onClose}>キャンセル</button>
        <button class="btn btn-sm btn-primary">保存</button>
      </div>
    </form>
  );
};

/** 折りたたんだ組。組は、外側の組からの名前をタブでつないだ path で見分ける。 */
type Fold = { collapsed: Set<string>; toggle: (path: string, open: boolean) => void };

type GroupsProps<T> = { spec: ListSpec<T>; items: T[]; by: Grouping<T>[]; fields: Set<string>; fold: Fold; parent?: string };

/** 項目を by の先頭の属性で組に分け、組の中を残りの属性でさらに分ける。組は見出しを押すと折りたためる。 */
const Groups = <T,>({ spec, items, by, fields, fold, parent = '' }: GroupsProps<T>) => {
  const [first, ...rest] = by;
  if (!first) return <ul class="jira-list">{items.map((item) => <spec.Row key={spec.keyOf(item)} item={item} fields={fields} />)}</ul>;
  const Heading = parent ? 'h4' : 'h3';
  return (
    <>
      {groupItems(items, first).map((g, i) => {
        const path = parent ? `${parent}\t${g.name}` : g.name;
        const open = !fold.collapsed.has(path);
        return (
          <details key={`${i}\t${g.name}`} class="inbox-section jira-group" open={open} onToggle={(e) => fold.toggle(path, e.currentTarget.open)}>
            {/* 絞り込みの欄からフォーカスを外さない。外すと欄が消えて見出しの行が縮み、離した位置がずれて開閉しない。 */}
            <summary onMouseDown={(e) => e.preventDefault()}>
              <Heading class="inbox-section-title">
                {spec.GroupName ? <spec.GroupName by={first} group={g} /> : g.name}
                <span class="count-pill">{g.items.length}</span>
                <span class="jira-rule" aria-hidden="true" />
                <span class="chevron" aria-hidden="true">{open ? '▾' : '▸'}</span>
              </Heading>
            </summary>
            {open && <Groups spec={spec} items={g.items} by={rest} fields={fields} fold={fold} parent={path} />}
          </details>
        );
      })}
    </>
  );
};

/**
 * 枠の中にフォーカスがあるときに文字を打つと、絞り込みの欄にフォーカスを移し、打った文字から欄に入れる。
 * 見出しや一覧の空いたところを押しただけでは枠のどこにもフォーカスが入らないので、押したら欄に移す。欄で打ち始めれば、日本語入力も 1 文字目から変換できる。
 * ダッシュボードの編集中は、矢印キーで枠を動かせるよう、フォーカスを移さない。
 */
const useFilterFocus = () => {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const input = ref.current;
      const target = e.target as Element;
      // 空白は、ボタンを押したり一覧をスクロールしたりするのに残す。
      if (e.key?.length !== 1 || e.key === ' ' || e.ctrlKey || e.metaKey || e.altKey) return;
      // 同じ枠に重ねたほかのタブを選んでいる間は、欄が隠れているので何もしない。
      if (!input?.checkVisibility() || !input.closest('.widget')?.contains(target) || target.closest('input, textarea, .is-editing')) return;
      input.focus();
    };
    addEventListener('keydown', onKey);
    return () => removeEventListener('keydown', onKey);
  }, []);
  // 組の見出しは押すと開閉するので、欄を出して見出しの位置をずらさないよう、フォーカスを移さない。
  const onClick = (e: MouseEvent) => {
    if (!(e.target as Element).closest('a, button, input, form, summary, .is-editing')) ref.current?.focus();
  };
  return { ref, onClick };
};

type ListWidgetProps<T> = {
  spec: ListSpec<T>;
  settings: WidgetSettings;
  onSettings: (next: WidgetSettings) => void;
  /** 設定した条件と件数で探した結果を返すフック。描くたびに同じ順で呼ぶ。 */
  useFound: (query: string, max: number) => Found<T>;
};

/** 探した項目を、絞り込みの欄に打った語でさらに絞る。空白で区切ると、すべての語を含む項目だけを残す。Esc で語を消して欄を閉じる。 */
export const ListWidget = <T,>({ spec, settings, onSettings, useFound }: ListWidgetProps<T>) => {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState('');
  const filter = useFilterFocus();
  const max = Number(settings.max) || spec.defaultMax;
  const fields = spec.fieldsOf(settings);
  const groupKeys = spec.groupKeysOf(settings);
  // 折りたたんだ組はウィジェットの設定に覚え、同じ条件のウィジェットを複数置いても別々に開閉する。
  const collapsed = new Set(settings.collapsed?.split('\n').filter(Boolean));
  const fold: Fold = {
    collapsed,
    toggle: (path, open) => {
      if (open === !collapsed.has(path)) return;
      onSettings({ ...settings, collapsed: [...collapsed].filter((p) => p !== path).concat(open ? [] : [path]).join('\n') });
    },
  };
  const { items, more, error } = useFound(settings[spec.query.key] || spec.query.fallback, max);
  const hit = matcher(text);
  const shown = items?.filter((i) => !spec.hide?.(i, groupKeys) && hit(...spec.textOf(i))) ?? [];
  const by = [...groupKeys].map((key) => spec.groupings.find((g) => g.key === key)).filter((g) => g !== undefined);
  return (
    <>
      <div class="section-head" onClick={filter.onClick}>
        <h2 class="section-title">
          {settings.title || spec.title}
          {items && !error && <span class="count-pill">{shown.length}{more && '+'}</span>}
        </h2>
        <div class="actions">
          <input
            ref={filter.ref}
            class="input input-sm jira-filter"
            type="search"
            placeholder="絞り込み"
            aria-label={`表示中の${spec.noun}を絞り込む`}
            value={text}
            onInput={(e) => setText(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Escape' || e.isComposing) return;
              setText('');
              e.currentTarget.blur();
            }}
          />
          <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
        </div>
      </div>
      <div class="widget-body" onClick={filter.onClick}>
        {editing && <ListSettings spec={spec} settings={settings} onSave={onSettings} onClose={() => setEditing(false)} />}
        {error ? (
          <Empty>{spec.noun}を探せませんでした（{error}）</Empty>
        ) : !items ? (
          <Loading />
        ) : items.length === 0 ? (
          <Empty>条件に合う{spec.noun}はありません。</Empty>
        ) : shown.length === 0 ? (
          <Empty>絞り込みに合う{spec.noun}はありません。</Empty>
        ) : (
          <Groups spec={spec} items={shown} by={by} fields={fields} fold={fold} />
        )}
      </div>
    </>
  );
};
