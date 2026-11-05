import assert from 'node:assert/strict';
import { test } from 'node:test';
import { nextSteps } from '../client/cdripSteps.ts';
import { DEFAULT_FEATURES } from '../server/cdrip.ts';
import type { CdRipView, CdTool, CdToolName } from '../shared/types.ts';

const found = (name: CdToolName): CdTool => ({ name, path: `/bin/${name}` });
const missing = (name: CdToolName, command: string, runnable = true): CdTool => ({ name, install: { label: command, command, runnable } });

const view = (tools: CdTool[], message?: string): CdRipView => ({
  tools,
  drive: { id: 'F:', model: 'm', offset: 667, media: 'none', message },
  accepting: false,
  settings: { drive: 'F:', driveModel: 'm', offset: 667, paranoia: 3, musicDir: '', stageDir: '', dest: { cd: '', club: '', music: '' }, features: { ...DEFAULT_FEATURES } },
  items: [],
  log: [],
  drives: [],
});

test('そろっていれば、次にすることは何も出さない', () => {
  assert.deepEqual(nextSteps(view(['cyanrip', 'flac', 'metaflac', 'ffmpeg', 'ffprobe'].map((n) => found(n as CdToolName)))), []);
});

test('取り込みに要る cyanrip が無ければ警告にして導入ボタンを出し、書き出しだけに使うツールは案内にとどめる', () => {
  const steps = nextSteps(view([missing('cyanrip', 'winget install --id cyanreg.cyanrip'), missing('ffmpeg', 'winget install --id Gyan.FFmpeg')]));
  assert.deepEqual(steps.map((s) => [s.key, s.tone, s.action?.tool]), [['cyanrip', 'warn', 'cyanrip'], ['ffmpeg', 'info', 'ffmpeg']]);
  assert.match(steps[0]?.text ?? '', /取り込み/);
  assert.match(steps[1]?.text ?? '', /書き出し/);
});

test('同じパッケージに入るツール（metaflac と flac、ffprobe と ffmpeg）は、案内を 1 つにまとめる', () => {
  const steps = nextSteps(view([missing('flac', 'winget install --id Xiph.FLAC'), missing('metaflac', 'winget install --id Xiph.FLAC'), missing('ffmpeg', 'x'), missing('ffprobe', 'x')]));
  assert.deepEqual(steps.map((s) => s.key), ['flac', 'ffmpeg']);
});

test('管理者権限が要るツールは、ボタンを出さず、端末で動かすコマンドを見せる。ドライブの注意も並べる', () => {
  const steps = nextSteps(view([missing('flac', 'sudo apt install flac', false)], 'オフセットが表に無い'));
  assert.equal(steps[0]?.action, undefined);
  assert.equal(steps[0]?.command, 'sudo apt install flac');
  assert.match(steps[0]?.text ?? '', /管理者権限/);
  assert.deepEqual([steps[1]?.key, steps[1]?.text], ['drive', 'オフセットが表に無い']);
});

test('yt-dlp が無いときは、XFD 動画の切り出しに使うと案内する', () => {
  assert.match(nextSteps(view([missing('yt-dlp', 'winget install --id yt-dlp.yt-dlp')]))[0]?.text ?? '', /XFD 動画/);
});
