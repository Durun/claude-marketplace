// ダッシュボードを動かしている端末で、screen や tmux のセッションにアタッチする「ターミナル」。アタッチするコマンドは、サーバーの TERMINAL_ATTACH_COMMAND で決める。
// ダッシュボードが起動し直して接続が切れたら、つなぎ直してアタッチし直す。端末の中身は screen や tmux に残る。
import type { FitAddon } from '@xterm/addon-fit';
import type { Terminal } from '@xterm/xterm';
import { useEffect, useRef } from 'preact/hooks';
import { api } from './api.ts';
import { DesktopIcon } from './apps.tsx';
import { baseName } from './explorerFavorites.ts';
import { type Float, FloatFrame, dock, floatUp } from './Float.tsx';

/** ウィンドウの保存キーで、ダッシュボードに置くアプリの種類の名前でもある。タブの id は「terminal:開いた時刻」にし、いくつも開けるようにする。 */
export const TERMINAL_KEY = 'terminal';
export const TERMINAL_TITLE = 'ターミナル';
export const TERMINAL_ICON = '/terminal.svg';
const SIZE = { w: 760, h: 480 };

/** 新しいウィンドウで、cwd を作業ディレクトリにしてアタッチする。cwd を省くと、ホームディレクトリで動かす。 */
export const openTerminal = (cwd?: string): void =>
  floatUp(TERMINAL_KEY, '', { id: `${TERMINAL_KEY}:${Date.now()}`, settings: cwd ? { cwd } : undefined }, SIZE);

/** ダッシュボードに置くアイコン。セッションに置いたものは、そのセッションの作業ディレクトリで動かす。 */
export const TerminalIcon = ({ cwd }: { cwd?: string }) => (
  <DesktopIcon icon={TERMINAL_ICON} title={TERMINAL_TITLE} hint="screen や tmux のセッションにアタッチします。ダブルクリックで開きます" onLaunch={() => openTerminal(cwd)} />
);

/**
 * el に描いた端末を、サーバーがアタッチした端末につなぐ。返す関数で切り離す。
 * サーバーは接続ごとに ready で id を渡すので、入力と大きさの変更はその id へ送る。つなぎ直したら描いた中身を消し、アタッチし直した端末の描き直しに任せる。
 */
const connect = (el: HTMLElement, term: Terminal, fit: FitAddon, cwd?: string): (() => void) => {
  term.loadAddon(fit);
  term.open(el);
  fit.fit();
  let id: string | undefined;
  // 打った順に届くよう、1 つずつ送る。届かなかった分は、つなぎ直すまでの間に閉じた端末へ送ったものなので捨てる。
  let queue: Promise<unknown> = Promise.resolve();
  const post = (send: (id: string) => Promise<unknown>): void => {
    queue = queue.then(() => id && send(id)).catch(() => undefined);
  };
  const resize = () => post((t) => api.resizeTerminal(t, term.cols, term.rows));
  const source = new EventSource(api.terminalEventsUrl(cwd));
  source.addEventListener('ready', (e) => {
    id = JSON.parse(e.data) as string;
    term.reset();
    resize();
  });
  source.onmessage = (e) => term.write(JSON.parse(e.data) as string);
  source.addEventListener('exit', (e) => {
    source.close();
    id = undefined;
    term.write(`\r\n${JSON.parse(e.data) as string}\r\n`);
  });
  term.onData((data) => post((t) => api.terminalInput(t, data)));
  term.onResize(resize);
  const observer = new ResizeObserver(() => fit.fit());
  observer.observe(el);
  term.focus();
  return () => {
    observer.disconnect();
    source.close();
    term.dispose();
  };
};

export const TerminalFloat = ({ f }: { f: Float }) => {
  const el = useRef<HTMLDivElement>(null);
  const cwd = f.tab.settings?.cwd;
  useEffect(() => {
    let live = true;
    let stop: (() => void) | undefined;
    // xterm.js は大きいので、ターミナルを開くときだけ読み込む。
    void Promise.all([import('@xterm/xterm'), import('@xterm/addon-fit')]).then(([{ Terminal }, { FitAddon }]) => {
      const host = el.current;
      if (!live || !host) return;
      stop = connect(host, new Terminal({ fontFamily: getComputedStyle(host).getPropertyValue('--mono').trim(), fontSize: 13 }), new FitAddon(), cwd);
    });
    return () => {
      live = false;
      stop?.();
    };
  }, [cwd]);
  const close = { kind: 'close' as const, label: '閉じる', onClick: () => dock(f.key, f.scope, f.tab.id) };
  return (
    <FloatFrame f={f} title={cwd ? `${baseName(cwd)} - ${TERMINAL_TITLE}` : TERMINAL_TITLE} icon={TERMINAL_ICON} controls={[close]}>
      <div class="terminal-host" ref={el} />
    </FloatFrame>
  );
};
