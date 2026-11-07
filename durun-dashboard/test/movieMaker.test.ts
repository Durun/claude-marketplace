import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import fs from 'node:fs';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import path from 'node:path';
import { test } from 'node:test';
import { describeMmChange, mmSteps } from '../client/mmSteps.ts';
import { buildPlan, installModeOf } from '../server/movieMakerInstall.ts';
import { findLink, hookText, isHuman, projectSummary, SseParser } from '../server/movieMakerLink.ts';
import { platform, platformFor } from '../server/platform.ts';
import type { MmChange, MovieMakerView } from '../shared/types.ts';
import { tempDir } from './tmp.ts';

// 実際の状態に触れないよう、データとトークンの置き場所を一時ディレクトリに向けてから中枢を読み込む。
const tmp = tempDir('claude-moviemaker-');
process.env.DURUN_DASHBOARD_DATA_DIR = path.join(tmp, 'data');
process.env.LOCALAPPDATA = path.join(tmp, 'local');
process.env.XDG_RUNTIME_DIR = path.join(tmp, 'local');
const { callMmTool, mmHookContext, streamMovieMaker } = await import('../server/movieMaker.ts');
const tokenDir = platform.movieMakerTokenDir(process.env) ?? '';

const change = (fields: Partial<MmChange>): MmChange => ({ version: 1, txn_id: 1, kind: 'apply', name: 'addMarker', actor: 'gui', touched: ['m1'], ...fields });

/** プロジェクトのフォルダーに、書き手の lock.info とトークンを置く。 */
const writeLock = (dir: string, info: object, token?: string): void => {
  fs.mkdirSync(path.join(dir, '.mm'), { recursive: true });
  fs.writeFileSync(path.join(dir, '.mm', 'lock.info'), JSON.stringify(info));
  if (token) {
    fs.mkdirSync(tokenDir, { recursive: true });
    fs.writeFileSync(path.join(tokenDir, 'tid'), `${token}\n`);
  }
};

test('lock.info の pid が生きていて、ポートとトークンがそろったときだけ、つなぐ先を返す', () => {
  const dir = tempDir('mm-project-');
  assert.match((findLink(dir, tokenDir) as { reason: string }).reason, /開いていません/);
  writeLock(dir, { pid: 4242, http_port: 5000, token_id: 'tid' }, 'secret');
  assert.match((findLink(dir, tokenDir, () => false) as { reason: string }).reason, /終了/);
  assert.deepEqual(findLink(dir, tokenDir, () => true), { pid: 4242, port: 5000, token: 'secret' });
  writeLock(dir, { pid: 4242, http_port: null, token_id: 'tid' });
  assert.match((findLink(dir, tokenDir, () => true) as { reason: string }).reason, /HTTP サーバー/);
});

test('人間の操作は actor が gui のものだけで、フックの文は生の値を 1 行ずつ並べ、多い分は数だけ添える', () => {
  assert.equal(isHuman(change({})), true);
  assert.equal(isHuman(change({ actor: 'mcp' })), false);
  assert.equal(isHuman(change({ actor: 'cli' })), false);
  assert.equal(hookText([], 5), undefined);
  const text = hookText([change({ txn_id: 1 }), change({ txn_id: 2 }), change({ txn_id: 3 })], 2) ?? '';
  const lines = text.split('\n');
  assert.match(lines[0] ?? '', /^ムービーメーカー: /);
  assert.deepEqual(lines.slice(1, 3).map((l) => (JSON.parse(l) as MmChange).txn_id), [2, 3]);
  assert.match(lines[3] ?? '', /ほかに 1 件/);
});

test('GET /project の中身を、コンポジション、レイヤー、sequence、素材の短い文にする', () => {
  const r = (num: number, den = 1) => ({ num, den });
  const text = projectSummary({
    version: 7,
    undo_depth: 2,
    project: {
      compositions: [{ id: 'main', width: 1920, height: 1080, fps: r(30000, 1001), duration: r(10), layers: [{ id: 'l1', source: { kind: 'sequence' }, in: r(0), out: r(10), lock: true }] }],
      sequences: [{ id: 'talk', cuts: [{}, {}] }],
      assets: [{ id: 'a1', path: 'clip.mp4' }],
    },
  });
  assert.equal(text, ['version 7、取り消せるトランザクション 2', 'composition main: 1920x1080、29.97 fps、10 秒', '  layer l1: sequence、0〜10 秒、ロック', 'sequence talk: カット 2 個', '素材 1 個: a1（clip.mp4）'].join('\n'));
});

test('SSE の流れは、塊の途中で切れても、イベントの名前とデータの組に分ける', () => {
  const parser = new SseParser();
  assert.deepEqual(parser.push('event: hello\ndata: {"version"'), []);
  assert.deepEqual(parser.push(':1}\n\n: keepalive\n\nevent: change\r\ndata: {}\r\n\r\n'), [
    { event: 'hello', data: '{"version":1}' },
    { event: 'change', data: '{}' },
  ]);
});

test('開発用のビルドは、専用の作業ツリーを origin/main に合わせ、UI、cargo の順で作る。FFMPEG_DIR があれば FFmpeg ありで作り、ライブラリも写す。配布用は環境変数で選ぶ', () => {
  const win = platformFor('win32');
  const paths = { repo: 'R', worktree: 'W', source: 'W/movie-maker', target: 'T', bin: 'B' };
  const commands = (exists: boolean) => buildPlan(paths, {}, win, () => [], () => exists).steps.map((s) => s.line ?? `${s.file} ${s.args?.join(' ')}`);
  assert.deepEqual(commands(false), ['git -C R fetch origin main', 'git -C R worktree add --detach W origin/main', 'pnpm install --frozen-lockfile && pnpm build', 'cargo build --release -p movie-maker -p mm']);
  assert.equal(commands(true)[1], 'git -C W checkout --detach --force origin/main');
  const plain = buildPlan(paths, {}, win, () => [], () => true);
  const cargo = plain.steps.at(-1);
  assert.deepEqual(plain.features, []);
  assert.deepEqual(cargo?.env, { CARGO_TARGET_DIR: 'T', CARGO_INCREMENTAL: '0' });
  assert.equal(cargo?.cwd, 'W/movie-maker');
  assert.deepEqual(plain.copies.map((c) => c.to), [path.join('B', 'movie-maker.exe'), path.join('B', 'mm.exe')]);
  const ff = buildPlan(paths, { FFMPEG_DIR: 'F' }, win, () => ['avcodec-62.dll', 'ffmpeg.exe'], () => true);
  assert.deepEqual(ff.features, ['ffmpeg']);
  assert.deepEqual(ff.steps.at(-1)?.args?.slice(-2), ['--features', 'movie-maker/ffmpeg,mm/ffmpeg']);
  assert.deepEqual(ff.copies.at(-1), { from: path.join('F', 'bin', 'avcodec-62.dll'), to: path.join('B', 'avcodec-62.dll') });
  assert.equal(installModeOf({}), 'build');
  assert.equal(installModeOf({ DURUN_DASHBOARD_DISTRIBUTION: '1' }), 'release');
});

const baseView = (fields: Partial<MovieMakerView>): MovieMakerView => ({ install: { mode: 'build', busy: false, output: [] }, hasStore: false, link: { status: 'no-project' }, log: [], ...fields });

test('次にすることは、入れる、プロジェクトを選ぶ、起動する、の順で、済んだら空になる', () => {
  assert.deepEqual(mmSteps(baseView({})).map((s) => s.action?.label ?? s.key), ['ビルドして入れる', 'project']);
  const installed = { mode: 'build' as const, path: 'B/movie-maker.exe', commit: 'aaa', sourceCommit: 'bbb', busy: false, output: [] };
  const waiting = baseView({ install: installed, project: 'D:/p', link: { status: 'waiting', message: 'movie-maker がこのプロジェクトを開いていません。' } });
  assert.deepEqual(mmSteps(waiting).map((s) => s.action?.label), ['作り直す', 'プロジェクトを作って起動する']);
  assert.deepEqual(mmSteps({ ...waiting, install: { ...installed, sourceCommit: 'aaa' }, hasStore: true, link: { status: 'connected' } }), []);
  assert.match(mmSteps(baseView({ install: { mode: 'release', busy: false, output: [], error: 'まだ' } }))[0]?.title ?? '', /入れられません/);
});

test('取り消しとやり直しの文は、戻した元の操作と、それが人間かエージェントかを添える', () => {
  assert.equal(describeMmChange(change({ touched: ['a', 'b'] })), 'addMarker（2 か所）');
  assert.equal(describeMmChange(change({ kind: 'undo', of: { txn_id: 3, name: 'ripple', actor: 'mcp', actor_name: 'claude' } })), '取り消し: ripple（エージェントの操作）');
});

/** 偽の mm-serve。トークンを確かめ、/project、/events、/mcp に答える。push で変更の通知を流す。 */
const fakeServe = async (token: string) => {
  const streams: http.ServerResponse[] = [];
  const calls: { name: string; arguments: unknown }[] = [];
  const push = (c: MmChange): void => streams.forEach((s) => s.write(`event: change\ndata: ${JSON.stringify(c)}\n\n`));
  const tools = [
    { name: 'addMarker', description: 'マーカーを足す。\n詳しく', inputSchema: { type: 'object', properties: { marker: { type: 'object' } } } },
    { name: 'undo', description: '取り消す。', inputSchema: { type: 'object', properties: {} } },
  ];
  const server = http.createServer((req, res) => {
    if (req.headers.authorization !== `Bearer ${token}`) return void res.writeHead(401).end();
    if (req.url === '/project') return void res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ version: 3, undo_depth: 1, project: { compositions: [] } }));
    if (req.url === '/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream' });
      res.write('event: hello\ndata: {"version":3}\n\n');
      streams.push(res);
      return;
    }
    let body = '';
    req.on('data', (c: Buffer) => (body += c.toString()));
    req.on('end', () => {
      const msg = JSON.parse(body) as { id: number; method: string; params: { name: string; arguments: unknown } };
      let result: object = { tools };
      if (msg.method === 'tools/call') {
        calls.push(msg.params);
        push(change({ actor: 'mcp', name: msg.params.name, version: 4 }));
        result = { content: [{ type: 'text', text: `called ${msg.params.name}` }] };
      }
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result }));
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { port: (server.address() as AddressInfo).port, push, calls, close: () => (streams.forEach((s) => s.end()), server.close()) };
};

/** ウィンドウの SSE の代わり。届いた view を覚える。 */
const fakeWindow = () => {
  const res = Object.assign(new EventEmitter(), {
    views: [] as MovieMakerView[],
    writableEnded: false,
    writeHead: () => res,
    write: (chunk: string) => {
      const m = /^event: view\ndata: (.*)\n\n$/s.exec(chunk);
      if (m?.[1]) res.views.push(JSON.parse(m[1]) as MovieMakerView);
      return true;
    },
  });
  return res;
};

const until = async (ok: () => boolean, what: string): Promise<void> => {
  for (let i = 0; i < 200 && !ok(); i++) await new Promise((r) => setTimeout(r, 10));
  assert.ok(ok(), `${what} を待ちきれなかった`);
};

const textOf = (r: { content: { text?: string }[] }): string => r.content.map((c) => c.text ?? '').join('\n');

test('試し台: ウィンドウを開くと動いている movie-maker につなぎ、mm_tool を中継し、人間の操作だけをフックで届ける', async () => {
  const serve = await fakeServe('tok');
  const dir = tempDir('mm-live-');
  fs.writeFileSync(path.join(dir, 'project.mmdb'), '');
  writeLock(dir, { pid: process.pid, http_port: serve.port, token_id: 'tid' }, 'tok');
  const win = fakeWindow();
  try {
    assert.match(textOf(await callMmTool('mm_state', 's1', {})), /動いていません/);
    streamMovieMaker(win as never, 's1', dir);
    await until(() => win.views.at(-1)?.link.status === 'connected', 'つながる');
    assert.equal(win.views.at(-1)?.hasStore, true);
    assert.match(textOf(await callMmTool('mm_state', 's2', {})), /別のセッション/);
    assert.match(textOf(await callMmTool('mm_state', 's1', {})), /version 3、取り消せるトランザクション 1/);
    assert.match(textOf(await callMmTool('mm_tool', 's1', {})), /addMarker: マーカーを足す。$/m);
    assert.match(textOf(await callMmTool('mm_tool', 's1', { tool: 'addMarker' })), /"marker"/);
    assert.equal(serve.calls.length, 0, '引数を省くとスキーマを返すだけで呼ばない');
    assert.equal(textOf(await callMmTool('mm_tool', 's1', { tool: 'undo' })), 'called undo');
    assert.equal(textOf(await callMmTool('mm_tool', 's1', { tool: 'addMarker', arguments: { marker: { id: 'm' } } })), 'called addMarker');
    assert.deepEqual(serve.calls, [{ name: 'undo', arguments: {} }, { name: 'addMarker', arguments: { marker: { id: 'm' } } }]);
    assert.ok((await callMmTool('mm_tool', 's1', { tool: 'nope' })).isError);
    serve.push(change({ txn_id: 9, name: 'ripple', version: 5 }));
    await until(() => win.views.at(-1)?.log.some((e) => e.who === 'you') ?? false, '人間の操作がログに出る');
    const hook = mmHookContext('s1') ?? '';
    assert.match(hook, /"txn_id":9/);
    assert.doesNotMatch(hook, /"actor":"mcp"/);
    assert.equal(mmHookContext('s1'), undefined, '届けた分は 2 度届けない');
    assert.equal(mmHookContext('s2'), undefined);
    assert.deepEqual(win.views.at(-1)?.log.map((e) => e.who), ['agent', 'agent', 'you']);
  } finally {
    win.emit('close');
    serve.close();
  }
  assert.match(textOf(await callMmTool('mm_state', 's1', {})), /動いていません/);
});
