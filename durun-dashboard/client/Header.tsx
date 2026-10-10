import { useContext, useEffect, useState } from 'preact/hooks';
import type { Attention, ImproveState, MemoryStatus, RateLimit, Session, Snapshot } from '../shared/types.ts';
import { api } from './api.ts';
import { createDashboard, duplicateDashboard, renameDashboard, switchDashboard, useDashboards } from './dashboards.ts';
import { clock, shortDateTime, toMs } from './format.ts';
import { NowContext } from './hooks.ts';
import { AttentionCard } from './Inbox.tsx';
import { setLightMode, useLightMode } from './lightMode.ts';
import { MarkdownPreview } from './Markdown.tsx';
import { ReplyArea, asksOf } from './Messaging.tsx';
import { notificationPermission } from './notify.ts';
import { useLiveReload } from './reload.ts';
import { href } from './router.ts';
import { EDIT_SHORTCUT, SEARCH_SHORTCUT, shortcutLabel } from './shortcut.ts';
import { Empty, Meter, Time, rateColor } from './ui.tsx';
import { openWallpaper } from './Wallpaper.tsx';

const RateMeter = ({ label, limit }: { label: string; limit: RateLimit }) => {
  const now = useContext(NowContext);
  const reset = limit.resetsAt ? shortDateTime(toMs(limit.resetsAt), now) : undefined;
  return (
    <div class="rate" title={reset ? `${label}の使用率。${reset} にリセット` : `${label}の使用率`}>
      <span class="rate-label">{label}</span>
      <Meter percent={limit.usedPercent} color={rateColor(limit.usedPercent)} label={`${label}の使用率`} />
      <span class="rate-value">{Math.round(limit.usedPercent)}%</span>
      {reset && <span class="rate-reset">{reset} リセット</span>}
    </div>
  );
};

const RATE_ID = 'rate-flyout';
/** 外周は線のリング。内周は線幅を半径の 2 倍にして中心まで塗り、扇形の円グラフにする。半径 5.5 の円で、外周の内側の縁 6.75 との間に隙間を残す。 */
const RINGS = { outer: { r: 8 }, inner: { r: 2.75, width: 5.5 } } as const;

/** 1 本のリング。pathLength を 100 にして、使用率をそのまま線の長さにする。12 時から時計回りに伸ばす。 */
const Ring = ({ r, width, limit }: { r: number; width?: number; limit?: RateLimit }) => (
  <>
    <circle class="rate-ring-track" cx="10" cy="10" r={r} stroke-width={width} />
    {limit && (
      <circle
        cx="10" cy="10" r={r} stroke-width={width} pathLength={100} transform="rotate(-90 10 10)"
        style={`stroke: ${rateColor(limit.usedPercent)}`} stroke-dasharray={`${Math.min(100, Math.max(0, limit.usedPercent))} 100`}
      />
    )}
  </>
);

/**
 * 通知領域に常駐する使用制限のアイコン。外周のリングが 5 時間、内側の円グラフが 7 日の使用率で、色はそれぞれの使用率で変わる。マウスを載せるかフォーカスすると内訳を開く。
 * タスクバーは overflow: hidden なので、内訳は popover にして最前面に出す。
 */
const RateTray = ({ fiveHour, sevenDay }: { fiveHour?: RateLimit; sevenDay?: RateLimit }) => {
  if (!fiveHour && !sevenDay) return null;
  const flyout = () => document.getElementById(RATE_ID);
  const show = () => flyout()?.showPopover();
  const hide = () => flyout()?.hidePopover();
  const summary = [fiveHour && `5 時間 ${Math.round(fiveHour.usedPercent)}%`, sevenDay && `7 日 ${Math.round(sevenDay.usedPercent)}%`].filter(Boolean).join('、');
  return (
    <>
      <button class="rate-tray" aria-label={`使用制限: ${summary}`} onPointerEnter={show} onPointerLeave={hide} onFocus={show} onBlur={hide}>
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <Ring {...RINGS.outer} limit={fiveHour} />
          <Ring {...RINGS.inner} limit={sevenDay} />
        </svg>
      </button>
      <div id={RATE_ID} class="rate-flyout" popover="manual">
        <b class="rate-head">使用制限</b>
        {fiveHour && <RateMeter label="5 時間" limit={fiveHour} />}
        {sevenDay && <RateMeter label="7 日" limit={sevenDay} />}
      </div>
    </>
  );
};

const LAST_MESSAGE_CHARS = 400;

/** 件数から開く一覧の 1 つのセッション。最後の発言と要対応を見ながら、その場で回答や返信を送れる。入力待ちの要対応は最後の発言と同じなので、発言だけを出す。 */
const QuickSession = ({ s, attention }: { s: Session; attention: Attention[] }) => {
  const asks = asksOf(attention, s.id);
  return (
    <section class="quick-session">
      <header class="quick-head">
        <a class="quick-name" href={href.session(s.id)}>{s.name}</a>
        <span class="muted nowrap"><Time at={s.updatedAt} /></span>
      </header>
      {s.lastMessage && <div class="card chat-claude"><MarkdownPreview text={s.lastMessage} max={LAST_MESSAGE_CHARS} /></div>}
      {attention.filter((a) => a.sessionId === s.id && a.kind !== 'reply' && !asks.includes(a)).map((a) => <AttentionCard key={a.id} a={a} session={s} />)}
      <ReplyArea s={s} asks={asks} />
    </section>
  );
};

/** tray は XP の通知領域に置く版。点滅する手のひらのアイコンの右下に件数を出し、1 件以上のときだけ見せる。 */
type StatusCountProps = { status: 'busy' | 'idle'; label: string; snapshot: Snapshot | null; live: Session[]; tray?: boolean };

/**
 * 作業中と入力待ちの件数。押すと、その状態のセッションの一覧を popover で開く。
 * 開いている間は開いたときのセッションを並べ続け、状態が変わっても書きかけの返信を消さない。返答の候補を作らせないよう、閉じている間は描かない。
 */
const StatusCount = ({ status, label, snapshot, live, tray }: StatusCountProps) => {
  const [ids, setIds] = useState<string[]>([]);
  const sessions = live.filter((s) => s.status === status);
  const id = tray ? `count-${status}-tray` : `count-${status}`;
  // リンクは popovertarget で閉じられないので、押したら閉じてから移る。
  const closeOnLink = (e: MouseEvent) => {
    if ((e.target as Element).closest('a')) (e.currentTarget as HTMLElement).hidePopover();
  };
  return (
    <span class={`count-wrap${tray ? ' count-tray' : ''}`}>
      {tray ? (
        // 0 件になっても、開いている一覧と書きかけの返信を消さないよう、ボタンだけを隠す。
        <button class={`count ${status}-tray`} popovertarget={id} hidden={sessions.length === 0} data-count={sessions.length} title={`${label} ${sessions.length} 件`} aria-label={`${label} ${sessions.length} 件`} />
      ) : (
        <button class="count" popovertarget={id}>
          <i class={`dot dot-${status}`} aria-hidden="true" />{label} <b>{sessions.length}</b>
        </button>
      )}
      <div id={id} class="count-flyout" popover onToggle={(e) => setIds(e.newState === 'open' ? sessions.map((s) => s.id) : [])} onClick={closeOnLink}>
        <b class="count-head">{label}のセッション</b>
        <div class="count-list">
          {ids.length === 0 && <Empty>{label}のセッションはありません。</Empty>}
          {snapshot?.sessions.filter((s) => ids.includes(s.id)).map((s) => (
            <QuickSession key={s.id} s={s} attention={snapshot?.attention ?? []} />
          ))}
        </div>
      </div>
    </span>
  );
};

const NotifyButton = () => {
  const [permission, setPermission] = useState(notificationPermission);
  if (permission !== 'default') return null;
  const enable = async () => setPermission(await Notification.requestPermission());
  return <button class="btn btn-sm" onClick={enable}>通知を有効にする</button>;
};

const SKINS = { default: 'デフォルト', xp: 'Windows XP' } as const;
type Skin = keyof typeof SKINS;
const SKIN_KEY = 'dashboard-skin';

const savedSkin = (): Skin => {
  const v = localStorage.getItem(SKIN_KEY) ?? '';
  return Object.hasOwn(SKINS, v) ? (v as Skin) : 'default';
};

// スキンは html の data-skin で CSS を切り替える。最初の描画から保存したスキンで描くよう、読み込んだ時点で当てる。
document.documentElement.dataset.skin = savedSkin();

const MENU_ID = 'app-menu';

type MenuProps = { onSearch: () => void; onEdit?: () => void };

/**
 * 名前の左のボタンで開くメニュー。外を押すか Esc で閉じるよう、popover で開く。
 * リンクは popovertarget で閉じられないので、項目を押したら、ここでまとめて閉じる。
 */
const AppMenu = ({ onSearch, onEdit }: MenuProps) => {
  const light = useLightMode();
  const [skin, setSkin] = useState(savedSkin);
  const changeSkin = (next: Skin) => {
    setSkin(next);
    document.documentElement.dataset.skin = next;
    localStorage.setItem(SKIN_KEY, next);
  };
  // Esc でも全画面が解けるので、押した状態は fullscreenchange から取る。
  const [fullscreen, setFullscreen] = useState(() => !!document.fullscreenElement);
  useEffect(() => {
    const sync = () => setFullscreen(!!document.fullscreenElement);
    document.addEventListener('fullscreenchange', sync);
    return () => document.removeEventListener('fullscreenchange', sync);
  }, []);
  const toggleFullscreen = () => {
    void (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen()).catch(() => {});
  };
  const dashboards = useDashboards();
  // ダッシュボードはホームの配置なので、ほかの画面で選んだらホームへ移る。
  const showHome = () => {
    location.hash = href.home();
  };
  const rename = () => {
    const name = prompt('ダッシュボードの名前', dashboards.list.find((d) => d.id === dashboards.current)?.name);
    if (name !== null) renameDashboard(name);
  };
  const closeOnItem = (e: MouseEvent) => {
    if ((e.target as Element).closest('a, button')) (e.currentTarget as HTMLElement).hidePopover();
  };
  return (
    <>
      <button class="menu-button" popovertarget={MENU_ID} aria-label="メニュー" title="メニュー">
        <span aria-hidden="true">☰</span>
      </button>
      <div id={MENU_ID} class="app-menu" popover onClick={closeOnItem}>
        <div class="app-menu-items">
          <button class="app-menu-item" title="セッション、要対応、資料、会話ログを探す" onClick={onSearch}>
            検索<kbd>{shortcutLabel(SEARCH_SHORTCUT)}</kbd>
          </button>
          <a class="app-menu-item" href={href.docs()}>資料</a>
          {onEdit && (
            <button class="app-menu-item dashboard-edit" onClick={onEdit}>
              ダッシュボードを編集<kbd>{shortcutLabel(EDIT_SHORTCUT)}</kbd>
            </button>
          )}
          <button class="app-menu-item" aria-pressed={fullscreen} onClick={toggleFullscreen}>全画面</button>
          <button class="app-menu-item" title="Windows XP のスキンの壁紙を、画像のファイルか URL で変える" onClick={openWallpaper}>壁紙</button>
          <button class="app-menu-item" aria-pressed={light} title="通信と描画を減らす。更新は 3 秒おきになり、アニメーションとイルカが止まる。タブが裏にある間は更新も通知も止まる" onClick={() => setLightMode(!light)}>軽量モード</button>
          <div class="app-menu-sub">
            <div class="app-menu-item" tabindex={0} aria-haspopup="true">ダッシュボード<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
            <div class="app-menu-submenu" role="group" aria-label="ダッシュボード">
              {dashboards.list.map((d) => (
                <button key={d.id} class="app-menu-item" aria-pressed={d.id === dashboards.current} onClick={() => (switchDashboard(d.id), showHome())}>{d.name}</button>
              ))}
              <hr class="app-menu-sep" />
              <button class="app-menu-item" onClick={() => (createDashboard(), showHome())}>新規作成</button>
              <button class="app-menu-item" onClick={() => (duplicateDashboard(), showHome())}>複製</button>
              <button class="app-menu-item" onClick={rename}>名前を変更</button>
            </div>
          </div>
          {/* XP のスタートメニューに倣い、マウスを載せるかフォーカスすると右に選択肢を出す。押してもメニューを閉じないよう、button にしない。 */}
          <div class="app-menu-sub">
            <div class="app-menu-item" tabindex={0} aria-haspopup="true">スキン<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
            <div class="app-menu-submenu" role="group" aria-label="スキン">
              {Object.entries(SKINS).map(([id, label]) => (
                <button key={id} class="app-menu-item" aria-pressed={skin === id} onClick={() => changeSkin(id as Skin)}>{label}</button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </>
  );
};

// ponytail: 時計は NowContext の 30 秒ごとの更新で進むので、分の切り替わりが最大 30 秒遅れる。秒まで合わせるなら専用のタイマーにする。
/** XP のスキンでだけ見せる、通知領域の時計。 */
const TaskbarClock = () => {
  const now = useContext(NowContext);
  return <span class="taskbar-clock">{clock(now)}</span>;
};

const MEMORY_TICK_MS = 5_000;
const LIGHT_MEMORY_TICK_MS = 30_000;
const MEMORY_ID = 'memory-flyout';
const GIB = 1024 ** 3;
const MIB = 1024 ** 2;
const PRESSURE: Record<MemoryStatus['level'], { label: string; color: string }> = {
  normal: { label: '正常', color: 'var(--good)' },
  warn: { label: '警告', color: 'var(--warning)' },
  critical: { label: '危険', color: 'var(--critical)' },
};

const fmtBytes = (n: number): string => (n >= GIB ? `${(n / GIB).toFixed(1)} GB` : `${Math.round(n / MIB)} MB`);

/**
 * 通知領域に常駐するメモリプレッシャーのアイコン。目盛りの高さで割合を、色でカーネルが判定した段階を示し、押すと内訳を開く。
 * メモリの変化はサーバーの Node から待ち受けられないので、アクティビティモニタの既定と同じく 5 秒ごとに読み直す。
 * 軽量モードでは 30 秒ごとにし、タブが裏にある間は読み直さない。
 */
const MemoryTray = () => {
  const light = useLightMode();
  const [memory, setMemory] = useState<MemoryStatus | null>(null);
  useEffect(() => {
    // 読めない間は、古い値を見せ続けないよう隠す。
    const load = () => api.memory().then(setMemory, () => setMemory(null));
    load();
    const timer = setInterval(() => (light && document.hidden) || load(), light ? LIGHT_MEMORY_TICK_MS : MEMORY_TICK_MS);
    return () => clearInterval(timer);
  }, [light]);
  if (!memory) return null;
  const { label, color } = PRESSURE[memory.level];
  const percent = Math.round(memory.pressurePercent);
  const summary = `メモリプレッシャー: ${label}（${percent}%）`;
  const rows: [name: string, bytes: number, sub?: boolean][] = [
    ['物理メモリ', memory.total],
    ['使用済みメモリ', memory.app + memory.wired + memory.compressed],
    ['App メモリ', memory.app, true],
    ['確保されているメモリ', memory.wired, true],
    ['圧縮', memory.compressed, true],
    ['キャッシュされたファイル', memory.cached],
    ['スワップ使用領域', memory.swapUsed],
  ];
  return (
    <>
      <button class="memory-tray" popovertarget={MEMORY_ID} title={summary} aria-label={summary} style={`--c: ${color}; --p: ${percent}%`} />
      <div id={MEMORY_ID} class="memory-flyout" popover>
        <b class="memory-head">メモリプレッシャー</b>
        <div class="memory-level">
          {label}
          <Meter percent={percent} color={color} label="メモリプレッシャー" />
          {percent}%
        </div>
        <dl class="memory-list">
          {rows.map(([name, bytes, sub]) => (
            <div key={name} class={sub ? 'is-sub' : undefined}>
              <dt>{name}</dt>
              <dd>{fmtBytes(bytes)}</dd>
            </div>
          ))}
        </dl>
      </div>
    </>
  );
};

/** XP のスキンでだけ見せる、通知領域の改善要望。右下に浮かぶボタンの代わりに置く。電球のアイコンにし、改善中は右下に矢印が回るバッジを重ねる。 */
const FeedbackTray = ({ improve, onClick }: { improve?: ImproveState; onClick: () => void }) =>
  improve?.running ? (
    <button class="fb-tray is-running" title={improve.current ? `改善中: ${improve.current}` : '改善中'} aria-label="改善中" onClick={onClick} />
  ) : (
    <button class="fb-tray" title="改善要望" aria-label="改善要望" onClick={onClick} />
  );

/**
 * 名前のボタン。押すとホームへ移り、そのままもう一度押すと元の画面へ戻る。ホームを覗いて、すぐ戻れるようにするため。
 * ホームからほかの画面へ移ったら、戻り先を忘れる。
 */
const Brand = ({ home }: { home: boolean }) => {
  const [back, setBack] = useState<string>();
  useEffect(() => {
    if (!home) setBack(undefined);
  }, [home]);
  const onClick = (e: MouseEvent) => {
    // 新しいタブやウィンドウで開くときは、この画面は移らない。
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    if (!home) setBack(location.hash);
    else if (back) {
      e.preventDefault();
      location.hash = back;
    }
  };
  return (
    <a class="brand" href={href.home()} aria-current={home ? 'page' : undefined} title={home && back ? '元の画面へ戻る' : undefined} onClick={onClick}>
      durun-dashboard
    </a>
  );
};

/** assets は、サーバーから届いた画面のファイルの更新時刻。前に開いていたときのデータでは、読み込み直すかを決めない。 */
type HeaderProps = { snapshot: Snapshot | null; assets?: Snapshot['assets']; connected: boolean; home: boolean; onFeedback: () => void } & MenuProps;

export const Header = ({ snapshot, assets, connected, home, onSearch, onEdit, onFeedback }: HeaderProps) => {
  const live = snapshot?.sessions.filter((s) => !s.archived) ?? [];
  const rate = snapshot?.rateLimits;
  const stale = useLiveReload(assets);
  return (
    <header class="topbar">
      <AppMenu onSearch={onSearch} onEdit={onEdit} />
      <Brand home={home} />
      {/* 浮かべたウィンドウのボタンは、Float.tsx の FloatFrame がここへ置く。 */}
      <div class="taskbar-windows" />
      <div class="counts">
        <StatusCount status="busy" label="作業中" snapshot={snapshot} live={live} />
        <StatusCount status="idle" label="入力待ち" snapshot={snapshot} live={live} />
      </div>
      <div class="topbar-end">
        {!connected && <span class="chip chip-bad">サーバーに接続できません</span>}
        {stale && (
          <button class="btn btn-sm btn-primary" title="書きかけの内容は消えます" onClick={() => location.reload()}>
            新しい画面を読み込む
          </button>
        )}
        <NotifyButton />
        <StatusCount status="idle" label="入力待ち" snapshot={snapshot} live={live} tray />
        <FeedbackTray improve={snapshot?.improve} onClick={onFeedback} />
        <RateTray fiveHour={rate?.fiveHour} sevenDay={rate?.sevenDay} />
        <MemoryTray />
        <TaskbarClock />
      </div>
    </header>
  );
};
