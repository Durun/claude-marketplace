import assert from 'node:assert/strict';
import { test } from 'node:test';
import { align, COALESCE_MS, describeChange, diffPlaylist, diffRows, diffSends, diffSong, followChannels, RecordLog, resolveChannels, restoreLines, type Row } from '../server/flChanges.ts';
import { gopherText } from '../server/flGopher.ts';
import { Assembler, asciiJson, CHUNK, DIRECTION, encodeMessages, parseFrame } from '../server/flMidi.ts';

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

test('音量は 0 から 1 の値で比べ、記録には dB で残し、書き戻す値を raw に持つ', () => {
  const [c] = diffRows('channels', [ch(0, 'Kick')], [ch(0, 'Kick', { vol: 0.5, db: -6.2 })]);
  assert.deepEqual(c, { section: 'channels', index: 0, target: 'channel 0「Kick」', field: 'vol', before: 0, after: -6.2, raw: [0.78, 0.5] });
  assert.equal(describeChange(c!), 'channel 0「Kick」の音量: 0 dB → -6.2 dB');
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
  const vol = (after: number, raw: number) => ({ section: 'mixer' as const, index: 1, target: 'mixer 1', field: 'vol', before: 0, after, raw: [0.8, raw] as [number, number] });
  log.add(vol(-1, 0.7), 'human', undefined, 1000);
  log.add({ section: 'channels', index: 0, target: 'channel 0', field: 'name', before: 'a', after: 'b' }, 'human', undefined, 1100);
  log.add(vol(-3, 0.6), 'human', undefined, 1200);
  assert.deepEqual(log.records.map((r) => [r.seq, r.field, r.after, r.raw?.[1]]), [[2, 'name', 'b', undefined], [3, 'vol', -3, 0.6]]);
  assert.deepEqual(log.since(2).map((r) => r.seq), [3]);
  // エージェントの変更は、人間の記録とまとめない。
  log.add(vol(-4, 0.5), 'agent', 'fl_python', 1300);
  assert.equal(log.records.length, 3);
  log.add(vol(0, 0.8), 'human', undefined, 1400);
  assert.deepEqual(log.records.map((r) => r.who), ['human', 'agent']);
  // 時間が空いたら、まとめない。
  log.add(vol(-2, 0.65), 'human', undefined, 1400 + COALESCE_MS + 1);
  log.add(vol(-5, 0.4), 'human', undefined, 1400 + 2 * COALESCE_MS + 2);
  assert.equal(log.records.filter((r) => r.who === 'human' && r.field === 'vol').length, 2);
});

test('記録から、前の値に戻す Python の行を作る。ソロは今と違うときだけ切り替え、戻せない変化は空にする', () => {
  assert.deepEqual(restoreLines({ section: 'channels', index: 2, target: '', field: 'name', before: 'Kick "A"', after: 'BD' }), ['channels.setChannelName(2, "Kick \\"A\\"")']);
  assert.deepEqual(restoreLines({ section: 'mixer', index: 1, target: '', field: 'vol', before: 0, after: -6, raw: [0.8, 0.4] }), ['mixer.setTrackVolume(1, 0.8)']);
  assert.deepEqual(restoreLines({ section: 'channels', index: 0, target: '', field: 'solo', before: false, after: true }), ['if channels.isChannelSolo(0) != False: channels.soloChannel(0)']);
  assert.deepEqual(restoreLines({ section: 'sends', index: 3, target: '', field: 'sends', before: [0], after: [0, 5] }), ['mixer.setRouteTo(3, 5, False)', 'mixer.afterRoutingChanged()']);
  assert.deepEqual(restoreLines({ section: 'channels', index: 1, target: '', field: 'added', before: null, after: '3x Osc' }), []);
  assert.deepEqual(restoreLines({ section: 'playlist', index: 2, target: '', field: 'color', before: null, after: 0x4080ff }), []);
  assert.deepEqual(restoreLines({ section: 'song', index: 0, target: '', field: 'tempo', before: 140, after: 128 }), []);
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
