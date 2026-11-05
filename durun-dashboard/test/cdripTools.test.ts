import assert from 'node:assert/strict';
import { test } from 'node:test';
import { firstLine, knownOffset, resolveTool, toolEnvName } from '../server/cdripTools.ts';
import { platformFor } from '../server/platform.ts';

test('機種名から、読み取りオフセットの表を単語ごとに引き、表に無い機種は undefined', () => {
  assert.equal(knownOffset('PIONEER BD-RW BDR-UD04 1.11'), 667);
  assert.equal(knownOffset('pioneer bdr-ud04'), 667);
  assert.equal(knownOffset('ASUS DRW-24D5MT'), undefined);
});

test('依存ツールを、環境変数、PATH、パッケージ管理の置き場所の順に探す', () => {
  const win = platformFor('win32');
  const only = (...files: string[]) => ({ exists: (f: string) => files.includes(f), list: () => [] });
  assert.equal(toolEnvName('yt-dlp'), 'CDRIP_YT_DLP_PATH');
  assert.equal(resolveTool('flac', { platform: win, env: { CDRIP_FLAC_PATH: 'X:\\my\\flac.exe', PATH: 'C:\\bin' }, dirs: only('X:\\my\\flac.exe', 'C:\\bin\\flac.exe') }), 'X:\\my\\flac.exe');
  assert.equal(resolveTool('flac', { platform: win, env: { CDRIP_FLAC_PATH: 'X:\\gone\\flac.exe', PATH: 'C:\\a;C:\\bin' }, dirs: only('C:\\bin\\flac.exe') }), 'C:\\bin\\flac.exe');
  assert.equal(resolveTool('flac', { platform: win, env: { PATH: 'C:\\a' }, dirs: only() }), undefined);
  const mac = platformFor('darwin');
  assert.equal(resolveTool('flac', { platform: mac, env: { PATH: '/usr/bin:/bin' }, dirs: only('/bin/flac') }), '/bin/flac');
  // PATH に無くても、brew の置き場所にあれば見つかる。
  assert.equal(resolveTool('flac', { platform: mac, env: { PATH: '/usr/bin' }, dirs: only('/opt/homebrew/bin/flac') }), '/opt/homebrew/bin/flac');
});

test('版の表示は、コマンドが出す最初の空でない行', () => {
  assert.equal(firstLine('\r\nflac 1.5.0\r\nCopyright'), 'flac 1.5.0');
  assert.equal(firstLine(''), undefined);
});
