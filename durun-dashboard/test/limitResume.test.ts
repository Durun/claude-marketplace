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
const { answerOffline, checkLimit, onTranscriptChange, setAutoContinue } = await import('../server/limitResume.ts');
const { answerAuth, authRequests } = await import('../server/auth.ts');
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

test('自動継続モードでなければ、通信の途切れで止まったセッションには送らず、確認のダイアログで OK を押したら「続けて」とだけ 1 度送る。× で閉じたら送らず、同じ停止ではもう尋ねない', async (t) => {
  const socketPath = platform.localSocketPath(tmp, 'ask');
  fs.writeFileSync(path.join(sessionsDir, 'ask.json'), JSON.stringify({ pid: process.pid, sessionId: 'ask', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  const { server, received } = await listen(socketPath);
  const auth = platform.inboxToken(process.pid, sessionsDir) ? [{ type: 'auth', token: 'test-token' }] : [];
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(path.join(sessionsDir, 'ask.json'));
  });

  const file = transcriptOf('ask', [offline(Date.now() - 2 * MINUTE)]);
  checkLimit('ask');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(received, []);

  await answerOffline('ask', true);
  await until(() => received.length > 0);
  assert.deepEqual(received, [[...auth, { type: 'user', message: { role: 'user', content: '続けて' } }]]);
  // 続きの依頼が会話ログに載る前に、ほかの画面の OK を押されても、2 度は送らない。
  await answerOffline('ask', true);

  // 続けた後に、また止まった。× で閉じたら送らず、その後に OK を押されても送らない。
  fs.appendFileSync(file, `${prompt(Date.now() - MINUTE, '続けて')}\n${offline(Date.now())}\n`);
  await answerOffline('ask', false);
  await answerOffline('ask', true);
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 1);
});

test('自動継続モードなら、通信の途切れで止まったセッションに、API に届くようになってから「続けて」とだけ 1 度送る。止まった後にモードを入れたときも送る', async (t) => {
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
    state.autoContinue = undefined;
    delete process.env.ANTHROPIC_BASE_URL;
    api.closeAllConnections();
    await Promise.all([new Promise((resolve) => api.close(resolve)), new Promise((resolve) => server.close(resolve))]);
    fs.rmSync(path.join(sessionsDir, 'away.json'));
  });

  const continued = [...auth, { type: 'user', message: { role: 'user', content: '続けて' } }];
  online = true;
  const file = transcriptOf('away', [offline(Date.now() - 2 * MINUTE)]);
  // 自動継続モードを入れるまでは、API に届いても送らない。
  checkLimit('away');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(received, []);
  setAutoContinue(true);
  await until(() => received.length > 0);
  assert.deepEqual(received, [continued]);

  // 続けた後に、また止まった。届かない間は送らない。
  fs.appendFileSync(file, `${prompt(Date.now() - MINUTE, '続けて')}\n${offline(Date.now())}\n`);
  online = false;
  checkLimit('away');
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 1);

  // 確かめ直したときに届けば送る。
  online = true;
  checkLimit('away');
  await until(() => received.length > 1);
  assert.deepEqual(received, [continued, continued]);
  checkLimit('away');
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 2);
});

/** Claude Code が、Claude のログインが切れて API に断られたときに会話ログへ残す合成の発言。 */
const loginExpired = (at: number, text = 'Login expired · Please run /login'): string => JSON.stringify({
  type: 'assistant', timestamp: new Date(at).toISOString(),
  message: { id: `synthetic-${at}`, model: '<synthetic>', role: 'assistant', content: [{ type: 'text', text }], usage: { input_tokens: 0, output_tokens: 0 } },
  error: 'authentication_failed', isApiErrorMessage: true,
});

test('summarizeLog は、ログインが切れた合成の発言から止まった時刻を拾い、続きのやり取りが載ったら消す。Claude が同じ文を書いても拾わない', () => {
  const at = Date.parse('2026-10-23T06:43:58.820Z');
  // ログインが切れたのと同じころに、トークンを更新できなかったセッションは別の文で止まる。
  const oauth = path.join(tmp, 'oauth.jsonl');
  fs.writeFileSync(oauth, `${loginExpired(at, 'Failed to authenticate: OAuth session expired and could not be refreshed')}\n`);
  assert.equal(summarizeLog(oauth).loginExpiredAt, at);
  const file = path.join(tmp, 'login.jsonl');
  fs.writeFileSync(file, `${loginExpired(at)}\n`);
  assert.equal(summarizeLog(file).loginExpiredAt, at);
  fs.appendFileSync(file, `${prompt(at + MINUTE, '続けて')}\n`);
  assert.equal(summarizeLog(file).loginExpiredAt, undefined);
  const quoted = { type: 'assistant', timestamp: new Date(at + 2 * MINUTE).toISOString(), message: { role: 'assistant', content: [{ type: 'text', text: 'Login expired · Please run /login と出て止まりました。' }] } };
  fs.appendFileSync(file, `${JSON.stringify(quoted)}\n`);
  assert.equal(summarizeLog(file).loginExpiredAt, undefined);
});

test('自動継続モードなら、ログインが切れて止まったセッションのために Claude のログインを 1 回だけ尋ね、OK なら claude auth login でログインし直してから「続けて」とだけ 1 度送る', async (t) => {
  const socketPath = platform.localSocketPath(tmp, 'expired');
  fs.writeFileSync(path.join(sessionsDir, 'expired.json'), JSON.stringify({ pid: process.pid, sessionId: 'expired', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  const { server, received } = await listen(socketPath);
  const auth = platform.inboxToken(process.pid, sessionsDir) ? [{ type: 'auth', token: 'test-token' }] : [];
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-bin-'));
  const argsFile = path.join(bin, 'args');
  useFakeClaude(bin, `fs.appendFileSync(${js(argsFile)}, lines([args.join(' ')]));`);
  t.after(async () => {
    state.autoContinue = undefined;
    state.claudeLoginAt = undefined;
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(path.join(sessionsDir, 'expired.json'));
  });

  setAutoContinue(true);
  transcriptOf('expired', [loginExpired(Date.now() - MINUTE)]);
  onTranscriptChange(path.join('proj', 'expired.jsonl'));
  await until(() => authRequests().length > 0);
  const [request] = authRequests();
  assert.deepEqual(request, { id: request?.id, title: 'Claude のログイン', command: 'claude auth login', sessionIds: ['expired'] });
  answerAuth(request!.id, true);
  await until(() => received.length > 0);
  assert.equal(fs.readFileSync(argsFile, 'utf8'), 'auth login\n');
  assert.deepEqual(received, [[...auth, { type: 'user', message: { role: 'user', content: '続けて' } }]]);

  // 続きの依頼が会話ログに載る前に呼ばれても、同じ停止ではもう尋ねない。
  checkLimit('expired');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(authRequests(), []);
  assert.equal(received.length, 1);
});

test('自動継続モードでなければ、Claude にログインし直しても送らない。ログインに失敗したらまた尋ね、却下したらログインせず、同じ停止ではもう尋ねない', async (t) => {
  const socketPath = platform.localSocketPath(tmp, 'relogin');
  fs.writeFileSync(path.join(sessionsDir, 'relogin.json'), JSON.stringify({ pid: process.pid, sessionId: 'relogin', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  const { server, received } = await listen(socketPath);
  const bin = fs.mkdtempSync(path.join(os.tmpdir(), 'durun-dashboard-bin-'));
  const argsFile = path.join(bin, 'args');
  const failFile = path.join(bin, 'fail');
  useFakeClaude(bin, `
    fs.appendFileSync(${js(argsFile)}, lines([args.join(' ')]));
    if (fs.existsSync(${js(failFile)})) {
      console.log('login failed');
      process.exitCode = 1;
    }
  `);
  t.after(async () => {
    state.claudeLoginAt = undefined;
    await new Promise((resolve) => server.close(resolve));
    fs.rmSync(path.join(sessionsDir, 'relogin.json'));
  });
  const logins = (): number => (fs.existsSync(argsFile) ? fs.readFileSync(argsFile, 'utf8').split('\n').filter(Boolean).length : 0);

  // 1 回目のログインは失敗し、また尋ねる。
  fs.writeFileSync(failFile, '');
  const file = transcriptOf('relogin', [loginExpired(Date.now() - 2 * MINUTE)]);
  checkLimit('relogin');
  await until(() => authRequests().length > 0);
  const first = authRequests()[0]!.id;
  answerAuth(first, true);
  await until(() => authRequests().some((r) => r.id !== first));
  assert.equal(logins(), 1);

  // ログインし直せても、自動継続モードでなければ送らない。
  fs.rmSync(failFile);
  answerAuth(authRequests()[0]!.id, true);
  await until(() => logins() === 2);
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(authRequests(), []);

  // 人間が続けて、またログインが切れた。却下したらログインせず、同じ停止ではもう尋ねない。
  fs.appendFileSync(file, `${prompt(Date.now() - MINUTE, '続けて')}\n${loginExpired(Date.now())}\n`);
  checkLimit('relogin');
  await until(() => authRequests().length > 0);
  answerAuth(authRequests()[0]!.id, false);
  await new Promise((r) => setTimeout(r, 300));
  checkLimit('relogin');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(authRequests(), []);
  assert.equal(logins(), 2);
  assert.deepEqual(received, []);
});

/** API が応答した発言。使ったトークンが 0 でない。 */
const answered = (at: number): string => JSON.stringify({
  type: 'assistant', timestamp: new Date(at).toISOString(),
  message: { id: `msg-${at}`, model: 'claude-test', role: 'assistant', content: [{ type: 'text', text: '終わりました。' }], usage: { input_tokens: 10, output_tokens: 5 } },
});

test('ログインが切れた後に、動いているほかのセッションに API が応答していれば、ログインは戻っているので尋ねず、自動継続モードのときだけ「続けて」とだけ 1 度送る', async (t) => {
  const socketPath = platform.localSocketPath(tmp, 'stale');
  fs.writeFileSync(path.join(sessionsDir, 'stale.json'), JSON.stringify({ pid: process.pid, sessionId: 'stale', cwd: tmp, status: 'idle', messagingSocketPath: socketPath }));
  fs.writeFileSync(path.join(sessionsDir, 'other.json'), JSON.stringify({ pid: process.pid, sessionId: 'other', cwd: tmp, status: 'idle' }));
  const { server, received } = await listen(socketPath);
  const auth = platform.inboxToken(process.pid, sessionsDir) ? [{ type: 'auth', token: 'test-token' }] : [];
  t.after(async () => {
    state.autoContinue = undefined;
    await new Promise((resolve) => server.close(resolve));
    for (const f of ['stale.json', 'other.json']) fs.rmSync(path.join(sessionsDir, f));
  });

  const other = transcriptOf('other', [answered(Date.now() - MINUTE)]);
  const stale = transcriptOf('stale', [loginExpired(Date.now() - 2 * MINUTE)]);
  checkLimit('stale');
  await new Promise((r) => setTimeout(r, 300));
  assert.deepEqual(authRequests(), []);
  assert.deepEqual(received, []);

  setAutoContinue(true);
  await until(() => received.length > 0);
  assert.deepEqual(received, [[...auth, { type: 'user', message: { role: 'user', content: '続けて' } }]]);

  // 続けた後に、また止まり、ほかのセッションも続けて止まった。止まる前の応答と、止まったときの合成の発言は、ログインが戻った印にならないので尋ねる。
  const at = Date.now();
  fs.appendFileSync(stale, `${prompt(at, '続けて')}\n${loginExpired(at)}\n`);
  fs.appendFileSync(other, `${prompt(at, '続けて')}\n${loginExpired(at + 1)}\n`);
  checkLimit('stale');
  await until(() => authRequests().length > 0);
  answerAuth(authRequests()[0]!.id, false);
  await new Promise((r) => setTimeout(r, 300));
  assert.equal(received.length, 1);
});
