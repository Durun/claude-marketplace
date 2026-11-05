// CD取り込みの導入で、次にすること。入っているものは出さず、無いツールだけを並べる。
import type { CdRipView, CdToolName } from '../shared/types.ts';

/** 次にすることの 1 つ。action があれば、ウィンドウに導入ボタンを出す。コマンドを画面から実行できないときは command を見せる。 */
export type CdStep = { key: string; tone: 'warn' | 'info'; title: string; text?: string; action?: { tool: CdToolName; label: string }; command?: string };

/** 取り込みに要るツールが無ければ警告、書き出しに要るツールが無ければ、書き出すまでに入れればよい案内にする。 */
const NEEDED_FOR_RIP: CdToolName[] = ['cyanrip'];

export const nextSteps = (v: CdRipView): CdStep[] => {
  const steps: CdStep[] = [];
  // metaflac は flac と、ffprobe は ffmpeg と同じパッケージなので、同じコマンドの案内は 1 つにする。
  const seen = new Set<string>();
  for (const t of v.tools) {
    if (t.path || !t.install || seen.has(t.install.command)) continue;
    seen.add(t.install.command);
    const rip = NEEDED_FOR_RIP.includes(t.name);
    const need = t.name === 'yt-dlp' ? 'XFD 動画のフレームの切り出しに' : rip ? '取り込みに' : '書き出しに';
    steps.push({
      key: t.name,
      tone: rip ? 'warn' : 'info',
      title: `${t.name} が見つかりません`,
      text: `${need}使います。${t.install.runnable ? '' : '管理者権限が要るので、端末で次のコマンドを実行してください。'}`,
      ...(t.install.runnable ? { action: { tool: t.name, label: `${t.name} を入れる` } } : { command: t.install.command }),
    });
  }
  if (v.drive.message) steps.push({ key: 'drive', tone: 'warn', title: 'ドライブの設定', text: v.drive.message });
  return steps;
};
