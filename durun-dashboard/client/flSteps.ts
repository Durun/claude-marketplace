// FL Connect の導入で、次にすること。済んだ手順は出さず、まだのものだけを並べる。
import type { FlConnectView } from '../shared/types.ts';

export type FlAction = 'loopback' | 'install' | 'launch';

/**
 * 次にすることの 1 つ。warn は、済まないと FL とつながらないもの。info は、使いたいときだけするもの。
 * action はウィンドウに出すボタンで、disabled があれば、その理由を添えて押せなくする。
 */
export type FlStep = { key: string; tone: 'warn' | 'info'; title: string; text?: string; action?: { kind: FlAction; label: string; disabled?: string } };

/** スクリプトが無いか、版が違うときの説明。FL が古い入口を動かしているなら、FL で読み直してもらう。 */
const scriptText = (v: FlConnectView): string => {
  const { installed, bundled, bundledCore } = v.setup;
  const h = v.midi.hello;
  if (!installed) return `FL のユーザーデータに、FL Connect のスクリプトがありません。置くと、FL の MIDI の設定で Controller type に「FL Connect」を選べるようになります。`;
  if (installed !== bundled) return `FL のユーザーデータに置いてあるスクリプトは ${installed} 版で、ダッシュボードの ${bundled} 版と違います。`;
  if (h?.coreError) return `FL がスクリプトの中身（flconnect_core.py）を読めません。${h.coreError.split('\n').slice(-2).join(' ')}`;
  if (h && h.script !== bundled) return `FL は ${h.script} 版の入口を動かしています。置き直してから、FL の View > Script output で FL Connect の「Reload script」を押すか、FL を起動し直してください。`;
  return `FL はスクリプトの中身の ${h?.core ?? '?'} 版を動かしています（ダッシュボードは ${bundledCore} 版）。置き直すと、FL が読み直します。`;
};

const stale = (v: FlConnectView): boolean => {
  const h = v.midi.hello;
  return v.setup.installed !== v.setup.bundled || (!!h && (!!h.coreError || h.script !== v.setup.bundled || h.core !== v.setup.bundledCore));
};

export const nextSteps = (v: FlConnectView): FlStep[] => {
  const steps: FlStep[] = [];
  const h = v.midi.hello;
  // ① の道（MIDI スクリプト）
  if (v.midi.status === 'no-port') steps.push({ key: 'loopback', tone: 'warn', title: `MIDI のループバック「${v.midi.ports.output}」がありません`, text: v.midi.message, action: { kind: 'loopback', label: 'ループバックを作る' } });
  if (!v.setup.userData) steps.push({ key: 'user-data', tone: 'warn', title: 'FL のユーザーデータのフォルダーが見つかりません', text: '~/.durun-dashboard/.env.local の FL_USER_DATA に、FL の File settings の「User data folder」を書いてください。' });
  else if (stale(v)) steps.push({ key: 'install', tone: 'warn', title: v.setup.installed ? 'FL Connect のスクリプトの版が違います' : 'FL Connect のスクリプトがありません', text: scriptText(v), action: { kind: 'install', label: 'Install Script' } });
  if (v.midi.status !== 'no-port' && !h) {
    if (v.setup.flRunning === false) steps.push({ key: 'start-fl', tone: 'info', title: 'FL Studio が動いていません', text: 'FL Studio を起動すると、FL Connect のスクリプトとつながります。' });
    else steps.push({ key: 'midi', tone: 'warn', title: 'FL が FL Connect のスクリプトを動かしていません', text: v.midi.message });
  }
  // ② の道（Gopher MCP）は、使いたいときだけ。デバッグのポートを開く設定で起動した FL でしか使えない。
  if (v.gopher.status === 'connected' || v.gopher.status === 'busy') return steps;
  if (v.gopher.debug) steps.push({ key: 'gopher', tone: 'warn', title: 'Gopher MCP not connected', text: 'Alt+F1を押してください（FL で Gopher を開くと、つながります）。' });
  else
    steps.push({
      key: 'launch',
      tone: 'info',
      title: 'Gopher MCP（任意）',
      text: 'FL 2026 の Gopher が使う口を、エージェントにも使わせられます。デバッグのポートを開く設定で、FL を起動します。ポートが開いている間は、この PC のどのプロセスからも FL を操作できます。',
      action: { kind: 'launch', label: 'Gopher MCPを開放して起動', disabled: v.setup.flRunning ? 'FL Studio が動いています。保存して FL を閉じてから押してください。動いている FL には、デバッグのポートを開く設定を付けられません。' : undefined },
    });
  return steps;
};
