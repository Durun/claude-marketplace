import assert from 'node:assert/strict';
import fs from 'node:fs';
import type { IncomingMessage } from 'node:http';
import path from 'node:path';
import { test } from 'node:test';
import { fillCallerPid } from '../server/callerPid.ts';
import { claudeCommand, ownerInNetstat, type Platform, platformFor, type Run } from '../server/platform.ts';

const NETSTAT = `
アクティブな接続

  プロトコル  ローカル アドレス      外部アドレス           状態            PID
  TCP         127.0.0.1:7337         0.0.0.0:0              LISTENING       29012
  TCP         127.0.0.1:7337         127.0.0.1:50692        ESTABLISHED     29012
  TCP         127.0.0.1:50692        127.0.0.1:7337         TIME_WAIT       0
  TCP         127.0.0.1:50692        127.0.0.1:7337         ESTABLISHED     33544
  TCP         127.0.0.1:150692       127.0.0.1:7337         ESTABLISHED     11111
  TCP         [::1]:50700            [::1]:7337             ESTABLISHED     22222
`;

/** 呼ばれたコマンドを記録し、コマンドごとに決めた出力を返す。 */
const fakeRun = (outputs: Record<string, string>): { run: Run; calls: string[] } => {
  const calls: string[] = [];
  const run: Run = async (file, args) => {
    calls.push([file, ...args].join(' '));
    return outputs[file] ?? '';
  };
  return { run, calls };
};

test('netstat の行から、クライアント側のポートでサーバーへ張った接続を持つプロセスを引き、閉じた接続とサーバー側の行は使わない', () => {
  assert.equal(ownerInNetstat(NETSTAT, 50692, 7337), 33544);
  assert.equal(ownerInNetstat(NETSTAT, 50700, 7337), 22222);
  assert.equal(ownerInNetstat(NETSTAT, 50999, 7337), undefined);
});

test('Windows では、シェルに書くパスを / 区切りにし、MCP の呼び出し元を netstat から引き、受信ソケットへは送らない', async () => {
  const win = platformFor('win32');
  assert.equal(win.shellPath('C:\\Users\\me\\.durun-dashboard'), 'C:/Users/me/.durun-dashboard');
  assert.equal(win.defaultChromePath({ PROGRAMFILES: 'D:\\Apps' }), 'D:\\Apps\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.defaultChromePath({}), 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.localSocketPath('C:\\Temp\\test-abc', 'inbox'), '\\\\.\\pipe\\test-abc-inbox');
  assert.equal(win.headersHelperSendsPid, false);
  assert.match(win.inboxBlocker ?? '', /認証/);

  const { run, calls } = fakeRun({ netstat: NETSTAT });
  assert.equal(await win.connectionOwner(50692, 7337, run), 33544);
  assert.deepEqual(calls, ['netstat -ano']);
  assert.equal(await win.readMemory(run), null);
});

test('macOS では、パスをそのまま使い、PID は headersHelper に任せ、メモリを sysctl と vm_stat から読む', async () => {
  const mac = platformFor('darwin');
  assert.equal(mac.shellPath('/Users/me/.durun-dashboard'), '/Users/me/.durun-dashboard');
  assert.equal(mac.defaultChromePath({}), '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome');
  assert.equal(mac.localSocketPath('/tmp/test-abc', 'inbox'), '/tmp/test-abc/inbox.sock');
  assert.equal(mac.headersHelperSendsPid, true);
  assert.equal(mac.inboxBlocker, undefined);

  const { run, calls } = fakeRun({
    '/usr/sbin/sysctl': '2\n40\n17179869184\ntotal = 2048.00M  used = 512.00M  free = 1536.00M  (encrypted)\n',
    '/usr/bin/vm_stat': 'Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages wired down:                        100.\n',
  });
  const memory = await mac.readMemory(run);
  assert.deepEqual(calls.map((c) => c.split(' ')[0]).sort(), ['/usr/bin/vm_stat', '/usr/sbin/sysctl']);
  assert.equal(memory?.level, 'warn');
  assert.equal(memory?.pressurePercent, 60);
  assert.equal(memory?.wired, 100 * 16384);
  assert.equal(memory?.swapUsed, 512 * 1024 * 1024);
});

test('ほかの OS では、Chrome の場所とメモリを分からないものとして返す', async () => {
  const linux = platformFor('linux');
  assert.equal(linux.name, 'other');
  assert.equal(linux.defaultChromePath({}), undefined);
  assert.equal(await linux.readMemory(), null);
  assert.equal(linux.headersHelperSendsPid, true);
});

test('MCP の呼び出しには、headersHelper が PID を送れない OS でだけ、接続の持ち主の PID を補う', async () => {
  const request = (): IncomingMessage => ({ headers: { 'x-claude-pid': '$PPID' }, socket: { remotePort: 50692, localPort: 7337 } }) as unknown as IncomingMessage;
  const lookups: number[][] = [];
  const withOwner = (headersHelperSendsPid: boolean, owner: number | undefined): Platform => ({
    ...platformFor('win32'),
    headersHelperSendsPid,
    connectionOwner: async (clientPort, serverPort) => {
      lookups.push([clientPort, serverPort]);
      return owner;
    },
  });

  const filled = request();
  await fillCallerPid(filled, withOwner(false, 33544));
  // 同じ接続の 2 回目は、持ち主を引き直さない。
  await fillCallerPid(filled, withOwner(false, 1));
  assert.equal(filled.headers['x-claude-pid'], '33544');
  assert.deepEqual(lookups, [[50692, 7337]]);

  const unknown = request();
  await fillCallerPid(unknown, withOwner(false, undefined));
  assert.equal(unknown.headers['x-claude-pid'], '$PPID');

  const sent = request();
  await fillCallerPid(sent, withOwner(true, 33544));
  assert.equal(sent.headers['x-claude-pid'], '$PPID');
});

test('claude は、偽物が指されていれば node でそれを動かし、なければ PATH の claude を起動する', () => {
  assert.deepEqual(claudeCommand(['--bg', '--', '依頼'], {}), { file: 'claude', args: ['--bg', '--', '依頼'] });
  assert.deepEqual(claudeCommand(['stop', 'j1'], { DURUN_DASHBOARD_FAKE_CLAUDE: '/tmp/fake.mjs' }), { file: process.execPath, args: ['/tmp/fake.mjs', 'stop', 'j1'] });
});

test('OS の判定と claude の起動は、互換レイヤーの外に書かない', () => {
  // 外に書くと、その OS でしか通らないコードが、もう片方の OS のテストをすり抜ける。
  const OS_CHECK = /process\.platform|os\.(platform|type|release)\(|path\.sep\b|process\.env\.(OS|OSTYPE)\b/;
  const CLAUDE_SPAWN = /(spawn|execFile)\(\s*'claude'/;
  const root = path.join(import.meta.dirname, '..');
  const offenders: string[] = [];
  for (const dir of ['server', 'test', 'shared']) {
    for (const name of fs.readdirSync(path.join(root, dir)).filter((f) => f.endsWith('.ts'))) {
      const file = `${dir}/${name}`;
      if (file === 'server/platform.ts' || file === 'test/platform.test.ts') continue;
      fs.readFileSync(path.join(root, file), 'utf8').split('\n').forEach((line, i) => {
        if (OS_CHECK.test(line) || CLAUDE_SPAWN.test(line)) offenders.push(`${file}:${i + 1}: ${line.trim()}`);
      });
    }
  }
  assert.deepEqual(offenders, []);
});
