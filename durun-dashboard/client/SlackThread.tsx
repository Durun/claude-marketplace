// 「スレッド」ウィジェット。登録した Slack のスレッドかチャンネルの会話を、昔の Skype の会話ウィンドウの見た目で並べる。
// 会話はタブで切り替え、チャンネルの返信のあるスレッドは押すとタブに足して開く。
// ホームでは設定に登録した 1 つと開いたスレッドを、セッションではセッションごとに足したものとチャットに出たリンクを並べる。
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Session, SlackChannel, SlackConversation, SlackMessage } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import type { Control } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { pickInWidget, useCached, usePicked } from './hooks.ts';
import { Markdown } from './Markdown.tsx';
import { Empty, Loading } from './ui.tsx';

export const SLACK_THREAD_TITLE = 'スレッド';
/** セッション内の「スレッド」ウィジェットの種類の名前。 */
export const SLACK_WIDGET = 'slack';
// タイトルバーのボタンはウィジェットの外にあるので、イベントで読み直させる。
const REFRESH_EVENT = 'slack-thread-refresh';
// 補完の候補を探すのは、打つ手が止まってから。1 文字ごとに Slack を探さない。
const SUGGEST_DELAY_MS = 300;
// 同じ人が続けた発言は、この間に収まれば名前とアバターを省いてまとめる。
const CONTINUE_SEC = 5 * 60;

export const slackThreadControls = (): Control[] => [{ kind: 'refresh', label: 'Slack の会話を読み直す', onClick: () => dispatchEvent(new Event(REFRESH_EVENT)) }];

const timeOf = (ts: string): string => new Date(Number(ts) * 1000).toLocaleString('ja-JP', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' });

// 発言者ごとに決まった色のアバターにする。Slack の画像は MCP から読めないので、頭文字で描く。
const AVATAR_COLORS = ['#00aff0', '#7fba00', '#f25022', '#ffb900', '#8e44ad', '#16a085'];
const colorOf = (name: string): string => AVATAR_COLORS[[...name].reduce((n, c) => n + c.codePointAt(0)!, 0) % AVATAR_COLORS.length]!;

const isUrl = (ref: string): boolean => /^https?:\/\//.test(ref);

/** 会話を読む前の名前。URL はどれも長いので、チャンネルかスレッドかだけを出す。 */
const shortLabel = (ref: string): string => (!isUrl(ref) ? ref : /\/p\d{16}|thread_ts=/.test(ref) ? 'スレッド' : 'チャンネル');

/** 投稿を Slack のアプリで開くリンク。ワークスペースの ID が分からなければ無い。スレッドの返信は、親の ts を添えてスレッドの中で開く。 */
const appLink = (c: SlackConversation, ts: string): string | undefined =>
  c.team && `slack://channel?team=${c.team}&id=${c.channel}&message=${ts}${c.thread && c.thread !== ts ? `&thread_ts=${c.thread}` : ''}`;

/** チャンネルの投稿のスレッドを指す、Slack の「リンクをコピー」と同じ形の URL。チャットに出たリンクと同じタブにまとまる。 */
const threadUrl = (c: SlackConversation, ts: string): string | undefined => c.domain && `https://${c.domain}.slack.com/archives/${c.channel}/p${ts.replace('.', '')}`;

type MessageProps = { m: SlackMessage; cont: boolean; c: SlackConversation; onOpenThread?: (ref: string) => void };

const Message = ({ m, cont, c, onOpenThread }: MessageProps) => {
  const link = appLink(c, m.ts);
  const thread = m.replies && onOpenThread && threadUrl(c, m.ts);
  return (
    <li class={`skype-msg${cont ? ' is-cont' : ''}`}>
      {cont ? <span /> : <span class="skype-avatar" style={`background: ${colorOf(m.user)}`} aria-hidden="true">{[...m.user][0]}</span>}
      <div class="skype-msg-main">
        {!cont && <b class="skype-name">{m.user}</b>}
        <div class="skype-text">
          <Markdown text={m.text} />
        </div>
        {m.replies && (thread ? <button class="skype-replies" title="スレッドをタブで開く" onClick={() => onOpenThread(thread)}>返信 {m.replies} 件</button> : <span class="skype-replies">返信 {m.replies} 件</span>)}
      </div>
      {link ? <a class="skype-time" href={link} title="Slack のアプリで開く">{timeOf(m.ts)}</a> : <span class="skype-time">{timeOf(m.ts)}</span>}
    </li>
  );
};

const useConversation = (ref: string) => useCached((fresh) => api.slack(ref, fresh), [ref]);

/** 選んだタブの会話。新しい発言が見える下端から見せる。 */
const Log = ({ slackRef, onOpenThread }: { slackRef: string; onOpenThread?: (ref: string) => void }) => {
  const { data, error, reload } = useConversation(slackRef);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    addEventListener(REFRESH_EVENT, reload);
    return () => removeEventListener(REFRESH_EVENT, reload);
  }, [reload]);
  const count = data?.messages.length;
  useLayoutEffect(() => {
    if (box.current) box.current.scrollTop = box.current.scrollHeight;
  }, [count]);
  return (
    <div class="skype-log" ref={box}>
      {error ? (
        <Empty>Slack を読めませんでした（{error}）</Empty>
      ) : !data ? (
        <Loading />
      ) : !data.messages.length ? (
        <Empty>発言はありません。</Empty>
      ) : (
        <ol>
          {data.messages.map((m, i) => {
            const prev = data.messages[i - 1];
            return <Message key={m.ts} m={m} c={data} onOpenThread={onOpenThread} cont={!!prev && prev.user === m.user && Number(m.ts) - Number(prev.ts) < CONTINUE_SEC} />;
          })}
        </ol>
      )}
    </div>
  );
};

/** タブの名前。会話を読んだら、チャンネルの名前かスレッドの親の発言の先頭にし、スレッドにはチャンネルの名前を小さく添える。 */
const TabLabel = ({ slackRef }: { slackRef: string }) => {
  const { data } = useConversation(slackRef);
  return (
    <>
      <span class="skype-presence" aria-hidden="true">✓</span>
      <span class="skype-tab-title">{data?.title ?? shortLabel(slackRef)}</span>
      {data?.thread && data.channelName && <small class="skype-channel">#{data.channelName}</small>}
    </>
  );
};

/** # で打ち始めると、名前に打った語を含むチャンネルを候補に出す。 */
const useChannelSuggestions = (value: string): SlackChannel[] => {
  const [list, setList] = useState<SlackChannel[]>([]);
  const q = value.match(/^#([^\s#]+)$/)?.[1];
  useEffect(() => {
    if (!q) return;
    let alive = true;
    const timer = setTimeout(() => {
      api.slackChannels(q).then(
        (found) => alive && setList(found),
        () => {},
      );
    }, SUGGEST_DELAY_MS);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [q]);
  return q ? list : [];
};

/** 登録する参照の欄。チャンネルの名前は、ブラウザの datalist で補う。 */
const RefInput = ({ value, onInput, autoFocus }: { value: string; onInput: (v: string) => void; autoFocus?: boolean }) => {
  const id = useId();
  const channels = useChannelSuggestions(value);
  return (
    <>
      <input
        class="input input-sm slack-ref"
        required
        autoFocus={autoFocus}
        list={id}
        // サーバーと同じく、リンク、チャンネルの ID、# で始めた名前だけを受け付ける。
        pattern="https://[^/]+\.slack\.com/archives/[A-Z0-9]+(/p\d{16})?/?(\?.*)?|[CGD][A-Z0-9]{6,}|#[^\s#]+"
        title="Slack のスレッドかチャンネルのリンク、チャンネルの ID（C で始まる）、# で始めたチャンネルの名前"
        placeholder="リンク、チャンネルの ID、#チャンネル名"
        value={value}
        onInput={(e) => onInput(e.currentTarget.value)}
      />
      <datalist id={id}>
        {channels.map((c) => (
          <option key={c.id} value={`#${c.name}`} />
        ))}
      </datalist>
    </>
  );
};

/** onAdd は足した参照を返す。サーバーが # で始めた名前を ID にして覚えるときは、覚えた参照のタブを選ぶため。 */
const AddForm = ({ onAdd, onDone }: { onAdd: (ref: string) => Promise<void>; onDone: () => void }) => {
  const [ref, setRef] = useState('');
  const add = async (e: SubmitEvent) => {
    e.preventDefault();
    await onAdd(ref.trim());
    onDone();
  };
  return (
    <form class="skype-add" onSubmit={add}>
      <RefInput value={ref} onInput={setRef} autoFocus />
      <button class="btn btn-sm btn-primary">追加</button>
      <button type="button" class="btn btn-sm" onClick={onDone}>キャンセル</button>
    </form>
  );
};

/**
 * Skype の会話ウィンドウ。上の帯に会話のタブを並べ、帯の右端の + で会話を足す。
 * closable の会話だけに × を出す。onOpenThread があれば、チャンネルの返信のあるスレッドを押して開ける。
 */
type PanelProps = {
  refs: string[];
  current?: string;
  onPick: (ref: string) => void;
  closable: (ref: string) => boolean;
  onClose: (ref: string) => void;
  onAdd: (ref: string) => Promise<void>;
  onOpenThread?: (ref: string) => void;
  empty: string;
};

const SkypePanel = ({ refs, current, onPick, closable, onClose, onAdd, onOpenThread, empty }: PanelProps) => {
  const [adding, setAdding] = useState(false);
  return (
    <div class="skype">
      <div class="skype-tabs" role="tablist" aria-label="会話">
        {refs.map((r) => (
          <span key={r} class={`skype-tab${r === current ? ' is-active' : ''}`}>
            <button role="tab" aria-selected={r === current} title={r} onClick={() => onPick(r)}>
              <TabLabel slackRef={r} />
            </button>
            {closable(r) && <button class="skype-tab-close" aria-label="この会話を閉じる" title="閉じる" onClick={() => onClose(r)}>×</button>}
          </span>
        ))}
        <button class="skype-tab-add" aria-label="会話を足す" title="会話を足す" aria-expanded={adding} onClick={() => setAdding((v) => !v)}>+</button>
      </div>
      {adding && <AddForm onAdd={onAdd} onDone={() => setAdding(false)} />}
      {current ? <Log key={current} slackRef={current} onOpenThread={onOpenThread} /> : !adding && <Empty>{empty}</Empty>}
    </div>
  );
};

const HomeSettings = ({ settings, onSave, onClose }: { settings: HomeWidgetProps['settings']; onSave: HomeWidgetProps['onSettings']; onClose?: () => void }) => {
  const [title, setTitle] = useState(settings.title ?? '');
  const [ref, setRef] = useState(settings.url ?? '');
  return (
    <form
      class="card panel stack jira-settings"
      onSubmit={(e) => {
        e.preventDefault();
        onSave({ ...settings, title: title.trim(), url: ref.trim() });
        onClose?.();
      }}
    >
      <label class="inline-field">
        見出し
        <input class="input input-sm" value={title} placeholder={SLACK_THREAD_TITLE} onInput={(e) => setTitle(e.currentTarget.value)} />
      </label>
      <label class="jira-jql">
        Slack のスレッドかチャンネル
        <RefInput value={ref} onInput={setRef} />
      </label>
      <div class="actions">
        <span class="spacer" />
        {onClose && <button type="button" class="btn btn-sm" onClick={onClose}>キャンセル</button>}
        <button class="btn btn-sm btn-primary">保存</button>
      </div>
    </form>
  );
};

/** 設定に、改行で区切った参照を持つ値を読む。 */
const linesOf = (value: string | undefined): string[] => (value ?? '').split('\n').filter(Boolean);

/** ホームのウィジェット。設定の url に登録した参照を、opened に + や返信から開いた参照を、改行で区切って持つ。 */
export const SlackThread = ({ settings, onSettings }: HomeWidgetProps) => {
  const [editing, setEditing] = useState(false);
  const main = settings.url;
  const opened = linesOf(settings.opened).filter((r) => r !== main);
  const refs = [...(main ? [main] : []), ...opened];
  const [pick, setPick] = useState<string>();
  const open = (ref: string) => {
    if (!refs.includes(ref)) onSettings({ ...settings, opened: [...opened, ref].join('\n') });
    setPick(ref);
  };
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">{settings.title || SLACK_THREAD_TITLE}</h2>
        {main && (
          <div class="actions">
            <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
          </div>
        )}
      </div>
      {/* 会話の欄の中だけがスクロールするので、設定は会話の上に置く。 */}
      {(editing || !main) && <HomeSettings settings={settings} onSave={onSettings} onClose={main ? () => setEditing(false) : undefined} />}
      {refs.length > 0 && (
        <div class="widget-body skype-body">
          <SkypePanel
            refs={refs}
            current={refs.find((r) => r === pick) ?? refs[0]}
            onPick={setPick}
            closable={(r) => r !== main}
            onClose={(r) => onSettings({ ...settings, opened: opened.filter((o) => o !== r).join('\n') })}
            onAdd={async (r) => open(r)}
            onOpenThread={open}
            empty="会話はありません。"
          />
        </div>
      )}
    </>
  );
};

/** セッションのウィジェット。セッションごとに足したスレッドと、チャットに出た Slack のリンクを、タブで切り替える。 */
export const SessionSlack = ({ s }: { s: Session }) => {
  const pick = usePicked(SLACK_WIDGET, s.id);
  const refs = s.slack ?? [];
  // サーバーは # で始めた名前を ID にして覚えるので、覚えた参照のタブを選ぶ。
  const add = async (ref: string) => {
    const added = await attempt('スレッドの追加', () => api.addSlack(s.id, ref));
    if (added) pickInWidget(SLACK_WIDGET, s.id, added.ref);
  };
  return (
    <>
      <h2 class="section-title">{SLACK_THREAD_TITLE}</h2>
      <div class="widget-body skype-body">
        <SkypePanel
          refs={refs}
          current={refs.find((r) => r === pick) ?? refs[0]}
          onPick={(r) => pickInWidget(SLACK_WIDGET, s.id, r)}
          closable={() => true}
          onClose={(r) => void attempt('スレッドを閉じる', () => api.removeSlack(s.id, r))}
          onAdd={add}
          onOpenThread={(r) => void add(r)}
          empty="Slack のスレッドはありません。チャットに Slack のリンクが出るか、+ で足すと並びます。"
        />
      </div>
    </>
  );
};
