import type { RefObject } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { AssistantMessage, AssistantReply } from '../shared/types.ts';
import { api, attempt } from './api.ts';
import type { Pose, createDolphin } from './dolphin3d.ts';
import { useFloats } from './Float.tsx';
import { MINESWEEPER_KEY, askMinesHint, canHint } from './Games.tsx';
import { type Fling, glide, pushSample, releaseVelocity, type Sample, type Wall, windUp } from './fling.ts';
import { Markdown } from './Markdown.tsx';
import { currentSessionId } from './router.ts';
import { onSubmitKey } from './ui.tsx';

const W = 160;
const H = 120;
const MARGIN = 8;
const SPEED = 70; // px/s。目で追えるくらいのゆっくりした速さ
const DRAG_THRESHOLD = 4;
const REST_CHANCE = 0.35;
// ときどき宙返りかバレルロールをする。平均の間隔と、1 回転にかける時間（秒）。
const TRICK_EVERY_S = 20;
const TRICK_S = 1.5;
// 宙返りで描く円の半径（px）。体の長さくらいにしないと、その場で回っているように見える。
const LOOP_R = 50;
// 飛ばされた後に泳ぎへ戻るとき、進んでいた向きのこの距離（px）だけ先を次の目的地にする。向きを急に変えないため。
const CARRY_ON = 240;
// 尾を振る速さの上限。飛んでいる間の速さにそのまま比例させると、尾が速すぎて震えて見える。
const MAX_STROKE = 4;
// 飛ばして壁に当たったとき、口先から壁に突き刺さる確率と、刺さるのに要る当たったときの速さ（px/s）。軽く当てただけでは刺さらない。
const STICK_CHANCE = 0.1;
const STICK_SPEED = 300;
// 口先が壁へ向いている度合い（壁に垂直なら 1）がこれより小さければ刺さらない。横腹や尾から当たったときは跳ね返る。
const STICK_FACING = 0.5;
// 壁にめり込んでいく速さの下限（px/s）。めり込むほど遅くなるが、止まりきらずに奥まで届かせる。
const SINK_MIN_SPEED = 40;
// 刺さっている間に尾を打つ速さと、抜け出そうとして体をよじる角度（rad）と速さ（回/s）。
const STUCK_STROKE = 5.25;
const STUCK_WRIGGLE = 0.08;
const STUCK_WRIGGLE_HZ = 2.3;
// 刺さっている間、壁から出ている半身の真ん中が、体の真ん中から尾の側へ離れている道のり（px）。吹き出しの尾をここへ向ける。
const STUCK_SHOWN = 24;
// 吹き出しの尾を、吹き出しの角からこれより内側に置く。角の丸みにかかると、尾が浮いて見える。
const TAIL_INSET = 16;

type Turn = AssistantMessage & { links?: AssistantReply['links'] };

/**
 * 突き刺さった壁と、刺さった向き。at は、刺さりきったときの壁に沿った位置（px）で、左右の壁なら y、上下の壁なら x。
 * ux, uy は口先の向き（画面上の単位ベクトル）。depth は当たってから刺さりきるまでに進む道のり（px）、left はその残り、speed は当たったときの速さ。
 * since は刺さりきった時刻で、めり込んでいる途中は無い。
 */
type Stuck = { wall: Wall; at: number; yaw: number; pitch: number; ux: number; uy: number; depth: number; left: number; speed: number; since?: number };

/**
 * 位置と速さ（px、px/s）と向きと、している技とその進み具合（0〜1）。毎フレーム変わるので、描き直しを起こさない ref に持つ。
 * fling は、勢いよく離されて慣性で飛んでいる間の速さと減速の強さ。stuck は、壁に突き刺さっている間の刺さった場所。
 */
type Motion = { x: number; y: number; vx: number; vy: number; tx: number; ty: number; restUntil: number; speed: number; yaw: number; pitch: number; trick?: 'flip' | 'roll'; trickT: number; fling?: Fling; stuck?: Stuck };

/** windUp は、引っぱったまま振り回して勢いをつけた道のり（px）。 */
type Drag = { dx: number; dy: number; x: number; y: number; moved: boolean; samples: Sample[]; windUp: number };

/** 毎フレームの処理が読む画面の状態。描き直しを待たずに読めるよう ref に持つ。 */
type Live = { hover: boolean; open: boolean; thinking: boolean; drag?: Drag };

const clamp = (v: number, min: number, max: number): number => Math.min(Math.max(v, min), Math.max(min, max));

/** 技の回転角。宙返りは円を描くよう一定の速さで回り、バレルロールは回り始めと終わりをゆっくりにする。 */
const spin = (m: Motion, trick: Motion['trick']): number => {
  if (m.trick !== trick) return 0;
  return trick === 'flip' ? 2 * Math.PI * m.trickT : Math.PI * (1 - Math.cos(Math.PI * m.trickT));
};

/**
 * 目的地へ向かって泳ぎ、着いたら次の目的地を選ぶ。ときどき休む。held の間は止まって人間の方を向く。
 * 進む向きは体の向き（技の回転を含む）から決めるので、横向きのまま上下へ滑ったり、宙返りの間に真横へ進んだりしない。
 */
const swim = (m: Motion, dt: number, now: number, held: boolean): void => {
  const maxX = innerWidth - W;
  const maxY = innerHeight - H;
  const resting = held || now < m.restUntil;
  m.tx = clamp(m.tx, 0, maxX);
  m.ty = clamp(m.ty, 0, maxY);
  const dx = m.tx - m.x;
  const dy = m.ty - m.y;
  if (!resting && Math.hypot(dx, dy) < 24) {
    m.tx = Math.random() * maxX;
    m.ty = Math.random() * maxY;
    if (Math.random() < REST_CHANCE) m.restUntil = now + 1500 + Math.random() * 2500;
  }
  // 真上や真下へ向かうときは、いま向いている側のまま頭を上げ下げする。
  const yaw = resting ? -Math.PI / 2 : dx < -8 ? -Math.PI : dx > 8 ? 0 : m.yaw < -Math.PI / 2 ? -Math.PI : 0;
  const pitch = resting ? 0 : Math.atan2(-dy, Math.abs(dx));
  const ease = Math.min(1, dt * 3);
  m.yaw += (yaw - m.yaw) * ease;
  m.pitch += (pitch - m.pitch) * ease;
  m.speed += ((resting ? 0 : SPEED) - m.speed) * Math.min(1, dt * (resting ? 3 : 1.2));
  // 画面に映る口先の向きへ進む。向きを変えている途中は手前か奥を向くので、横へはほとんど進まない。
  // 宙返りの間は、1 回転で半径 LOOP_R の円周を進む速さに上げる。
  const p = m.pitch + spin(m, 'flip');
  const speed = m.trick === 'flip' ? (2 * Math.PI * LOOP_R) / TRICK_S : m.speed;
  m.vx = speed * Math.cos(m.yaw) * Math.cos(p);
  m.vy = -speed * Math.sin(p);
  m.x = clamp(m.x + m.vx * dt, 0, maxX);
  m.y = clamp(m.y + m.vy * dt, 0, maxY);
};

/**
 * 勢いよく離されたら、慣性で滑りながら、進む向きへ体を向ける。泳ぐ速さまで落ちたら、そのまま前へ泳ぎ続ける。
 * 触られていても止めない。飛ばしたのは人間なので、カーソルが上に残っていても飛んでいく。
 */
const fly = (m: Motion, fling: Fling, dt: number, now: number): void => {
  const maxX = innerWidth - W;
  const maxY = innerHeight - H;
  const { wall, ...next } = glide(m.x, m.y, fling, dt, { maxX, maxY });
  const speed0 = Math.hypot(fling.vx, fling.vy);
  const stuck = wall && speed0 >= STICK_SPEED && Math.random() < STICK_CHANCE ? stickInto(m, wall, speed0) : undefined;
  if (stuck) {
    Object.assign(m, { fling: undefined, stuck });
    return;
  }
  Object.assign(m, next);
  m.fling = { vx: next.vx, vy: next.vy, drag: fling.drag };
  // 真上や真下へ飛ぶときは、いま向いている側のまま頭を上げ下げする。
  const yaw = next.vx < -8 ? -Math.PI : next.vx > 8 ? 0 : m.yaw < -Math.PI / 2 ? -Math.PI : 0;
  // 泳ぐときより速く向きを合わせる。飛んでいる間に向きが遅れると、横滑りして見える。
  const ease = Math.min(1, dt * 12);
  m.yaw += (yaw - m.yaw) * ease;
  m.pitch += (Math.atan2(-next.vy, Math.abs(next.vx)) - m.pitch) * ease;
  const speed = Math.hypot(next.vx, next.vy);
  if (speed > SPEED) return;
  m.fling = undefined;
  const ahead = speed > 0 ? CARRY_ON / speed : 0;
  Object.assign(m, { speed, restUntil: now, tx: clamp(m.x + next.vx * ahead, 0, maxX), ty: clamp(m.y + next.vy * ahead, 0, maxY) });
};

// 各々の壁へ入っていく向き。
const INTO: Record<Wall, [number, number]> = { left: [-1, 0], right: [1, 0], top: [0, -1], bottom: [0, 1] };

/** 刺さりきったときの位置。背びれは体の真ん中にあるので、canvas の真ん中を壁の線に重ねる。壁の向こうへはみ出した頭は画面の外なので見えない。 */
const sunkAt = ({ wall, at }: Stuck): { x: number; y: number } =>
  ({ left: { x: -W / 2, y: at }, right: { x: innerWidth - W / 2, y: at }, top: { x: at, y: -H / 2 }, bottom: { x: at, y: innerHeight - H / 2 } })[wall];

/**
 * 壁に当たったイルカを、当たったときの向きのまま、口先の向きへ背びれが壁の線に届くまでめり込ませる刺さり方。
 * 口先が壁へ向いていないか、刺さりきる場所が画面の外になるなら刺さらない。
 */
const stickInto = (m: Motion, wall: Wall, speed: number): Stuck | undefined => {
  const ux = Math.cos(m.yaw) * Math.cos(m.pitch);
  const uy = -Math.sin(m.pitch);
  const [nx, ny] = INTO[wall];
  const facing = ux * nx + uy * ny;
  if (facing < STICK_FACING) return undefined;
  const gap = { left: m.x + W / 2, right: innerWidth - m.x - W / 2, top: m.y + H / 2, bottom: innerHeight - m.y - H / 2 }[wall];
  const depth = Math.max(0, gap) / facing;
  const side = wall === 'left' || wall === 'right';
  const at = side ? m.y + uy * depth : m.x + ux * depth;
  if (at < (side ? -H / 2 : -W / 2) || at > (side ? innerHeight - H / 2 : innerWidth - W / 2)) return undefined;
  return { wall, at, yaw: m.yaw, pitch: m.pitch, ux, uy, depth, left: depth, speed };
};

/**
 * 壁にめり込み、刺さりきったら、背びれのあたりまで埋まったまま尾を打って体をよじる。
 * めり込む間は、当たった速さから一定に減速して、刺さりきるところで止まる。
 */
const wriggle = (m: Motion, stuck: Stuck, dt: number, now: number): void => {
  if (stuck.left > 0) {
    const v = Math.max(SINK_MIN_SPEED, stuck.speed * Math.sqrt(stuck.left / stuck.depth));
    stuck.left = Math.max(0, stuck.left - v * dt);
    if (stuck.left === 0) stuck.since = now;
  }
  const end = sunkAt(stuck);
  const twist = stuck.since === undefined ? 0 : STUCK_WRIGGLE * Math.sin((2 * Math.PI * STUCK_WRIGGLE_HZ * (now - stuck.since)) / 1000);
  Object.assign(m, { x: end.x - stuck.ux * stuck.left, y: end.y - stuck.uy * stuck.left, yaw: stuck.yaw, pitch: stuck.pitch + twist, vx: 0, vy: 0, speed: 0, trick: undefined });
};

/**
 * 横へ泳いでいる間だけ、ときどき技を始める。始めた技は触られても最後まで回る。
 * 横向きに限るのは、正面向きや向きを変えている途中では、回る軸が体の軸からずれて見えるため。
 * 宙返りは、上へ描く円が画面に収まるときだけ始める。画面の端に当たると円がつぶれるため。
 */
const play = (m: Motion, dt: number, free: boolean): void => {
  if (m.trick) {
    m.trickT += dt / TRICK_S;
    if (m.trickT >= 1) m.trick = undefined;
  } else if (free && Math.abs(m.vx) > SPEED / 2 && Math.random() < dt / TRICK_EVERY_S) {
    m.trick = Math.random() < 0.5 && m.y > 2 * LOOP_R && m.x > LOOP_R && m.x < innerWidth - W - LOOP_R ? 'flip' : 'roll';
    m.trickT = 0;
  }
};

/**
 * 吹き出しを、イルカの上（入らなければ下）に、画面からはみ出さないよう置く。尾はイルカを指す。
 * 壁に刺さっている間は、体の真ん中が壁の線にあり頭は画面の外なので、壁から出ている半身を指す。
 */
const placeChat = (chat: HTMLElement, m: Motion): void => {
  const center = clamp(m.x + W / 2 - (m.stuck ? m.stuck.ux * STUCK_SHOWN : 0), MARGIN, innerWidth - MARGIN);
  const left = clamp(center - chat.offsetWidth / 2, MARGIN, innerWidth - MARGIN - chat.offsetWidth);
  chat.style.left = `${left - m.x}px`;
  chat.style.setProperty('--tail', `${clamp(center - left, TAIL_INSET, chat.offsetWidth - TAIL_INSET)}px`);
  chat.classList.toggle('is-below', m.y < chat.offsetHeight + MARGIN * 2);
};

/** 毎フレーム、泳がせて向きを変え、イルカと吹き出しを動かして描く。止める関数を返す。 */
const animate = (create: typeof createDolphin, canvas: HTMLCanvasElement, box: HTMLElement, chat: RefObject<HTMLElement>, m: Motion, live: Live): (() => void) => {
  let view: ReturnType<typeof createDolphin>;
  try {
    view = create(canvas, W, H);
  } catch {
    // GPU が使えないなどで WebGL を使えないブラウザでは、描けないのでイルカを出さない。
    box.hidden = true;
    return () => {};
  }
  // 動きを減らす設定（prefers-reduced-motion）は見ない。Windows でアニメーション効果を切っているだけでも止まり、壊れて見えるため。
  Object.assign(m, { tx: m.x, ty: m.y, restUntil: performance.now() + 2000 });
  let last = performance.now();
  let frame = 0;
  const loop = (now: number) => {
    const dt = Math.min(0.05, (now - last) / 1000);
    last = now;
    const dragging = !!live.drag?.moved;
    const stopped = live.hover || live.open || live.thinking;
    if (m.stuck) wriggle(m, m.stuck, dt, now);
    else {
      play(m, dt, !stopped && !dragging && !m.fling);
      if (m.fling) fly(m, m.fling, dt, now);
      else if (!dragging) swim(m, dt, now, stopped);
    }
    box.style.transform = `translate(${m.x}px, ${m.y}px)`;
    if (chat.current) placeChat(chat.current, m);
    const moving = Math.min(MAX_STROKE, 0.6 + Math.hypot(m.vx, m.vy) / SPEED);
    const stroke: Pose['stroke'] = m.stuck ? STUCK_STROKE : m.fling ? moving : live.thinking || dragging ? 2.5 : stopped ? 0.4 : moving;
    view.draw(dt, { yaw: m.yaw, pitch: m.pitch + spin(m, 'flip'), roll: spin(m, 'roll'), stroke });
    frame = requestAnimationFrame(loop);
  };
  frame = requestAnimationFrame(loop);
  return () => {
    cancelAnimationFrame(frame);
    view.dispose();
  };
};

/**
 * 押して離せば吹き出しを開け閉めし、引っぱればイルカを動かす。勢いよく離せば、その速さで飛ばす。離す前に長く振り回すほど、遠くまで飛ぶ。
 * 飛ばして壁に当てると、まれに壁に突き刺さり、引っぱるまで抜けない。
 */
const pointerHandlers = (m: Motion, live: Live, toggle: () => void) => ({
  onPointerDown: (e: PointerEvent) => {
    // 飛んでいる途中でつかまれたら、そこで止める。
    m.fling = undefined;
    live.drag = { dx: e.clientX - m.x, dy: e.clientY - m.y, x: e.clientX, y: e.clientY, moved: false, samples: [], windUp: 0 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  },
  onPointerMove: (e: PointerEvent) => {
    const d = live.drag;
    if (!d || (!d.moved && Math.hypot(e.clientX - d.x, e.clientY - d.y) < DRAG_THRESHOLD)) return;
    d.moved = true;
    // 壁に刺さっていたら、引っぱると抜ける。
    m.stuck = undefined;
    const sample = { t: performance.now(), x: e.clientX, y: e.clientY };
    d.windUp = windUp(d.windUp, d.samples.at(-1), sample);
    d.samples = pushSample(d.samples, sample);
    const x = clamp(e.clientX - d.dx, 0, innerWidth - W);
    const y = clamp(e.clientY - d.dy, 0, innerHeight - H);
    // 引っぱる向きに体を向けるため、動いた量を速さとして残す。
    Object.assign(m, { vx: (x - m.x) * 30, vy: (y - m.y) * 30, x, y, tx: x, ty: y, restUntil: performance.now() + 1500 });
  },
  onPointerUp: () => {
    const d = live.drag;
    live.drag = undefined;
    if (!d) return;
    if (!d.moved) return toggle();
    const fling = releaseVelocity(d.samples, performance.now(), d.windUp);
    if (!fling) return;
    // 回っている技は止める。技の回転が残ると、進む向きを向けない。
    Object.assign(m, { fling, trick: undefined });
    // イルカがカーソルの下から飛び去っても、カーソルを動かすまで pointerleave は来ない。泳ぎに戻ったときに触られたままにしない。
    live.hover = false;
  },
  onPointerCancel: () => {
    live.drag = undefined;
  },
  onPointerEnter: () => {
    live.hover = true;
  },
  onPointerLeave: () => {
    live.hover = false;
  },
});

const DolphinSays = ({ turn: t }: { turn: Turn }) => (
  <div class="dolphin-say">
    <Markdown text={t.text} />
    {t.links && t.links.length > 0 && (
      <div class="actions">{t.links.map((l) => <a key={l.href} class="btn btn-sm" href={l.href}>{l.label}</a>)}</div>
    )}
  </div>
);

/** onHint は、マインスイーパのヒントを出せるときだけ渡す。 */
type ChatProps = { turns: Turn[]; thinking: boolean; onAsk: (text: string) => void; onHint?: () => void; onClose: () => void; chatRef: RefObject<HTMLElement> };

const DolphinChat = ({ turns, thinking, onAsk, onHint, onClose, chatRef }: ChatProps) => {
  const [text, setText] = useState('');
  const log = useRef<HTMLDivElement>(null);
  useEffect(() => log.current?.scrollTo({ top: log.current.scrollHeight }), [turns.length, thinking]);
  const ask = () => {
    if (thinking || !text.trim()) return;
    onAsk(text.trim());
    setText('');
  };
  return (
    <section class="card dolphin-chat" ref={chatRef} aria-label="イルカのアシスタント" onKeyDown={(e) => e.key === 'Escape' && onClose()}>
      <header class="dolphin-chat-head">
        <p class="dolphin-say">何について調べますか？</p>
        <button class="dolphin-close" aria-label="閉じる" onClick={onClose}>×</button>
      </header>
      <div class="dolphin-log" ref={log}>
        {turns.map((t, i) => (t.role === 'human' ? <p key={i} class="dolphin-you plain">{t.text}</p> : <DolphinSays key={i} turn={t} />))}
        {thinking && <p class="dolphin-say muted">考えています…</p>}
      </div>
      <textarea
        class="input"
        rows={2}
        autoFocus
        placeholder="お前を消す方法"
        value={text}
        onInput={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => onSubmitKey(e, ask)}
      />
      <div class="actions">
        {onHint && <button class="btn btn-sm" disabled={thinking} title="マインスイーパの各マスに地雷がある確率を Jev で推定し、高いほど赤く塗る" onClick={onHint}>ヒント</button>}
        <button class="btn btn-sm btn-primary" disabled={thinking || !text.trim()} onClick={ask}>聞く</button>
      </div>
    </section>
  );
};

/**
 * 画面を泳ぎ回るイルカ。ドラッグで動かせ、押すと吹き出しでダッシュボードのことを答える。
 * jev は Jev を使えるか。使えてマインスイーパを開いていれば、吹き出しからヒントを出せる。
 */
export const Dolphin = ({ jev }: { jev?: boolean }) => {
  const [open, setOpen] = useState(false);
  const [turns, setTurns] = useState<Turn[]>([]);
  const [thinking, setThinking] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const chat = useRef<HTMLElement>(null);
  const motion = useRef<Motion>({ x: innerWidth - W - 16, y: innerHeight - H - 52, vx: 0, vy: 0, tx: 0, ty: 0, restUntil: 0, speed: 0, yaw: 0, pitch: 0, trickT: 0 });
  const live = useRef<Live>({ hover: false, open, thinking });
  Object.assign(live.current, { open, thinking });
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    // 3D の描画は大きいので、最初の画面を出した後で読み込む。読み込めなければ、描けないのでイルカを出さない。
    import('./dolphin3d.ts').then(
      ({ createDolphin }) => {
        if (!gone && canvas.current && box.current) stop = animate(createDolphin, canvas.current, box.current, chat, motion.current, live.current);
      },
      () => {
        if (box.current) box.current.hidden = true;
      },
    );
    return () => {
      gone = true;
      stop?.();
    };
  }, []);

  const mines = useFloats().find((f) => f.key === MINESWEEPER_KEY);
  const toggle = () => setOpen((o) => !o);
  const ask = async (text: string) => {
    const next: Turn[] = [...turns, { role: 'human', text }];
    setTurns(next);
    setThinking(true);
    const res = await attempt('イルカへの質問', () => api.assistant(next.map(({ role, text: t }) => ({ role, text: t })), currentSessionId()));
    setThinking(false);
    if (res) setTurns([...next, { role: 'dolphin', text: res.answer, links: res.links }]);
  };
  const hint = async (f: NonNullable<typeof mines>) => {
    setThinking(true);
    const done = await attempt('マインスイーパのヒント', async () => {
      await askMinesHint(f);
      return true;
    });
    setThinking(false);
    if (done) setTurns((t) => [...t, { role: 'dolphin', text: 'マインスイーパのマスを、地雷がありそうなほど赤く塗りました。マスに乗せると確率が出ます。' }]);
  };

  return (
    <div class="dolphin" ref={box}>
      {open && <DolphinChat turns={turns} thinking={thinking} onAsk={ask} onHint={jev && mines && canHint(mines) ? () => hint(mines) : undefined} onClose={() => setOpen(false)} chatRef={chat} />}
      <canvas
        ref={canvas}
        class="dolphin-canvas"
        role="button"
        tabIndex={0}
        aria-label="イルカのアシスタント。押すと吹き出しが開きます。ドラッグで動かせます。"
        aria-expanded={open}
        {...pointerHandlers(motion.current, live.current, toggle)}
        onKeyDown={(e) => {
          if (e.key !== 'Enter' && e.key !== ' ') return;
          e.preventDefault();
          toggle();
        }}
      />
    </div>
  );
};
