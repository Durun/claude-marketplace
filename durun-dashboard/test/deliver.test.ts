import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import type { OutboxMessage, QueuedMessage } from '../shared/types.ts';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-deliver-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { flushQueue, forkSession, postToInbox, redeliver, redeliverInterrupted, removeSession, sendMessage, startSession, switchSession } = await import('../server/deliver.ts');
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

test('再送すると、届けられなかったメッセージだけを、新しいメッセージを足さずに届け直す', async () => {
  const socketPath = platform.localSocketPath(tmp, 'retry');
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's11.json'), JSON.stringify({ pid: process.pid, sessionId: 's11', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  ensureSession('s11', tmp);

  const lost = sendMessage('s11', '届かない').message;
  await new Promise<void>((resolve) => onChange(() => lost.error && resolve()));

  const inbox = await listenInbox(socketPath);
  assert.equal(redeliver('s11'), 'sent');
  const lines = await inbox.lines;
  assert.equal(lines.at(-1)?.message?.content, '[durun-dashboard] 人間から:\n届かない');
  assert.ok(lost.deliveredAt);
  assert.equal(lost.error, undefined);
  assert.equal(state.outbox.filter((m) => m.sessionId === 's11').length, 1);
  await close(inbox.server);
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

test('保留したメッセージは、入力待ちで質問の無いセッションへ古い順に 1 つずつ送り、前に送ったものに返事が来るまでと、先頭を書き直している間は送らない', async () => {
  const socketPath = platform.localSocketPath(tmp, 'queue');
  const live = (status: string): void =>
    fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 'q1.json'), JSON.stringify({ pid: process.pid, sessionId: 'q1', cwd: tmp, status, messagingSocketPath: socketPath }));
  const dir = path.join(tmp, 'claude', 'projects', 'pq');
  fs.mkdirSync(dir, { recursive: true });
  const reply = (text: string): void =>
    fs.appendFileSync(path.join(dir, 'q1.jsonl'), `${JSON.stringify({ type: 'assistant', timestamp: new Date().toISOString(), message: { content: [{ type: 'text', text }] } })}\n`);
  reply('前の返事');
  ensureSession('q1', tmp);
  const queued = (id: string, text: string): QueuedMessage => ({ id, sessionId: 'q1', text, createdAt: Date.now() });
  const second = queued('k2', '二つ目');
  state.queue.push(queued('k1', '一つ目'), second);
  const left = (): string[] => state.queue.map((m) => m.text);
  const received = async (inbox: Awaited<ReturnType<typeof listenInbox>>): Promise<string | undefined> => {
    const lines = await inbox.lines;
    await close(inbox.server);
    return lines.at(-1)?.message?.content;
  };

  live('busy');
  flushQueue();
  assert.deepEqual(left(), ['一つ目', '二つ目']);
  live('idle');
  state.attention.push({ id: 'aq', kind: 'question', sessionId: 'q1', title: '質問', createdAt: Date.now() });
  flushQueue();
  assert.deepEqual(left(), ['一つ目', '二つ目']);
  state.attention = state.attention.filter((a) => a.id !== 'aq');
  // 質問がまだ記録されていなくても、会話ログで答えを待っているツールの呼び出しがあれば送らない。
  fs.appendFileSync(path.join(dir, 'q1.jsonl'), `${JSON.stringify({ type: 'assistant', message: { content: [{ type: 'tool_use', name: 'AskUserQuestion', input: {} }] } })}\n`);
  flushQueue();
  assert.deepEqual(left(), ['一つ目', '二つ目']);
  reply('質問への答えを受けた返事');

  const first = await listenInbox(socketPath);
  flushQueue();
  assert.deepEqual(left(), ['二つ目']);
  assert.equal(await received(first), '[durun-dashboard] 人間から:\n一つ目');
  const delivered = (): boolean => !state.outbox.some((m) => m.sessionId === 'q1' && m.sending);
  if (!delivered()) await new Promise<void>((resolve) => onChange(() => delivered() && resolve()));
  // 届けた後も、Claude が読み始めるまでは入力待ちに見える。
  flushQueue();
  assert.deepEqual(left(), ['二つ目']);

  second.editing = true;
  await new Promise((resolve) => setTimeout(resolve, 5));
  reply('一つ目への返事');
  flushQueue();
  assert.deepEqual(left(), ['二つ目']);
  second.editing = undefined;
  const next = await listenInbox(socketPath);
  flushQueue();
  assert.deepEqual(left(), []);
  assert.equal(await received(next), '[durun-dashboard] 人間から:\n二つ目');
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

test('新しいセッションにテーマを渡すと、起動の出力のジョブの id から引いたセッションに付け、引けなくても起動は成功として扱う', async () => {
  // 偽の claude は、起動したジョブ j20 を agents の一覧に載せる。依頼が lost のときだけ、一覧に無いジョブ j99 を起動したと書く。
  const bin = path.join(tmp, 'bin-theme');
  const jobs = [{ id: 'j20', sessionId: 's20', kind: 'background', pid: 1 }];
  useFakeClaude(
    bin,
    `if (args[0] === 'agents') console.log(${js(JSON.stringify(jobs))});
if (args[0] === '--bg') console.log('backgrounded · ' + (args.at(-1) === 'lost' ? 'j99' : 'j20'));`,
  );

  await startSession(tmp, '依頼', undefined, undefined, '起動時のテーマ');
  assert.deepEqual([state.sessions.s20?.cwd, state.sessions.s20?.theme], [tmp, '起動時のテーマ']);

  const before = Object.keys(state.sessions).length;
  await startSession(tmp, 'lost', undefined, undefined, '付かないテーマ');
  assert.equal(Object.keys(state.sessions).length, before);
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

test('モデルと effort を変えるときは、止めてからフラグを付けて再開し、複製ができたら設定とやり取りを複製へ移して元をアーカイブする', async () => {
  // 偽の claude は呼ばれた引数を書き足す。フラグ付きの再開は、本物と同じく複製のジョブ j9 を起動したと書く。agents は、元の j8 と複製の j9 を返す。
  const bin = path.join(tmp, 'bin-switch');
  const callsFile = path.join(tmp, 'calls-switch');
  const roster = path.join(tmp, 'claude', 'daemon', 'roster.json');
  const jobs = [
    { id: 'j8', sessionId: 's8', kind: 'background' },
    { id: 'j9', sessionId: 's9', kind: 'background', pid: 1 },
  ];
  fs.writeFileSync(roster, JSON.stringify({ workers: { j8: {} } }));
  useFakeClaude(
    bin,
    `fs.appendFileSync(${js(callsFile)}, lines([...args, '']));
if (args[0] === 'agents') console.log(${js(JSON.stringify(jobs))});
if (args[0] === 'stop') later(300, ${js(dropWorkers(roster))});
if (args[0] === '--bg') console.log('note: the flags you passed started a copy as j9. backgrounded · j9');`,
  );
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's8.json'), JSON.stringify({ pid: process.pid, sessionId: 's8', cwd: tmp, status: 'idle', kind: 'bg', jobId: 'j8' }));
  Object.assign(ensureSession('s8', tmp), { title: '調査', theme: 'テーマ', pinned: true });
  state.docs.push({ id: 'd8', sessionId: 's8', title: '資料', format: 'markdown', content: '', createdAt: 0, seen: true });
  state.sessionOrder = ['s8'];

  assert.equal(await switchSession('s8', '続けて', 'opus', 'high'), 's9');
  const calls = fs.readFileSync(callsFile, 'utf8').split('\n\n').slice(0, -1).map((c) => c.split('\n'));
  assert.deepEqual(calls, [
    ['stop', 'j8'],
    ['--bg', '--resume', 's8', '--model', 'opus', '--effort', 'high', '--', '続けて'],
    ['agents', '--json', '--all'],
    ['agents', '--json', '--all'],
    ['rm', 'j8'],
  ]);
  const copy = state.sessions.s9;
  assert.deepEqual([copy?.title, copy?.theme, copy?.pinned, copy?.archived], ['調査', 'テーマ', true, false]);
  assert.equal(state.sessions.s8?.archived, true);
  assert.equal(state.docs.find((d) => d.id === 'd8')?.sessionId, 's9');
  assert.deepEqual(state.sessionOrder, ['s9']);
  const sent = state.outbox.find((m) => m.text === '続けて');
  assert.equal(sent?.sessionId, 's9');
  assert.ok(sent?.deliveredAt);

  // 作業中のセッションは止められないので、変えない。
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's10.json'), JSON.stringify({ pid: process.pid, sessionId: 's10', cwd: tmp, status: 'busy', kind: 'bg', jobId: 'j10' }));
  ensureSession('s10', tmp);
  await assert.rejects(switchSession('s10', '続けて', 'opus'), /変えられません/);
  assert.equal(state.outbox.some((m) => m.sessionId === 's10'), false);
});

test('分岐するときは、元のセッションを止めずに --fork-session を付けて再開し、分岐先にテーマと JIRA の課題を付けて、渡した依頼を記録する', async () => {
  // 偽の claude は呼ばれた引数を書き足す。分岐は、新しいジョブ j12 を起動したと書く。agents は、作業中の元の j11 と分岐先の j12 を返す。
  const bin = path.join(tmp, 'bin-fork');
  const callsFile = path.join(tmp, 'calls-fork');
  const jobs = [
    { id: 'j11', sessionId: 's11', kind: 'background', pid: 1 },
    { id: 'j12', sessionId: 's12', kind: 'background', pid: 1 },
  ];
  useFakeClaude(
    bin,
    `fs.appendFileSync(${js(callsFile)}, lines([...args, '']));
if (args[0] === 'agents') console.log(${js(JSON.stringify(jobs))});
if (args[0] === '--bg') console.log('backgrounded · j12');`,
  );
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's11.json'), JSON.stringify({ pid: process.pid, sessionId: 's11', cwd: tmp, status: 'busy', kind: 'bg', jobId: 'j11' }));
  Object.assign(ensureSession('s11', tmp), { title: '調査', theme: 'テーマ', jira: ['PROJ-1'], pinned: true });

  assert.equal(await forkSession('s11', '別の案で', 'opus', 'high'), 's12');
  const calls = fs.readFileSync(callsFile, 'utf8').split('\n\n').slice(0, -1).map((c) => c.split('\n'));
  assert.deepEqual(calls, [
    ['--bg', '--resume', 's11', '--fork-session', '--model', 'opus', '--effort', 'high', '--', '別の案で'],
    ['agents', '--json', '--all'],
  ]);
  const fork = state.sessions.s12;
  assert.deepEqual([fork?.title, fork?.theme, fork?.jira, fork?.pinned], [undefined, 'テーマ', ['PROJ-1'], undefined]);
  assert.notEqual(fork?.jira, state.sessions.s11?.jira);
  assert.deepEqual([state.sessions.s11?.archived, state.sessions.s11?.pinned], [false, true]);
  const sent = state.outbox.find((m) => m.text === '別の案で');
  assert.equal(sent?.sessionId, 's12');
  assert.ok(sent?.deliveredAt);
});
