// ダッシュボードの「JIRA Issues」ウィジェット。設定した JQL で JIRA の課題を探して並べる。
import { useState } from 'preact/hooks';
import { JIRA_MAX_RESULTS, type JiraIssue } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { type Control, floatUp } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { useFetch } from './hooks.ts';
import { JIRA_KEY } from './JiraView.tsx';
import type { WidgetSettings } from './layout.ts';
import { Chip, Empty, Ext, Time, onSubmitKey } from './ui.tsx';

export const JIRA_TITLE = 'JIRA Issues';
const DEFAULT_JQL = 'assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC';
const DEFAULT_MAX = 20;

/** 一覧の行に出せる課題の属性。設定では、選んだものの key をこの順にカンマで区切って持つ。 */
const FIELDS = [
  { key: 'priority', label: '優先度' },
  { key: 'parent', label: '親' },
  { key: 'labels', label: 'ラベル' },
  { key: 'reporter', label: '報告者' },
  { key: 'assignee', label: '担当者' },
  { key: 'dueDate', label: '期限' },
] as const;
type FieldKey = (typeof FIELDS)[number]['key'];
// 設定が無いときは、属性を選べるようになる前と同じく担当者だけを出す。
const DEFAULT_FIELDS = 'assignee';
const fieldsOf = (settings: WidgetSettings): Set<string> => new Set((settings.fields ?? DEFAULT_FIELDS).split(',').filter(Boolean));

type SettingsProps = { settings: WidgetSettings; onSave: (next: WidgetSettings) => void; onClose: () => void };

/** 件数の範囲はフォームの検証で守る。Cmd/Ctrl + Enter でも検証を通すため、requestSubmit で送る。 */
const JiraSettings = ({ settings, onSave, onClose }: SettingsProps) => {
  const [title, setTitle] = useState(settings.title ?? '');
  const [jql, setJql] = useState(settings.jql || DEFAULT_JQL);
  const [max, setMax] = useState(settings.max || String(DEFAULT_MAX));
  const [fields, setFields] = useState(() => fieldsOf(settings));
  const toggle = (key: FieldKey, on: boolean) => setFields((prev) => new Set(FIELDS.map((f) => f.key).filter((k) => (k === key ? on : prev.has(k)))));
  const save = (e: SubmitEvent) => {
    e.preventDefault();
    onSave({ title: title.trim(), jql: jql.trim(), max, fields: [...fields].join(',') });
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
        <input class="input input-sm jira-max" type="number" min={1} max={JIRA_MAX_RESULTS} required value={max} onInput={(e) => setMax(e.currentTarget.value)} />
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

const IssueRow = ({ issue, fields }: { issue: JiraIssue; fields: Set<string> }) => (
  <li>
    <Ext class="jira-row" href={issue.url}>
      <span class="jira-head">
        <span class="jira-key">
          {fields.has('parent') && issue.parent && (
            <><span class="jira-parent" title={issue.parent.summary} onClick={openParent(issue.parent.key)}>{issue.parent.key} {issue.parent.summary}</span> / </>
          )}
          {issue.key}
        </span>
        {issue.status && <Chip tone={CATEGORY_TONE[issue.statusCategory ?? '']}>{issue.status}</Chip>}
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

/** タイトルバー右端の更新のボタン。サーバーに JIRA の情報を取り直させ、JQL が同じでも探し直す。 */
export const jiraControls = (): Control[] => [
  { kind: 'refresh', label: 'JIRA の課題の情報を取り直す', onClick: () => void attempt('JIRA の更新', api.refreshJira) },
];

export const JiraIssues = ({ snapshot, settings, onSettings }: HomeWidgetProps) => {
  const [editing, setEditing] = useState(false);
  const jql = settings.jql || DEFAULT_JQL;
  const max = Number(settings.max) || DEFAULT_MAX;
  const fields = fieldsOf(settings);
  const { data, error } = useFetch(() => api.jira(jql, max), [jql, max, snapshot.jiraRefreshedAt]);
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">
          {settings.title || JIRA_TITLE}
          {data && !error && <span class="count-pill">{data.length}</span>}
        </h2>
        <div class="actions">
          <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
        </div>
      </div>
      <div class="widget-body">
        {editing && <JiraSettings settings={settings} onSave={onSettings} onClose={() => setEditing(false)} />}
        {error ? (
          <Empty>課題を探せませんでした（{error}）</Empty>
        ) : !data ? (
          <Empty>探しています…</Empty>
        ) : data.length === 0 ? (
          <Empty>条件に合う課題はありません。</Empty>
        ) : (
          <ul class="jira-list">{data.map((issue) => <IssueRow key={issue.key} issue={issue} fields={fields} />)}</ul>
        )}
      </div>
    </>
  );
};
