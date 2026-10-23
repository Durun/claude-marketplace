import { useContext, useEffect, useState } from 'preact/hooks';
import type { Attention, DocMeta, ImproveState, MemoryStatus, RateLimit, Session, Snapshot } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import { AppIcon, useApps } from './apps.tsx';
import { createDashboard, duplicateDashboard, renameDashboard, switchDashboard, useDashboards } from './dashboards.ts';
import { EXPLORER_ICON, EXPLORER_KEY, EXPLORER_TITLE, openExplorer } from './Explorer.tsx';
import { baseName, useFavorites } from './explorerFavorites.ts';
import { MINESWEEPER_ICON, MINESWEEPER_TITLE, SOLITAIRE_ICON, SOLITAIRE_TITLE, openMinesweeper, openSolitaire } from './Games.tsx';
import { clock, shortDateTime, toMs } from './format.ts';
import { NowContext } from './hooks.ts';
import { AttentionCard } from './Inbox.tsx';
import { IE_ICON, IE_KEY, IE_TITLE, openInternetExplorer } from './InternetExplorer.tsx';
import { setLightMode, useLightMode } from './lightMode.ts';
import { LoginTray } from './LoginTray.tsx';
import { MarkdownPreview } from './Markdown.tsx';
import { ReplyArea, asksOf } from './Messaging.tsx';
import { notificationPermission } from './notify.ts';
import { setRateWeekdays, useRateWeekdays } from './rateWeekdays.ts';
import { DOT_RADIUS, FIVE_HOURS_MS, GRAPH_HEIGHT, GRAPH_WIDTH, PLOT, RING_STROKE, SEVEN_DAYS_MS, type RateGraphShape, paceOf, rateGraphShape } from './rateGraph.ts';
import { useLiveReload } from './reload.ts';
import { href } from './router.ts';
import { SKINS, setSkin, useSkin, type Skin } from './skin.ts';
import { EDIT_SHORTCUT, SEARCH_SHORTCUT, shortcutLabel } from './shortcut.ts';
import { Empty, Loading, Meter, Time, paceColor, rateColor } from './ui.tsx';
import { openWallpaper } from './Wallpaper.tsx';
import { XpDropdown } from './XpMenu.tsx';

// 使用率の数字を、四角の左上の角から離す距離。
const VALUE_INSET = 5;

/** 今の枠の使用率の推移。破線は、枠の始まりで 0%、リセットで 100% になる均等なペース。左上に今の使用率を出す。 */
const RateGraph = ({ label, limit, shape, weekdays }: { label: string; limit: RateLimit; shape: RateGraphShape; weekdays?: boolean }) => {
  const used = Math.round(limit.usedPercent);
  const pace = Math.round(shape.pace);
  const gap = used - pace;
  const versus = gap === 0 ? 'と同じ' : `より ${Math.abs(gap)} ポイント${gap > 0 ? '高い' : '低い'}`;
  return (
    <svg
      class="rate-graph" width={GRAPH_WIDTH} height={GRAPH_HEIGHT} viewBox={`0 0 ${GRAPH_WIDTH} ${GRAPH_HEIGHT}`} role="img" style={`--c: ${rateColor(limit.usedPercent)}`}
      aria-label={`${label}の使用率の推移。今は ${used}% で、均等なペースの ${pace}% ${versus}`}
    >
      <title>{`破線は、枠の始まりで 0%、リセットで 100% になる均等なペース${weekdays ? '。背景の色が違う土日祝日は、使わない前提で数えない' : ''}`}</title>
      {shape.off.map((o) => <rect key={o.x} class="rate-graph-off" x={o.x} y={PLOT.top} width={o.width} height={PLOT.bottom - PLOT.top} />)}
      <rect class="rate-graph-frame" x={PLOT.left} y={PLOT.top} width={PLOT.right - PLOT.left} height={PLOT.bottom - PLOT.top} />
      <path class="rate-graph-pace" d={shape.pacePath} />
      <path class="rate-graph-line" d={shape.path} />
      <circle class="rate-graph-dot" cx={shape.dot.x} cy={shape.dot.y} r={DOT_RADIUS} stroke-width={RING_STROKE} />
      <text class="rate-graph-value" x={PLOT.left + VALUE_INSET} y={PLOT.top + VALUE_INSET}>{used}%</text>
    </svg>
  );
};

/** 枠の長さと、均等なペースを平日だけで数えるか。7 日の枠を平日だけで数えるかは、使用制限のアイコンのメニューで切り替える。 */
type RateWindow = { label: string; windowMs: number; weekdays?: boolean };
const FIVE_HOURS: RateWindow = { label: '5 時間', windowMs: FIVE_HOURS_MS };
const SEVEN_DAYS: RateWindow = { label: '7 日', windowMs: SEVEN_DAYS_MS };

/** 使用ペース。リセットの時刻が分からない値は、枠の始まりが決まらないので出せない。 */
const paceIn = (limit: RateLimit, { windowMs, weekdays }: RateWindow, now: number) => {
  const shape = rateGraphShape(limit, windowMs, now, weekdays);
  return { shape, pace: shape && paceOf(limit.usedPercent, shape.pace) };
};

/** バーは使用ペースで、半分が 100%（リセットでちょうど使い切る）。リセットの時刻が分からず、ペースを出せない値は、使用率の数字を出す。 */
const RateMeter = ({ limit, rateWindow }: { limit: RateLimit; rateWindow: RateWindow }) => {
  const { label, weekdays } = rateWindow;
  const now = useContext(NowContext);
  const { shape, pace } = paceIn(limit, rateWindow, now);
  const reset = limit.resetsAt ? shortDateTime(toMs(limit.resetsAt), now) : undefined;
  const title = pace === undefined ? `${label}の使用率` : `${label}の使用ペースは ${Math.round(pace)}%。100% なら、今のペースのままリセットでちょうど使い切る。バーの半分が 100%`;
  return (
    <div class="rate-item">
      <div class="rate" title={reset ? `${title}。${reset} にリセット` : title}>
        <span class="rate-label">{label}</span>
        {pace === undefined ? <span class="rate-value">{Math.round(limit.usedPercent)}%</span> : <Meter percent={pace / 2} color={paceColor(pace)} label={`${label}の使用ペース（半分が 100%）`} />}
        {reset && <span class="rate-reset">{reset} リセット</span>}
      </div>
      {shape && <RateGraph label={label} limit={limit} shape={shape} weekdays={weekdays} />}
    </div>
  );
};

const RATE_ID = 'rate-flyout';
const RATE_MENU_ID = 'rate-menu';
/** 外周は線のリング。内周は線幅を半径の 2 倍にして中心まで塗り、扇形の円グラフにする。半径 5.5 の円で、外周の内側の縁 6.75 との間に隙間を残す。 */
const RINGS = { outer: { r: 8 }, inner: { r: 2.75, width: 5.5 } } as const;

/** 1 本のリング。pathLength を 100 にして、使用率をそのまま線の長さにする。12 時から時計回りに伸ばす。色は使用ペースで決め、ペースを出せない値は使用率で決める。 */
const Ring = ({ r, width, limit, rateWindow }: { r: number; width?: number; limit?: RateLimit; rateWindow: RateWindow }) => {
  const now = useContext(NowContext);
  const pace = limit && paceIn(limit, rateWindow, now).pace;
  return (
    <>
      <circle class="rate-ring-track" cx="10" cy="10" r={r} stroke-width={width} />
      {limit && (
        <circle
          cx="10" cy="10" r={r} stroke-width={width} pathLength={100} transform="rotate(-90 10 10)"
          style={`stroke: ${pace === undefined ? rateColor(limit.usedPercent) : paceColor(pace)}`} stroke-dasharray={`${Math.min(100, Math.max(0, limit.usedPercent))} 100`}
        />
      )}
    </>
  );
};

/**
 * 通知領域に常駐する使用制限のアイコン。外周のリングが 5 時間、内側の円グラフが 7 日の使用率で、色はそれぞれの使用ペースで変わる。マウスを載せるかフォーカスすると内訳を開く。
 * 押すと、XP のアプリのメニューと同じ形で、ペースの数え方を切り替えるメニューを開く。内訳と重ならないよう、内訳は閉じる。
 * タスクバーは overflow: hidden なので、内訳は popover にして最前面に出す。
 * 値がまだ無いときも空のリングを残し、内訳で出し方を案内する。消すと、出せることに気づけない。
 * loading は、スナップショットがまだ届いていないこと。値が無いとは限らないので、案内の代わりにスケルトンを出す。
 */
const RateTray = ({ fiveHour, sevenDay, loading }: { fiveHour?: RateLimit; sevenDay?: RateLimit; loading: boolean }) => {
  const empty = !fiveHour && !sevenDay;
  const weekdays = useRateWeekdays();
  const sevenDays = { ...SEVEN_DAYS, weekdays };
  const flyout = () => document.getElementById(RATE_ID);
  const show = () => flyout()?.showPopover();
  const hide = () => flyout()?.hidePopover();
  // メニューの項目を押すと、メニューを閉じる処理の中でフォーカスがアイコンに戻る。その場で開くと popover の操作が重なって例外になるので、閉じ終えてから開く。
  const showLater = () => queueMicrotask(show);
  const summary = [fiveHour && `5 時間 ${Math.round(fiveHour.usedPercent)}%`, sevenDay && `7 日 ${Math.round(sevenDay.usedPercent)}%`].filter(Boolean).join('、');
  return (
    <>
      <button class="rate-tray" aria-label={`使用制限: ${loading ? '読み込み中' : empty ? 'まだ値がありません' : summary}`} onPointerEnter={show} onPointerLeave={hide} onFocus={showLater} onBlur={hide} onClick={hide} popovertarget={RATE_MENU_ID}>
        <svg viewBox="0 0 20 20" aria-hidden="true">
          <Ring {...RINGS.outer} limit={fiveHour} rateWindow={FIVE_HOURS} />
          <Ring {...RINGS.inner} limit={sevenDay} rateWindow={sevenDays} />
        </svg>
      </button>
      <div id={RATE_ID} class="rate-flyout" popover="manual">
        <b class="rate-head">使用制限</b>
        {fiveHour && <RateMeter limit={fiveHour} rateWindow={FIVE_HOURS} />}
        {sevenDay && <RateMeter limit={sevenDay} rateWindow={sevenDays} />}
        {loading ? <Loading /> : empty && (
          <p class="rate-empty">
            まだ値がありません。<code>pnpm install-statusline</code> で statusline のラッパーを登録し、Claude Code のセッションでやり取りすると出ます。
          </p>
        )}
      </div>
      <XpDropdown id={RATE_MENU_ID} items={[{ label: '7 日のペースで土日祝日を除く(W)', checked: weekdays, onClick: () => setRateWeekdays(!weekdays) }]} />
    </>
  );
};

const LAST_MESSAGE_CHARS = 400;

/** 件数から開く一覧の 1 つのセッション。最後の発言と要対応を見ながら、その場で回答や返信を送れる。回答待ちの要対応は最後の発言と同じなので、発言だけを出す。 */
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
 * 作業中と回答待ちの件数。押すと、その状態のセッションの一覧を popover で開く。入力待ちのうち、実行中と待機中のセッションは数えない。
 * 開いている間は開いたときのセッションを並べ続け、状態が変わっても書きかけの返信を消さない。返答の候補を作らせないよう、閉じている間は描かない。
 */
const StatusCount = ({ status, label, snapshot, live, tray }: StatusCountProps) => {
  const [ids, setIds] = useState<string[]>([]);
  const sessions = live.filter((s) => s.status === status && (status === 'busy' || s.waitingFor === 'human'));
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

const MENU_ID = 'app-menu';

type MenuProps = { onSearch: () => void; onEdit?: () => void };

/**
 * 名前の左のボタンで開くメニュー。外を押すか Esc で閉じるよう、popover で開く。
 * リンクは popovertarget で閉じられないので、項目を押したら、ここでまとめて閉じる。
 */
const AppMenu = ({ onSearch, onEdit, docs, autoContinue }: MenuProps & { docs: DocMeta[]; autoContinue: boolean }) => {
  const light = useLightMode();
  const skin = useSkin();
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
  // Internet Explorer は先頭に固定し、エクスプローラーはお気に入りのフォルダーを選べる「マイ コンピュータ」に置くので、ほかのアプリとは並べない。
  const apps = useApps().filter((a) => a.kind !== EXPLORER_KEY && a.kind !== IE_KEY);
  // XP の「最近使ったファイル」と同じく、新しい順に 15 件まで出す。
  const recent = [...docs].sort((a, b) => b.createdAt - a.createdAt).slice(0, 15);
  const favorites = useFavorites();
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
        {/* Windows XP のスタートメニューに倣い、左にプログラム、右に場所と設定を並べる。XP 以外のスキンでは縦に積む。 */}
        <div class="app-menu-items">
          <div class="app-menu-col">
            <button class="app-menu-item app-menu-pinned" onClick={() => openInternetExplorer()}>
              <AppIcon icon={IE_ICON} size={32} />
              <span><b>インターネット</b><small>{IE_TITLE}</small></span>
            </button>
            {apps.length > 0 && <hr class="app-menu-sep" />}
            {apps.map((a) => (
              <button key={a.kind} class="app-menu-item" onClick={a.launch}><AppIcon icon={a.icon} />{a.title}</button>
            ))}
            <hr class="app-menu-sep app-menu-all-sep" />
            {/* マウスを載せるかフォーカスすると右に選択肢を出す。押してもメニューを閉じないよう、button にしない。 */}
            <div class="app-menu-sub">
              <div class="app-menu-item app-menu-all" tabindex={0} aria-haspopup="true"><span>すべてのプログラム(<u>P</u>)</span><span class="app-menu-arrow" aria-hidden="true">▶</span></div>
              <div class="app-menu-submenu" role="group" aria-label="すべてのプログラム">
                <button class="app-menu-item" onClick={() => openInternetExplorer()}><AppIcon icon={IE_ICON} />{IE_TITLE}</button>
                <button class="app-menu-item" title="ホームディレクトリを開く" onClick={() => openExplorer()}><AppIcon icon={EXPLORER_ICON} />{EXPLORER_TITLE}</button>
                {apps.map((a) => (
                  <button key={a.kind} class="app-menu-item" onClick={a.launch}><AppIcon icon={a.icon} />{a.title}</button>
                ))}
                <div class="app-menu-sub">
                  <div class="app-menu-item" tabindex={0} aria-haspopup="true">ゲーム<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
                  <div class="app-menu-submenu" role="group" aria-label="ゲーム">
                    <button class="app-menu-item" onClick={openSolitaire}><AppIcon icon={SOLITAIRE_ICON} />{SOLITAIRE_TITLE}</button>
                    <button class="app-menu-item" onClick={openMinesweeper}><AppIcon icon={MINESWEEPER_ICON} />{MINESWEEPER_TITLE}</button>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <div class="app-menu-col app-menu-places">
            <a class="app-menu-item" href={href.docs()}><AppIcon icon="📁" />マイ ドキュメント</a>
            <div class="app-menu-sub">
              <div class="app-menu-item" tabindex={0} aria-haspopup="true"><AppIcon icon="🕘" /><span>最近使ったファイル(<u>D</u>)</span><span class="app-menu-arrow" aria-hidden="true">▶</span></div>
              <div class="app-menu-submenu" role="group" aria-label="最近使ったファイル">
                {recent.map((d) => (
                  <a key={d.id} class="app-menu-item" href={href.doc(d.id)}>{d.title}</a>
                ))}
                {recent.length === 0 && <p class="app-menu-note">(なし)</p>}
              </div>
            </div>
            <div class="app-menu-sub">
              <div class="app-menu-item" tabindex={0} aria-haspopup="true"><AppIcon icon={EXPLORER_ICON} />マイ コンピュータ<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
              <div class="app-menu-submenu" role="group" aria-label="マイ コンピュータ">
                <button class="app-menu-item" title="ホームディレクトリを開く" onClick={() => openExplorer()}><AppIcon icon="🏠" />ホーム</button>
                <hr class="app-menu-sep" />
                {favorites.map((p) => (
                  <button key={p} class="app-menu-item" title={p} onClick={() => openExplorer(p)}><AppIcon icon="📁" />{baseName(p)}</button>
                ))}
                {favorites.length === 0 && <p class="app-menu-note">エクスプローラーの ☆ で、フォルダーをお気に入りに登録できます。</p>}
              </div>
            </div>
            <hr class="app-menu-sep" />
            <div class="app-menu-sub">
              <div class="app-menu-item" tabindex={0} aria-haspopup="true"><AppIcon icon="🖥️" /><span>コントロール パネル(<u>C</u>)</span><span class="app-menu-arrow" aria-hidden="true">▶</span></div>
              <div class="app-menu-submenu" role="group" aria-label="コントロール パネル">
                <button class="app-menu-item" title="Windows XP のスキンの壁紙を、画像のファイルか URL で変える" onClick={openWallpaper}>壁紙</button>
                <div class="app-menu-sub">
                  <div class="app-menu-item" tabindex={0} aria-haspopup="true">スキン<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
                  <div class="app-menu-submenu" role="group" aria-label="スキン">
                    {Object.entries(SKINS).map(([id, label]) => (
                      <button key={id} class="app-menu-item" aria-pressed={skin === id} onClick={() => setSkin(id as Skin)}>{label}</button>
                    ))}
                  </div>
                </div>
              </div>
            </div>
            <div class="app-menu-sub">
              <div class="app-menu-item" tabindex={0} aria-haspopup="true"><AppIcon icon="🗂️" />ダッシュボード<span class="app-menu-arrow" aria-hidden="true">▶</span></div>
              <div class="app-menu-submenu" role="group" aria-label="ダッシュボード">
                {dashboards.list.map((d) => (
                  <button key={d.id} class="app-menu-item" aria-pressed={d.id === dashboards.current} onClick={() => (switchDashboard(d.id), showHome())}>{d.name}</button>
                ))}
                <hr class="app-menu-sep" />
                {onEdit && (
                  <button class="app-menu-item" onClick={onEdit}>
                    編集<kbd>{shortcutLabel(EDIT_SHORTCUT)}</kbd>
                  </button>
                )}
                <button class="app-menu-item" onClick={() => (createDashboard(), showHome())}>新規作成</button>
                <button class="app-menu-item" onClick={() => (duplicateDashboard(), showHome())}>複製</button>
                <button class="app-menu-item" onClick={rename}>名前を変更</button>
              </div>
            </div>
            <hr class="app-menu-sep" />
            <button class="app-menu-item" title="セッション、要対応、資料、会話ログを探す" onClick={onSearch}>
              <AppIcon icon="🔍" /><span>検索(<u>S</u>)</span><kbd>{shortcutLabel(SEARCH_SHORTCUT)}</kbd>
            </button>
          </div>
        </div>
        {/* XP の「ログオフ」「終了オプション」の位置に、画面全体の切り替えと、自動継続モードを置く。 */}
        <div class="app-menu-foot">
          <button class="app-menu-item" aria-pressed={fullscreen} onClick={toggleFullscreen}>
            <svg class="app-menu-foot-icon is-orange" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 6V3h3M10 3h3v3M13 10v3h-3M6 13H3v-3" /></svg>全画面
          </button>
          <button class="app-menu-item" aria-pressed={light} title="通信と描画を減らす。更新は 3 秒おきになり、アニメーションとイルカが止まる。タブが裏にある間は更新も通知も止まる" onClick={() => setLightMode(!light)}>
            <svg class="app-menu-foot-icon is-red" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 13C3 7 7 3 13 3c0 6-4 10-10 10zM3 13l6-6" /></svg>軽量モード
          </button>
          <button class="app-menu-item" aria-pressed={autoContinue} title="スリープや通信の途切れで止まったセッションを、確認のダイアログを出さずに、API に届くようになったら続けさせる。Claude のログインが切れて止まったセッションも、ログインが戻ったら続けさせる" onClick={() => void attempt('自動継続モードの切り替え', () => api.autoContinue(!autoContinue))}>
            <svg class="app-menu-foot-icon is-green" viewBox="0 0 16 16" aria-hidden="true"><path d="M5 3.5v9l7-4.5z" /></svg>自動継続モード
          </button>
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
 * Windows では、メモリプレッシャーの代わりに物理メモリの使用率を出す。
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
  const percent = Math.round(memory.percent);
  const title = memory.kind === 'pressure' ? 'メモリプレッシャー' : 'メモリ使用率';
  const summary = `${title}: ${label}（${percent}%）`;
  const rows: [name: string, bytes: number, sub?: boolean][] =
    memory.kind === 'pressure'
      ? [
          ['物理メモリ', memory.total],
          ['使用済みメモリ', memory.app + memory.wired + memory.compressed],
          ['App メモリ', memory.app, true],
          ['確保されているメモリ', memory.wired, true],
          ['圧縮', memory.compressed, true],
          ['キャッシュされたファイル', memory.cached],
          ['スワップ使用領域', memory.swapUsed],
        ]
      : [
          ['物理メモリ', memory.total],
          ['使用中', memory.used],
          ['利用可能', memory.available],
        ];
  return (
    <>
      <button class="memory-tray" popovertarget={MEMORY_ID} title={summary} aria-label={summary} style={`--c: ${color}; --p: ${percent}%`} />
      <div id={MEMORY_ID} class="memory-flyout" popover>
        <b class="memory-head">{title}</b>
        <div class="memory-level">
          {label}
          <Meter percent={percent} color={color} label={title} />
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
      <AppMenu onSearch={onSearch} onEdit={onEdit} docs={snapshot?.docs ?? []} autoContinue={!!snapshot?.autoContinue} />
      <Brand home={home} />
      {/* 浮かべたウィンドウのボタンは、Float.tsx の FloatFrame がここへ置く。 */}
      <div class="taskbar-windows" />
      {/* スナップショットが届くまでは件数が分からないので、0 件と出さないよう件数ごと出さない。 */}
      {snapshot && (
        <div class="counts">
          <StatusCount status="busy" label="作業中" snapshot={snapshot} live={live} />
          <StatusCount status="idle" label="回答待ち" snapshot={snapshot} live={live} />
        </div>
      )}
      <div class="topbar-end">
        {!connected && <span class="chip chip-bad">サーバーに接続できません</span>}
        {stale && (
          <button class="btn btn-sm btn-primary" title="書きかけの内容は消えます" onClick={() => location.reload()}>
            新しい画面を読み込む
          </button>
        )}
        <NotifyButton />
        <StatusCount status="idle" label="回答待ち" snapshot={snapshot} live={live} tray />
        <FeedbackTray improve={snapshot?.improve} onClick={onFeedback} />
        <LoginTray login={snapshot?.claudeLogin} />
        <RateTray fiveHour={rate?.fiveHour} sevenDay={rate?.sevenDay} loading={!snapshot} />
        <MemoryTray />
        <TaskbarClock />
      </div>
    </header>
  );
};
