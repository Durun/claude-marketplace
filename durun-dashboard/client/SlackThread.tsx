// ダッシュボードの「スレッド」ウィジェット。登録した Slack のスレッドかチャンネルの会話を、昔の Skype の会話ウィンドウの見た目で並べる。
import { useEffect, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { SlackMessage } from '../shared/types.ts';
import { api } from './api.ts';
import type { Control } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { useCached } from './hooks.ts';
import { Markdown } from './Markdown.tsx';
import { Empty, Ext, Loading } from './ui.tsx';

export const SLACK_THREAD_TITLE = 'スレッド';
// タイトルバーのボタンはウィジェットの外にあるので、イベントで読み直させる。
const REFRESH_EVENT = 'slack-thread-refresh';

export const slackThreadControls = (): Control[] => [{ kind: 'refresh', label: 'Slack の会話を読み直す', onClick: () => dispatchEvent(new Event(REFRESH_EVENT)) }];

const timeOf = (ts: string): string => new Date(Number(ts) * 1000).toLocaleString('ja-JP', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });

// 発言者ごとに決まった色のアバターにする。Slack の画像は MCP から読めないので、頭文字で描く。
const AVATAR_COLORS = ['#00aff0', '#7fba00', '#f25022', '#ffb900', '#8e44ad', '#16a085'];
const colorOf = (name: string): string => AVATAR_COLORS[[...name].reduce((n, c) => n + c.codePointAt(0)!, 0) % AVATAR_COLORS.length]!;

const Message = ({ m, cont }: { m: SlackMessage; cont: boolean }) => (
  <li class={`skype-msg${cont ? ' is-cont' : ''}`}>
    <span class="skype-avatar" style={`background: ${colorOf(m.user)}`} aria-hidden="true">{cont ? '' : [...m.user][0]}</span>
    <div class="skype-msg-main">
      {!cont && (
        <div class="skype-msg-head">
          <b class="skype-name">{m.user}</b>
        </div>
      )}
      <div class="skype-text">
        <Markdown text={m.text} />
        {m.replies && <span class="skype-replies">返信 {m.replies} 件</span>}
      </div>
    </div>
    <span class="skype-time">{timeOf(m.ts)}</span>
  </li>
);

const Conversation = ({ url }: { url: string }) => {
  const { data, error, reload } = useCached((fresh) => api.slack(url, fresh), [url]);
  const list = useRef<HTMLOListElement>(null);
  useEffect(() => {
    addEventListener(REFRESH_EVENT, reload);
    return () => removeEventListener(REFRESH_EVENT, reload);
  }, [reload]);
  // チャットのように、新しい発言が見える下端から見せる。
  const count = data?.messages.length;
  useLayoutEffect(() => {
    const el = list.current?.parentElement;
    if (el) el.scrollTop = el.scrollHeight;
  }, [count]);
  if (error) return <Empty>Slack を読めませんでした（{error}）</Empty>;
  if (!data) return <Loading />;
  if (!data.messages.length) return <Empty>発言はありません。</Empty>;
  return (
    <ol class="skype-log" ref={list}>
      {data.messages.map((m, i) => (
        // 同じ人が 5 分以内に続けた発言は、名前とアバターを省いてまとめる。
        <Message key={m.ts} m={m} cont={i > 0 && data.messages[i - 1]!.user === m.user && Number(m.ts) - Number(data.messages[i - 1]!.ts) < 300} />
      ))}
    </ol>
  );
};

const Settings = ({ settings, onSave, onClose }: { settings: HomeWidgetProps['settings']; onSave: HomeWidgetProps['onSettings']; onClose?: () => void }) => {
  const [title, setTitle] = useState(settings.title ?? '');
  const [url, setUrl] = useState(settings.url ?? '');
  return (
    <form
      class="card panel stack jira-settings"
      onSubmit={(e) => {
        e.preventDefault();
        onSave({ ...settings, title: title.trim(), url: url.trim() });
        onClose?.();
      }}
    >
      <label class="inline-field">
        見出し
        <input class="input input-sm" value={title} placeholder={SLACK_THREAD_TITLE} onInput={(e) => setTitle(e.currentTarget.value)} />
      </label>
      <label class="jira-jql">
        Slack のスレッドかチャンネルのリンク
        {/* Slack の「リンクをコピー」の形だけを受け付ける。サーバーも同じ形で読む先を決める。 */}
        <input
          class="input mono"
          type="url"
          required
          pattern="https://[^/]+\.slack\.com/archives/[A-Z0-9]+(/p\d{16})?/?(\?.*)?"
          placeholder="https://example.slack.com/archives/C0123456789/p1700000000123456"
          value={url}
          onInput={(e) => setUrl(e.currentTarget.value)}
        />
      </label>
      <div class="actions">
        <span class="spacer" />
        {onClose && <button type="button" class="btn btn-sm" onClick={onClose}>キャンセル</button>}
        <button class="btn btn-sm btn-primary">保存</button>
      </div>
    </form>
  );
};

export const SlackThread = ({ settings, onSettings }: HomeWidgetProps) => {
  const [editing, setEditing] = useState(false);
  const url = settings.url;
  return (
    <>
      {(editing || !url) && <Settings settings={settings} onSave={onSettings} onClose={url ? () => setEditing(false) : undefined} />}
      {url && (
        <div class="skype">
          <div class="skype-head">
            <span class="skype-presence" aria-hidden="true">✓</span>
            <h2 class="skype-title"><Ext href={url}>{settings.title || SLACK_THREAD_TITLE}</Ext></h2>
            <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
          </div>
          <div class="widget-body skype-body">
            <Conversation url={url} />
          </div>
        </div>
      )}
    </>
  );
};
