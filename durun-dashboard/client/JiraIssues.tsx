// ダッシュボードの「JIRA Issues」ウィジェット。設定した JQL で JIRA の課題を探して並べる。
import { useEffect, useRef, useState } from 'preact/hooks';
import { JIRA_LIST_MAX, type JiraIssue } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { type Control, floatUp } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { useCached } from './hooks.ts';
import { GROUPINGS, type Grouping, type IssueGroup, groupIssues } from './jiraGroups.ts';
import { JIRA_KEY } from './JiraView.tsx';
import type { WidgetSettings } from './layout.ts';
import { ARROW_STEP, moveTo } from './order.ts';
import { matcher } from './searchItems.ts';
import { Chip, Empty, Ext, Loading, Time, onSubmitKey } from './ui.tsx';

export const JIRA_TITLE = 'JIRA Issues';
const DEFAULT_JQL = 'assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC';
const DEFAULT_MAX = 20;

/** 一覧の行に出せる課題の属性。設定では、選んだものの key をこの順にカンマで区切って shown に持つ。 */
const FIELDS = [
  { key: 'status', label: 'ステータス' },
  { key: 'priority', label: '優先度' },
  { key: 'parent', label: '親' },
  { key: 'labels', label: 'ラベル' },
  { key: 'reporter', label: '報告者' },
  { key: 'assignee', label: '担当者' },
  { key: 'dueDate', label: '期限' },
] as const;
type FieldKey = (typeof FIELDS)[number]['key'];
// 設定が無いときは、ステータスと担当者だけを出す。
const DEFAULT_FIELDS = 'assignee';
/** fields は、ステータスをいつも出し、ほかの属性だけを選んでいた形の設定なので、ステータスを足して読む。 */
const fieldsOf = (settings: WidgetSettings): Set<string> => new Set((settings.shown ?? `status,${settings.fields ?? DEFAULT_FIELDS}`).split(',').filter(Boolean));
/** グループ化の属性の key を、外側の組から順にカンマで区切って持つ。groups の無い設定は、group の属性 1 つと、groupProject のときはその外側のプロジェクトで分ける。 */
const groupKeysOf = (settings: WidgetSettings): string[] =>
  (settings.groups ?? [settings.groupProject && 'project', settings.group].filter(Boolean).join(',')).split(',').filter(Boolean);
const labelOf = (key: string): string => GROUPINGS.find((g) => g.key === key)?.label ?? key;

type SettingsProps = { settings: WidgetSettings; onSave: (next: WidgetSettings) => void; onClose: () => void };

/** 件数の範囲はフォームの検証で守る。Cmd/Ctrl + Enter でも検証を通すため、requestSubmit で送る。 */
const JiraSettings = ({ settings, onSave, onClose }: SettingsProps) => {
  const [title, setTitle] = useState(settings.title ?? '');
  const [jql, setJql] = useState(settings.jql || DEFAULT_JQL);
  const [max, setMax] = useState(settings.max || String(DEFAULT_MAX));
  const [fields, setFields] = useState(() => fieldsOf(settings));
  const [groups, setGroups] = useState(() => groupKeysOf(settings));
  const [dragging, setDragging] = useState<string>();
  const toggle = (key: FieldKey, on: boolean) => setFields((prev) => new Set(FIELDS.map((f) => f.key).filter((k) => (k === key ? on : prev.has(k)))));
  const save = (e: SubmitEvent) => {
    e.preventDefault();
    onSave({ ...settings, title: title.trim(), jql: jql.trim(), max, shown: [...fields].join(','), groups: groups.join(',') });
    onClose();
  };
  return (
    <form class="card panel stack jira-settings" onSubmit={save}>
      <label class="inline-field">
        見出し
        <input class="input input-sm" value={title} placeholder={JIRA_TITLE} onInput={(e) => setTitle(e.currentTarget.value)} />
      </label>
      <label class="jira-jql">
        条件（JQL）
        <textarea
          class="input mono"
          rows={3}
          required
          value={jql}
          onInput={(e) => setJql(e.currentTarget.value)}
          onKeyDown={(e) => onSubmitKey(e, () => e.currentTarget.form?.requestSubmit())}
        />
      </label>
      <label class="inline-field">
        表示する件数
        <input class="input input-sm jira-max" type="number" min={1} max={JIRA_LIST_MAX} required value={max} onInput={(e) => setMax(e.currentTarget.value)} />
      </label>
      <fieldset class="jira-fields">
        <legend>表示する属性</legend>
        {FIELDS.map((f) => (
          <label key={f.key} class="check">
            <input type="checkbox" checked={fields.has(f.key)} onChange={(e) => toggle(f.key, e.currentTarget.checked)} />
            {f.label}
          </label>
        ))}
      </fieldset>
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
        {groups.length < GROUPINGS.length && (
          <select class="input input-sm jira-group-add" value="" aria-label="グループ化の属性を足す" onChange={(e) => setGroups([...groups, e.currentTarget.value])}>
            <option value="">＋ 追加</option>
            {GROUPINGS.filter((g) => !groups.includes(g.key)).map((g) => <option key={g.key} value={g.key}>{g.label}</option>)}
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

/** JIRA のステータスカテゴリの key。new は未着手、indeterminate は進行中、done は完了。 */
const CATEGORY_TONE: Record<string, 'muted' | 'info' | 'ok' | undefined> = { new: 'muted', indeterminate: 'info', done: 'ok' };
/** JIRA の既定の優先度名。独自の優先度名は色を付けずに出す。 */
const PRIORITY_TONE: Record<string, 'bad' | 'warn' | 'info' | undefined> = { Highest: 'bad', High: 'bad', Medium: 'warn', Low: 'info', Lowest: 'info' };

/** 選んだ属性のうち、値のあるものを出す。担当者は、いないことも分かるよう未割り当てと出す。優先度と親は別の場所に出す。 */
const metaOf = (issue: JiraIssue, fields: Set<string>): string[] =>
  [
    fields.has('reporter') && issue.reporter && `報告 ${issue.reporter}`,
    fields.has('assignee') && (issue.assignee ?? '未割り当て'),
    fields.has('dueDate') && issue.dueDate && `期限 ${issue.dueDate}`,
  ].filter((v) => typeof v === 'string');

/** 行は課題そのものへのリンクで、a を入れ子にできないため、親はクリックで行のリンクを打ち消して親の課題を開く。 */
const openParent = (key: string) => (e: MouseEvent) => {
  e.preventDefault();
  floatUp(JIRA_KEY, '', { id: key });
};

export const IssueRow = ({ issue, fields }: { issue: JiraIssue; fields: Set<string> }) => (
  <li>
    <Ext class="jira-row" href={issue.url}>
      <span class="jira-head">
        <span class="jira-key">
          {fields.has('parent') && issue.parent && (
            <><span class="jira-parent" title={issue.parent.summary} onClick={openParent(issue.parent.key)}>{issue.parent.key} {issue.parent.summary}</span> / </>
          )}
          {issue.key}
        </span>
        {fields.has('status') && issue.status && <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{issue.status}</Chip>}
        {fields.has('priority') && issue.priority && <Chip tone={PRIORITY_TONE[issue.priority]}>{issue.priority}</Chip>}
        {fields.has('labels') && issue.labels?.map((l) => <Chip key={l}>{l}</Chip>)}
        <span class="jira-meta">
          {metaOf(issue, fields).join(' · ')}
          {issue.updatedAt !== undefined && <>{metaOf(issue, fields).length > 0 && ' · '}<Time at={issue.updatedAt} /></>}
        </span>
      </span>
      <span class="jira-summary">{issue.summary}</span>
    </Ext>
  </li>
);

/** 組の見出しの名前。ステータス、優先度、エピックは、一覧の行と同じ色と装飾で出す。 */
const GroupName = ({ by, group }: { by: Grouping; group: IssueGroup }) => {
  // 組の課題はどれも同じ値を持つので、先頭の課題から色を決める。値の無い組は名前だけを出す。
  const [issue] = group.issues;
  if (!issue || by.of(issue) === undefined) return <>{group.name}</>;
  if (by.key === 'status') return <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{group.name}</Chip>;
  if (by.key === 'priority') return <Chip tone={PRIORITY_TONE[group.name]}>{group.name}</Chip>;
  if (by.key === 'epic' && issue.parent) {
    return <span class="jira-parent" title={issue.parent.summary} onClick={openParent(issue.parent.key)}>{group.name}</span>;
  }
  return <>{group.name}</>;
};

/** 折りたたんだ組。組は、外側の組からの名前をタブでつないだ path で見分ける。 */
type Fold = { collapsed: Set<string>; toggle: (path: string, open: boolean) => void };

/** 課題を by の先頭の属性で組に分け、組の中を残りの属性でさらに分ける。組は見出しを押すと折りたためる。 */
const IssueGroups = ({ issues, by, fields, fold, parent = '' }: { issues: JiraIssue[]; by: Grouping[]; fields: Set<string>; fold: Fold; parent?: string }) => {
  const [first, ...rest] = by;
  if (!first) return <ul class="jira-list">{issues.map((issue) => <IssueRow key={issue.key} issue={issue} fields={fields} />)}</ul>;
  const Heading = parent ? 'h4' : 'h3';
  return (
    <>
      {groupIssues(issues, first).map((g) => {
        const path = parent ? `${parent}\t${g.name}` : g.name;
        const open = !fold.collapsed.has(path);
        return (
          <details key={g.name} class="inbox-section jira-group" open={open} onToggle={(e) => fold.toggle(path, e.currentTarget.open)}>
            <summary>
              <Heading class="inbox-section-title">
                <GroupName by={first} group={g} />
                <span class="count-pill">{g.issues.length}</span>
                <span class="chevron" aria-hidden="true">{open ? '▾' : '▸'}</span>
              </Heading>
            </summary>
            {open && <IssueGroups issues={g.issues} by={rest} fields={fields} fold={fold} parent={path} />}
          </details>
        );
      })}
    </>
  );
};

/** タイトルバー右端の更新のボタン。サーバーに JIRA の情報を取り直させ、JQL が同じでも探し直す。 */
export const jiraControls = (): Control[] => [
  { kind: 'refresh', label: 'JIRA の課題の情報を取り直す', onClick: () => void attempt('JIRA の更新', api.refreshJira) },
];

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
  const onClick = (e: MouseEvent) => {
    if (!(e.target as Element).closest('a, button, input, form, .is-editing')) ref.current?.focus();
  };
  return { ref, onClick };
};

/** JQL で探した課題を、絞り込みの欄に打った語でさらに絞る。空白で区切ると、すべての語を含む課題だけを残す。Esc で語を消して欄を閉じる。 */
export const JiraIssues = ({ snapshot, settings, onSettings }: HomeWidgetProps) => {
  const [editing, setEditing] = useState(false);
  const [query, setQuery] = useState('');
  const filter = useFilterFocus();
  const jql = settings.jql || DEFAULT_JQL;
  const max = Number(settings.max) || DEFAULT_MAX;
  const fields = fieldsOf(settings);
  const by = groupKeysOf(settings).map((key) => GROUPINGS.find((g) => g.key === key)).filter((g) => g !== undefined);
  // 折りたたんだ組はウィジェットの設定に覚え、同じ JQL のウィジェットを複数置いても別々に開閉する。
  const collapsed = new Set(settings.collapsed?.split('\n').filter(Boolean));
  const fold: Fold = {
    collapsed,
    toggle: (path, open) => {
      if (open === !collapsed.has(path)) return;
      onSettings({ ...settings, collapsed: [...collapsed].filter((p) => p !== path).concat(open ? [] : [path]).join('\n') });
    },
  };
  // 1 件多く探し、表示する件数を超える課題があれば、件数に + を添える。
  const { data: found, error } = useCached(() => api.jira(jql, max + 1), [jql, max, snapshot.jiraRefreshedAt]);
  const data = found?.slice(0, max);
  const more = (found?.length ?? 0) > max;
  const hit = matcher(query);
  const shown = data?.filter((i) => hit(i.key, i.summary, i.status, i.priority, i.parent?.key, i.parent?.summary, i.assignee, i.reporter, ...(i.labels ?? []))) ?? [];
  return (
    <>
      <div class="section-head" onClick={filter.onClick}>
        <h2 class="section-title">
          {settings.title || JIRA_TITLE}
          {data && !error && <span class="count-pill">{shown.length}{more && '+'}</span>}
        </h2>
        <div class="actions">
          <input
            ref={filter.ref}
            class="input input-sm jira-filter"
            type="search"
            placeholder="絞り込み"
            aria-label="表示中の課題を絞り込む"
            value={query}
            onInput={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Escape' || e.isComposing) return;
              setQuery('');
              e.currentTarget.blur();
            }}
          />
          <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
        </div>
      </div>
      <div class="widget-body" onClick={filter.onClick}>
        {editing && <JiraSettings settings={settings} onSave={onSettings} onClose={() => setEditing(false)} />}
        {error ? (
          <Empty>課題を探せませんでした（{error}）</Empty>
        ) : !data ? (
          <Loading />
        ) : data.length === 0 ? (
          <Empty>条件に合う課題はありません。</Empty>
        ) : shown.length === 0 ? (
          <Empty>絞り込みに合う課題はありません。</Empty>
        ) : (
          <IssueGroups issues={shown} by={by} fields={fields} fold={fold} />
        )}
      </div>
    </>
  );
};
