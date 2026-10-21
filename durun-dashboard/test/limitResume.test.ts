import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { js, useFakeClaude } from './fakeClaude.ts';

// 実際の状態やセッションに触れないよう、データと Claude Code の置き場所を一時ディレクトリに向けてから読み込む。
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-limit-'));
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.CLAUDE_CONFIG_DIR = path.join(tmp, 'claude');
const { summarizeLog } = await import('../server/claude.ts');
const { checkLimit, onTranscriptChange } = await import('../server/limitResume.ts');
const { ensureImprover, improveState, nextImprovement } = await import('../server/improve.ts');
const { ensureSession, state } = await import('../server/store.ts');
const { platform } = await import('../server/platform.ts');
const sessionsDir = path.join(tmp, 'claude', 'sessions');
const projectDir = path.join(tmp, 'claude', 'projects', 'proj');
fs.mkdirSync(sessionsDir, { recursive: true });
fs.mkdirSync(projectDir, { recursive: true });
// 受信ソケットに認証の要る OS が読む、このプロセスに見立てたセッションの鍵。要らない OS では読まれない。
fs.writeFileSync(path.join(sessionsDir, `${process.pid}.test.key`), JSON.stringify({ peerToken: 'test-token' }));

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
// リセットの時刻は秒単位で書かれる。
const toSecond = (ms: number): number => Math.floor(ms / SECOND) * SECOND;

/** Claude Code が、使用制限で断られたときに会話ログへ残す合成の発言。resetsAt は epoch ミリ秒で受け、秒にして書く。 */
const limited = (at: number, resetsAt: number): string => JSON.stringify({
  type: 'assistant', timestamp: new Date(at).toISOString(),
  message: { model: '<synthetic>', role: 'assistant', content: [{ type: 'text', text: "You've hit your session limit · resets 2:50pm (Asia/Tokyo)" }] },
  apiError: 'usage_limit_reached', apiErrorParams: { rate_limit_info: { status: 'rejected', resetsAt: resetsAt / SECOND, rateLimitType: 'five_hour' } },
  error: 'rate_limit', isApiErrorMessage: true,
});
const prompt = (at: number, text: string): string => JSON.stringify({ type: 'user', timestamp: new Date(at).toISOString(), origin: { kind: 'human' }, message: { role: 'user', content: text } });

/** projects の下に、session の会話ログを lines で書き、ダッシュボードのセッションとして登録する。 */
const transcriptOf = (sessionId: string, lines: string[]): string => {
  const file = path.join(projectDir, `${sessionId}.jsonl`);
  fs.writeFileSync(file, lines.map((l) => `${l}\n`).join(''));
  ensureSession(sessionId, tmp).transcriptPath = file;
  return file;
};

/** 受信ソケットに見立てたサーバー。届いた接続ごとに、届いた行を集める。 */
const listen = async (socketPath: string): Promise<{ server: net.Server; received: unknown[][] }> => {
  const server = net.createServer();
  const received: unknown[][] = [];
  server.on('connection', (c) => {
    let buf = '';
    c.on('data', (chunk) => (buf += chunk.toString()));
    c.on('close', () => received.push(buf.split('\n').filter(Boolean).map((line) => JSON.parse(line) as unknown)));
  });
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  return { server, received };
};

const until = async (ok: () => boolean): Promise<void> => {
  for (let i = 0; i < 100 && !ok(); i++) await new Promise((r) => setTimeout(r, 50));
};

test('summarizeLog は、使用制限で断られた発言から止まった時刻とリセットの時刻を拾い、続きのやり取りが載ったら消す', () => {
  const at = Date.parse('2026-10-14T05:29:58.712Z');
  const resetsAt = Date.parse('2026-10-14T05:50:00Z');
  const file = path.join(tmp, 'summary.jsonl');
  // 断られた後にも、やり取りではない記録が続く。
  fs.writeFileSync(file, `${limited(at, resetsAt)}\n${JSON.stringify({ type: 'last-prompt', lastPrompt: '依頼' })}\n`);
  assert.deepEqual(summarizeLog(file).limit, { at, resetsAt });
  fs.appendFileSync(file, `${prompt(resetsAt + MINUTE, '続けて')}\n`);
  assert.equal(summarizeLog(file).limit, undefined);
});

test('制限が明けたセッションには、前置きを付けずに「続けて」とだけ 1 度送り、明けていなければ送らずに待つ', async () => {
  const socketPath = platform.localSocketPath(tmp, 'inbox');
  fs.writeFileSync(path.join(sessionsDir, 'term.json'), JSON.stringify({ pid: process.pid, sessionId: 'term', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  const { server, received } = await listen(socketPath);
  const auth = platform.inboxToken(process.pid, sessionsDir) ? [{ type: 'auth', token: 'test-token' }] : [];
  const now = Date.now();

  const file = transcriptOf('term', [limited(now - 3 * HOUR, toSecond(now + HOUR))]);
  onTranscriptChange(path.join('proj', 'term.jsonl'));
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(received, []);

  // 人間が続きを頼んで、また制限で止まり、その制限がもう明けている。サブエージェントの会話ログは見ない。
  fs.appendFileSync(file, `${prompt(now - 2 * HOUR, '続けて')}\n${limited(now - HOUR, toSecond(now - 2 * MINUTE))}\n`);
  onTranscriptChange(path.join('proj', 'term', 'subagents', 'agent-1.jsonl'));
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(received, []);
  onTranscriptChange(path.join('proj', 'term.jsonl'));
  await until(() => received.length > 0);
  assert.deepEqual(received, [[...auth, { type: 'user', message: { role: 'user', content: '続けて' } }]]);

  // 続きの依頼が会話ログに載る前に呼ばれても、同じ制限では頼み直さない。
  checkLimit('term');
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 1);
  await new Promise((resolve) => server.close(resolve));
});

test('使用制限で止まった改善のウェーブは区切らずに待ち、制限が明けたら同じセッションに「続けて」と頼んで続ける', async () => {
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-bin-'));
  const argsFile = path.join(bin, 'args');
  useFakeClaude(bin, `fs.writeFileSync(${js(argsFile)}, lines(args));`);
  const resetsAt = toSecond(Date.now() - 2 * MINUTE);
  transcriptOf('wave', [limited(Date.now() - HOUR, resetsAt)]);
  // プロセスは制限で終わっている。
  state.improve.run = { sessionId: 'wave', pid: 0, items: [], summary: '2 件の要望のウェーブ', startedAt: 0 };

  nextImprovement('wave', "You've hit your session limit · resets 2:50pm (Asia/Tokyo)");
  ensureImprover();
  assert.equal(state.improve.run?.sessionId, 'wave');
  assert.equal(state.improve.error, undefined);
  assert.deepEqual([improveState().running, improveState().current, improveState().limitResetsAt], [false, '2 件の要望のウェーブ', resetsAt]);

  checkLimit('wave');
  await until(() => fs.existsSync(argsFile));
  const args = fs.readFileSync(argsFile, 'utf8').trim().split('\n');
  assert.deepEqual(args.slice(0, 3), ['-p', '--resume', 'wave']);
  assert.equal(args.at(-1), '続けて');
  assert.equal(state.improve.run?.sessionId, 'wave');
});

/** Claude Code が、応答の途中でスリープしたときなどに会話ログへ残す合成の発言。 */
const offline = (at: number, text = 'API Error: Your computer went to sleep mid-response. The response above may be incomplete.'): string => JSON.stringify({
  type: 'assistant', timestamp: new Date(at).toISOString(),
  message: { model: '<synthetic>', role: 'assistant', content: [{ type: 'text', text }] },
  error: 'server_error', isApiErrorMessage: true,
});

test('summarizeLog は、通信の途切れで止まった合成の発言から止まった時刻を拾い、続きのやり取りが載ったら消す。Claude が同じ文を書いても拾わない', () => {
  const at = Date.parse('2026-10-19T14:46:08.480Z');
  const file = path.join(tmp, 'offline.jsonl');
  fs.writeFileSync(file, `${offline(at, "API Error: Can't reach the API server — check your internet or DNS (ENOTFOUND)")}\n`);
  assert.equal(summarizeLog(file).offlineAt, at);
  fs.appendFileSync(file, `${prompt(at + MINUTE, '続けて')}\n`);
  assert.equal(summarizeLog(file).offlineAt, undefined);
  const quoted = { type: 'assistant', timestamp: new Date(at + 2 * MINUTE).toISOString(), message: { role: 'assistant', content: [{ type: 'text', text: 'Request timed out と出て止まりました。' }] } };
  fs.appendFileSync(file, `${JSON.stringify(quoted)}\n`);
  assert.equal(summarizeLog(file).offlineAt, undefined);
});

test('通信の途切れで止まったセッションには、API に届くようになってから「続けて」とだけ 1 度送る', async (t) => {
  const socketPath = platform.localSocketPath(tmp, 'away');
  fs.writeFileSync(path.join(sessionsDir, 'away.json'), JSON.stringify({ pid: process.pid, sessionId: 'away', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  const { server, received } = await listen(socketPath);
  const auth = platform.inboxToken(process.pid, sessionsDir) ? [{ type: 'auth', token: 'test-token' }] : [];
  // API のサーバーに見立てる。つながらない間は、応えずに接続を切る。
  let online = false;
  const api = http.createServer((req, res) => (online ? res.end() : req.socket.destroy()));
  await new Promise<void>((resolve) => api.listen(0, '127.0.0.1', resolve));
  process.env.ANTHROPIC_BASE_URL = `http://127.0.0.1:${(api.address() as net.AddressInfo).port}`;
  // 失敗しても、サーバーを閉じてテストを終わらせる。
  t.after(async () => {
    delete process.env.ANTHROPIC_BASE_URL;
    api.closeAllConnections();
    await Promise.all([new Promise((resolve) => api.close(resolve)), new Promise((resolve) => server.close(resolve))]);
    fs.rmSync(path.join(sessionsDir, 'away.json'));
  });

  transcriptOf('away', [offline(Date.now() - MINUTE)]);
  checkLimit('away');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(received, []);

  // 確かめ直したときに届けば送る。
  online = true;
  checkLimit('away');
  await until(() => received.length > 0);
  assert.deepEqual(received, [[...auth, { type: 'user', message: { role: 'user', content: '続けて' } }]]);
  checkLimit('away');
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 1);
});
