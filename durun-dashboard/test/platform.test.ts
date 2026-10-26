import assert from 'node:assert/strict';
import fs from 'node:fs';
import type { IncomingMessage } from 'node:http';
import path from 'node:path';
import { test } from 'node:test';
import { fillCallerPid } from '../server/callerPid.ts';
import { toMemoryUsage } from '../server/memory.ts';
import { claudeCommand, type Files, hostsInTailscaleStatus, ownerInNetstat, type Platform, platformFor, type Run } from '../server/platform.ts';

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

test('Windows では、シェルに書くパスを / 区切りにし、MCP の呼び出し元を netstat から引く', async () => {
  const win = platformFor('win32');
  assert.equal(win.shellPath('C:\\Users\\me\\.durun-dashboard'), 'C:/Users/me/.durun-dashboard');
  assert.equal(win.defaultChromePath({ PROGRAMFILES: 'D:\\Apps' }), 'D:\\Apps\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.defaultChromePath({}), 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe');
  assert.equal(win.localSocketPath('C:\\Temp\\test-abc', 'inbox'), '\\\\.\\pipe\\test-abc-inbox');
  assert.equal(win.headersHelperSendsPid, false);

  const { run, calls } = fakeRun({ netstat: NETSTAT });
  assert.equal(await win.connectionOwner(50692, 7337, run), 33544);
  assert.deepEqual(calls, ['netstat -ano']);
});

test('Windows では、物理メモリの合計と空きから使用率を求め、使用率で段階を分ける', async () => {
  const GIB = 1024 ** 3;
  assert.deepEqual(toMemoryUsage(32 * GIB, 12 * GIB), { kind: 'usage', level: 'normal', percent: 62.5, total: 32 * GIB, used: 20 * GIB, available: 12 * GIB });
  assert.equal(toMemoryUsage(100, 21).level, 'normal');
  assert.equal(toMemoryUsage(100, 20).level, 'warn');
  assert.equal(toMemoryUsage(100, 10).level, 'critical');
  assert.equal(toMemoryUsage(0, 0).percent, 0);

  // 合計と空きは Node がどの OS でも返すので、外のコマンドを呼ばずに読める。
  const { run, calls } = fakeRun({});
  const memory = await platformFor('win32').readMemory(run);
  assert.deepEqual(calls, []);
  assert.equal(memory?.kind, 'usage');
  assert.ok(memory && memory.total > 0 && memory.percent >= 0 && memory.percent <= 100);
});

test('Windows では、受信ソケットのトークンを、送り先のセッションの鍵から読み、読めなければ理由を返す', () => {
  const win = platformFor('win32');
  const dir = 'C:\\Users\\me\\.claude\\sessions';
  const files = (contents: Record<string, string>): Files => ({
    list: () => Object.keys(contents),
    read: (file) => contents[file.slice(dir.length + 1)] ?? assert.fail(`無いファイルを読んだ: ${file}`),
  });
  // PID が前方一致するだけのほかのセッションの鍵と、セッションファイルは選ばない。
  const sessions = files({
    '123.json': '{}',
    '1234.aaaa.key': JSON.stringify({ peerToken: 'other' }),
    '123.bbbb.key': JSON.stringify({ peerToken: 'mine', procStartFt: '1', pidDomain: 'win32:core' }),
  });
  assert.equal(win.inboxToken(123, dir, sessions), 'mine');
  assert.throws(() => win.inboxToken(999, dir, sessions), /鍵がありません/);
  assert.throws(() => win.inboxToken(123, dir, files({ '123.bbbb.key': JSON.stringify({ token: 'renamed' }) })), /鍵の形が変わって/);
});

test('supervisor の制御ソケットは、POSIX では設定の場所のハッシュで分けた /tmp の下に、Windows では pipe.key の値で名付けた名前付きパイプにある', () => {
  for (const name of ['darwin', 'linux']) assert.equal(platformFor(name).supervisorSocket('/home/me/.claude'), `/tmp/cc-daemon-${process.getuid?.() ?? 0}/cb67f5c9/control.sock`);
  const pipeKey = (key: string): Files => ({ list: () => [], read: (file) => (file === 'C:\\Users\\me\\.claude\\daemon\\pipe.key' ? key : assert.fail(`無いファイルを読んだ: ${file}`)) });
  const win = platformFor('win32');
  assert.equal(win.supervisorSocket('C:\\Users\\me\\.claude', pipeKey('0123456789abcdef\n')), '\\\\.\\pipe\\cc-daemon-0123456789abcdef-control');
  assert.throws(() => win.supervisorSocket('C:\\Users\\me\\.claude', pipeKey('')), /pipe\.key の形が変わって/);
});

test('macOS では、パスをそのまま使い、PID は headersHelper に任せ、メモリを sysctl と vm_stat から読む', async () => {
  const mac = platformFor('darwin');
  assert.equal(mac.shellPath('/Users/me/.durun-dashboard'), '/Users/me/.durun-dashboard');
  assert.equal(mac.defaultChromePath({}), '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome');
  assert.equal(mac.localSocketPath('/tmp/test-abc', 'inbox'), '/tmp/test-abc/inbox.sock');
  assert.equal(mac.headersHelperSendsPid, true);
  // Unix ソケットには認証が要らないので、鍵を読みに行かない。
  assert.equal(mac.inboxToken(123, '/sessions', { list: () => assert.fail('読まない'), read: () => assert.fail('読まない') }), undefined);

  const { run, calls } = fakeRun({
    '/usr/sbin/sysctl': '2\n40\n17179869184\ntotal = 2048.00M  used = 512.00M  free = 1536.00M  (encrypted)\n',
    '/usr/bin/vm_stat': 'Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages wired down:                        100.\n',
  });
  const memory = await mac.readMemory(run);
  assert.deepEqual(calls.map((c) => c.split(' ')[0]).sort(), ['/usr/bin/vm_stat', '/usr/sbin/sysctl']);
  assert.equal(memory?.level, 'warn');
  assert.equal(memory?.kind, 'pressure');
  assert.equal(memory?.percent, 60);
  assert.equal(memory?.wired, 100 * 16384);
  assert.equal(memory?.swapUsed, 512 * 1024 * 1024);
});

test('load average は、POSIX では直近 1 分の値を返し、Node が負荷によらず 0 を返す Windows では測れないものとする', () => {
  for (const name of ['darwin', 'linux']) assert.equal(typeof platformFor(name).loadAverage(), 'number');
  assert.equal(platformFor('win32').loadAverage(), undefined);
});

test('ほかの OS では、Chrome の場所とメモリを分からないものとして返す', async () => {
  const linux = platformFor('linux');
  assert.equal(linux.name, 'other');
  assert.equal(linux.defaultChromePath({}), undefined);
  assert.equal(await linux.readMemory(), null);
  assert.equal(linux.headersHelperSendsPid, true);
});

test('画面に渡すパスは、Windows では区切りを / にそろえ、エクスプローラーの最上位には、Windows ではつながっているドライブを、ほかの OS では / を並べる', () => {
  const win = platformFor('win32');
  assert.equal(win.displayPath('C:\\Users\\me\\dev'), 'C:/Users/me/dev');
  const asked: string[] = [];
  const exists = (dir: string) => {
    asked.push(dir);
    return dir === 'C:\\' || dir === 'E:\\';
  };
  assert.deepEqual(win.fileRoots(exists), ['C:/', 'E:/']);
  assert.equal(asked.length, 26);

  for (const name of ['darwin', 'linux']) {
    const p = platformFor(name);
    assert.equal(p.displayPath('/Users/me/dev'), '/Users/me/dev');
    assert.deepEqual(p.fileRoots(() => assert.fail('確かめない')), ['/']);
  }
});

const TAILSCALE_STATUS =JSON.stringify({ BackendState: 'Running', Self: { HostName: 'Core PC', DNSName: 'core.tail1234.ts.net.', TailscaleIPs: ['100.88.0.1', 'fd7a:115c:a1e0::1'] } });
const TAILNET_HOSTS = ['core.tail1234.ts.net', 'core', '100.88.0.1', '[fd7a:115c:a1e0::1]'];

test('tailscale の状態から、この端末の MagicDNS の名前と IP を取り出し、つながっていなければ空にする', () => {
  assert.deepEqual(hostsInTailscaleStatus(TAILSCALE_STATUS), TAILNET_HOSTS);
  assert.deepEqual(hostsInTailscaleStatus(JSON.stringify({ BackendState: 'Stopped', Self: { DNSName: 'core.tail1234.ts.net.', TailscaleIPs: [] } })), []);
  assert.deepEqual(hostsInTailscaleStatus(JSON.stringify({ BackendState: 'Running', Self: { DNSName: '', TailscaleIPs: ['100.88.0.1'] } })), ['100.88.0.1']);
});

test('tailnet での名前は、どの OS でも tailscale の CLI に聞き、macOS ではアプリの中の CLI も試し、CLI が無ければ空にする', async () => {
  const MAC_APP = '/Applications/Tailscale.app/Contents/MacOS/Tailscale';
  for (const name of ['win32', 'darwin', 'linux']) {
    const { run, calls } = fakeRun({ tailscale: TAILSCALE_STATUS });
    assert.deepEqual(await platformFor(name).tailnetHosts(run), TAILNET_HOSTS);
    assert.deepEqual(calls, ['tailscale status --json --peers=false']);
  }
  const tried: string[] = [];
  const onlyAt =
    (found: string): Run =>
    async (file) => {
      tried.push(file);
      if (file !== found) throw new Error('ENOENT');
      return TAILSCALE_STATUS;
    };
  assert.deepEqual(await platformFor('darwin').tailnetHosts(onlyAt(MAC_APP)), TAILNET_HOSTS);
  assert.deepEqual(tried, ['tailscale', MAC_APP]);
  assert.deepEqual(await platformFor('win32').tailnetHosts(onlyAt(MAC_APP)), []);
  assert.deepEqual(await platformFor('linux').tailnetHosts(onlyAt(MAC_APP)), []);
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

test('設定に書いたコマンドの行は、Windows では cmd.exe に行のまま渡し、ほかの OS では sh -c で動かす', () => {
  const line = 'tmux new-session -A -s "$(basename "$PWD")"';
  assert.deepEqual(platformFor('darwin').shellCommand(line), { file: '/bin/sh', args: ['-c', line] });
  assert.deepEqual(platformFor('other').shellCommand(line), { file: '/bin/sh', args: ['-c', line] });
  assert.deepEqual(platformFor('win32').shellCommand('wsl tmux new -A -s "a b"'), { file: 'cmd.exe', args: '/d /s /c "wsl tmux new -A -s "a b""' });
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
