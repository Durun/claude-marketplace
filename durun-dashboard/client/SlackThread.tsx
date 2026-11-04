// 「スレッド」ウィジェット。登録した Slack のスレッドかチャンネルの会話を、昔の Skype の会話ウィンドウの見た目で並べる。
// ホームでは設定に登録した 1 つを、セッションではセッションごとに足したものとチャットに出たリンクを、タブで切り替えて出す。
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { Session, SlackChannel, SlackMessage } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import type { Control } from './Float.tsx';
import type { HomeWidgetProps } from './Home.tsx';
import { pickInWidget, useCached, usePicked } from './hooks.ts';
import { Markdown } from './Markdown.tsx';
import { Empty, Ext, Loading } from './ui.tsx';

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

const Message = ({ m, cont }: { m: SlackMessage; cont: boolean }) => (
  <li class={`skype-msg${cont ? ' is-cont' : ''}`}>
    {cont ? <span /> : <span class="skype-avatar" style={`background: ${colorOf(m.user)}`} aria-hidden="true">{[...m.user][0]}</span>}
    <div class="skype-msg-main">
      {!cont && <b class="skype-name">{m.user}</b>}
      <div class="skype-text">
        <Markdown text={m.text} />
      </div>
      {m.replies && <span class="skype-replies">返信 {m.replies} 件</span>}
    </div>
    <span class="skype-time">{timeOf(m.ts)}</span>
  </li>
);

/** 会話。見出しの帯に会話の名前を出し、Slack で開けるものはリンクにする。 */
const Conversation = ({ slackRef }: { slackRef: string }) => {
  const { data, error, reload } = useCached((fresh) => api.slack(slackRef, fresh), [slackRef]);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    addEventListener(REFRESH_EVENT, reload);
    return () => removeEventListener(REFRESH_EVENT, reload);
  }, [reload]);
  // チャットのように、新しい発言が見える下端から見せる。
  const count = data?.messages.length;
  useLayoutEffect(() => {
    const body = box.current?.closest('.widget-body');
    if (body) body.scrollTop = body.scrollHeight;
  }, [count]);
  const title = data?.title ?? shortLabel(slackRef);
  return (
    <div class="skype" ref={box}>
      <div class="skype-head">
        <span class="skype-presence" aria-hidden="true">✓</span>
        <span class="skype-title">{isUrl(slackRef) ? <Ext href={slackRef} title="Slack で開く">{title}</Ext> : title}</span>
      </div>
      {error ? (
        <Empty>Slack を読めませんでした（{error}）</Empty>
      ) : !data ? (
        <Loading />
      ) : !data.messages.length ? (
        <Empty>発言はありません。</Empty>
      ) : (
        <ol class="skype-log">
          {data.messages.map((m, i) => {
            const prev = data.messages[i - 1];
            return <Message key={m.ts} m={m} cont={!!prev && prev.user === m.user && Number(m.ts) - Number(prev.ts) < CONTINUE_SEC} />;
          })}
        </ol>
      )}
    </div>
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

type HomeSettingsProps = { settings: HomeWidgetProps['settings']; onSave: HomeWidgetProps['onSettings']; onClose?: () => void };

const HomeSettings = ({ settings, onSave, onClose }: HomeSettingsProps) => {
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

/** ホームのウィジェット。設定の url に、登録した参照（リンク、ID、# で始めた名前）を持つ。 */
export const SlackThread = ({ settings, onSettings }: HomeWidgetProps) => {
  const [editing, setEditing] = useState(false);
  const ref = settings.url;
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">{settings.title || SLACK_THREAD_TITLE}</h2>
        {ref && (
          <div class="actions">
            <button class="btn btn-sm" data-icon="settings" aria-expanded={editing} onClick={() => setEditing((v) => !v)}>設定</button>
          </div>
        )}
      </div>
      {/* 会話の欄は下端までスクロールするので、設定はスクロールする欄の外に置く。 */}
      {(editing || !ref) && <HomeSettings settings={settings} onSave={onSettings} onClose={ref ? () => setEditing(false) : undefined} />}
      {ref && (
        <div class="widget-body">
          <Conversation key={ref} slackRef={ref} />
        </div>
      )}
    </>
  );
};

/** タブの名前。会話を読んだら、チャンネルの名前かスレッドの親の発言の先頭にする。 */
const TabLabel = ({ slackRef }: { slackRef: string }) => {
  const { data } = useCached((fresh) => api.slack(slackRef, fresh), [slackRef]);
  return <>{data?.title ?? shortLabel(slackRef)}</>;
};

const AddForm = ({ s, onDone }: { s: Session; onDone: () => void }) => {
  const [ref, setRef] = useState('');
  const add = async (e: SubmitEvent) => {
    e.preventDefault();
    // サーバーは # で始めた名前を ID にして覚えるので、覚えた参照のタブを選ぶ。
    const added = await attempt('スレッドの追加', () => api.addSlack(s.id, ref.trim()));
    if (!added) return;
    pickInWidget(SLACK_WIDGET, s.id, added.ref);
    onDone();
  };
  return (
    <form class="slack-add" onSubmit={add}>
      <RefInput value={ref} onInput={setRef} autoFocus />
      <button class="btn btn-sm btn-primary">追加</button>
      <button type="button" class="btn btn-sm" onClick={onDone}>キャンセル</button>
    </form>
  );
};

/** セッションのウィジェット。セッションごとに足したスレッドと、チャットに出た Slack のリンクを、タブで切り替える。 */
export const SessionSlack = ({ s }: { s: Session }) => {
  const [adding, setAdding] = useState(false);
  const pick = usePicked(SLACK_WIDGET, s.id);
  const refs = s.slack ?? [];
  const current = refs.find((r) => r === pick) ?? refs[0];
  return (
    <>
      <div class="section-head">
        <h2 class="section-title">{SLACK_THREAD_TITLE}</h2>
        <div class="actions">
          <button class="btn btn-sm" data-icon="add" aria-expanded={adding} onClick={() => setAdding((v) => !v)}>追加</button>
        </div>
      </div>
      {/* 会話の欄は下端までスクロールするので、追加の欄とタブはスクロールする欄の外に置く。 */}
      {adding && <AddForm s={s} onDone={() => setAdding(false)} />}
      {refs.length > 0 && (
        <div class="pick-tabs" role="tablist" aria-label="スレッド">
          {refs.map((r) => (
            <span key={r} class="slack-tab">
              <button role="tab" aria-selected={r === current} title={r} onClick={() => pickInWidget(SLACK_WIDGET, s.id, r)}>
                <TabLabel slackRef={r} />
              </button>
              <button class="slack-tab-close" aria-label="このスレッドを閉じる" title="閉じる" onClick={() => void attempt('スレッドを閉じる', () => api.removeSlack(s.id, r))}>×</button>
            </span>
          ))}
        </div>
      )}
      <div class="widget-body">
        {current ? <Conversation key={current} slackRef={current} /> : !adding && <Empty>Slack のスレッドはありません。チャットに Slack のリンクが出るか、「追加」で足すと並びます。</Empty>}
      </div>
    </>
  );
};
