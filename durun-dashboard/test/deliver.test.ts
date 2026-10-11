import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import type { OutboxMessage } from '../shared/types.ts';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-deliver-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { postToInbox, redeliverInterrupted, removeSession, sendMessage, startSession } = await import('../server/deliver.ts');
const { ensureSession, onChange, state } = await import('../server/store.ts');
const { platform } = await import('../server/platform.ts');
fs.mkdirSync(path.join(tmp, 'claude', 'sessions'), { recursive: true });
// 受信ソケットに認証の要る OS が読む、このプロセスに見立てたセッションの鍵。要らない OS では読まれない。
fs.writeFileSync(path.join(tmp, 'claude', 'sessions', `${process.pid}.test.key`), JSON.stringify({ peerToken: 'test-token' }));

type InboxLine = { type: string; token?: string; message?: { role?: string; content: string } };

/** supervisor が名簿から動いているジョブを消すコード。書きかけを読ませないよう、別名で書いてから置き換える。 */
const dropWorkers = (roster: string): string => `fs.writeFileSync(${js(`${roster}.tmp`)}, '{"workers":{}}'); fs.renameSync(${js(`${roster}.tmp`)}, ${js(roster)});`;

/** 受信ソケットに見立てたサーバーを開き、次の接続に届く行をすべて返す。rejectAuth なら、本物が認証を拒むときと同じく、何も返さずにすぐ閉じる。 */
const listenInbox = async (socketPath: string, rejectAuth = false): Promise<{ server: net.Server; lines: Promise<InboxLine[]> }> => {
  const server = net.createServer();
  const lines = new Promise<InboxLine[]>((resolve) =>
    server.once('connection', (c) => {
      let buf = '';
      c.on('data', (chunk) => {
        buf += chunk.toString();
        if (rejectAuth) c.destroy();
      });
      c.on('close', () => resolve(buf.split('\n').filter(Boolean).map((line) => JSON.parse(line) as InboxLine)));
    }),
  );
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  return { server, lines };
};

const close = (server: net.Server): Promise<unknown> => new Promise((resolve) => server.close(resolve));

test('受信ソケットへは、トークンがあれば認証の行を先に送り、拒まれて閉じられたら届いたことにしない', async () => {
  const socketPath = platform.localSocketPath(tmp, 'direct');
  const message = { type: 'user', message: { role: 'user', content: 'こんにちは' } };

  const open = await listenInbox(socketPath);
  await postToInbox(socketPath, 'こんにちは');
  assert.deepEqual(await open.lines, [message]);
  await close(open.server);

  const keyed = await listenInbox(socketPath);
  await postToInbox(socketPath, 'こんにちは', 'secret');
  assert.deepEqual(await keyed.lines, [{ type: 'auth', token: 'secret' }, message]);
  await close(keyed.server);

  const refusing = await listenInbox(socketPath, true);
  await assert.rejects(postToInbox(socketPath, 'こんにちは', 'wrong'), /認証を拒まれました/);
  assert.deepEqual(await refusing.lines, [{ type: 'auth', token: 'wrong' }]);
  await close(refusing.server);
});

test('動いているセッションには受信ソケットへ送り、届かなければ未配信に戻して、次に送るときに一緒に届け直す', async () => {
  // 自分のプロセスを、受信ソケットを持って生きているセッションに見立てる。
  const socketPath = platform.localSocketPath(tmp, 'inbox');
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's1.json'), JSON.stringify({ pid: process.pid, sessionId: 's1', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  ensureSession('s1', tmp);
  // 認証の要る OS では、セッションの鍵のトークンを先に名乗る。
  const auth = platform.inboxToken(process.pid, path.join(tmp, 'claude', 'sessions')) ? [{ type: 'auth', token: 'test-token' }] : [];
  const user = (content: string): InboxLine => ({ type: 'user', message: { role: 'user', content } });

  const first = await listenInbox(socketPath);
  assert.equal(sendMessage('s1', 'こんにちは').delivery, 'sent');
  assert.deepEqual(await first.lines, [...auth, user('[durun-dashboard] 人間から:\nこんにちは')]);
  await close(first.server);

  const lost = sendMessage('s1', '届かない').message;
  await new Promise<void>((resolve) => onChange(() => lost.error && resolve()));
  assert.equal(lost.deliveredAt, undefined);

  const second = await listenInbox(socketPath);
  sendMessage('s1', 'もう一度');
  assert.deepEqual(await second.lines, [...auth, user('[durun-dashboard] 人間から:\n届かない\n---\nもう一度')]);
  assert.ok(lost.deliveredAt);
  await close(second.server);
});

test('入力待ちのバックグラウンドのセッションは、止めてから前置きを付けずに再開して渡し、失敗したら未配信に戻す', async () => {
  // 偽の claude は、cmux のラッパーを素通しさせる環境変数と、呼ばれた引数を 1 回ごとに書き足す。
  // stop はすぐに戻り、少し後で supervisor の名簿から j2 を消す。再開の --bg は、名簿に j2 が残っていれば複製になるので、それを理由に失敗する。
  const bin = path.join(tmp, 'bin-restart');
  const callsFile = path.join(tmp, 'calls');
  const roster = path.join(tmp, 'claude', 'daemon', 'roster.json');
  fs.mkdirSync(path.dirname(roster), { recursive: true });
  fs.writeFileSync(roster, JSON.stringify({ workers: { j2: {} } }));
  useFakeClaude(
    bin,
    `fs.appendFileSync(${js(callsFile)}, lines([process.env.CMUX_CLAUDE_HOOKS_DISABLED ?? '', ...args, '']));
if (args[0] === 'stop') later(300, ${js(dropWorkers(roster))});
if (args[0] === '--bg') {
  console.error(fs.readFileSync(${js(roster)}, 'utf8').includes('j2') ? '複製になります' : '再開できません');
  process.exitCode = 1;
}`,
  );
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's2.json'), JSON.stringify({ pid: process.pid, sessionId: 's2', cwd: tmp, status: 'idle', kind: 'bg', jobId: 'j2' }));
  ensureSession('s2', tmp);

  const { message, delivery } = sendMessage('s2', '- push して');
  assert.equal(delivery, 'resumed');
  await new Promise<void>((resolve) => onChange(() => message.error && resolve()));
  const calls = fs.readFileSync(callsFile, 'utf8').split('\n\n').slice(0, -1).map((c) => c.split('\n'));
  assert.deepEqual(calls, [
    ['1', 'stop', 'j2'],
    ['1', '--bg', '--resume', 's2', '--', '- push して'],
  ]);
  assert.equal(message.error, '再開できません');
  assert.equal(message.deliveredAt, undefined);
});

test('届けている途中でサーバーが止まったメッセージは、次に起動したときに届け直す', async () => {
  // 前のサーバーがセッションを止めた後、再開する前に止まった状態。配信済みの記録と sending が残り、セッションはもう動いていない。
  const bin = path.join(tmp, 'bin-interrupted');
  const argsFile = path.join(tmp, 'args-interrupted');
  useFakeClaude(bin, `fs.writeFileSync(${js(argsFile)}, lines(args));`);
  ensureSession('s6', tmp);
  const cut = { id: 'cut', sessionId: 's6', text: '途中で消えた', createdAt: 2, deliveredAt: 2, via: 'resume' as const, sending: true };
  const done = { id: 'done', sessionId: 's6', text: '届いている', createdAt: 1, deliveredAt: 1, via: 'resume' as const };
  state.outbox.push(done, cut);

  redeliverInterrupted();
  await new Promise<void>((resolve) => onChange(() => !cut.sending && resolve()));
  assert.deepEqual(fs.readFileSync(argsFile, 'utf8').split('\n').slice(0, -1), ['--bg', '--resume', 's6', '--', '途中で消えた']);
  assert.ok(cut.deliveredAt > 2);

  // 届け終えたものは、もう一度起動しても送らない。
  fs.rmSync(argsFile);
  redeliverInterrupted();
  await new Promise((resolve) => setTimeout(resolve, 300));
  assert.equal(fs.existsSync(argsFile), false);
});

test('渡し終えてから記録する前にサーバーが止まったメッセージは、会話ログに本文が残っているので、届け直さない', async () => {
  const bin = path.join(tmp, 'bin-arrived');
  const argsFile = path.join(tmp, 'args-arrived');
  useFakeClaude(bin, `fs.writeFileSync(${js(argsFile)}, lines(args));`);
  ensureSession('s7', tmp);
  const at = Date.now();
  const iso = (ms: number): string => new Date(ms).toISOString();
  // 再開して渡した本文は人間の発言に、受信ソケットで渡した本文は受信の記録に残る。送る前からあった同じ文面の発言は、届いた証拠にしない。
  const log = [
    { type: 'user', timestamp: iso(at - 60_000), message: { role: 'user', content: '前にも言った\n改行つき' } },
    { type: 'user', timestamp: iso(at + 1000), message: { role: 'user', content: '再開で渡した' } },
    { type: 'queue-operation', operation: 'enqueue', timestamp: iso(at + 1000), content: '[durun-dashboard] 人間から:\nソケットで渡した' },
  ];
  const dir = path.join(tmp, 'claude', 'projects', 'p7');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 's7.jsonl'), log.map((e) => `${JSON.stringify(e)}\n`).join(''));
  const sent = (id: string, text: string): OutboxMessage => ({ id, sessionId: 's7', text, createdAt: at, deliveredAt: at, via: 'resume', sending: true });
  const [resumed, socket, lost] = [sent('a1', '再開で渡した'), sent('a2', 'ソケットで渡した'), sent('a3', '前にも言った\n改行つき')];
  state.outbox.push(resumed, socket, lost);

  redeliverInterrupted();
  await new Promise<void>((resolve) => onChange(() => !lost.sending && lost.deliveredAt && resolve()));
  assert.deepEqual(fs.readFileSync(argsFile, 'utf8').split('\n').slice(0, -2), ['--bg', '--resume', 's7', '--', '前にも言った']);
  assert.deepEqual([resumed.deliveredAt, socket.deliveredAt, resumed.sending, socket.sending], [at, at, undefined, undefined]);
});

test('新しいセッションは依頼を -- の後に置いて claude --bg で起動し、選んだモデルと effort を渡し、失敗したら理由を返す', async () => {
  // 受け取った引数を書き出し、依頼が fail なら失敗する偽の claude を置く。
  const bin = path.join(tmp, 'bin');
  const argsFile = path.join(tmp, 'args');
  useFakeClaude(bin, `fs.writeFileSync(${js(argsFile)}, lines(args)); if (args[2] === 'fail') { console.error('起動できません'); process.exitCode = 1; }`);

  await startSession(tmp, '- 箇条書きの依頼');
  assert.deepEqual(fs.readFileSync(argsFile, 'utf8').split('\n').slice(0, -1), ['--bg', '--', '- 箇条書きの依頼']);
  await startSession(tmp, '依頼', 'opus', 'max');
  assert.deepEqual(fs.readFileSync(argsFile, 'utf8').split('\n').slice(0, -1), ['--bg', '--model', 'opus', '--effort', 'max', '--', '依頼']);
  await assert.rejects(startSession(tmp, 'fail'), /起動できません/);
});

test('アーカイブで消すときは、動いているバックグラウンドのセッションを止めてから claude rm し、端末のセッションは消さず、消せなければ理由を返す', async () => {
  // 偽の claude は呼ばれた引数を書き足す。agents は、動いている j3、止まっている j5、端末の s4 を返す。
  // stop は少し後で supervisor の名簿から j3 を消す。rm は、名簿に残っている j3 と、未コミットの変更がある j5 を断る。
  const bin = path.join(tmp, 'bin-remove');
  const callsFile = path.join(tmp, 'calls-remove');
  const roster = path.join(tmp, 'claude', 'daemon', 'roster.json');
  const jobs = [
    { id: 'j3', sessionId: 's3', kind: 'background', pid: 1 },
    { id: 'j5', sessionId: 's5', kind: 'background' },
    { sessionId: 's4', kind: 'interactive', pid: 1 },
  ];
  fs.mkdirSync(path.dirname(roster), { recursive: true });
  fs.writeFileSync(roster, JSON.stringify({ workers: { j3: {} } }));
  useFakeClaude(
    bin,
    `fs.appendFileSync(${js(callsFile)}, lines([...args, '']));
const fail = (reason) => { console.error(reason); process.exitCode = 1; };
if (args[0] === 'agents') console.log(${js(JSON.stringify(jobs))});
if (args[0] === 'stop') later(300, ${js(dropWorkers(roster))});
if (args[0] === 'rm' && args[1] === 'j3' && fs.readFileSync(${js(roster)}, 'utf8').includes('j3')) fail('動いています');
if (args[0] === 'rm' && args[1] === 'j5') fail('未コミットの変更があります');`,
  );
  const calls = (): string[][] => fs.readFileSync(callsFile, 'utf8').split('\n\n').slice(0, -1).map((c) => c.split('\n'));

  await removeSession('s3');
  assert.deepEqual(calls(), [['agents', '--json', '--all'], ['stop', 'j3'], ['rm', 'j3']]);
  fs.rmSync(callsFile);
  await removeSession('s4');
  assert.deepEqual(calls(), [['agents', '--json', '--all']]);
  await assert.rejects(removeSession('s5'), /未コミットの変更があります/);
});
