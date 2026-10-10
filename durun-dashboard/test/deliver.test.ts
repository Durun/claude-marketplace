import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-deliver-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { postToInbox, removeSession, sendMessage, startSession } = await import('../server/deliver.ts');
const { ensureSession, onChange } = await import('../server/store.ts');
const { platform, platformFor } = await import('../server/platform.ts');
fs.mkdirSync(path.join(tmp, 'claude', 'sessions'), { recursive: true });

/** supervisor が名簿から動いているジョブを消すコード。書きかけを読ませないよう、別名で書いてから置き換える。 */
const dropWorkers = (roster: string): string => `fs.writeFileSync(${js(`${roster}.tmp`)}, '{"workers":{}}'); fs.renameSync(${js(`${roster}.tmp`)}, ${js(roster)});`;

/** 受信ソケットに見立てたサーバーを開き、次に届く 1 行を返す。 */
const listenInbox = async (socketPath: string): Promise<{ server: net.Server; line: Promise<{ type: string; message: { role?: string; content: string } }> }> => {
  const server = net.createServer();
  const line = new Promise<{ type: string; message: { role?: string; content: string } }>((resolve) =>
    server.once('connection', (c) => {
      let buf = '';
      c.on('data', (chunk) => (buf += chunk.toString()));
      c.on('end', () => resolve(JSON.parse(buf)));
    }),
  );
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  return { server, line };
};

test('受信ソケットへ送れない OS では、届いたことにせずに理由を返し、送れる OS では 1 行の JSON で届ける', async () => {
  const socketPath = platform.localSocketPath(tmp, 'direct');
  await assert.rejects(postToInbox(socketPath, 'こんにちは', platformFor('win32')), /認証が要り/);

  const inbox = await listenInbox(socketPath);
  await postToInbox(socketPath, 'こんにちは', platformFor('darwin'));
  assert.deepEqual(await inbox.line, { type: 'user', message: { role: 'user', content: 'こんにちは' } });
  await new Promise((resolve) => inbox.server.close(resolve));
});

test('動いているセッションには受信ソケットへ送り、届かなければ未配信に戻して、次に送るときに一緒に届け直す', { skip: platform.inboxBlocker }, async () => {
  // 自分のプロセスを、受信ソケットを持って生きているセッションに見立てる。
  const socketPath = platform.localSocketPath(tmp, 'inbox');
  fs.writeFileSync(path.join(tmp, 'claude', 'sessions', 's1.json'), JSON.stringify({ pid: process.pid, sessionId: 's1', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  ensureSession('s1', tmp);

  const first = await listenInbox(socketPath);
  assert.equal(sendMessage('s1', 'こんにちは').delivery, 'sent');
  const line = await first.line;
  assert.equal(line.type, 'user');
  assert.equal(line.message.content, '[durun-dashboard] 人間から:\nこんにちは');
  await new Promise((resolve) => first.server.close(resolve));

  const lost = sendMessage('s1', '届かない').message;
  await new Promise<void>((resolve) => onChange(() => lost.error && resolve()));
  assert.equal(lost.deliveredAt, undefined);

  const second = await listenInbox(socketPath);
  sendMessage('s1', 'もう一度');
  assert.equal((await second.line).message.content, '[durun-dashboard] 人間から:\n届かない\n---\nもう一度');
  assert.ok(lost.deliveredAt);
  await new Promise((resolve) => second.server.close(resolve));
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
