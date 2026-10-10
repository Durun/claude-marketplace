import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-deliver-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { removeSession, sendMessage, startSession } = await import('../server/deliver.ts');
const { ensureSession, onChange } = await import('../server/store.ts');

/** 受信ソケットに見立てたサーバーを開き、次に届く 1 行を返す。 */
const listenInbox = async (socketPath: string): Promise<{ server: net.Server; line: Promise<{ type: string; message: { content: string } }> }> => {
  const server = net.createServer();
  const line = new Promise<{ type: string; message: { content: string } }>((resolve) =>
    server.once('connection', (c) => {
      let buf = '';
      c.on('data', (chunk) => (buf += chunk.toString()));
      c.on('end', () => resolve(JSON.parse(buf)));
    }),
  );
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  return { server, line };
};

test('動いているセッションには受信ソケットへ送り、届かなければ未配信に戻して、次に送るときに一緒に届け直す', async () => {
  // 自分のプロセスを、受信ソケットを持って生きているセッションに見立てる。
  const socketPath = path.join(tmp, 'inbox.sock');
  fs.mkdirSync(path.join(tmp, 'claude', 'sessions'), { recursive: true });
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
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(path.dirname(roster), { recursive: true });
  fs.writeFileSync(roster, JSON.stringify({ workers: { j2: {} } }));
  fs.writeFileSync(
    path.join(bin, 'claude'),
    [
      '#!/bin/sh',
      `printf '%s\\n' "$CMUX_CLAUDE_HOOKS_DISABLED" "$@" '' >> '${callsFile}'`,
      `[ "$1" != stop ] || { (sleep 0.3; echo '{"workers":{}}' > '${roster}.tmp'; mv '${roster}.tmp' '${roster}') & }`,
      `[ "$1" != --bg ] || { grep -q j2 '${roster}' && echo 複製になります >&2 || echo 再開できません >&2; exit 1; }`,
    ].join('\n'),
    { mode: 0o755 },
  );
  process.env.PATH = `${bin}${path.delimiter}${process.env.PATH}`;
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
  // 受け取った引数を書き出し、依頼が fail なら失敗する偽の claude を PATH の先頭に置く。
  const bin = path.join(tmp, 'bin');
  const argsFile = path.join(tmp, 'args');
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, 'claude'), `#!/bin/sh\nprintf '%s\\n' "$@" > '${argsFile}'\n[ "$3" != fail ] || { echo 起動できません >&2; exit 1; }\n`, { mode: 0o755 });
  process.env.PATH = `${bin}${path.delimiter}${process.env.PATH}`;

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
  fs.mkdirSync(bin, { recursive: true });
  fs.mkdirSync(path.dirname(roster), { recursive: true });
  fs.writeFileSync(roster, JSON.stringify({ workers: { j3: {} } }));
  fs.writeFileSync(
    path.join(bin, 'claude'),
    [
      '#!/bin/sh',
      `printf '%s\\n' "$@" '' >> '${callsFile}'`,
      `[ "$1" != agents ] || echo '${JSON.stringify(jobs)}'`,
      `[ "$1" != stop ] || { (sleep 0.3; echo '{"workers":{}}' > '${roster}.tmp'; mv '${roster}.tmp' '${roster}') & }`,
      `[ "$1 $2" != 'rm j3' ] || ! grep -q j3 '${roster}' || { echo 動いています >&2; exit 1; }`,
      `[ "$1 $2" != 'rm j5' ] || { echo 未コミットの変更があります >&2; exit 1; }`,
    ].join('\n'),
    { mode: 0o755 },
  );
  process.env.PATH = `${bin}${path.delimiter}${process.env.PATH}`;
  const calls = (): string[][] => fs.readFileSync(callsFile, 'utf8').split('\n\n').slice(0, -1).map((c) => c.split('\n'));

  await removeSession('s3');
  assert.deepEqual(calls(), [['agents', '--json', '--all'], ['stop', 'j3'], ['rm', 'j3']]);
  fs.rmSync(callsFile);
  await removeSession('s4');
  assert.deepEqual(calls(), [['agents', '--json', '--all']]);
  await assert.rejects(removeSession('s5'), /未コミットの変更があります/);
});
