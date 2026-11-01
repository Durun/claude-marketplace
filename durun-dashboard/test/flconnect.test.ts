import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { nextSteps } from '../client/flSteps.ts';
import { align, COALESCE_MS, diffLevels, diffPlaylist, diffRec, diffRows, diffSends, diffSong, followChannels, pluginChanges, RecordLog, recLabel, resolveChannels, restoreLines, type Row, undoChange } from '../server/flChanges.ts';
import { backupFileName, backupPlan, type FileLookup, flMajorOf, projectFileOf } from '../server/flBackup.ts';
import { gopherText } from '../server/flGopher.ts';
import { appendLog, isoLocal, jsonLine, logFilePath } from '../server/flLogFile.ts';
import { Assembler, asciiJson, CHUNK, DIRECTION, encodeMessages, parseFrame } from '../server/flMidi.ts';
import { describeAgent, describeChange, undoDelta, windowName } from '../shared/flText.ts';
import type { FlConnectView } from '../shared/types.ts';

test('SysEx の JSON は ASCII の文字だけにし、日本語は \\u の形にして、読み戻すと元の値になる', () => {
  const value = { name: 'キック 🥁', n: 1 };
  const json = asciiJson(value);
  assert.match(json, /^[\x00-\x7e]*$/);
  assert.deepEqual(JSON.parse(json), value);
});

test('長いメッセージは 240 バイトずつの SysEx に分け、番号と何通目と全部で何通を 7 ビットずつの 2 バイトで持たせ、組み立てると元に戻る', () => {
  const payload = { op: 'exec', code: 'x = 1\n'.repeat(200) };
  const messages = encodeMessages(DIRECTION.request, 300, payload);
  assert.equal(messages.length, Math.ceil(asciiJson(payload).length / CHUNK));
  for (const m of messages) {
    assert.deepEqual(m.slice(0, 5), [0xf0, 0x7d, 0x46, 0x43, 1]);
    assert.equal(m.at(-1), 0xf7);
    assert.ok(m.every((b, k) => k === 0 || k === m.length - 1 || b < 0x80), 'SysEx のデータは 0x80 未満');
    assert.ok(m.length - 12 <= CHUNK);
  }
  const first = parseFrame(messages[0] ?? []);
  assert.deepEqual({ ...first, body: undefined }, { direction: 1, id: 300, index: 0, total: messages.length, body: undefined });
  const assembler = new Assembler();
  const done = messages.map((m) => assembler.push(parseFrame(m)!)).filter(Boolean);
  assert.deepEqual(done, [{ direction: 1, id: 300, value: payload }]);
});

test('FL Connect の印の無い MIDI のメッセージは読まず、抜けのある溜まりは先頭の 1 通がまた届いたら捨てる', () => {
  assert.equal(parseFrame([0x90, 60, 100]), undefined);
  assert.equal(parseFrame([0xf0, 0x7d, 0x46, 0x44, 1, 0, 1, 0, 0, 0, 1, 0x7b, 0x7d, 0xf7]), undefined);
  const [a0, , a2] = encodeMessages(DIRECTION.response, 5, { text: 'x'.repeat(600) });
  const assembler = new Assembler();
  assert.equal(assembler.push(parseFrame(a0!)!), undefined);
  assert.equal(assembler.push(parseFrame(a2!)!), undefined);
  // 2 通目が抜けたまま、同じ番号で新しいメッセージが届いた。
  const [b0] = encodeMessages(DIRECTION.response, 5, { ok: true });
  assert.deepEqual(assembler.push(parseFrame(b0!)!), { direction: 2, id: 5, value: { ok: true } });
});

const ch = (i: number, name: string, extra: Partial<Row> = {}): Row => ({ i, name, color: 0x808080, vol: 0.78, db: 0, pan: 0, mute: false, solo: false, mixer: 0, plugin: name, type: 2, sel: false, ...extra });

test('チャンネルの数が同じなら、同じ位置どうしを比べ、名前を変えたものも見分ける。選んでいるかは比べない', () => {
  const before = [ch(0, 'Kick'), ch(1, 'Snare')];
  const after = [ch(0, 'BD', { plugin: 'Kick' }), ch(1, 'Snare', { color: 0xe04848, sel: true })];
  assert.deepEqual(diffRows('channels', before, after).map(describeChange), ['channel 0「BD」の名前: 「Kick」 → 「BD」', 'channel 1「Snare」の色: #808080 → #E04848']);
});

test('記録は生の値のまま持つ。音量は書き戻せる 0 から 1 の値にし、FL が返した dB を添え、画面が dB で見せる', () => {
  const [c] = diffRows('channels', [ch(0, 'Kick')], [ch(0, 'Kick', { vol: 0.5, db: -6.2 })]);
  assert.deepEqual(c, { section: 'channels', index: 0, name: 'Kick', field: 'vol', before: 0.78, after: 0.5, db: [0, -6.2] });
  assert.equal(describeChange(c!), 'channel 0「Kick」の音量: 0 dB → -6.2 dB');
  // FL が名前と値の文字を付けた項目は、FL の言葉のまま見せる。
  assert.equal(describeChange({ section: 'channels', index: 0, name: 'Sampler', field: 'rec:4097', label: 'Sampler - Volume envelope - Attack', before: 100, after: 900, text: ['0:00', '0:25'] }), 'channel 0「Sampler」の「Sampler - Volume envelope - Attack」: 0:00 → 0:25');
});

test('チャンネルの数が変わったら、名前とプラグインの並びを合わせて、足したものと消したものを見分ける', () => {
  const before = [ch(0, 'Kick'), ch(1, 'Snare'), ch(2, 'Hat')];
  const added = [ch(0, 'Kick'), ch(1, '3x Osc'), ch(2, 'Snare'), ch(3, 'Hat')];
  assert.deepEqual(diffRows('channels', before, added).map(describeChange), ['channel 1「3x Osc」を足した']);
  const removed = [ch(0, 'Kick'), ch(1, 'Hat')];
  assert.deepEqual(diffRows('channels', before, removed).map(describeChange), ['channel 1「Snare」を消した']);
  assert.deepEqual([...align(['a', 'b', 'c'], ['a', 'c'])], [[0, 0], [2, 1]]);
});

test('② で足したチャンネルの番号は、前のチャンネルが消えたら詰め、そのチャンネル自身が消えたら外す', () => {
  const before = [ch(0, 'Kick'), ch(1, 'Snare'), ch(2, '3x Osc')];
  assert.deepEqual(followChannels([2], before, [ch(0, 'Snare'), ch(1, '3x Osc')]), [1]);
  assert.deepEqual(followChannels([2], before, [ch(0, 'Kick'), ch(1, 'Snare')]), []);
  // 数が同じなら、名前を変えても同じ位置のまま。
  assert.deepEqual(followChannels([2], before, [ch(0, 'Kick'), ch(1, 'Snare'), ch(2, 'Lead')]), [2]);
});

test('プレイリストのトラックは、末尾の触っていない行が省かれていても、既定のトラックとして比べる', () => {
  const t = (i: number, extra: Partial<Row> = {}): Row => ({ i, name: `Track ${i}`, color: 0x5a6470, mute: false, solo: false, ...extra });
  const changes = diffPlaylist([t(1, { name: 'Drums' })], [t(1, { name: 'Drums' }), t(2, { name: 'Bass', color: 0x4080ff })]);
  assert.deepEqual(changes.map(describeChange), ['playlist 2「Bass」の名前: 「Track 2」 → 「Bass」', 'playlist 2「Bass」の色: （既定） → #4080FF']);
});

test('ミキサーの送り先と、テンポと再生の変化を記録にする', () => {
  const names = new Map([[3, 'Bass']]);
  assert.deepEqual(diffSends({ '3': [0] }, { '3': [0, 5], '4': [0] }, names).map(describeChange), ['mixer 3「Bass」の送り先: mixer 0 → mixer 0、mixer 5']);
  assert.deepEqual(diffSong({ tempo: 140, playing: false, recording: false }, { tempo: 128, playing: true, recording: false }).map(describeChange), ['songのテンポ: 140 BPM → 128 BPM', 'songの再生: オフ → オン']);
  assert.deepEqual(diffSong({}, { tempo: 128 }), []);
});

test('同じ人の、同じ対象の同じ項目の変化が続いたら 1 件にまとめて末尾に移し、元の値に戻ったら消す', () => {
  const log = new RecordLog();
  const vol = (after: number, db: number) => ({ section: 'mixer' as const, index: 1, name: 'Bass', field: 'vol', before: 0.8, after, db: [0, db] as [number, number] });
  log.add(vol(0.7, -1), 'human', undefined, 1000);
  log.add({ section: 'channels', index: 0, field: 'name', before: 'a', after: 'b' }, 'human', undefined, 1100);
  log.add(vol(0.6, -3), 'human', undefined, 1200);
  assert.deepEqual(log.records.map((r) => [r.seq, r.field, r.after, r.db]), [[2, 'name', 'b', undefined], [3, 'vol', 0.6, [0, -3]]]);
  assert.deepEqual(log.since(2).map((r) => r.seq), [3]);
  // エージェントの変更は、人間の記録とまとめない。
  log.add(vol(0.5, -4), 'agent', 'fl_python', 1300);
  assert.equal(log.records.length, 3);
  log.add(vol(0.8, 0), 'human', undefined, 1400);
  assert.deepEqual(log.records.map((r) => r.who), ['human', 'agent']);
  // 時間が空いたら、まとめない。
  log.add(vol(0.65, -2), 'human', undefined, 1400 + COALESCE_MS + 1);
  log.add(vol(0.4, -5), 'human', undefined, 1400 + 2 * COALESCE_MS + 2);
  assert.equal(log.records.filter((r) => r.who === 'human' && r.field === 'vol').length, 2);
});

test('記録から、前の値に戻す Python の行を作る。ソロは今と違うときだけ切り替え、戻せない変化は空にする', () => {
  assert.deepEqual(restoreLines({ section: 'channels', index: 2, field: 'name', before: 'Kick "A"', after: 'BD' }), ['channels.setChannelName(2, "Kick \\"A\\"")']);
  assert.deepEqual(restoreLines({ section: 'mixer', index: 1, field: 'vol', before: 0.8, after: 0.4, db: [0, -6] }), ['mixer.setTrackVolume(1, 0.8)']);
  assert.deepEqual(restoreLines({ section: 'channels', index: 0, field: 'solo', before: false, after: true }), ['if channels.isChannelSolo(0) != False: channels.soloChannel(0)']);
  assert.deepEqual(restoreLines({ section: 'sends', index: 3, field: 'sends', before: [0], after: [0, 5] }), ['mixer.setRouteTo(3, 5, False)', 'mixer.afterRoutingChanged()']);
  assert.deepEqual(restoreLines({ section: 'channels', index: 1, field: 'added', before: null, after: '3x Osc' }), []);
  assert.deepEqual(restoreLines({ section: 'playlist', index: 2, field: 'color', before: null, after: 0x4080ff }), []);
  assert.deepEqual(restoreLines({ section: 'song', index: 0, field: 'tempo', before: 140, after: 128 }), []);
  assert.deepEqual(restoreLines({ section: 'undo', index: 0, field: 'history', before: [1, 0], after: [3, 0] }), []);
});

test('remove_channels の指定を、1 からの番号、名前（大文字と小文字を区別しない）、all、selected から、消えるチャンネルの番号にする', () => {
  const rows = [ch(0, 'Kick'), ch(1, 'Snare', { sel: true }), ch(2, 'kick')];
  assert.deepEqual(resolveChannels('2, KICK', rows), { indices: [0, 1, 2], unknown: [] });
  assert.deepEqual(resolveChannels('selected', rows), { indices: [1], unknown: [] });
  assert.deepEqual(resolveChannels('all', rows).indices, [0, 1, 2]);
  assert.deepEqual(resolveChannels('4, Bass', rows), { indices: [], unknown: ['4', 'Bass'] });
});

test('Gopher の返事は、JSON-RPC として読めればその文を、中の " がエスケープされずに読めなければ text から isError の手前までを返す', () => {
  assert.deepEqual(gopherText('{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Successfully removed 1 channel(s)."}],"isError":false}}'), { text: 'Successfully removed 1 channel(s).', isError: false });
  assert.deepEqual(gopherText('{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"Project "Song" has 2 channels\\nOK"}],"isError":false}}'), { text: 'Project "Song" has 2 channels\nOK', isError: false });
  assert.deepEqual(gopherText('{"jsonrpc":"2.0","id":1,"error":{"code":-32000,"message":"FL Studio timeout"}}'), { text: 'FL Studio timeout', isError: true });
  assert.deepEqual(gopherText('plain answer'), { text: 'plain answer', isError: false });
});

test('元に戻す履歴の [数, 戻した数] の前と後から、新しく増えた操作、Ctrl+Z で戻した数、やり直した数を出す', () => {
  assert.deepEqual(undoDelta([1, 0], [3, 0]), { added: 2, undone: 0, redone: 0 });
  assert.deepEqual(undoDelta([5, 0], [5, 2]), { added: 0, undone: 2, redone: 0 });
  assert.deepEqual(undoDelta([5, 2], [5, 1]), { added: 0, undone: 0, redone: 1 });
  // 2 回戻してから新しく操作すると、FL は戻した 2 件を消してから 1 件足す。
  assert.deepEqual(undoDelta([5, 2], [4, 0]), { added: 1, undone: 0, redone: 0 });
  // 別のプロジェクトを読み込むと履歴は減るが、操作ではない。
  assert.deepEqual(undoDelta([17, 0], [1, 0]), { added: 0, undone: 0, redone: 0 });
});

const SAMPLER_WINDOW = `Sampler${String.fromCodePoint(0xe5a0)} (Master)${String.fromCodePoint(0xe5ff)}`;

test('元に戻す履歴の変化は、ほかの変化で説明できないときだけ生の値で記録し、画面がウィンドウの名前からアイコンの文字を外して訳す', () => {
  assert.equal(windowName(SAMPLER_WINDOW), 'Sampler (Master)');
  const op = undoChange([1, 0], [3, 0], SAMPLER_WINDOW, false);
  assert.deepEqual(op, { section: 'undo', index: 0, name: SAMPLER_WINDOW, field: 'history', before: [1, 0], after: [3, 0] });
  assert.equal(describeChange(op!), '「Sampler (Master)」で、FL Connect が読まない値を変えた（FL の元に戻す履歴が 2 件増えた）');
  assert.equal(undoChange([1, 0], [3, 0], SAMPLER_WINDOW, true), undefined);
  // 元に戻したことは、戻って変わった値が見えても残す。
  assert.equal(describeChange(undoChange([5, 0], [5, 1], '', true)!), 'FL で元に戻した（1 回）');
  assert.equal(describeChange(undoChange([5, 3], [5, 0], 'Mixer', false)!), '「Mixer」でやり直した（3 回）');
});

test('同じウィンドウで続いた元に戻す履歴の変化は、最初の前と最後の後で 1 件にし、別のウィンドウのものはまとめない', () => {
  const log = new RecordLog();
  log.add(undoChange([1, 0], [3, 0], 'A', false)!, 'human', undefined, 1000);
  log.add(undoChange([3, 0], [4, 0], 'A', false)!, 'human', undefined, 1500);
  log.add(undoChange([4, 0], [5, 0], 'B', false)!, 'human', undefined, 1600);
  assert.deepEqual(log.records.map((r) => [r.name, r.before, r.after]), [['A', [1, 0], [4, 0]], ['B', [4, 0], [5, 0]]]);
});

const at = new Date(2026, 10, 1, 9, 30, 5).getTime();

test('ログの 1 件を、時刻を読める形にした JSONL の 1 行にし、ウィンドウのための id は書かない', () => {
  assert.match(isoLocal(at), /^2026-11-01T09:30:05\.000[+-]\d\d:\d\d$/);
  assert.equal(jsonLine({ id: 'r1', at, who: 'you', section: 'channels', index: 0, name: 'Kick', field: 'vol', before: 0.78, after: 0.5, db: [0, -6.2] }), `{"at":"${isoLocal(at)}","who":"you","section":"channels","index":0,"name":"Kick","field":"vol","before":0.78,"after":0.5,"db":[0,-6.2]}`);
  const agent = { id: 'a', at, who: 'agent' as const, tool: 'fl_python', args: { code: 'print(1)' }, output: '1', ok: true, changes: [] };
  assert.equal(jsonLine(agent), `{"at":"${isoLocal(at)}","who":"agent","tool":"fl_python","args":{"code":"print(1)"},"output":"1","ok":true,"changes":[]}`);
  assert.equal(describeAgent(agent), 'fl_python: print(1)');
});

test('ログは、セッションごとのフォルダーの、日ごとの JSONL のファイルに書き足し、新しく作ったファイルだけを返す', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fl-log-'));
  try {
    const next = at + 24 * 3600_000;
    const you = (id: string, when: number, after: string) => ({ id, at: when, who: 'you' as const, section: 'channels' as const, index: 0, field: 'name', before: 'a', after });
    const created = appendLog(dir, 's1', [{ at, event: 'connect', fl: '26.1.7' }, you('r1', at, 'A')]);
    assert.deepEqual(created, [logFilePath(dir, 's1', at)]);
    assert.match(created[0] ?? '', /FL Connect 2026-11-01\.jsonl$/);
    assert.deepEqual(appendLog(dir, 's1', [you('r2', at, 'B'), you('r3', next, 'C')]), [logFilePath(dir, 's1', next)]);
    const lines = fs.readFileSync(logFilePath(dir, 's1', at), 'utf8').trimEnd().split('\n').map((l) => JSON.parse(l) as Record<string, unknown>);
    assert.deepEqual(lines.map((l) => l.event ?? l.after), ['connect', 'A', 'B']);
    assert.equal(JSON.parse(fs.readFileSync(logFilePath(dir, 's1', next), 'utf8')).after, 'C');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});


const view = (patch: { midi?: Partial<FlConnectView['midi']>; gopher?: Partial<FlConnectView['gopher']>; setup?: Partial<FlConnectView['setup']> } = {}): FlConnectView => ({
  midi: { status: 'connected', ports: { input: 'FL Connect (B)', output: 'FL Connect (B)' }, hello: { script: '0.1.0', core: '0.2.0', fl: '26.1.7', api: 45, python: '3.12' }, ...patch.midi },
  gopher: { status: 'no-port', port: 9222, debug: false, ...patch.gopher },
  readOnly: false,
  setup: { userData: 'D:/Image-Line/FL Studio', installed: '0.1.0', bundled: '0.1.0', bundledCore: '0.2.0', flRunning: true, ...patch.setup },
  log: [],
});

const stepKeys = (v: FlConnectView) => nextSteps(v).map((s) => s.key);

test('導入で次にすることは、済んでいないものだけを出し、Gopher MCP は使いたいときだけの案内にする', () => {
  // 全部済んでいて、FL をふつうに起動している。Gopher MCP を開放して起動するボタンは、FL を閉じるまで押せない。
  const ready = nextSteps(view());
  assert.deepEqual(ready.map((s) => [s.key, s.tone]), [['launch', 'info']]);
  assert.match(ready[0]?.action?.disabled ?? '', /FL Studio が動いています/);
  assert.equal(nextSteps(view({ setup: { flRunning: false }, midi: { status: 'silent', hello: undefined } })).find((s) => s.key === 'launch')?.action?.disabled, undefined);
  // デバッグのポートを開いて起動したが、Gopher を開いていない。
  assert.deepEqual(nextSteps(view({ gopher: { debug: true } })).map((s) => [s.key, s.title, s.text]), [['gopher', 'Gopher MCP not connected', 'Alt+F1を押してください（FL で Gopher を開くと、つながります）。']]);
  assert.deepEqual(stepKeys(view({ gopher: { status: 'connected', debug: true } })), []);
});

test('スクリプトが無いか版が違うときだけ Install Script を出し、FL が名乗らなければ、FL が動いているかで案内を変える', () => {
  const gopher = { status: 'connected' as const, debug: true };
  assert.deepEqual(nextSteps(view({ gopher, setup: { installed: undefined }, midi: { status: 'silent', hello: undefined, message: 'MIDI の設定' } })).map((s) => [s.key, s.action?.label]), [['install', 'Install Script'], ['midi', undefined]]);
  assert.deepEqual(stepKeys(view({ gopher, midi: { hello: { script: '0.1.0', core: '0.1.0', fl: '26.1.7', api: 45, python: '3.12' } } })), ['install']);
  assert.deepEqual(stepKeys(view({ gopher, setup: { flRunning: false }, midi: { status: 'silent', hello: undefined } })), ['start-fl']);
  assert.deepEqual(stepKeys(view({ gopher, midi: { status: 'no-port', hello: undefined } })), ['loopback']);
  assert.deepEqual(stepKeys(view({ gopher, setup: { userData: undefined } })), ['user-data']);
});

test('チャンネルのつまみは、イベントの番号ごとに FL の生の値で比べ、FL が付けた名前と値の文字を添え、画面が FL の言葉のまま見せる', () => {
  const offsets = [2, 0x1003];
  const before = { '0': [[1073741824, '100%'], [326539860, '31%']] as [number, string][] };
  const after = { '0': [[1073741824, '100%'], [490630242, '46%']] as [number, string][] };
  const labels = new Map([[0x1003, recLabel('Kick - Panning envelope - Attack time', 'Kick')]]);
  const [c] = diffRec(before, after, offsets, labels, new Map([[0, 'Kick']]));
  assert.deepEqual(c, { section: 'channels', index: 0, name: 'Kick', field: 'rec:4099', label: 'Panning envelope - Attack time', before: 326539860, after: 490630242, text: ['31%', '46%'] });
  assert.equal(describeChange(c!), 'channel 0「Kick」の「Panning envelope - Attack time」: 31% → 46%');
  // FL が値の文字を付けないつまみは、生の値を見せる。
  assert.equal(describeChange({ ...c!, field: 'rec:9', label: 'Gate', before: 0, after: 1073741824, text: ['', ''] }), 'channel 0「Kick」の「Gate」: 0 → 1073741824');
  // 名前の頭がチャンネルの名前でなければ、そのまま使う。
  assert.equal(recLabel('Sampler - Gate', 'Kick'), 'Sampler - Gate');
});

test('ミキサーの EQ と送りの量も比べ、EQ は変わった帯だけを、送りの量は送り先ごとに見せる', () => {
  const m = (eq: number[]): Row => ({ i: 0, name: 'Master', color: 0, vol: 0.8, db: 0, pan: 0, mute: false, solo: false, stereo: 0, eq, enabled: true, polarity: false, swap: false, armed: false, fx: true });
  const [eq] = diffRows('mixer', [m([0.5, 0.08, 0.26, 0.5, 0.5, 0.26])], [m([0.5, 0.08, 0.26, 0.62, 0.5, 0.26])]);
  assert.equal(describeChange(eq!), 'mixer 0「Master」の EQ: 帯 2 の Gain 0.5 → 0.62');
  const sends = { before: { '1': [0, 5] }, after: { '1': [0, 5] } };
  const [level] = diffLevels(sends, { before: { '1': [0.8, 0.5] }, after: { '1': [0.8, 0.3] } }, new Map([[1, 'Bass']]));
  assert.deepEqual(level, { section: 'sends', index: 1, name: 'Bass', field: 'level:5', before: 0.5, after: 0.3 });
  assert.equal(describeChange(level!), 'mixer 1「Bass」の mixer 5 への送りの量: 0.5 → 0.3');
  // 送り先が変わったトラックは、送りの量を比べない。
  assert.deepEqual(diffLevels({ before: { '1': [0] }, after: { '1': [0, 5] } }, { before: { '1': [0.8] }, after: { '1': [0.8, 1] } }, new Map()), []);
  assert.equal(describeChange({ section: 'song', index: 0, field: 'bulk', before: null, after: 539 }), '一度に 539 件の値が変わった（プロジェクトを読み込んだか、まとめて変えた）');
});

test('前面のプラグインの変わったパラメーターを、プラグインの名前と FL の名前と値の文字で記録し、多すぎればまとめる', () => {
  const [synth] = pluginChanges({ plugin: 'Serum 2', index: 1, slot: -1, changes: [[0, 'Main Vol', 0.5, 0.62, '', '62% [-6.4 dB] ']] }, 'Serum 2');
  assert.deepEqual(synth, { section: 'channels', index: 1, name: 'Serum 2', plugin: 'Serum 2', field: 'param:0', label: 'Main Vol', before: 0.5, after: 0.62, text: ['', '62% [-6.4 dB] '] });
  // 前の値の文字が分からないときは、生の値を見せる。
  assert.equal(describeChange(synth!), 'channel 1「Serum 2」の Serum 2 の「Main Vol」: 0.5 → 62% [-6.4 dB] ');
  const [eq] = pluginChanges({ plugin: 'Pro-Q 4', index: 1, slot: 0, changes: [[2, 'Band 1 Frequency', 0.575, 0.6, '1000.0 Hz ', '1200.0 Hz ']] }, 'Insert 1');
  assert.equal(eq?.field, 'slot:0:param:2');
  assert.equal(describeChange(eq!), 'mixer 1「Insert 1」のスロット 1 の Pro-Q 4 の「Band 1 Frequency」: 1000.0 Hz  → 1200.0 Hz ');
  const preset = pluginChanges({ plugin: 'Serum 2', index: 1, slot: -1, changes: Array.from({ length: 101 }, (_, p): [number, string, number, number, string, string] => [p, `P${p}`, 0, 1, '', '']) }, 'Serum 2');
  assert.deepEqual(preset.map(describeChange), ['channel 1「Serum 2」の Serum 2 で、一度に 101 件のパラメーターが変わった（プリセットを読み込んだなど）']);
});

test('バックアップは、ウィンドウの題名のファイルを、最近のファイルのフォルダーと Projects の下から探していちばん新しいものにし、名前の無いプロジェクトと見つからないものは取らない', () => {
  // FL 26.1.7 は、名前を付けて保存したばかりのプロジェクトを、FL を閉じるまで最近のファイル（レジストリ）に書かない。
  const files: Record<string, number> = {
    'D:\\FL\\Projects\\new\\new.flp': 300,
    'D:\\FL\\Projects\\new\\Backup\\new.flp': 400,
    'D:\\FL\\Projects\\song\\song.flp': 100,
    'E:\\DTM\\song\\song.flp': 200,
    'E:\\DTM\\song\\song_2.flp': 250,
  };
  const io: FileLookup = {
    list: (dir) => {
      const names = new Map<string, boolean>();
      for (const f of Object.keys(files)) {
        if (!f.toLowerCase().startsWith(`${dir.toLowerCase()}\\`)) continue;
        const [head, ...rest] = f.slice(dir.length + 1).split('\\');
        names.set(head ?? '', rest.length > 0);
      }
      return [...names].map(([name, dir]) => ({ name, dir }));
    },
    mtime: (file) => files[file],
  };
  const recent = ['E:\\DTM\\song\\song_2.flp', 'D:\\FL\\Projects\\song\\song.flp', 'E:\\DTM\\pack.zip'];
  assert.deepEqual(projectFileOf({ title: 'song.flp - FL Studio 2026', recent }, 'D:\\FL\\Projects', io), { kind: 'file', file: 'E:\\DTM\\song\\song.flp', name: 'song' });
  assert.deepEqual(projectFileOf({ title: 'new.flp - FL Studio 2026', recent }, 'D:\\FL\\Projects', io), { kind: 'file', file: 'D:\\FL\\Projects\\new\\new.flp', name: 'new' });
  assert.deepEqual(projectFileOf({ title: 'new.flp* - FL Studio 2026', recent }, 'D:\\FL\\Projects', io), { kind: 'file', file: 'D:\\FL\\Projects\\new\\new.flp', name: 'new' });
  assert.equal(projectFileOf({ title: 'new.flp - FL Studio 2026', recent }, undefined, io).kind, 'unknown');
  assert.deepEqual(projectFileOf({ title: 'FL Studio 2026', recent }, 'D:\\FL\\Projects', io), { kind: 'untitled' });
  assert.equal(projectFileOf({ title: 'other.flp - FL Studio 2026', recent }, 'D:\\FL\\Projects', io).kind, 'unknown');
  assert.equal(projectFileOf({ title: 'pack.zip - FL Studio 2026', recent }, 'D:\\FL\\Projects', io).kind, 'unknown');
  assert.equal(projectFileOf({ title: '', recent }, 'D:\\FL\\Projects', io).kind, 'unknown');
  assert.equal(projectFileOf(undefined, 'D:\\FL\\Projects', io).kind, 'unknown');
  // 最近のファイルのレジストリのキーは、FL が名乗る版の上の桁で分かれる（FL Studio 26）。
  assert.equal(flMajorOf('Producer Edition v26.1.7 [build 5654]'), 26);
  assert.ok(Number.isNaN(flMajorOf('?')));
  assert.ok(Number.isNaN(flMajorOf(undefined)));
});

test('バックアップは、人間が触ったあとの最初の書き込みの前にだけ取り、保存していない変更にエージェントの変更が混ざっていれば上書き保存しない', () => {
  const base = { changed: 1, humanSinceBackup: true, agentUnsaved: false, recording: false };
  assert.deepEqual(backupPlan(base), { copy: true, save: true });
  assert.equal(backupPlan({ ...base, changed: 2 })?.save, true);
  assert.deepEqual(backupPlan({ ...base, changed: 0 }), { copy: true, save: false });
  assert.equal(backupPlan({ ...base, humanSinceBackup: false }), undefined);
  assert.deepEqual(backupPlan({ ...base, agentUnsaved: true }), { copy: true, save: false, note: '保存していない変更にエージェントの変更が混ざっているので、上書き保存しなかった' });
  assert.equal(backupPlan({ ...base, recording: true })?.save, false);
  assert.equal(backupFileName('song', new Date(2026, 10, 1, 9, 5, 7).getTime(), 'saved'), 'song 2026-11-01 09-05-07 前回の保存.flp');
  assert.equal(backupFileName('song', new Date(2026, 10, 1, 9, 5, 7).getTime(), 'before'), 'song 2026-11-01 09-05-07 エージェントの前.flp');
  assert.equal(describeAgent({ id: 'a', at: 0, who: 'agent', tool: 'fl_backup', args: {}, detail: 'a.flp\nb.flp', ok: true, changes: [] }), 'エージェントが書き込む前に、プロジェクトのバックアップを取った（2 個のファイル）');
  assert.equal(describeAgent({ id: 'b', at: 0, who: 'agent', tool: 'fl_backup', args: {}, output: '名前の無いプロジェクトなので、バックアップを取らなかった', ok: true, changes: [] }), 'バックアップを取らなかった: 名前の無いプロジェクトなので、バックアップを取らなかった');
});
